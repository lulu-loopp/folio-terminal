//! The XDG directories a Linux headless Chromium child owns.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::admission::WorkerCtx;

/// The three roots the browser child receives: a persistent profile, disposable
/// disk cache, and per-user temporary root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxWebDirs {
    pub profile: PathBuf,
    pub cache: PathBuf,
    pub runtime: PathBuf,
}

/// The browser profile under Folio's existing data namespace.
///
/// Pure path construction for the app's WebHost request. The browser worker
/// creates it when it starts Chromium.
#[must_use]
pub fn linux_web_profile(data_root: &Path) -> PathBuf {
    data_root.join("Chromium")
}

/// Resolve and prepare the paths for one Linux Chromium process.
///
/// Called by the browser worker, never by the window thread. The cache is
/// namespaced with the data directory's existing kernel-claim tag. Runtime
/// temporary files use the validated XDG runtime directory when available and
/// the system-derived per-user temporary directory otherwise.
pub fn prepare_linux_web_dirs(
    _worker: &WorkerCtx,
    data_root: &Path,
) -> Result<LinuxWebDirs, String> {
    let cache_home =
        cache_home_from(|name: &str| std::env::var_os(name)).map_err(|error| error.to_string())?;
    let namespace = crate::instance::directory_tag(data_root);
    let profile = linux_web_profile(data_root);
    let cache = prepare_cache_directory(_worker, &cache_home, &namespace)?;
    let runtime =
        crate::instance::prepare_chromium_temporary_directory(_worker).map_err(|error| {
            format!("could not prepare the Linux Chromium runtime directory: {error}")
        })?;
    Ok(LinuxWebDirs {
        profile,
        cache,
        runtime,
    })
}

fn cache_home_from(env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf, String> {
    if let Some(explicit) = env("XDG_CACHE_HOME") {
        let path = PathBuf::from(explicit);
        if !path.as_os_str().is_empty() && path.is_absolute() {
            return Ok(path);
        }
    }
    let home = env("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty() && path.is_absolute())
        .ok_or_else(|| "Linux Chromium cache has no absolute XDG_CACHE_HOME or HOME".to_owned())?;
    Ok(home.join(".cache"))
}

fn cache_directory(cache_home: &Path, namespace: &str) -> PathBuf {
    cache_home.join("Folio").join(namespace).join("Chromium")
}

fn prepare_cache_directory(
    _worker: &WorkerCtx,
    cache_home: &Path,
    namespace: &str,
) -> Result<PathBuf, String> {
    let cache = cache_directory(cache_home, namespace);
    std::fs::create_dir_all(&cache)
        .map_err(|error| format!("could not prepare the Linux Chromium cache: {error}"))?;
    Ok(cache)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;

    use super::{cache_directory, cache_home_from, linux_web_profile, prepare_cache_directory};
    use crate::admission::WorkerCtx;

    fn on_worker<T: Send + 'static>(work: impl FnOnce(&WorkerCtx) -> T + Send + 'static) -> T {
        let (answer, wait) = mpsc::channel();
        crate::spawn_at_priority(
            "bt-linux-web-dir-test",
            crate::ThreadPriority::BelowNormal,
            move |worker| {
                let _ = answer.send(work(worker));
            },
        )
        .expect("start the controlled directory worker");
        wait.recv()
            .expect("the directory worker returns its answer")
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(*value)))
            .collect::<std::collections::HashMap<_, _>>();
        move |name| pairs.get(name).cloned()
    }

    #[test]
    fn profile_is_a_pure_data_root_child() {
        let data = Path::new("/home/dev/.local/share/Folio");
        assert_eq!(linux_web_profile(data), data.join("Chromium"));
    }

    #[test]
    fn cache_home_uses_absolute_override_or_the_xdg_default() {
        assert_eq!(
            cache_home_from(env(&[
                ("XDG_CACHE_HOME", "/home/dev/cache-here"),
                ("HOME", "/home/dev"),
            ]))
            .expect("the absolute override is usable"),
            PathBuf::from("/home/dev/cache-here")
        );
        for invalid in ["", "relative/cache"] {
            assert_eq!(
                cache_home_from(env(&[("XDG_CACHE_HOME", invalid), ("HOME", "/home/dev")]))
                    .expect("an invalid XDG value falls back to HOME"),
                PathBuf::from("/home/dev/.cache")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn cache_paths_preserve_non_utf8_home_bytes_and_use_the_claim_tag() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let home = OsString::from_vec(b"/home/dev/\xffcache".to_vec());
        let cache_home = cache_home_from(|name| match name {
            "XDG_CACHE_HOME" => None,
            "HOME" => Some(home.clone()),
            _ => None,
        })
        .expect("the HOME path bytes are retained");
        assert_eq!(
            cache_home.as_os_str().as_bytes(),
            b"/home/dev/\xffcache/.cache"
        );
        let data_root = Path::new("/home/dev/.local/share/Folio");
        let tag = crate::instance::directory_tag(data_root);
        assert_eq!(
            cache_directory(&cache_home, &tag),
            cache_home.join("Folio").join(tag).join("Chromium")
        );
    }

    #[test]
    fn the_cache_directory_is_created_on_the_worker_under_its_data_tag() {
        let root = bt_testpath::temp_path("folio-linux-web-dir");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("create the test root");
        let cache_home = root.join("cache");
        let worker_cache = cache_home.clone();
        let cache = on_worker(move |worker| {
            prepare_cache_directory(worker, &worker_cache, "same-data-namespace")
                .expect("prepare the cache on its worker")
        });
        assert_eq!(
            cache,
            cache_home
                .join("Folio")
                .join("same-data-namespace")
                .join("Chromium")
        );
        assert!(cache.is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cache_without_an_absolute_home_has_no_guessed_root() {
        assert!(
            cache_home_from(env(&[("XDG_CACHE_HOME", "relative"), ("HOME", "relative")])).is_err()
        );
    }
}
