//! The Linux system appearance fact, read from the XDG Settings portal.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::task::Poll;
use std::thread::JoinHandle;

use futures_util::future::{FutureExt, poll_fn, select};
use futures_util::stream::{self, Stream, StreamExt, TryStreamExt};
use futures_util::task::AtomicWaker;
use futures_util::{pin_mut, select as select_macro};
use zbus::message::Type;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::admission::WorkerCtx;
use crate::{NativeWindow, SystemNews, ThreadPriority};

const SETTINGS_WORKER: &str = "folio-system-settings";
const PORTAL_SERVICE: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_INTERFACE: &str = "org.freedesktop.portal.Settings";
const DBUS_SERVICE: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const APPEARANCE_NAMESPACE: &str = "org.freedesktop.appearance";
const COLOR_SCHEME_KEY: &str = "color-scheme";
const SIGNAL_QUEUE_CAPACITY: usize = 16;

const CACHE_UNKNOWN: u64 = 0;
const CACHE_NO_PREFERENCE: u64 = 1;
const CACHE_DARK: u64 = 2;
const CACHE_LIGHT: u64 = 3;
const CACHE_VALUE_MASK: u64 = 0b11;
const CACHE_GENERATION_MASK: u64 = u64::MAX >> 2;

type Wake = Arc<dyn Fn(SystemNews) + Send + Sync + 'static>;

struct Subscriber {
    generation: u64,
    wake: Wake,
}

struct Shared {
    generation: AtomicU64,
    cache: AtomicU64,
    subscribers: Mutex<HashMap<u64, Subscriber>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            cache: AtomicU64::new(pack_cache(0, CACHE_UNKNOWN)),
            subscribers: Mutex::new(HashMap::new()),
        }
    }

    fn begin_generation(&self) -> u64 {
        let current = self.generation.load(Ordering::Relaxed);
        let next = current.wrapping_add(1) & CACHE_GENERATION_MASK;
        self.generation.store(next, Ordering::Release);
        self.cache
            .store(pack_cache(next, CACHE_UNKNOWN), Ordering::Release);
        next
    }

    fn has_subscribers(&self, generation: u64) -> bool {
        lock(&self.subscribers)
            .values()
            .any(|subscriber| subscriber.generation == generation)
    }

    fn publish(&self, generation: u64, value: Option<bool>) {
        let code = cache_code(value);
        let mut old = self.cache.load(Ordering::Acquire);
        loop {
            if unpack_generation(old) != generation {
                return;
            }
            let next = pack_cache(generation, code);
            match self
                .cache
                .compare_exchange_weak(old, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    if cache_value_code(old) != code {
                        let wakes: Vec<Wake> = lock(&self.subscribers)
                            .values()
                            .filter(|subscriber| subscriber.generation == generation)
                            .map(|subscriber| Arc::clone(&subscriber.wake))
                            .collect();
                        for wake in wakes {
                            wake(SystemNews::Preferences);
                        }
                    }
                    return;
                }
                Err(actual) => old = actual,
            }
        }
    }

    fn cached_light_apps(&self) -> Option<bool> {
        let packed = self.cache.load(Ordering::Acquire);
        match cache_value_code(packed) {
            CACHE_DARK => Some(false),
            CACHE_LIGHT => Some(true),
            CACHE_UNKNOWN | CACHE_NO_PREFERENCE => None,
            _ => None,
        }
    }
}

struct Worker {
    commands: mpsc::Sender<Command>,
    command_waker: Arc<AtomicWaker>,
    thread: JoinHandle<Result<(), String>>,
    address: Option<String>,
}

struct ProcessState {
    next_subscriber: u64,
    worker: Option<Worker>,
}

impl ProcessState {
    fn new() -> Self {
        Self {
            next_subscriber: 1,
            worker: None,
        }
    }
}

static PROCESS: OnceLock<Mutex<ProcessState>> = OnceLock::new();
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

fn process() -> &'static Mutex<ProcessState> {
    PROCESS.get_or_init(|| Mutex::new(ProcessState::new()))
}

fn shared() -> &'static Arc<Shared> {
    SHARED.get_or_init(|| Arc::new(Shared::new()))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A nonblocking subscription to the process-wide desktop appearance source.
pub struct SystemSettingsWatch {
    id: u64,
    generation: u64,
    shared: Arc<Shared>,
}

impl SystemSettingsWatch {
    /// Subscribe this window's event-loop wake to the one process-wide portal
    /// reader. The window handle is accepted to match the other settings-watch
    /// backends; the portal setting belongs to the session, not a surface.
    pub fn install(
        window: NativeWindow,
        wake: Box<dyn Fn(SystemNews) + Send + Sync + 'static>,
    ) -> Result<Self, String> {
        subscribe(window, wake, None)
    }

    #[cfg(test)]
    fn install_at_address(
        window: NativeWindow,
        wake: Box<dyn Fn(SystemNews) + Send + Sync + 'static>,
        address: String,
    ) -> Result<Self, String> {
        subscribe(window, wake, Some(address))
    }
}

fn subscribe(
    window: NativeWindow,
    wake: Box<dyn Fn(SystemNews) + Send + Sync + 'static>,
    address: Option<String>,
) -> Result<SystemSettingsWatch, String> {
    let _ = window;
    let mut process = lock(process());
    let shared = Arc::clone(shared());
    let mut subscribers = lock(&shared.subscribers);
    let first_subscriber = subscribers.is_empty();
    let generation = if first_subscriber {
        shared.begin_generation()
    } else {
        shared.generation.load(Ordering::Acquire)
    };
    let id = next_subscriber(&mut process.next_subscriber, &subscribers);
    subscribers.insert(
        id,
        Subscriber {
            generation,
            wake: Arc::from(wake),
        },
    );
    drop(subscribers);

    if process.worker.is_none() {
        match start_worker(Arc::clone(&shared), address.clone()) {
            Ok(worker) => process.worker = Some(worker),
            Err(error) => {
                lock(&shared.subscribers).remove(&id);
                if lock(&shared.subscribers).is_empty() {
                    let empty_generation = shared.begin_generation();
                    shared.cache.store(
                        pack_cache(empty_generation, CACHE_UNKNOWN),
                        Ordering::Release,
                    );
                }
                return Err(format!(
                    "could not start Linux system settings worker: {error}"
                ));
            }
        }
    } else if process
        .worker
        .as_ref()
        .is_some_and(|worker| worker.address != address)
    {
        lock(&shared.subscribers).remove(&id);
        return Err("a Linux system settings worker is already connected to another bus".into());
    }

    if first_subscriber {
        let worker = process
            .worker
            .as_ref()
            .expect("the first subscription starts the process worker");
        let _ = worker.commands.send(Command::Changed);
        worker.command_waker.wake();
    }

    Ok(SystemSettingsWatch {
        id,
        generation,
        shared,
    })
}

fn next_subscriber(next: &mut u64, subscribers: &HashMap<u64, Subscriber>) -> u64 {
    loop {
        let candidate = *next;
        *next = next.wrapping_add(1).max(1);
        if !subscribers.contains_key(&candidate) {
            return candidate;
        }
    }
}

fn start_worker(shared: Arc<Shared>, address: Option<String>) -> std::io::Result<Worker> {
    let (commands, receiver) = mpsc::channel();
    let command_waker = Arc::new(AtomicWaker::new());
    let worker_waker = Arc::clone(&command_waker);
    let worker_address = address.clone();
    let thread = crate::spawn_at_priority(
        SETTINGS_WORKER,
        ThreadPriority::BelowNormal,
        move |worker| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| {
                    format!("could not build Linux system settings runtime: {error}")
                })?;
            runtime.block_on(worker_loop(
                worker,
                receiver,
                worker_waker,
                shared,
                worker_address,
            ));
            Ok(())
        },
    )?;
    Ok(Worker {
        commands,
        command_waker,
        thread,
        address,
    })
}

/// The window owns only a callback subscription. Retiring the last one sends
/// a command and returns; the portal stream is dropped by its worker.
impl Drop for SystemSettingsWatch {
    fn drop(&mut self) {
        let process = lock(process());
        let removed = lock(&self.shared.subscribers)
            .remove(&self.id)
            .is_some_and(|subscriber| subscriber.generation == self.generation);
        if !removed || !lock(&self.shared.subscribers).is_empty() {
            return;
        }
        let generation = self.shared.begin_generation();
        self.shared
            .cache
            .store(pack_cache(generation, CACHE_UNKNOWN), Ordering::Release);
        if let Some(worker) = &process.worker {
            let _ = worker.commands.send(Command::Changed);
            worker.command_waker.wake();
        }
    }
}

/// The last callback returns to its event loop immediately. The named exit
/// worker calls this after windows have dropped their subscriptions, so the
/// portal worker is joined before the process leaves.
pub fn shutdown_system_settings(_worker: &WorkerCtx) -> Result<(), String> {
    let worker = {
        let mut process = lock(process());
        let shared = Arc::clone(shared());
        let generation = shared.begin_generation();
        shared
            .cache
            .store(pack_cache(generation, CACHE_UNKNOWN), Ordering::Release);
        lock(&shared.subscribers).clear();
        process.worker.take()
    };
    let Some(worker) = worker else {
        return Ok(());
    };
    let _ = worker.commands.send(Command::Shutdown);
    worker.command_waker.wake();
    let thread: JoinHandle<Result<(), String>> = worker.thread;
    match thread.join() {
        Ok(result) => result,
        Err(_) => Err("Linux system settings worker did not stop cleanly".to_owned()),
    }
}

/// Cached system preference. The window-thread getter never contacts D-Bus.
#[must_use]
pub fn system_uses_light_apps() -> Option<bool> {
    shared().cached_light_apps()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command {
    Changed,
    Shutdown,
}

async fn worker_loop(
    _worker: &WorkerCtx,
    receiver: mpsc::Receiver<Command>,
    command_waker: Arc<AtomicWaker>,
    shared: Arc<Shared>,
    address: Option<String>,
) {
    loop {
        let generation = shared.generation.load(Ordering::Acquire);
        if !shared.has_subscribers(generation) {
            let Some(command) = next_command(&receiver, &command_waker).await else {
                return;
            };
            if command == Command::Shutdown {
                return;
            }
            continue;
        }
        match run_source(
            &receiver,
            &command_waker,
            &shared,
            generation,
            address.as_deref(),
        )
        .await
        {
            SourceEnd::Shutdown => return,
            SourceEnd::Restart => continue,
            SourceEnd::Stopped => {
                let Some(command) = next_command(&receiver, &command_waker).await else {
                    return;
                };
                if command == Command::Shutdown {
                    return;
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceEnd {
    Stopped,
    Restart,
    Shutdown,
}

async fn run_source(
    receiver: &mpsc::Receiver<Command>,
    command_waker: &AtomicWaker,
    shared: &Shared,
    generation: u64,
    address: Option<&str>,
) -> SourceEnd {
    if !shared.has_subscribers(generation) {
        return SourceEnd::Stopped;
    }
    let connection = match connect_or_command(receiver, command_waker, address).await {
        Race::Value(Ok(connection)) => connection,
        Race::Value(Err(_)) => {
            shared.publish(generation, None);
            return SourceEnd::Stopped;
        }
        Race::Command(Some(Command::Shutdown)) | Race::Command(None) => return SourceEnd::Shutdown,
        Race::Command(Some(Command::Changed)) => return source_end_for(shared, generation),
    };

    let portal_rule = match portal_signal_rule() {
        Ok(rule) => rule,
        Err(_) => {
            shared.publish(generation, None);
            let _ = connection.close().await;
            return SourceEnd::Stopped;
        }
    };
    let owner_rule = match portal_owner_rule() {
        Ok(rule) => rule,
        Err(_) => {
            shared.publish(generation, None);
            let _ = connection.close().await;
            return SourceEnd::Stopped;
        }
    };
    let portal_stream = match race_command(
        MessageStream::for_match_rule(portal_rule, &connection, Some(SIGNAL_QUEUE_CAPACITY)),
        receiver,
        command_waker,
    )
    .await
    {
        Race::Value(Ok(stream)) => stream,
        Race::Value(Err(_)) => {
            shared.publish(generation, None);
            let _ = connection.close().await;
            return SourceEnd::Stopped;
        }
        Race::Command(Some(Command::Shutdown)) | Race::Command(None) => {
            let _ = connection.close().await;
            return SourceEnd::Shutdown;
        }
        Race::Command(Some(Command::Changed)) => {
            let end = source_end_for(shared, generation);
            let _ = connection.close().await;
            return end;
        }
    };
    let owner_stream = match race_command(
        MessageStream::for_match_rule(owner_rule, &connection, Some(SIGNAL_QUEUE_CAPACITY)),
        receiver,
        command_waker,
    )
    .await
    {
        Race::Value(Ok(stream)) => stream,
        Race::Value(Err(_)) => {
            shared.publish(generation, None);
            drop(portal_stream);
            let _ = connection.close().await;
            return SourceEnd::Stopped;
        }
        Race::Command(Some(Command::Shutdown)) | Race::Command(None) => {
            drop(portal_stream);
            let _ = connection.close().await;
            return SourceEnd::Shutdown;
        }
        Race::Command(Some(Command::Changed)) => {
            let end = source_end_for(shared, generation);
            drop(portal_stream);
            let _ = connection.close().await;
            return end;
        }
    };
    let mut signals = stream::select(portal_stream, owner_stream);

    let mut portal_owner =
        match race_command(get_portal_owner(&connection), receiver, command_waker).await {
            Race::Value(Ok(owner)) => Some(owner),
            Race::Value(Err(_)) => None,
            Race::Command(Some(Command::Shutdown)) | Race::Command(None) => {
                drop(signals);
                let _ = connection.close().await;
                return SourceEnd::Shutdown;
            }
            Race::Command(Some(Command::Changed)) => {
                let end = source_end_for(shared, generation);
                drop(signals);
                let _ = connection.close().await;
                return end;
            }
        };
    let value = if portal_owner.is_some() {
        match race_command(read_color_scheme(&connection), receiver, command_waker).await {
            Race::Value(Ok(value)) => value,
            Race::Value(Err(_)) => None,
            Race::Command(Some(Command::Shutdown)) | Race::Command(None) => {
                drop(signals);
                let _ = connection.close().await;
                return SourceEnd::Shutdown;
            }
            Race::Command(Some(Command::Changed)) => {
                let end = source_end_for(shared, generation);
                drop(signals);
                let _ = connection.close().await;
                return end;
            }
        }
    } else {
        None
    };
    let value = match settle_read(
        value,
        &mut signals,
        &mut portal_owner,
        ReadContext {
            connection: &connection,
            receiver,
            command_waker,
            shared,
            generation,
        },
    )
    .await
    {
        Ok(value) => value,
        Err(SourceEnd::Shutdown) => {
            drop(signals);
            let _ = connection.close().await;
            return SourceEnd::Shutdown;
        }
        Err(end) => {
            drop(signals);
            let _ = connection.close().await;
            return end;
        }
    };
    shared.publish(generation, value);

    loop {
        if shared.generation.load(Ordering::Acquire) != generation {
            let end = source_end_for(shared, generation);
            drop(signals);
            let _ = connection.close().await;
            return end;
        }
        if !shared.has_subscribers(generation) {
            drop(signals);
            let _ = connection.close().await;
            return SourceEnd::Stopped;
        }
        let next_signal = signals.try_next().fuse();
        let next_command = next_command(receiver, command_waker).fuse();
        pin_mut!(next_signal, next_command);
        select_macro! {
            signal = next_signal => match signal {
                Ok(Some(message)) => {
                    if let Some(event) = appearance_signal(&message, portal_owner.as_deref()) {
                        match event {
                            PortalEvent::Setting(value) => {
                                shared.publish(generation, value);
                            }
                            PortalEvent::Owner(owner) => {
                                portal_owner = owner;
                                let value = if portal_owner.is_some() {
                                    match race_command(
                                        read_color_scheme(&connection),
                                        receiver,
                                        command_waker,
                                    ).await {
                                        Race::Value(Ok(value)) => value,
                                        Race::Value(Err(_)) => None,
                                        Race::Command(Some(Command::Shutdown)) | Race::Command(None) => {
                                            drop(signals);
                                            let _ = connection.close().await;
                                            return SourceEnd::Shutdown;
                                        }
                                        Race::Command(Some(Command::Changed)) => {
                                            let end = source_end_for(shared, generation);
                                            drop(signals);
                                            let _ = connection.close().await;
                                            return end;
                                        }
                                    }
                                } else {
                                    None
                                };
                                let value = match settle_read(
                                    value,
                                    &mut signals,
                                    &mut portal_owner,
                                    ReadContext {
                                        connection: &connection,
                                        receiver,
                                        command_waker,
                                        shared,
                                        generation,
                                    },
                                )
                                .await
                                {
                                    Ok(value) => value,
                                    Err(SourceEnd::Shutdown) => {
                                        drop(signals);
                                        let _ = connection.close().await;
                                        return SourceEnd::Shutdown;
                                    }
                                    Err(end) => {
                                        drop(signals);
                                        let _ = connection.close().await;
                                        return end;
                                    }
                                };
                                shared.publish(generation, value);
                            }
                        }
                    }
                }
                Ok(None) | Err(_) => {
                    shared.publish(generation, None);
                    drop(signals);
                    let _ = connection.close().await;
                    return SourceEnd::Stopped;
                }
            },
            command = next_command => match command {
                Some(Command::Shutdown) | None => {
                    drop(signals);
                    let _ = connection.close().await;
                    return SourceEnd::Shutdown;
                }
                Some(Command::Changed) => {
                    if shared.generation.load(Ordering::Acquire) != generation {
                        let end = source_end_for(shared, generation);
                        drop(signals);
                        let _ = connection.close().await;
                        return end;
                    }
                    if !shared.has_subscribers(generation) {
                        drop(signals);
                        let _ = connection.close().await;
                        return SourceEnd::Stopped;
                    }
                }
            }
        }
    }
}

fn source_end_for(shared: &Shared, _generation: u64) -> SourceEnd {
    let current = shared.generation.load(Ordering::Acquire);
    if shared.has_subscribers(current) {
        SourceEnd::Restart
    } else {
        SourceEnd::Stopped
    }
}

struct ReadContext<'a> {
    connection: &'a Connection,
    receiver: &'a mpsc::Receiver<Command>,
    command_waker: &'a AtomicWaker,
    shared: &'a Shared,
    generation: u64,
}

async fn settle_read<S>(
    mut value: Option<bool>,
    signals: &mut S,
    portal_owner: &mut Option<String>,
    context: ReadContext<'_>,
) -> Result<Option<bool>, SourceEnd>
where
    S: Stream<Item = zbus::Result<Message>> + Unpin,
{
    let mut latest_setting = None;
    loop {
        let (signal, owner_changed) = drain_initial_signals(signals, portal_owner);
        if owner_changed.is_some() {
            latest_setting = None;
        }
        if signal.is_some() {
            latest_setting = signal;
        }
        match owner_changed {
            Some(false) => return Ok(None),
            Some(true) => {
                value = match race_command(
                    read_color_scheme(context.connection),
                    context.receiver,
                    context.command_waker,
                )
                .await
                {
                    Race::Value(Ok(value)) => value,
                    Race::Value(Err(_)) => None,
                    Race::Command(Some(Command::Shutdown)) | Race::Command(None) => {
                        return Err(SourceEnd::Shutdown);
                    }
                    Race::Command(Some(Command::Changed)) => {
                        return Err(source_end_for(context.shared, context.generation));
                    }
                };
            }
            None => return Ok(latest_setting.unwrap_or(value)),
        }
    }
}

enum Race<T> {
    Value(T),
    Command(Option<Command>),
}

async fn race_command<F, T>(
    future: F,
    receiver: &mpsc::Receiver<Command>,
    waker: &AtomicWaker,
) -> Race<T>
where
    F: Future<Output = T> + Send,
{
    let future = future.fuse();
    let command = next_command(receiver, waker).fuse();
    pin_mut!(future, command);
    match select(future, command).await {
        futures_util::future::Either::Left((value, _)) => Race::Value(value),
        futures_util::future::Either::Right((command, _)) => Race::Command(command),
    }
}

async fn connect_or_command(
    receiver: &mpsc::Receiver<Command>,
    waker: &AtomicWaker,
    address: Option<&str>,
) -> Race<zbus::Result<Connection>> {
    let connection = async move {
        let builder = match address {
            Some(address) => zbus::connection::Builder::address(address)?,
            None => zbus::connection::Builder::session()?,
        };
        builder.build().await
    };
    race_command(connection, receiver, waker).await
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

fn portal_signal_rule() -> zbus::Result<zbus::OwnedMatchRule> {
    Ok(MatchRule::builder()
        .msg_type(Type::Signal)
        .path(PORTAL_PATH)?
        .interface(PORTAL_INTERFACE)?
        .member("SettingChanged")?
        .add_arg(APPEARANCE_NAMESPACE)?
        .add_arg(COLOR_SCHEME_KEY)?
        .build()
        .into())
}

fn portal_owner_rule() -> zbus::Result<zbus::OwnedMatchRule> {
    Ok(MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(DBUS_SERVICE)?
        .path(DBUS_PATH)?
        .interface(DBUS_INTERFACE)?
        .member("NameOwnerChanged")?
        .add_arg(PORTAL_SERVICE)?
        .build()
        .into())
}

async fn get_portal_owner(connection: &Connection) -> zbus::Result<String> {
    connection
        .call_method(
            Some(DBUS_SERVICE),
            DBUS_PATH,
            Some(DBUS_INTERFACE),
            "GetNameOwner",
            &(PORTAL_SERVICE,),
        )
        .await?
        .body()
        .deserialize()
}

async fn read_color_scheme(connection: &Connection) -> zbus::Result<Option<bool>> {
    let namespaces = vec![APPEARANCE_NAMESPACE.to_owned()];
    let reply = connection
        .call_method(
            Some(PORTAL_SERVICE),
            PORTAL_PATH,
            Some(PORTAL_INTERFACE),
            "ReadAll",
            &namespaces,
        )
        .await?;
    let namespaces: HashMap<String, HashMap<String, OwnedValue>> = reply.body().deserialize()?;
    Ok(light_apps_from_settings(&namespaces))
}

fn light_apps_from_settings(
    namespaces: &HashMap<String, HashMap<String, OwnedValue>>,
) -> Option<bool> {
    namespaces
        .get(APPEARANCE_NAMESPACE)
        .and_then(|values| values.get(COLOR_SCHEME_KEY))
        .and_then(color_scheme_value)
}

fn color_scheme_value(value: &OwnedValue) -> Option<bool> {
    u32::try_from(value)
        .ok()
        .and_then(light_apps_for_portal_value)
}

fn light_apps_for_portal_value(value: u32) -> Option<bool> {
    match value {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

#[derive(Debug, Eq, PartialEq)]
enum PortalEvent {
    Setting(Option<bool>),
    Owner(Option<String>),
}

fn appearance_signal(message: &Message, portal_owner: Option<&str>) -> Option<PortalEvent> {
    let header = message.header();
    let member = header.member()?.as_str();
    match member {
        "SettingChanged" => {
            if header.sender()?.as_str() != portal_owner? {
                return None;
            }
            let (namespace, key, value): (String, String, OwnedValue) =
                message.body().deserialize().ok()?;
            (namespace == APPEARANCE_NAMESPACE && key == COLOR_SCHEME_KEY)
                .then(|| PortalEvent::Setting(color_scheme_value(&value)))
        }
        "NameOwnerChanged" => {
            let (name, _old_owner, new_owner): (String, String, String) =
                message.body().deserialize().ok()?;
            (name == PORTAL_SERVICE).then_some(PortalEvent::Owner(
                (!new_owner.is_empty()).then_some(new_owner),
            ))
        }
        _ => None,
    }
}

fn drain_initial_signals<S>(
    signals: &mut S,
    portal_owner: &mut Option<String>,
) -> (Option<Option<bool>>, Option<bool>)
where
    S: Stream<Item = zbus::Result<Message>> + Unpin,
{
    let mut latest_setting = None;
    let mut latest_owner = None;
    loop {
        let Some(next) = signals.next().now_or_never().flatten() else {
            break;
        };
        let Ok(message) = next else {
            break;
        };
        if let Some(event) = appearance_signal(&message, portal_owner.as_deref()) {
            match event {
                PortalEvent::Setting(value) => latest_setting = Some(value),
                PortalEvent::Owner(owner) => {
                    latest_owner = Some(owner.is_some());
                    *portal_owner = owner;
                    latest_setting = None;
                }
            }
        }
    }
    (latest_setting, latest_owner)
}

fn cache_code(value: Option<bool>) -> u64 {
    match value {
        None => CACHE_NO_PREFERENCE,
        Some(false) => CACHE_DARK,
        Some(true) => CACHE_LIGHT,
    }
}

const fn pack_cache(generation: u64, value: u64) -> u64 {
    ((generation & CACHE_GENERATION_MASK) << 2) | (value & CACHE_VALUE_MASK)
}

const fn unpack_generation(cache: u64) -> u64 {
    cache >> 2
}

const fn cache_value_code(cache: u64) -> u64 {
    cache & CACHE_VALUE_MASK
}

#[cfg(test)]
mod tests {
    use super::{
        APPEARANCE_NAMESPACE, COLOR_SCHEME_KEY, Message, MessageStream, NativeWindow,
        PORTAL_INTERFACE, PORTAL_PATH, PORTAL_SERVICE, PortalEvent, SystemNews,
        SystemSettingsWatch,
        appearance_signal, light_apps_for_portal_value, light_apps_from_settings, lock,
        portal_signal_rule, shutdown_system_settings, system_uses_light_apps,
    };
    use futures_util::TryStreamExt;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};
    use std::num::NonZeroU32;
    use std::process::{Child, Command as ProcessCommand, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, OnceLock, mpsc};

    struct PrivateBus {
        child: Child,
        address: String,
    }

    impl PrivateBus {
        fn start() -> Self {
            let mut child = ProcessCommand::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address=1", "--nopidfile"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("start the controlled private D-Bus helper");
            let stdout = child.stdout.take().expect("the bus address pipe");
            let mut address = String::new();
            if let Err(error) = BufReader::new(stdout).read_line(&mut address) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("could not read the private bus address: {error}");
            }
            if address.trim().is_empty() {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the private bus did not print a non-empty address");
            }
            let address = address.trim().to_owned();
            Self { child, address }
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    struct FakeSettings {
        scheme: Arc<AtomicU64>,
        read_gate: Mutex<Option<ReadGate>>,
    }

    struct ReadGate {
        started: mpsc::Sender<()>,
        release: tokio::sync::oneshot::Receiver<()>,
    }

    struct ReleaseRead(Option<tokio::sync::oneshot::Sender<()>>);

    impl ReleaseRead {
        fn release(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    impl Drop for ReleaseRead {
        fn drop(&mut self) {
            self.release();
        }
    }

    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl FakeSettings {
        async fn read_all(
            &self,
            namespaces: Vec<String>,
        ) -> HashMap<String, HashMap<String, zbus::zvariant::OwnedValue>> {
            if !namespaces
                .iter()
                .any(|namespace| namespace == APPEARANCE_NAMESPACE)
            {
                return HashMap::new();
            }
            let scheme = self.scheme.load(Ordering::Acquire) as u32;
            let read_gate = { lock(&self.read_gate).take() };
            if let Some(gate) = read_gate {
                let _ = gate.started.send(());
                let _ = gate.release.await;
            }
            HashMap::from([(
                APPEARANCE_NAMESPACE.to_owned(),
                HashMap::from([(
                    COLOR_SCHEME_KEY.to_owned(),
                    zbus::zvariant::OwnedValue::from(scheme),
                )]),
            )])
        }
    }

    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("build the controlled D-Bus test runtime")
    }

    fn start_fake_settings(
        runtime: &tokio::runtime::Runtime,
        address: &str,
        scheme: Arc<AtomicU64>,
        read_gate: Option<ReadGate>,
    ) -> zbus::Connection {
        runtime.block_on(async {
            zbus::connection::Builder::address(address)
                .expect("the private bus address is valid")
                .name(PORTAL_SERVICE)
                .expect("the fake portal name is valid")
                .serve_at(
                    PORTAL_PATH,
                    FakeSettings {
                        scheme: Arc::clone(&scheme),
                        read_gate: Mutex::new(read_gate),
                    },
                )
                .expect("register the fake portal settings object")
                .build()
                .await
                .expect("connect the controlled fake portal")
        })
    }

    fn change_scheme(runtime: &tokio::runtime::Runtime, service: &zbus::Connection, value: u32) {
        runtime
            .block_on(service.emit_signal(
                None::<&str>,
                PORTAL_PATH,
                PORTAL_INTERFACE,
                "SettingChanged",
                &(
                    APPEARANCE_NAMESPACE,
                    COLOR_SCHEME_KEY,
                    zbus::zvariant::OwnedValue::from(value),
                ),
            ))
            .expect("emit a controlled portal SettingChanged signal");
    }

    fn watch_one_portal_signal(
        runtime: &tokio::runtime::Runtime,
        address: &str,
    ) -> (zbus::Connection, MessageStream) {
        let connection = runtime
            .block_on(
                zbus::connection::Builder::address(address)
                    .expect("the private bus address is valid")
                    .build(),
            )
            .expect("connect the controlled signal probe");
        let stream = runtime
            .block_on(MessageStream::for_match_rule(
                portal_signal_rule().expect("the portal signal rule is valid"),
                &connection,
                Some(1),
            ))
            .expect("subscribe the controlled signal probe");
        (connection, stream)
    }

    fn wait_for_wake(receiver: &mpsc::Receiver<SystemNews>, what: &str) {
        let news = receiver
            .recv()
            .unwrap_or_else(|error| panic!("{what} was not delivered: {error}"));
        assert_eq!(news, SystemNews::Preferences, "{what} must wake for preferences");
    }

    fn reap_provider() -> Result<(), String> {
        let worker = crate::spawn_at_priority(
            "bt-settings-test-retire",
            crate::ThreadPriority::Normal,
            shutdown_system_settings,
        )
        .expect("start the controlled worker-only settings reaper");
        worker
            .join()
            .expect("the worker-only reaper joined the portal worker")
    }

    fn native_window() -> NativeWindow {
        NativeWindow::from_x11(NonZeroU32::new(17).expect("nonzero test window id"))
    }

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn a_private_portal_reads_changes_restarts_and_reaps_its_one_worker() {
        let _serial = test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let runtime = test_runtime();
        let bus = PrivateBus::start();
        let scheme = Arc::new(AtomicU64::new(2));
        let (wake_first, first_wakes) = mpsc::channel();
        let first = SystemSettingsWatch::install_at_address(
            native_window(),
            Box::new(move |news| {
                let _ = wake_first.send(news);
            }),
            bus.address.clone(),
        )
        .expect("subscribe the first window to the shared portal worker");
        wait_for_wake(&first_wakes, "the unsupported initial portal reading");
        assert_eq!(system_uses_light_apps(), None);

        let (read_started, read_started_rx) = mpsc::channel();
        let (release_read_sender, release_read_rx) = tokio::sync::oneshot::channel();
        let portal = start_fake_settings(
            &runtime,
            &bus.address,
            Arc::clone(&scheme),
            Some(ReadGate {
                started: read_started,
                release: release_read_rx,
            }),
        );
        let mut release_read = ReleaseRead(Some(release_read_sender));
        read_started_rx
            .recv()
            .expect("portal appearance is noticed after the initial unsupported answer");

        let (wake_second_window, second_window_wakes) = mpsc::channel();
        let second_window = SystemSettingsWatch::install_at_address(
            native_window(),
            Box::new(move |news| {
                let _ = wake_second_window.send(news);
            }),
            bus.address.clone(),
        )
        .expect("a second window subscribes to the same process source");
        let (_signal_probe_connection, mut signal_probe) =
            watch_one_portal_signal(&runtime, &bus.address);

        scheme.store(1, Ordering::Release);
        change_scheme(&runtime, &portal, 1);
        let observed_signal = runtime
            .block_on(signal_probe.try_next())
            .expect("the portal signal stream remained connected");
        let observed_signal =
            observed_signal.expect("the portal emitted a matching setting signal");
        let owner = portal
            .unique_name()
            .expect("the fake portal has a unique bus owner")
            .as_str();
        assert_eq!(
            appearance_signal(&observed_signal, Some(owner)),
            Some(PortalEvent::Setting(Some(false)))
        );
        release_read.release();
        wait_for_wake(&first_wakes, "the portal color-scheme change");
        wait_for_wake(&second_window_wakes, "the second window's portal wake");
        assert_eq!(system_uses_light_apps(), Some(false));

        scheme.store(2, Ordering::Release);
        change_scheme(&runtime, &portal, 2);
        wait_for_wake(&first_wakes, "the next portal color-scheme change");
        wait_for_wake(&second_window_wakes, "the second window's next portal wake");
        assert_eq!(system_uses_light_apps(), Some(true));

        drop(first);
        assert_eq!(
            system_uses_light_apps(),
            Some(true),
            "one remaining window keeps the process fact subscribed"
        );
        drop(second_window);
        assert_eq!(system_uses_light_apps(), None, "last drop clears the cache");

        let (wake_second, second_wakes) = mpsc::channel();
        let second = SystemSettingsWatch::install_at_address(
            native_window(),
            Box::new(move |news| {
                let _ = wake_second.send(news);
            }),
            bus.address.clone(),
        )
        .expect("a later window can restart the single process source");
        wait_for_wake(&second_wakes, "the restarted portal reading");
        assert_eq!(system_uses_light_apps(), Some(true));

        drop(second);
        reap_provider().expect("shutdown reaps the process-scoped portal worker");
        assert_eq!(system_uses_light_apps(), None);
        {
            let _entered = runtime.enter();
            drop(signal_probe);
            drop(_signal_probe_connection);
            drop(portal);
        }
        drop(bus);
        drop(runtime);
    }

    #[test]
    fn dropping_the_last_watch_does_not_wait_for_a_portal_reply() {
        let _serial = test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let runtime = test_runtime();
        let bus = PrivateBus::start();
        let (wake, wakes) = mpsc::channel();
        let watch = SystemSettingsWatch::install_at_address(
            native_window(),
            Box::new(move |news| {
                let _ = wake.send(news);
            }),
            bus.address.clone(),
        )
        .expect("subscribe the test window to the process source");
        wait_for_wake(&wakes, "the initial unsupported portal reading");

        let (read_started, read_started_rx) = mpsc::channel();
        let (release_read_sender, release_read_rx) = tokio::sync::oneshot::channel();
        let portal = start_fake_settings(
            &runtime,
            &bus.address,
            Arc::new(AtomicU64::new(2)),
            Some(ReadGate {
                started: read_started,
                release: release_read_rx,
            }),
        );
        let mut release_read = ReleaseRead(Some(release_read_sender));
        read_started_rx
            .recv()
            .expect("the controlled portal enters its blocked read");

        drop(watch);
        assert_eq!(
            system_uses_light_apps(),
            None,
            "the last window clears the cache"
        );
        release_read.release();
        reap_provider().expect("shutdown reaps the cancelled provider worker");

        {
            let _entered = runtime.enter();
            drop(portal);
        }
        drop(bus);
        drop(runtime);
    }

    #[test]
    fn portal_color_scheme_values_map_only_the_standardized_answers() {
        assert_eq!(light_apps_for_portal_value(0), None);
        assert_eq!(light_apps_for_portal_value(1), Some(false));
        assert_eq!(light_apps_for_portal_value(2), Some(true));
        assert_eq!(light_apps_for_portal_value(3), None);
    }

    #[test]
    fn unsupported_namespaces_missing_keys_and_wrong_types_have_no_answer() {
        let empty = HashMap::new();
        assert_eq!(light_apps_from_settings(&empty), None);

        let namespace_without_key =
            HashMap::from([(APPEARANCE_NAMESPACE.to_owned(), HashMap::new())]);
        assert_eq!(light_apps_from_settings(&namespace_without_key), None);

        let wrong_type = HashMap::from([(
            APPEARANCE_NAMESPACE.to_owned(),
            HashMap::from([(
                COLOR_SCHEME_KEY.to_owned(),
                zbus::zvariant::OwnedValue::from(true),
            )]),
        )]);
        assert_eq!(light_apps_from_settings(&wrong_type), None);
    }

    #[test]
    fn a_portal_setting_changed_signal_decodes_its_variant_payload() {
        let message = Message::signal(PORTAL_PATH, PORTAL_INTERFACE, "SettingChanged")
            .expect("signal header")
            .sender(":1.42")
            .expect("unique sender")
            .build(&(
                APPEARANCE_NAMESPACE,
                COLOR_SCHEME_KEY,
                zbus::zvariant::OwnedValue::from(2_u32),
            ))
            .expect("signal body");
        assert_eq!(
            super::appearance_signal(&message, Some(":1.42")),
            Some(super::PortalEvent::Setting(Some(true)))
        );
    }
}
