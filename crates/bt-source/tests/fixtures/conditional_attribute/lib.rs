// `#[cfg_attr(…, path = …)]` names two different files and lets a predicate
// choose, and this reading is `cfg`-blind on purpose (§2.4): there is no answer
// to give, so the declaration is refused rather than resolved by its default
// spelling.
#[cfg_attr(windows, path = "on_windows.rs")]
mod platform;
