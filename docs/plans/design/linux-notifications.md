# Linux desktop notifications

## Authority and existing contract

Issue 14 requires notification delivery, asynchronous failure reporting, and supported click behavior. `bt-app::notify::NotificationRoute` owns the click payload format (`w=<window>&t=<tab>&s=<seat>`) and its strict parser. `NotificationDesk` owns lazy construction, one-time refusal reporting, and delivery of accepted routes to `Runtime::open_from_notification`. The Linux adapter must preserve those interfaces and pass the original launch string back to the parser.

The desktop contract is the freedesktop Notifications D-Bus protocol. Clients query `GetCapabilities`; `actions` is the capability that permits an action request. The standard `default` action is the ordinary notification click, and `ActionInvoked(id, action_key)` returns the notification id and invoked key. Servers may omit click signals, so a notification is clickable only where the service advertises and emits the supported action ([protocol](https://specifications.freedesktop.org/notification/1.2/protocol.html), [default action](https://specifications.freedesktop.org/notification/latest/basic-design.html)).

## Interface

`bt_platform::Notifier` keeps its existing `new`, `without_registration`, `show`, `take_activations`, and `take_failures` interface. Linux creates no D-Bus connection at construction. The first `show` lazily starts one worker for that `Notifier`; later calls queue requests through its private command door. `show` returns after queueing, while connection, capability, and `Notify` failures take the existing one-time failure-and-wake path.

When `GetCapabilities` includes `actions`, each `Notify` request carries the `default` action labelled `Open`. On a click, the worker returns the request's unchanged launch string only when all of these hold:

- the signal sender is the notification service's current unique D-Bus owner;
- the notification id came from a successful `Notify` request owned by this worker;
- the action key is exactly `default`;
- that notification has not already activated.

`NotificationClosed` retires its id. `NameOwnerChanged` retires every id from the previous service owner. Unknown ids, unknown action keys, malformed signals, and signals from a former owner produce no activation. The existing `NotificationRoute::parse` remains the final validation before any pane is activated.

If the service does not advertise `actions`, Folio sends an ordinary notification without actions and does not retain a click route. The Linux backend does not promise clicks on servers that cannot report them.

## Ownership and shutdown

The worker is process-owned and registered in `linux_notifications`, rather than in the generic short-lived helper list. Dropping `Notifier` sets `closing`, sends `Shutdown`, and wakes the command future. Every connection, match-rule, capability, owner, and `Notify` future races that same cancellation door; queued `Show` commands are preserved while setup calls are in flight. Shutdown therefore drops an unanswered D-Bus future instead of carrying the library's default method timeout into process retirement.

`bt_platform::shutdown_notifications(&WorkerCtx)` sends shutdown to all registered workers before joining them. The application calls it from its shutdown worker after dropping notification owners and before leaving Linux process services. The window thread only drops the owner and sends the wakeable cancellation request.

## Verification

The Linux unit tests start `dbus-daemon` with a test-owned configuration whose only service directory is a private empty directory and which has no standard service directories or system includes. They exercise the real zbus adapter against a fake `org.freedesktop.Notifications` service: exact `Notify` fields, click from an active id, rejection after close, rejection of foreign ids/actions, a server without action support, missing-service `ServiceUnknown`, a D-Bus method error, and shutdown while `GetCapabilities` is held open. No user session bus or desktop notification service is used.
