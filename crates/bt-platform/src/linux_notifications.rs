//! Desktop notifications through the freedesktop session notification service.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::task::Poll;
use std::thread::JoinHandle;

use futures_util::future::{FutureExt, poll_fn, select};
use futures_util::stream::{self, StreamExt, TryStreamExt};
use futures_util::task::AtomicWaker;
use futures_util::{pin_mut, select as select_macro};
use zbus::message::Type;
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::ThreadPriority;
use crate::admission::WorkerCtx;

const NOTIFIER_THREAD: &str = "bt-linux-notification";
const NOTIFICATIONS_SERVICE: &str = "org.freedesktop.Notifications";
const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";
const NOTIFICATIONS_INTERFACE: &str = "org.freedesktop.Notifications";
const DBUS_SERVICE: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const SIGNAL_QUEUE_CAPACITY: usize = 64;

struct NotifierState {
    wake: Mutex<Box<dyn Fn() + Send + 'static>>,
    closing: AtomicBool,
    failed: AtomicBool,
    activations: Mutex<Vec<String>>,
    failure: Mutex<Option<String>>,
}

impl NotifierState {
    fn wake(&self) {
        self.wake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)();
    }

    fn record_failure(&self, message: String) {
        if self.closing.load(Ordering::Acquire) {
            return;
        }
        let mut failure = self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if failure.is_none() {
            self.failed.store(true, Ordering::Release);
            *failure = Some(message);
            drop(failure);
            self.wake();
        }
    }

    fn record_activation(&self, launch: String) {
        if self.closing.load(Ordering::Acquire) {
            return;
        }
        self.activations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(launch);
        self.wake();
    }
}

enum Command {
    Show(Notification),
    Shutdown,
}

struct Notification {
    title: String,
    body: String,
    launch: String,
}

struct WorkerDoor {
    commands: mpsc::Sender<Command>,
    command_waker: Arc<AtomicWaker>,
}

struct RegisteredWorker {
    commands: mpsc::Sender<Command>,
    command_waker: Arc<AtomicWaker>,
    thread: JoinHandle<()>,
}

fn registered_workers() -> &'static Mutex<Vec<RegisteredWorker>> {
    static WORKERS: OnceLock<Mutex<Vec<RegisteredWorker>>> = OnceLock::new();
    WORKERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Stop every notification worker and join it from the application's shutdown worker.
///
/// Owners request cancellation in [`Notifier::drop`]. The shutdown road repeats
/// that request before joining so a caller that is already retiring the
/// application cannot leave a D-Bus method future behind.
pub fn shutdown_notifications(_worker: &WorkerCtx) -> Result<(), String> {
    let workers = std::mem::take(
        &mut *registered_workers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    for worker in &workers {
        let _ = worker.commands.send(Command::Shutdown);
        worker.command_waker.wake();
    }
    let mut panicked = 0;
    for worker in workers {
        let thread: JoinHandle<()> = worker.thread;
        if thread.join().is_err() {
            panicked += 1;
        }
    }
    if panicked == 0 {
        Ok(())
    } else {
        Err(format!("{panicked} Linux notification worker(s) panicked"))
    }
}

/// Linux desktop notification delivery.
///
/// `show` queues one notification for a worker and does not wait for the desktop
/// service. A D-Bus failure is returned once by [`Self::take_failures`] after
/// the wake callback runs. Click routes are retained only for notification ids
/// accepted by this process and are removed when the service closes them.
pub struct Notifier {
    state: Arc<NotifierState>,
    worker: Option<WorkerDoor>,
    bus_address: Option<String>,
}

impl Notifier {
    /// Construct lazily; no bus connection or desktop service is contacted here.
    pub fn new(wake: Box<dyn Fn() + Send>) -> Result<Self, String> {
        Self::without_registration(wake)
    }

    /// There is no persistent sender identity to write for D-Bus notifications.
    pub fn register_identity() -> Result<(), String> {
        Ok(())
    }

    /// Construct without a persistent sender identity write.
    pub fn without_registration(wake: Box<dyn Fn() + Send>) -> Result<Self, String> {
        Ok(Self {
            state: Arc::new(NotifierState {
                wake: Mutex::new(wake),
                closing: AtomicBool::new(false),
                failed: AtomicBool::new(false),
                activations: Mutex::new(Vec::new()),
                failure: Mutex::new(None),
            }),
            worker: None,
            bus_address: None,
        })
    }

    /// Queue one notification without waiting for the desktop or its reader.
    pub fn show(&mut self, title: &str, body: &str, launch: &str) -> Result<(), String> {
        if let Some(error) = self
            .state
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            return Err(error);
        }
        if self.state.closing.load(Ordering::Acquire) {
            return Err("the Linux notifier is closing".to_owned());
        }
        if self.state.failed.load(Ordering::Acquire) {
            return Err("Linux notification delivery has already failed".to_owned());
        }
        if self.worker.is_none() {
            self.start_worker()?;
        }
        let worker = self
            .worker
            .as_ref()
            .expect("starting a Linux notification creates its worker door");
        worker
            .commands
            .send(Command::Show(Notification {
                title: title.to_owned(),
                body: body.to_owned(),
                launch: launch.to_owned(),
            }))
            .map_err(|_| "the Linux notification worker stopped".to_owned())?;
        worker.command_waker.wake();
        Ok(())
    }

    fn start_worker(&mut self) -> Result<(), String> {
        let (commands, receiver) = mpsc::channel();
        let command_waker = Arc::new(AtomicWaker::new());
        let worker_waker = Arc::clone(&command_waker);
        let state = Arc::clone(&self.state);
        let address = self.bus_address.clone();
        let thread =
            crate::spawn_at_priority(NOTIFIER_THREAD, ThreadPriority::BelowNormal, move |_| {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        state.record_failure(format!(
                            "could not build the Linux notification runtime: {error}"
                        ));
                        return;
                    }
                };
                runtime.block_on(worker_loop(receiver, worker_waker, state, address));
            })
            .map_err(|error| format!("could not start the Linux notification worker: {error}"))?;
        registered_workers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(RegisteredWorker {
                commands: commands.clone(),
                command_waker: Arc::clone(&command_waker),
                thread,
            });
        self.worker = Some(WorkerDoor {
            commands,
            command_waker,
        });
        Ok(())
    }

    /// Take click routes delivered since the last call.
    #[must_use]
    pub fn take_activations(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .state
                .activations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Take the asynchronous delivery error once.
    #[must_use]
    pub fn take_failures(&self) -> Vec<String> {
        self.state
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .into_iter()
            .collect()
    }
}

impl Drop for Notifier {
    fn drop(&mut self) {
        self.state.closing.store(true, Ordering::Release);
        if let Some(worker) = &self.worker {
            let _ = worker.commands.send(Command::Shutdown);
            worker.command_waker.wake();
        }
    }
}

struct PendingNotification {
    launch: String,
    activated: bool,
}

async fn worker_loop(
    receiver: mpsc::Receiver<Command>,
    command_waker: Arc<AtomicWaker>,
    state: Arc<NotifierState>,
    address: Option<String>,
) {
    let first = match next_command(&receiver, &command_waker).await {
        Some(Command::Show(notification)) => notification,
        Some(Command::Shutdown) | None => return,
    };
    let mut queued = VecDeque::from([first]);

    let connection = match race_shutdown(
        connect(address.as_deref()),
        &receiver,
        &command_waker,
        &mut queued,
    )
    .await
    {
        Ok(Ok(connection)) => connection,
        Ok(Err(error)) => {
            state.record_failure(format!(
                "could not connect to the Linux notification service: {error}"
            ));
            return;
        }
        Err(()) => return,
    };
    let notification_rule = match notification_signal_rule() {
        Ok(rule) => rule,
        Err(error) => {
            state.record_failure(format!(
                "could not watch Linux notification actions: {error}"
            ));
            return;
        }
    };
    let notification_signals = match race_shutdown(
        MessageStream::for_match_rule(notification_rule, &connection, Some(SIGNAL_QUEUE_CAPACITY)),
        &receiver,
        &command_waker,
        &mut queued,
    )
    .await
    {
        Ok(Ok(signals)) => signals,
        Ok(Err(error)) => {
            state.record_failure(format!(
                "could not watch Linux notification actions: {error}"
            ));
            return;
        }
        Err(()) => return,
    };
    let owner_rule = match notification_owner_rule() {
        Ok(rule) => rule,
        Err(error) => {
            state.record_failure(format!(
                "could not watch the Linux notification service owner: {error}"
            ));
            return;
        }
    };
    let owner_signals = match race_shutdown(
        MessageStream::for_match_rule(owner_rule, &connection, Some(SIGNAL_QUEUE_CAPACITY)),
        &receiver,
        &command_waker,
        &mut queued,
    )
    .await
    {
        Ok(Ok(signals)) => signals,
        Ok(Err(error)) => {
            state.record_failure(format!(
                "could not watch the Linux notification service owner: {error}"
            ));
            return;
        }
        Err(()) => return,
    };
    let mut signals = stream::select(notification_signals, owner_signals);

    let capabilities = match race_shutdown(
        get_capabilities(&connection),
        &receiver,
        &command_waker,
        &mut queued,
    )
    .await
    {
        Ok(Ok(capabilities)) => capabilities,
        Ok(Err(error)) => {
            state.record_failure(format!(
                "could not query Linux notification capabilities: {error}"
            ));
            return;
        }
        Err(()) => return,
    };
    let actions_supported = capabilities
        .iter()
        .any(|capability| capability == "actions");
    let mut owner = match race_shutdown(
        get_notification_owner(&connection),
        &receiver,
        &command_waker,
        &mut queued,
    )
    .await
    {
        Ok(Ok(owner)) => owner,
        Ok(Err(error)) => {
            state.record_failure(format!(
                "could not identify the Linux notification service: {error}"
            ));
            return;
        }
        Err(()) => return,
    };
    let mut pending: HashMap<u32, PendingNotification> = HashMap::new();

    loop {
        if state.closing.load(Ordering::Acquire) {
            return;
        }
        if let Some(next) = signals.next().now_or_never().flatten() {
            match next {
                Ok(message) => handle_signal(&message, &mut owner, &mut pending, &state),
                Err(error) => {
                    state.record_failure(format!(
                        "could not read Linux notification service signals: {error}"
                    ));
                    return;
                }
            }
            continue;
        }
        if let Some(notification) = queued.pop_front() {
            match show_notification(
                &connection,
                actions_supported,
                &mut pending,
                notification,
                &receiver,
                &command_waker,
                &mut queued,
            )
            .await
            {
                Ok(()) => {}
                Err(ShowFailure::Delivery(error)) => {
                    state.record_failure(error);
                    return;
                }
                Err(ShowFailure::Shutdown) => return,
            }
            continue;
        }
        match receiver.try_recv() {
            Ok(Command::Show(notification)) => {
                queued.push_back(notification);
                continue;
            }
            Ok(Command::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => return,
            Err(mpsc::TryRecvError::Empty) => {}
        }

        let next_signal = signals.try_next().fuse();
        let next_command = next_command(&receiver, &command_waker).fuse();
        pin_mut!(next_signal, next_command);
        select_macro! {
            signal = next_signal => match signal {
                Ok(Some(message)) => {
                    handle_signal(&message, &mut owner, &mut pending, &state);
                }
                Ok(None) => {
                    state.record_failure("the Linux notification service closed its signal stream".to_owned());
                    return;
                }
                Err(error) => {
                    state.record_failure(format!("could not read Linux notification service signals: {error}"));
                    return;
                }
            },
            command = next_command => match command {
                Some(Command::Show(notification)) => queued.push_back(notification),
                Some(Command::Shutdown) | None => return,
            }
        }
    }
}

async fn connect(address: Option<&str>) -> zbus::Result<Connection> {
    let builder = match address {
        Some(address) => zbus::connection::Builder::address(address)?,
        None => zbus::connection::Builder::session()?,
    };
    builder.build().await
}

async fn get_capabilities(connection: &Connection) -> zbus::Result<Vec<String>> {
    connection
        .call_method(
            Some(NOTIFICATIONS_SERVICE),
            NOTIFICATIONS_PATH,
            Some(NOTIFICATIONS_INTERFACE),
            "GetCapabilities",
            &(),
        )
        .await?
        .body()
        .deserialize()
}

async fn get_notification_owner(connection: &Connection) -> zbus::Result<String> {
    connection
        .call_method(
            Some(DBUS_SERVICE),
            DBUS_PATH,
            Some(DBUS_INTERFACE),
            "GetNameOwner",
            &(NOTIFICATIONS_SERVICE,),
        )
        .await?
        .body()
        .deserialize()
}

async fn show_notification(
    connection: &Connection,
    actions_supported: bool,
    pending: &mut HashMap<u32, PendingNotification>,
    notification: Notification,
    receiver: &mpsc::Receiver<Command>,
    command_waker: &AtomicWaker,
    queued: &mut VecDeque<Notification>,
) -> Result<(), ShowFailure> {
    let actions = if actions_supported {
        vec!["default".to_owned(), "Open".to_owned()]
    } else {
        Vec::new()
    };
    let hints = HashMap::<String, zbus::zvariant::OwnedValue>::new();
    let reply = match race_shutdown(
        connection.call_method(
            Some(NOTIFICATIONS_SERVICE),
            NOTIFICATIONS_PATH,
            Some(NOTIFICATIONS_INTERFACE),
            "Notify",
            &(
                "Folio",
                0_u32,
                "",
                notification.title,
                notification.body,
                actions,
                hints,
                -1_i32,
            ),
        ),
        receiver,
        command_waker,
        queued,
    )
    .await
    {
        Ok(Ok(reply)) => reply,
        Ok(Err(error)) => {
            return Err(ShowFailure::Delivery(format!(
                "could not send a Linux desktop notification: {error}"
            )));
        }
        Err(()) => return Err(ShowFailure::Shutdown),
    };
    let id = reply.body().deserialize::<u32>().map_err(|error| {
        ShowFailure::Delivery(format!(
            "Linux notification service returned an invalid id: {error}"
        ))
    })?;
    if actions_supported {
        if pending.contains_key(&id) {
            return Err(ShowFailure::Delivery(format!(
                "Linux notification service reused active notification id {id}"
            )));
        }
        pending.insert(
            id,
            PendingNotification {
                launch: notification.launch,
                activated: false,
            },
        );
    }
    Ok(())
}

enum ShowFailure {
    Shutdown,
    Delivery(String),
}

#[derive(Debug, Eq, PartialEq)]
enum NotificationSignal {
    Action { id: u32, key: String },
    Closed { id: u32 },
    OwnerChanged { old: String, new: String },
}

fn notification_signal(message: &Message, owner: &str) -> Option<NotificationSignal> {
    let header = message.header();
    let member = header.member()?.as_str();
    match member {
        "ActionInvoked" => {
            if header.sender()?.as_str() != owner {
                return None;
            }
            let (id, key): (u32, String) = message.body().deserialize().ok()?;
            Some(NotificationSignal::Action { id, key })
        }
        "NotificationClosed" => {
            if header.sender()?.as_str() != owner {
                return None;
            }
            let (id, _reason): (u32, u32) = message.body().deserialize().ok()?;
            Some(NotificationSignal::Closed { id })
        }
        "NameOwnerChanged" => {
            if header.sender()?.as_str() != DBUS_SERVICE {
                return None;
            }
            let (name, old, new): (String, String, String) = message.body().deserialize().ok()?;
            (name == NOTIFICATIONS_SERVICE).then_some(NotificationSignal::OwnerChanged { old, new })
        }
        _ => None,
    }
}

fn handle_signal(
    message: &Message,
    owner: &mut String,
    pending: &mut HashMap<u32, PendingNotification>,
    state: &NotifierState,
) {
    match notification_signal(message, owner) {
        Some(NotificationSignal::Action { id, key }) if key == "default" => {
            if let Some(notification) = pending.get_mut(&id)
                && !notification.activated
            {
                notification.activated = true;
                state.record_activation(notification.launch.clone());
            }
        }
        Some(NotificationSignal::Closed { id }) => {
            pending.remove(&id);
        }
        Some(NotificationSignal::OwnerChanged { old, new }) => {
            if &old == owner && old != new {
                pending.clear();
                *owner = new;
            }
        }
        Some(NotificationSignal::Action { .. }) | None => {}
    }
}

fn notification_signal_rule() -> zbus::Result<zbus::OwnedMatchRule> {
    Ok(MatchRule::builder()
        .msg_type(Type::Signal)
        .path(NOTIFICATIONS_PATH)?
        .interface(NOTIFICATIONS_INTERFACE)?
        .build()
        .into())
}

fn notification_owner_rule() -> zbus::Result<zbus::OwnedMatchRule> {
    Ok(MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(DBUS_SERVICE)?
        .path(DBUS_PATH)?
        .interface(DBUS_INTERFACE)?
        .member("NameOwnerChanged")?
        .add_arg(NOTIFICATIONS_SERVICE)?
        .build()
        .into())
}

async fn race_shutdown<F, T>(
    future: F,
    receiver: &mpsc::Receiver<Command>,
    waker: &AtomicWaker,
    queued: &mut VecDeque<Notification>,
) -> Result<T, ()>
where
    F: Future<Output = T>,
{
    let future = future.fuse();
    let shutdown = shutdown_or_queue(receiver, waker, queued).fuse();
    pin_mut!(future, shutdown);
    match select(future, shutdown).await {
        futures_util::future::Either::Left((value, _)) => Ok(value),
        futures_util::future::Either::Right(((), _)) => Err(()),
    }
}

async fn shutdown_or_queue(
    receiver: &mpsc::Receiver<Command>,
    waker: &AtomicWaker,
    queued: &mut VecDeque<Notification>,
) {
    poll_fn(|context| {
        loop {
            match receiver.try_recv() {
                Ok(Command::Show(notification)) => queued.push_back(notification),
                Ok(Command::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => {
                    return Poll::Ready(());
                }
                Err(mpsc::TryRecvError::Empty) => {
                    waker.register(context.waker());
                    match receiver.try_recv() {
                        Ok(Command::Show(notification)) => queued.push_back(notification),
                        Ok(Command::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => {
                            return Poll::Ready(());
                        }
                        Err(mpsc::TryRecvError::Empty) => return Poll::Pending,
                    }
                }
            }
        }
    })
    .await
}

async fn next_command(receiver: &mpsc::Receiver<Command>, waker: &AtomicWaker) -> Option<Command> {
    poll_fn(|context| match receiver.try_recv() {
        Ok(command) => Poll::Ready(Some(command)),
        Err(mpsc::TryRecvError::Disconnected) => Poll::Ready(None),
        Err(mpsc::TryRecvError::Empty) => {
            waker.register(context.waker());
            match receiver.try_recv() {
                Ok(command) => Poll::Ready(Some(command)),
                Err(mpsc::TryRecvError::Disconnected) => Poll::Ready(None),
                Err(mpsc::TryRecvError::Empty) => Poll::Pending,
            }
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command as ProcessCommand, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, OnceLock, mpsc};

    use super::{
        NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH, NOTIFICATIONS_SERVICE, NotificationSignal,
        Notifier, notification_signal,
    };

    struct PrivateBus {
        child: Child,
        address: String,
        directory: std::path::PathBuf,
    }

    impl PrivateBus {
        fn start() -> Self {
            static NEXT_BUS: AtomicU32 = AtomicU32::new(0);
            let directory = std::env::temp_dir().join(format!(
                "folio-private-notifications-{}-{}",
                std::process::id(),
                NEXT_BUS.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).expect("create a private bus config directory");
            let services = directory.join("services");
            std::fs::create_dir(&services).expect("create an empty private bus service directory");
            let config = directory.join("session.conf");
            let config_text = format!(
                "<busconfig>\n\
                 <type>session</type>\n\
                 <listen>unix:tmpdir=/tmp</listen>\n\
                 <auth>EXTERNAL</auth>\n\
                 <servicedir>{}</servicedir>\n\
                 <policy context=\"default\">\n\
                 <allow send_destination=\"*\" eavesdrop=\"true\"/>\n\
                 <allow eavesdrop=\"true\"/>\n\
                 <allow own=\"*\"/>\n\
                 </policy>\n\
                 </busconfig>\n",
                services.display()
            );
            std::fs::write(&config, config_text).expect("write an isolated bus config");
            let mut child = ProcessCommand::new("dbus-daemon")
                .arg(format!("--config-file={}", config.display()))
                .args(["--nofork", "--print-address=1", "--nopidfile"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("start the controlled private D-Bus helper");
            let stdout = child.stdout.take().expect("the private bus address pipe");
            let mut address = String::new();
            if let Err(error) = BufReader::new(stdout).read_line(&mut address) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&directory);
                panic!("could not read the private bus address: {error}");
            }
            if address.trim().is_empty() {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&directory);
                panic!("the private bus did not print a non-empty address");
            }
            Self {
                child,
                address: address.trim().to_owned(),
                directory,
            }
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    struct NotificationCall {
        id: u32,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        expire_timeout: i32,
    }

    struct FakeNotifications {
        calls: mpsc::Sender<NotificationCall>,
        next_id: AtomicU32,
        capabilities: Vec<String>,
        capability_error: Option<String>,
        capability_gate: Mutex<Option<CapabilityGate>>,
    }

    struct CapabilityGate {
        started: mpsc::Sender<()>,
        release: tokio::sync::oneshot::Receiver<()>,
    }

    struct ReleaseCapability(Option<tokio::sync::oneshot::Sender<()>>);

    impl ReleaseCapability {
        fn release(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    impl Drop for ReleaseCapability {
        fn drop(&mut self) {
            self.release();
        }
    }

    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl FakeNotifications {
        async fn get_capabilities(&self) -> zbus::fdo::Result<Vec<String>> {
            let gate = self
                .capability_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(gate) = gate {
                let _ = gate.started.send(());
                let _ = gate.release.await;
            }
            if let Some(error) = &self.capability_error {
                return Err(zbus::fdo::Error::Failed(error.clone()));
            }
            Ok(self.capabilities.clone())
        }

        #[expect(
            clippy::too_many_arguments,
            reason = "the freedesktop Notify wire signature has eight arguments"
        )]
        fn notify(
            &self,
            app_name: String,
            replaces_id: u32,
            app_icon: String,
            summary: String,
            body: String,
            actions: Vec<String>,
            _hints: HashMap<String, zbus::zvariant::OwnedValue>,
            expire_timeout: i32,
        ) -> u32 {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            self.calls
                .send(NotificationCall {
                    id,
                    app_name,
                    replaces_id,
                    app_icon,
                    summary,
                    body,
                    actions,
                    expire_timeout,
                })
                .expect("the test reads every notification call");
            id
        }

        fn close_notification(&self, _id: u32) {}
    }

    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("build the controlled D-Bus test runtime")
    }

    fn start_fake_notifications(
        runtime: &tokio::runtime::Runtime,
        address: &str,
        calls: mpsc::Sender<NotificationCall>,
        capabilities: &[&str],
        capability_error: Option<&str>,
        capability_gate: Option<CapabilityGate>,
    ) -> zbus::Connection {
        runtime.block_on(async {
            zbus::connection::Builder::address(address)
                .expect("the private bus address is valid")
                .name(NOTIFICATIONS_SERVICE)
                .expect("the fake notification service name is valid")
                .serve_at(
                    NOTIFICATIONS_PATH,
                    FakeNotifications {
                        calls,
                        next_id: AtomicU32::new(101),
                        capabilities: capabilities
                            .iter()
                            .map(|value| (*value).to_owned())
                            .collect(),
                        capability_error: capability_error.map(str::to_owned),
                        capability_gate: Mutex::new(capability_gate),
                    },
                )
                .expect("register the fake notification object")
                .build()
                .await
                .expect("connect the controlled notification service")
        })
    }

    fn emit_signal(
        runtime: &tokio::runtime::Runtime,
        service: &zbus::Connection,
        member: &str,
        body: &(u32, &str),
    ) {
        runtime
            .block_on(service.emit_signal(
                None::<&str>,
                NOTIFICATIONS_PATH,
                NOTIFICATIONS_INTERFACE,
                member,
                body,
            ))
            .expect("emit the controlled notification signal");
    }

    fn emit_closed(runtime: &tokio::runtime::Runtime, service: &zbus::Connection, id: u32) {
        runtime
            .block_on(service.emit_signal(
                None::<&str>,
                NOTIFICATIONS_PATH,
                NOTIFICATIONS_INTERFACE,
                "NotificationClosed",
                &(id, 2_u32),
            ))
            .expect("emit the controlled notification close signal");
    }

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn an_action_signal_from_a_different_service_owner_is_not_a_route() {
        let foreign =
            zbus::Message::signal(NOTIFICATIONS_PATH, NOTIFICATIONS_INTERFACE, "ActionInvoked")
                .expect("the signal header is valid")
                .sender(":1.91")
                .expect("the signal sender is a unique bus name")
                .build(&(101_u32, "default"))
                .expect("the signal body is valid");
        assert_eq!(notification_signal(&foreign, ":1.17"), None);

        let owned =
            zbus::Message::signal(NOTIFICATIONS_PATH, NOTIFICATIONS_INTERFACE, "ActionInvoked")
                .expect("the signal header is valid")
                .sender(":1.17")
                .expect("the signal sender is a unique bus name")
                .build(&(101_u32, "default"))
                .expect("the signal body is valid");
        assert_eq!(
            notification_signal(&owned, ":1.17"),
            Some(NotificationSignal::Action {
                id: 101,
                key: "default".to_owned(),
            })
        );
    }

    fn reap_notifications() {
        let worker = crate::spawn_at_priority(
            "bt-notification-test-retire",
            crate::ThreadPriority::Normal,
            super::shutdown_notifications,
        )
        .expect("start the controlled helper reaper");
        worker
            .join()
            .expect("the helper reaper joined")
            .expect("notification workers stop cleanly");
    }

    #[test]
    fn a_private_notification_service_delivers_only_its_supported_default_click_route() {
        let _serial = test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let runtime = test_runtime();
        let bus = PrivateBus::start();
        let (call_tx, calls) = mpsc::channel();
        let service =
            start_fake_notifications(&runtime, &bus.address, call_tx, &["actions"], None, None);
        let (wake, wakes) = mpsc::channel();
        let mut notifier = Notifier::without_registration(Box::new(move || {
            let _ = wake.send(());
        }))
        .expect("construct the test notifier");
        notifier.bus_address = Some(bus.address.clone());
        notifier
            .show("Build finished", "Ready", "w=11&t=22&s=33")
            .expect("queue the notification");

        let call = calls.recv().expect("the fake server receives Notify");
        assert_eq!(call.id, 101);
        assert_eq!(call.app_name, "Folio");
        assert_eq!(call.replaces_id, 0);
        assert_eq!(call.app_icon, "");
        assert_eq!(call.summary, "Build finished");
        assert_eq!(call.body, "Ready");
        assert_eq!(call.actions, ["default", "Open"]);
        assert_eq!(call.expire_timeout, -1);

        emit_closed(&runtime, &service, call.id);
        emit_signal(&runtime, &service, "ActionInvoked", &(call.id, "default"));
        emit_signal(&runtime, &service, "ActionInvoked", &(999, "default"));
        emit_signal(&runtime, &service, "ActionInvoked", &(call.id, "foreign"));
        // A second notification provides the completion signal after the closed
        // id and foreign action have passed through the same ordered D-Bus stream.
        notifier
            .show("Still running", "Ready", "w=44&t=55&s=66")
            .expect("queue the second notification");
        let second = calls
            .recv()
            .expect("the fake server receives the second Notify");
        emit_signal(&runtime, &service, "ActionInvoked", &(second.id, "default"));
        wakes.recv().expect("the valid click wakes the application");
        assert_eq!(
            notifier.take_activations(),
            ["w=44&t=55&s=66"],
            "only an active id and its default action return the launch route"
        );
        assert!(notifier.take_failures().is_empty());

        drop(notifier);
        reap_notifications();
        {
            let _entered = runtime.enter();
            drop(service);
        }
        drop(bus);
        drop(runtime);
    }

    #[test]
    fn a_private_bus_without_a_notification_service_reports_the_async_failure_once() {
        let _serial = test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bus = PrivateBus::start();
        let (wake, wakes) = mpsc::channel();
        let mut notifier = Notifier::without_registration(Box::new(move || {
            let _ = wake.send(());
        }))
        .expect("construct the test notifier");
        notifier.bus_address = Some(bus.address.clone());
        notifier
            .show("No server", "Failure arrives later", "w=1&t=2&s=3")
            .expect("the queued worker has not contacted D-Bus synchronously");
        wakes
            .recv()
            .expect("the asynchronous failure wakes the application");
        let failures = notifier.take_failures();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("ServiceUnknown"), "{}", failures[0]);
        assert!(notifier.take_failures().is_empty());
        assert!(
            notifier
                .show("After refusal", "No second delivery", "w=1&t=2&s=3")
                .is_err()
        );

        drop(notifier);
        reap_notifications();
        drop(bus);
    }

    #[test]
    fn a_private_service_method_error_is_reported_as_an_async_failure_once() {
        let _serial = test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let runtime = test_runtime();
        let bus = PrivateBus::start();
        let (call_tx, _calls) = mpsc::channel();
        let service = start_fake_notifications(
            &runtime,
            &bus.address,
            call_tx,
            &["actions"],
            Some("controlled capabilities refusal"),
            None,
        );
        let (wake, wakes) = mpsc::channel();
        let mut notifier = Notifier::without_registration(Box::new(move || {
            let _ = wake.send(());
        }))
        .expect("construct the test notifier");
        notifier.bus_address = Some(bus.address.clone());
        notifier
            .show("Method error", "Failure arrives later", "w=7&t=8&s=9")
            .expect("the queued worker has not contacted D-Bus synchronously");
        wakes
            .recv()
            .expect("the asynchronous method error wakes the application");
        let failures = notifier.take_failures();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("controlled capabilities refusal"));
        assert!(notifier.take_failures().is_empty());

        drop(notifier);
        reap_notifications();
        {
            let _entered = runtime.enter();
            drop(service);
        }
        drop(bus);
        drop(runtime);
    }

    #[test]
    fn dropping_the_notifier_cancels_a_dbus_call_before_helper_shutdown_joins() {
        let _serial = test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let runtime = test_runtime();
        let bus = PrivateBus::start();
        let (call_tx, _calls) = mpsc::channel();
        let (started, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let mut release = ReleaseCapability(Some(release_tx));
        let service = start_fake_notifications(
            &runtime,
            &bus.address,
            call_tx,
            &["actions"],
            None,
            Some(CapabilityGate {
                started,
                release: release_rx,
            }),
        );
        let mut notifier = Notifier::without_registration(Box::new(|| {})).expect("test notifier");
        notifier.bus_address = Some(bus.address.clone());
        notifier
            .show("Held method", "Shutdown cancels it", "w=1&t=2&s=3")
            .expect("queue notification");
        started_rx
            .recv()
            .expect("the fake server entered its controlled method wait");

        drop(notifier);
        reap_notifications();
        release.release();
        {
            let _entered = runtime.enter();
            drop(service);
        }
        drop(bus);
        drop(runtime);
    }

    #[test]
    fn a_server_without_action_capability_receives_no_click_action() {
        let _serial = test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let runtime = test_runtime();
        let bus = PrivateBus::start();
        let (call_tx, calls) = mpsc::channel();
        let service =
            start_fake_notifications(&runtime, &bus.address, call_tx, &["body"], None, None);
        let mut notifier = Notifier::without_registration(Box::new(|| {})).expect("test notifier");
        notifier.bus_address = Some(bus.address.clone());
        notifier
            .show("Passive", "No actions", "w=1&t=2&s=3")
            .expect("queue passive notification");
        let call = calls.recv().expect("the fake server receives Notify");
        assert!(call.actions.is_empty());
        assert!(notifier.take_activations().is_empty());

        drop(notifier);
        reap_notifications();
        {
            let _entered = runtime.enter();
            drop(service);
        }
        drop(bus);
        drop(runtime);
    }
}
