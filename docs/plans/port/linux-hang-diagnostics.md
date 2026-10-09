# Linux hang diagnostics

Linux reports use the shared watchdog's phases, deadlines, startup grace and recovery records. Thread IDs are kernel TIDs, matching `/proc/<pid>/task/<tid>`.

When the watchdog suspects a stall, it posts a numeric question through winit's event-loop proxy. The user-event handler acknowledges it before application work. The existing one-second response budget covers enqueue and reply. One unanswered question can remain queued; a timeout does not append more questions behind it. Late replies retire their own question. Stopping the event loop unregisters the proxy and releases an active wait.

This checks whether Folio's winit handler runs. Windows checks native window messages; macOS checks a Core Foundation block in the main run loop's common modes, including native modal loops. Linux does not claim that independent native-loop measurement. Linux's interactive resize hands control to the window manager or compositor rather than running a USER32/AppKit modal loop inside Folio.

With `BT_PERF_TRACE` enabled, the watchdog also asks once when the event loop becomes available, so tracing proves the delivery path even while caret frames keep the loop active. `BT_HANG_PROBE dispatched=<id>` records that the real handler received a question. The private X11 and Wayland smoke tests require that record alongside presented PTY output and refuse a hang report for that responsive run. Unit tests cover response, timeout, late replies, send failure and shutdown.

## Crash and stack reports

The shared Rust panic hook records Rust panics. It does not catch native signals such as `SIGSEGV`. Linux stack capture remains unavailable in the application, matching macOS's lack of an in-process hang stack sampler.

On a system configured to collect cores through systemd-coredump, use `coredumpctl info folio` to locate a crash and `coredumpctl debug folio` to open its core in a debugger. Use the binary and debug information from the exact build named in the report. A core is a crashed process's snapshot; it does not supply a stack for a process that is still hung.
