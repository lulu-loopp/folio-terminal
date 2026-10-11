# Linux XDG directories

## Authority

Issue 14 requires explicit XDG configuration, data, cache, and runtime roots. The [XDG Base Directory Specification 0.8](https://specifications.freedesktop.org/basedir/0.8/) defines the four environment variables, their home-directory defaults, and that configured roots are absolute paths. Linux support must follow those roots for real consumers while preserving the paths that identify Folio's existing data writer and inter-process endpoints.

## Consumers in this tree and the Linux web worker

`bt-app::persist::storage_location(OtherUnix)` already places Folio's common store under `$XDG_DATA_HOME/Folio`, falling back to `~/.local/share/Folio`. `storage_dir()` is used by more than two dozen consumers. It contains:

- `session.json`, session crash sentinel, diagnostics, pins, and custom schemes;
- generated shell scripts and shell integration records;
- agent-hook installation/recovery records;
- the data-directory writer claim input, launch socket input, and attention socket input;
- updater/recovery and uninstall inventory inputs.

`settings.json` and `keybindings.json` are configuration files but currently sit beside that data. `SessionStore` and both settings stores derive their writer decision from the directory containing the file. `storage_watch` also watches the common directory. Moving those two documents therefore changes active paths, writer claims, external-edit observation, and uninstall ownership. It cannot be implemented by changing one filename helper.

The Linux Chromium web host adds three genuine consumers, as coordinated with the web owner:

| Root | Consumer | Planned location |
|---|---|---|
| Config | app preferences and keybindings | `$XDG_CONFIG_HOME/Folio/<data-directory-tag>/` |
| Data | Folio session, pins, schemes, shell/agent records, and browser profile | `$XDG_DATA_HOME/Folio/`, including `Chromium/` for browser user data |
| Cache | Chromium disk cache | `$XDG_CACHE_HOME/Folio/<data-directory-tag>/Chromium/` |
| Runtime | Chromium child temporary files | child-only `TMPDIR=$XDG_RUNTIME_DIR/Folio/`, after validation and private directory creation |

Unset config, data, and cache variables use `$HOME/.config`, `$HOME/.local/share`, and `$HOME/.cache`, respectively. The current data resolver remains the authority for the data root so existing Linux files and the data claim keep their spelling. The XDG runtime path is only for the Chromium child. It does not name Folio's data claim, launch socket, or attention socket.

New config/cache/runtime resolution ignores empty or relative XDG values as the specification requires and keeps paths as `OsString`/`PathBuf` bytes. If neither an absolute `XDG_CONFIG_HOME` nor an absolute `HOME` can name config, settings keep the existing data-root path instead of guessing a new root. `storage_location(OtherUnix)` currently accepts a relative or empty `XDG_DATA_HOME`, interpreting it relative to the launch directory. Changing that existing data rule can strand files from a previous launch whose working directory is not recoverable. Keep the current data resolver until a migration for that legacy case is designed; do not silently switch roots as part of settings migration.

The existing launch and attention sockets plus the Unix data claim live under the stable per-user runtime directory returned by `bt-platform::instance`. Keep those objects there: their identity must not depend on a process environment override that two Folio launches can disagree about. A private Chromium `TMPDIR` is a separate runtime consumer and does not take ownership of or change those kernel objects. If `XDG_RUNTIME_DIR` is unset, invalid, or unusable, create a private Chromium child directory under the existing system-derived per-user temp directory.

## Settings migration and writer ownership

The active Linux locations of `settings.json` and `keybindings.json` change from the data root to the config root namespaced by the existing data-directory identity. Reuse `bt-platform::instance::directory_tag(data_root)`; do not add a new hash or a second lock. Equal data-directory identities therefore reach one config directory and remain protected by the existing data claim, while distinct data roots retain independent settings. The migration must have these rules before implementation:

1. A document already in the config namespace is authoritative. Never overwrite it with a data-root copy.
2. If only the existing data-root document is present, copy its original bytes into the config root atomically. Parsing and reserializing during migration could discard unknown fields or turn a damaged document into defaults.
3. `SettingsStore::open` and `KeybindingsStore::open` read the new config path first and fall back to the existing data-root file only when the config file is missing. A present but unreadable config file is authoritative and uses the existing damaged-file path. If migration fails, the original remains readable through that fallback; later user changes still write to the new config path through the existing synchronous store path and report any write error there.
4. Preserve the original document under the existing data root as a recovery copy. If the active config file is missing later, the normal legacy fallback can recover those bytes.
5. Defer migration and config writes in an update trial, following `update_trial::Writer::Settings`.
6. Keep `storage_dir()` and its current data-directory claim unchanged. The config directory is namespaced by the exact existing `directory_tag(data_root)`, so one existing data claim continues to be the sole writer decision for that data namespace. `claim_name(data_root)`, `launch_socket_path(data_root)`, and `attention_socket_path(data_root)` retain their existing inputs. The new config/cache namespaces add no lock, alter no kernel identity, and do not merge separate data namespaces.
7. Keep `StorageWatch` on the data root for `profiles.json`, `pins.json`, session, and other data consumers. Settings and keybindings have not been hot-reloaded from hand edits; keep that behavior. They read the config path on the next launch, with a legacy fallback only when the config file is missing.

The migration entry is `start_linux_config_migration()`. It starts a one-shot worker registered in the existing Linux desktop-helper registry; the existing `DesktopRetire` shutdown drains and joins that registry off the window thread. The worker resolves the data identity tag, then copies missing legacy documents into the new config namespace. It does not create the namespace: `SettingsStore::open` and `KeybindingsStore::open` already call `make_data_folder(dir)`, so their existing synchronous write and error-reporting behavior can stay unchanged. The migration uses the readers' 16 MiB document limit; an oversized legacy file remains untouched and continues through the legacy read fallback. A trial defers migration. After a trial releases the existing `DataFolder` gate and creates the config namespace, its release path starts migration again without waiting for completion.

Migration is per document, create-new, and recoverable. A worker first checks the config target. If present, it is authoritative and the data-root source remains untouched. If absent, the worker copies the source's exact bytes to a temporary file under the config namespace, syncs it, and installs it without replacing a target created by a user edit or another process. The original source remains as a recovery copy. If the copy fails, the source remains active and the next launch retries. An invalid JSON source is copied byte-for-byte and reported by the existing settings/keybindings reader at the new active path.

The stores read the config target first and fall back to the legacy data file only when the target is missing. A present but unreadable config target is authoritative and uses the existing damaged-file path. Their `path` fields always name the config target, so edits write only to the new namespace. The existing `make_data_folder` calls create that directory and the current synchronous write path preserves its error feedback. The migration starts after both stores open. It does not change the values already loaded by the current run; its copy has the same bytes the fallback reader just used. These rules preserve existing data and Windows/macOS behavior. The config root must be retained by the user-data cleanup flow.

## Web path interface

The app owns the existing data-root decision; the platform owns Linux runtime-directory validation. Add a Linux-only path seam and a worker preparation door:

```rust
pub struct LinuxWebDirs {
    pub profile: PathBuf,
    pub cache: PathBuf,
    pub runtime: PathBuf,
}

pub fn linux_web_profile(data_root: &Path) -> PathBuf;
pub fn prepare_linux_web_dirs(worker: &WorkerCtx, data_root: &Path)
    -> Result<LinuxWebDirs, String>;
```

`linux_web_profile` is pure and performs no filesystem access; it returns `data_root/Chromium`, which is the path seam used by the app's WebHost request. `prepare_linux_web_dirs` runs inside the Chromium worker. Its private resolver accepts the data root, home, `XDG_CACHE_HOME`, and `XDG_RUNTIME_DIR` values. Filesystem preparation validates the XDG runtime parent as absolute, owned by the current user, and mode `0700`; it creates and verifies the `Folio` child with mode `0700`. If the XDG runtime parent is missing or invalid, use the existing system-derived per-user temp directory and a private `chromium` child. Neither route changes `instance::runtime_directory()` or the existing data claim and socket names.

The web owner keeps `WebHost::request_environment(folder, generation)` as the existing browser-start door. `bt-app::webhost::user_data_folder` resolves the existing data root through `persist::storage_directory_in(platform, env)` and passes `linux_web_profile(data_root)` without filesystem access. That thin storage resolver preserves `storage_location`'s exact XDG_DATA_HOME behavior, including its relative/empty legacy cases; WebHost does not reimplement it. The Linux browser actor calls `prepare_linux_web_dirs` from its worker, verifies the passed folder equals `dirs.profile`, then launches Chromium with `--user-data-dir=profile`, `--disk-cache-dir=cache`, and child-only `TMPDIR=runtime`; Folio's process environment is untouched. The helper uses `directory_tag(data_root)` for the data-independent cache namespace, and its `profile` path remains the direct data-root child. The headless-shell smoke already used separate user-data, disk-cache, and runtime temporary roots this way. The Chromium temporary-directory source audit remains part of runtime-row verification.

## Validation before issue checkboxes move

- Resolver tests pin custom roots, config/cache home-directory defaults, empty/relative config/cache/runtime values, path-byte preservation, and the unchanged existing data path.
- Migration tests pin config-wins, read-new/fallback-old, byte-preserving no-clobber copy, failure fallback, update-trial deferral, and data-tag separation. Include two distinct data roots sharing one XDG config base: the existing tag gives them distinct namespaces and each keeps the existing one-writer behavior.
- Web-host tests inspect the actual Chromium argv and child-only `TMPDIR` received by a private fake executable or process seam.
- Runtime-directory tests refuse links, wrong owners, and unsafe modes; the fallback is private and per-user.
- Existing `instance` tests continue to pin the same data claim and both socket paths. Windows and macOS persistence tests keep their exact paths.
- Uninstall tests prove config and data roots are discoverable and only the current data-tagged config/cache roots are removed under explicit purge; ordinary cleanup preserves them.

The cache and runtime rows remain unchecked until the headless-Chromium actor receives and uses the returned paths in its real child process. The CFT smoke used separate profile, disk-cache, and temporary roots; it observed network cache entries under the cache root and persistent browser state under the profile. It did not observe a singleton socket or another file under the temporary root, so runtime is described as Chromium's child temporary-file root, not as a singleton endpoint. No cache or runtime directory is created without that consumer.
