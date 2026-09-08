#[cfg(unix)]
use anyhow::Context;
#[cfg(feature = "serde_support")]
use serde_derive::*;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(unix)]
use std::path::Component;
use std::path::Path;

/// Used to deal with Windows having case-insensitive environment variables.
#[derive(Clone, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde_support", derive(Serialize, Deserialize))]
struct EnvEntry {
    /// Whether or not this environment variable came from the base environment,
    /// as opposed to having been explicitly set by the caller.
    is_from_base_env: bool,

    /// For case-insensitive platforms, the environment variable key in its preferred casing.
    preferred_key: OsString,

    /// The environment variable value.
    value: OsString,
}

impl EnvEntry {
    fn map_key(k: OsString) -> OsString {
        if cfg!(windows) {
            // Best-effort lowercase transformation of an os string
            match k.to_str() {
                Some(s) => s.to_lowercase().into(),
                None => k,
            }
        } else {
            k
        }
    }
}

#[cfg(unix)]
fn get_shell() -> String {
    use nix::unistd::{access, AccessFlags};
    use std::ffi::CStr;
    use std::str;

    let ent = unsafe { libc::getpwuid(libc::getuid()) };
    if !ent.is_null() {
        let shell = unsafe { CStr::from_ptr((*ent).pw_shell) };
        match shell.to_str().map(str::to_owned) {
            Err(err) => {
                log::warn!(
                    "passwd database shell could not be \
                     represented as utf-8: {err:#}, \
                     falling back to /bin/sh"
                );
            }
            Ok(shell) => {
                if let Err(err) = access(Path::new(&shell), AccessFlags::X_OK) {
                    log::warn!(
                        "passwd database shell={shell:?} which is \
                         not executable ({err:#}), falling back to /bin/sh"
                    );
                } else {
                    return shell;
                }
            }
        }
    }
    "/bin/sh".into()
}

fn get_base_env() -> BTreeMap<OsString, EnvEntry> {
    let mut env: BTreeMap<OsString, EnvEntry> = std::env::vars_os()
        .map(|(key, value)| {
            (
                EnvEntry::map_key(key.clone()),
                EnvEntry {
                    is_from_base_env: true,
                    preferred_key: key,
                    value,
                },
            )
        })
        .collect();

    #[cfg(unix)]
    {
        let key = EnvEntry::map_key("SHELL".into());
        // Only set the value of SHELL if it isn't already set
        if !env.contains_key(&key) {
            env.insert(
                EnvEntry::map_key("SHELL".into()),
                EnvEntry {
                    is_from_base_env: true,
                    preferred_key: "SHELL".into(),
                    value: get_shell().into(),
                },
            );
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        use winapi::um::processenv::ExpandEnvironmentStringsW;
        use winreg::enums::{RegType, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::types::FromRegValue;
        use winreg::{RegKey, RegValue};

        fn reg_value_to_string(value: &RegValue) -> anyhow::Result<OsString> {
            match value.vtype {
                RegType::REG_EXPAND_SZ => {
                    // **Terminated here and nowhere else** (review row R2-2).
                    // `ExpandEnvironmentStringsW` reads its source until a NUL,
                    // and what the registry crate hands back is the value's
                    // bytes exactly as `RegEnumValueW` reported them — with no
                    // terminator added and none guaranteed by the registry
                    // itself, whose own documentation warns that a string value
                    // "may not have been stored with the proper terminating null
                    // characters". Handing that buffer straight to Win32 reads
                    // past the allocation on every pane spawn.
                    let src = wide_terminated(&value.bytes);
                    let size =
                        unsafe { ExpandEnvironmentStringsW(src.as_ptr(), std::ptr::null_mut(), 0) };
                    if size == 0 {
                        anyhow::bail!("ExpandEnvironmentStringsW could not size the value");
                    }
                    let mut buf = vec![0u16; size as usize + 1];
                    let written = unsafe {
                        ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32)
                    };
                    if written == 0 || written as usize > buf.len() {
                        anyhow::bail!("ExpandEnvironmentStringsW could not expand the value");
                    }

                    let mut buf = buf.as_slice();
                    while let Some(0) = buf.last() {
                        buf = &buf[0..buf.len() - 1];
                    }
                    Ok(OsString::from_wide(buf))
                }
                _ => Ok(OsString::from_reg_value(value)?),
            }
        }

        let mut from_registry: BTreeMap<OsString, EnvEntry> = BTreeMap::new();

        if let Ok(sys_env) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey("System\\CurrentControlSet\\Control\\Session Manager\\Environment")
        {
            for res in sys_env.enum_values() {
                if let Ok((name, value)) = res {
                    if name.to_ascii_lowercase() == "username" {
                        continue;
                    }
                    if let Ok(value) = reg_value_to_string(&value) {
                        log::trace!("adding SYS env: {:?} {:?}", name, value);
                        from_registry.insert(
                            EnvEntry::map_key(name.clone().into()),
                            EnvEntry {
                                is_from_base_env: true,
                                preferred_key: name.into(),
                                value,
                            },
                        );
                    }
                }
            }
        }

        if let Ok(sys_env) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment") {
            for res in sys_env.enum_values() {
                if let Ok((name, value)) = res {
                    if let Ok(value) = reg_value_to_string(&value) {
                        // Merge the system and user paths together
                        let value = if name.to_ascii_lowercase() == "path" {
                            match from_registry.get(&EnvEntry::map_key(name.clone().into())) {
                                Some(entry) => {
                                    let mut result = OsString::new();
                                    result.push(&entry.value);
                                    result.push(";");
                                    result.push(&value);
                                    result
                                }
                                None => value,
                            }
                        } else {
                            value
                        };

                        log::trace!("adding USER env: {:?} {:?}", name, value);
                        from_registry.insert(
                            EnvEntry::map_key(name.clone().into()),
                            EnvEntry {
                                is_from_base_env: true,
                                preferred_key: name.into(),
                                value,
                            },
                        );
                    }
                }
            }
        }

        fill_the_gaps(&mut env, from_registry);
    }

    env
}

/// The bytes of a registry string as a **NUL-terminated** wide buffer, which is
/// the only shape a Win32 string argument may be handed (review row R2-2).
///
/// Three things the registry can hand back and this answers all of them: a
/// trailing odd byte belongs to no `u16` and is dropped, an embedded NUL ends
/// the string because that is what a NUL means to every reader of one, and a
/// value that carried no terminator gets the one it needs. The result always
/// ends in exactly one NUL, so an empty value is a lone terminator rather than
/// an empty slice with nothing to stop on.
///
/// Public so that the terminal above this crate can pin the rule where its own
/// tests run: this package is a vendored fork and not a workspace member, so
/// nothing here is reached by `cargo test --workspace`.
#[cfg(windows)]
pub fn wide_terminated(bytes: &[u8]) -> Vec<u16> {
    let mut wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    if let Some(end) = wide.iter().position(|unit| *unit == 0) {
        wide.truncate(end);
    }
    wide.push(0);
    wide
}

/// Lay the registry's account of the environment **under** the process's own,
/// never over it (review row R2-22).
///
/// A pane is a child of this window, and the environment a child inherits is the
/// one its parent is standing in: the `PATH` the launching shell exported, the
/// variable a developer set for this session alone, the `NODE_OPTIONS` an
/// outer tool put there on the way in. Reading the two `Environment` keys and
/// writing them over that made every pane a child of the *machine* instead —
/// the window's own `PATH` was discarded, so the program `bt-app` resolved off
/// `PATH` and the program the child found under the same name could be two
/// different files.
///
/// The registry is still read, and this is what it is still for: a variable
/// added through System Properties after this window started exists nowhere in
/// this process, and a pane opened afterwards is entitled to see it. So it fills
/// the gaps and only the gaps.
#[cfg(windows)]
fn fill_the_gaps(
    env: &mut BTreeMap<OsString, EnvEntry>,
    from_registry: BTreeMap<OsString, EnvEntry>,
) {
    for (key, entry) in from_registry {
        env.entry(key).or_insert(entry);
    }
}

/// `CommandBuilder` is used to prepare a command to be spawned into a pty.
/// The interface is intentionally similar to that of `std::process::Command`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde_support", derive(Serialize, Deserialize))]
pub struct CommandBuilder {
    args: Vec<OsString>,
    envs: BTreeMap<OsString, EnvEntry>,
    cwd: Option<OsString>,
    #[cfg(unix)]
    pub(crate) umask: Option<libc::mode_t>,
    controlling_tty: bool,
    /// A command line for an interpreter that parses its own tail — see
    /// [`CommandBuilder::set_interpreter_line`]. `None` for every ordinary
    /// program, which is nearly all of them.
    #[cfg(windows)]
    interpreter_line: Option<OsString>,
}

impl CommandBuilder {
    /// Create a new builder instance with argv\[0\] set to the specified
    /// program.
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        Self {
            args: vec![program.as_ref().to_owned()],
            envs: get_base_env(),
            cwd: None,
            #[cfg(unix)]
            umask: None,
            controlling_tty: true,
            #[cfg(windows)]
            interpreter_line: None,
        }
    }

    /// Create a new builder instance from a pre-built argument vector
    pub fn from_argv(args: Vec<OsString>) -> Self {
        Self {
            args,
            envs: get_base_env(),
            cwd: None,
            #[cfg(unix)]
            umask: None,
            controlling_tty: true,
            #[cfg(windows)]
            interpreter_line: None,
        }
    }

    /// Set whether we should set the pty as the controlling terminal.
    /// The default is true, which is usually what you want, but you
    /// may need to set this to false if you are crossing container
    /// boundaries (eg: flatpak) to workaround issues like:
    /// <https://github.com/flatpak/flatpak/issues/3697>
    /// <https://github.com/flatpak/flatpak/issues/3285>
    pub fn set_controlling_tty(&mut self, controlling_tty: bool) {
        self.controlling_tty = controlling_tty;
    }

    pub fn get_controlling_tty(&self) -> bool {
        self.controlling_tty
    }

    /// Create a new builder instance that will run some idea of a default
    /// program.  Such a builder will panic if `arg` is called on it.
    pub fn new_default_prog() -> Self {
        Self {
            args: vec![],
            envs: get_base_env(),
            cwd: None,
            #[cfg(unix)]
            umask: None,
            controlling_tty: true,
            #[cfg(windows)]
            interpreter_line: None,
        }
    }

    /// Returns true if this builder was created via `new_default_prog`
    pub fn is_default_prog(&self) -> bool {
        self.args.is_empty()
    }

    /// Append an argument to the current command line.
    /// Will panic if called on a builder created via `new_default_prog`.
    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) {
        if self.is_default_prog() {
            panic!("attempted to add args to a default_prog builder");
        }
        self.args.push(arg.as_ref().to_owned());
    }

    /// Append a sequence of arguments to the current command line
    pub fn args<I, S>(&mut self, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            self.arg(arg);
        }
    }

    pub fn get_argv(&self) -> &Vec<OsString> {
        &self.args
    }

    pub fn get_argv_mut(&mut self) -> &mut Vec<OsString> {
        &mut self.args
    }

    /// Override the value of an environmental variable
    pub fn env<K, V>(&mut self, key: K, value: V)
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let key: OsString = key.as_ref().into();
        let value: OsString = value.as_ref().into();
        self.envs.insert(
            EnvEntry::map_key(key.clone()),
            EnvEntry {
                is_from_base_env: false,
                preferred_key: key,
                value: value,
            },
        );
    }

    pub fn env_remove<K>(&mut self, key: K)
    where
        K: AsRef<OsStr>,
    {
        let key = key.as_ref().into();
        self.envs.remove(&EnvEntry::map_key(key));
    }

    pub fn env_clear(&mut self) {
        self.envs.clear();
    }

    pub fn get_env<K>(&self, key: K) -> Option<&OsStr>
    where
        K: AsRef<OsStr>,
    {
        let key = key.as_ref().into();
        self.envs.get(&EnvEntry::map_key(key)).map(
            |EnvEntry {
                 is_from_base_env: _,
                 preferred_key: _,
                 value,
             }| value.as_os_str(),
        )
    }

    pub fn cwd<D>(&mut self, dir: D)
    where
        D: AsRef<OsStr>,
    {
        self.cwd = Some(dir.as_ref().to_owned());
    }

    pub fn clear_cwd(&mut self) {
        self.cwd.take();
    }

    pub fn get_cwd(&self) -> Option<&OsString> {
        self.cwd.as_ref()
    }

    /// Iterate over the configured environment. Only includes environment
    /// variables set by the caller via `env`, not variables set in the base
    /// environment.
    pub fn iter_extra_env_as_str(&self) -> impl Iterator<Item = (&str, &str)> {
        self.envs.values().filter_map(
            |EnvEntry {
                 is_from_base_env,
                 preferred_key,
                 value,
             }| {
                if *is_from_base_env {
                    None
                } else {
                    let key = preferred_key.to_str()?;
                    let value = value.to_str()?;
                    Some((key, value))
                }
            },
        )
    }

    pub fn iter_full_env_as_str(&self) -> impl Iterator<Item = (&str, &str)> {
        self.envs.values().filter_map(
            |EnvEntry {
                 preferred_key,
                 value,
                 ..
             }| {
                let key = preferred_key.to_str()?;
                let value = value.to_str()?;
                Some((key, value))
            },
        )
    }

    /// Return the configured command and arguments as a single string,
    /// quoted per the unix shell conventions.
    pub fn as_unix_command_line(&self) -> anyhow::Result<String> {
        let mut strs = vec![];
        for arg in &self.args {
            let s = arg
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("argument cannot be represented as utf8"))?;
            strs.push(s);
        }
        Ok(shell_words::join(strs))
    }
}

#[cfg(unix)]
impl CommandBuilder {
    pub fn umask(&mut self, mask: Option<libc::mode_t>) {
        self.umask = mask;
    }

    fn resolve_path(&self) -> Option<&OsStr> {
        self.get_env("PATH")
    }

    fn search_path(&self, exe: &OsStr, cwd: &OsStr) -> anyhow::Result<OsString> {
        use nix::unistd::{access, AccessFlags};

        let exe_path: &Path = exe.as_ref();
        if exe_path.is_relative() {
            let cwd: &Path = cwd.as_ref();
            let mut errors = vec![];

            // If the requested executable is explicitly relative to cwd,
            // then check only there.
            if is_cwd_relative_path(exe_path) {
                let abs_path = cwd.join(exe_path);

                if abs_path.is_dir() {
                    anyhow::bail!(
                        "Unable to spawn {} because it is a directory",
                        abs_path.display()
                    );
                } else if access(&abs_path, AccessFlags::X_OK).is_ok() {
                    return Ok(abs_path.into_os_string());
                } else if access(&abs_path, AccessFlags::F_OK).is_ok() {
                    anyhow::bail!(
                        "Unable to spawn {} because it is not executable",
                        abs_path.display()
                    );
                }

                anyhow::bail!(
                    "Unable to spawn {} because it does not exist",
                    abs_path.display()
                );
            }

            if let Some(path) = self.resolve_path() {
                for path in std::env::split_paths(&path) {
                    let candidate = cwd.join(&path).join(&exe);

                    if candidate.is_dir() {
                        errors.push(format!("{} exists but is a directory", candidate.display()));
                    } else if access(&candidate, AccessFlags::X_OK).is_ok() {
                        return Ok(candidate.into_os_string());
                    } else if access(&candidate, AccessFlags::F_OK).is_ok() {
                        errors.push(format!(
                            "{} exists but is not executable",
                            candidate.display()
                        ));
                    }
                }
                errors.push(format!("No viable candidates found in PATH {path:?}"));
            } else {
                errors.push("Unable to resolve the PATH".to_string());
            }
            anyhow::bail!(
                "Unable to spawn {} because:\n{}",
                exe_path.display(),
                errors.join(".\n")
            );
        } else if exe_path.is_dir() {
            anyhow::bail!(
                "Unable to spawn {} because it is a directory",
                exe_path.display()
            );
        } else {
            if let Err(err) = access(exe_path, AccessFlags::X_OK) {
                if access(exe_path, AccessFlags::F_OK).is_ok() {
                    anyhow::bail!(
                        "Unable to spawn {} because it is not executable ({err:#})",
                        exe_path.display()
                    );
                } else {
                    anyhow::bail!(
                        "Unable to spawn {} because it doesn't exist on the filesystem ({err:#})",
                        exe_path.display()
                    );
                }
            }

            Ok(exe.to_owned())
        }
    }

    /// Convert the CommandBuilder to a `std::process::Command` instance.
    pub(crate) fn as_command(&self) -> anyhow::Result<std::process::Command> {
        use std::os::unix::process::CommandExt;

        let home = self.get_home_dir()?;
        let dir: &OsStr = self
            .cwd
            .as_ref()
            .map(|dir| dir.as_os_str())
            .filter(|dir| std::path::Path::new(dir).is_dir())
            .unwrap_or(home.as_ref());
        let shell = self.get_shell();

        let mut cmd = if self.is_default_prog() {
            let mut cmd = std::process::Command::new(&shell);

            // Run the shell as a login shell by prefixing the shell's
            // basename with `-` and setting that as argv0
            let basename = shell.rsplit('/').next().unwrap_or(&shell);
            cmd.arg0(&format!("-{}", basename));
            cmd
        } else {
            let resolved = self.search_path(&self.args[0], dir)?;
            let mut cmd = std::process::Command::new(&resolved);
            cmd.arg0(&self.args[0]);
            cmd.args(&self.args[1..]);
            cmd
        };

        cmd.current_dir(dir);

        cmd.env_clear();
        cmd.env("SHELL", shell);
        cmd.envs(self.envs.values().map(
            |EnvEntry {
                 is_from_base_env: _,
                 preferred_key,
                 value,
             }| (preferred_key.as_os_str(), value.as_os_str()),
        ));

        Ok(cmd)
    }

    /// Determine which shell to run.
    /// We take the contents of the $SHELL env var first, then
    /// fall back to looking it up from the password database.
    pub fn get_shell(&self) -> String {
        use nix::unistd::{access, AccessFlags};

        if let Some(shell) = self.get_env("SHELL").and_then(OsStr::to_str) {
            match access(shell, AccessFlags::X_OK) {
                Ok(()) => return shell.into(),
                Err(err) => log::warn!(
                    "$SHELL -> {shell:?} which is \
                     not executable ({err:#}), falling back to password db lookup"
                ),
            }
        }

        get_shell().into()
    }

    fn get_home_dir(&self) -> anyhow::Result<String> {
        if let Some(home_dir) = self.get_env("HOME").and_then(OsStr::to_str) {
            return Ok(home_dir.into());
        }

        let ent = unsafe { libc::getpwuid(libc::getuid()) };
        if ent.is_null() {
            Ok("/".into())
        } else {
            use std::ffi::CStr;
            use std::str;
            let home = unsafe { CStr::from_ptr((*ent).pw_dir) };
            home.to_str()
                .map(str::to_owned)
                .context("failed to resolve home dir")
        }
    }
}

#[cfg(windows)]
impl CommandBuilder {
    fn search_path(&self, exe: &OsStr) -> OsString {
        if let Some(path) = self.get_env("PATH") {
            let extensions = self.get_env("PATHEXT").unwrap_or(OsStr::new(".EXE"));
            for path in std::env::split_paths(&path) {
                // Check for exactly the user's string in this path dir
                let candidate = path.join(&exe);
                if candidate.exists() {
                    return candidate.into_os_string();
                }

                // otherwise try tacking on some extensions.
                // Note that this really replaces the extension in the
                // user specified path, so this is potentially wrong.
                for ext in std::env::split_paths(&extensions) {
                    // PATHEXT includes the leading `.`, but `with_extension`
                    // doesn't want that
                    let ext = ext.to_str().expect("PATHEXT entries must be utf8");
                    let path = path.join(&exe).with_extension(&ext[1..]);
                    if path.exists() {
                        return path.into_os_string();
                    }
                }
            }
        }

        exe.to_owned()
    }

    pub(crate) fn current_directory(&self) -> Option<Vec<u16>> {
        let home: Option<&OsStr> = self
            .get_env("USERPROFILE")
            .filter(|path| Path::new(path).is_dir());
        let cwd: Option<&OsStr> = self.cwd.as_deref().filter(|path| Path::new(path).is_dir());
        let dir: Option<&OsStr> = cwd.or(home);

        dir.map(|dir| {
            let mut wide = vec![];

            if Path::new(dir).is_relative() {
                if let Ok(ccwd) = std::env::current_dir() {
                    wide.extend(ccwd.join(dir).as_os_str().encode_wide());
                } else {
                    wide.extend(dir.encode_wide());
                }
            } else {
                wide.extend(dir.encode_wide());
            }

            wide.push(0);
            wide
        })
    }

    /// Constructs an environment block for this spawn attempt.
    /// Uses the current process environment as the base and then
    /// adds/replaces the environment that was specified via the
    /// `env` methods.
    pub fn environment_block(&self) -> Vec<u16> {
        // encode the environment as wide characters
        let mut block = vec![];

        for EnvEntry {
            is_from_base_env: _,
            preferred_key,
            value,
        } in self.envs.values()
        {
            // **A name that cannot be spelled in a block is left out of it**
            // (review row R2-22). The block is a run of `NAME=VALUE` strings
            // separated by NULs, so the first `=` in an entry is where the name
            // ends and a NUL is where the entry does: a name carrying either
            // does not name a variable, it renames the one beside it or cuts
            // the block short. Windows itself uses a leading `=` for the
            // per-drive current directories (`=C:`), which is why the test is
            // for an `=` anywhere in the name rather than only at the front.
            if !a_block_can_carry(preferred_key, value) {
                continue;
            }
            block.extend(preferred_key.encode_wide());
            block.push(b'=' as u16);
            block.extend(value.encode_wide());
            block.push(0);
        }
        // and a final terminator for CreateProcessW
        block.push(0);

        block
    }

    pub fn get_shell(&self) -> String {
        let exe: OsString = self
            .get_env("ComSpec")
            .unwrap_or(OsStr::new("cmd.exe"))
            .into();
        exe.into_string()
            .unwrap_or_else(|_| "%CompSpec%".to_string())
    }

    /// Hand the interpreter named by `argv[0]` a command line it will parse for
    /// **itself**, written through exactly as given.
    ///
    /// `cmd.exe` after `/c` does not read argv: it reads the rest of the line
    /// with its own quoting rules, where a backslash is not an escape and a
    /// double quote is the only thing that makes `&`, `|`, `<`, `>` and `^`
    /// ordinary characters. Argv quoting therefore cannot express what that
    /// line has to say, and a caller who knows it is addressing an interpreter
    /// writes the line and says so here. Everything before it — `argv[0]` and
    /// the switches — is still quoted the ordinary way.
    ///
    /// A line carrying a NUL is refused by [`Self::cmdline`], the same as an
    /// argument that carries one.
    pub fn set_interpreter_line<S: AsRef<OsStr>>(&mut self, line: S) {
        self.interpreter_line = Some(line.as_ref().to_os_string());
    }

    /// The line this builder hands its interpreter, if it was given one.
    pub fn get_interpreter_line(&self) -> Option<&OsStr> {
        self.interpreter_line.as_deref()
    }

    pub fn cmdline(&self) -> anyhow::Result<(Vec<u16>, Vec<u16>)> {
        let mut cmdline = Vec::<u16>::new();

        let exe: OsString = if self.is_default_prog() {
            self.get_env("ComSpec")
                .unwrap_or(OsStr::new("cmd.exe"))
                .into()
        } else {
            self.search_path(&self.args[0])
        };

        Self::append_quoted(&exe, &mut cmdline);

        // Ensure that we nul terminate the module name, otherwise we'll
        // ask CreateProcessW to start something random!
        let mut exe: Vec<u16> = exe.encode_wide().collect();
        exe.push(0);

        for arg in self.args.iter().skip(1) {
            cmdline.push(' ' as u16);
            anyhow::ensure!(
                !arg.encode_wide().any(|c| c == 0),
                "invalid encoding for command line argument {:?}",
                arg
            );
            Self::append_quoted(arg, &mut cmdline);
        }
        // The interpreter's own line, written through as given: the caller
        // quoted it by the interpreter's rules and this must not quote it again.
        // See [`Self::set_interpreter_line`].
        if let Some(line) = &self.interpreter_line {
            anyhow::ensure!(
                !line.encode_wide().any(|c| c == 0),
                "invalid encoding for interpreter command line {:?}",
                line
            );
            cmdline.push(' ' as u16);
            cmdline.extend(line.encode_wide());
        }
        // Ensure that the command line is nul terminated too!
        cmdline.push(0);
        Ok((exe, cmdline))
    }

    // Borrowed from https://github.com/hniksic/rust-subprocess/blob/873dfed165173e52907beb87118b2c0c05d8b8a1/src/popen.rs#L1117
    // which in turn was translated from ArgvQuote at http://tinyurl.com/zmgtnls
    fn append_quoted(arg: &OsStr, cmdline: &mut Vec<u16>) {
        if !arg.is_empty()
            && !arg.encode_wide().any(|c| {
                c == ' ' as u16
                    || c == '\t' as u16
                    || c == '\n' as u16
                    || c == '\x0b' as u16
                    || c == '\"' as u16
            })
        {
            cmdline.extend(arg.encode_wide());
            return;
        }
        cmdline.push('"' as u16);

        let arg: Vec<_> = arg.encode_wide().collect();
        let mut i = 0;
        while i < arg.len() {
            let mut num_backslashes = 0;
            while i < arg.len() && arg[i] == '\\' as u16 {
                i += 1;
                num_backslashes += 1;
            }

            if i == arg.len() {
                for _ in 0..num_backslashes * 2 {
                    cmdline.push('\\' as u16);
                }
                break;
            } else if arg[i] == b'"' as u16 {
                for _ in 0..num_backslashes * 2 + 1 {
                    cmdline.push('\\' as u16);
                }
                cmdline.push(arg[i]);
            } else {
                for _ in 0..num_backslashes {
                    cmdline.push('\\' as u16);
                }
                cmdline.push(arg[i]);
            }
            i += 1;
        }
        cmdline.push('"' as u16);
    }
}

/// Whether a `NAME=VALUE` pair can be spelled in an environment block at all
/// (review row R2-22).
///
/// The block's own grammar is the whole rule: entries are separated by NULs and
/// a name ends at the first `=`, so a name carrying either character does not
/// name a variable — it renames the entry beside it, or ends the block early and
/// takes every entry after it away. A value may carry an `=` (a great many do)
/// but not a NUL, for the same reason.
///
/// An empty name is refused here too: `=NAME=VALUE` is how Windows spells the
/// per-drive current directory, and a caller's empty name would be writing one.
///
/// Public for [`wide_terminated`]'s reason.
#[cfg(windows)]
pub fn a_block_can_carry(name: &OsStr, value: &OsStr) -> bool {
    !name.is_empty()
        && !name
            .encode_wide()
            .any(|unit| unit == 0 || unit == b'=' as u16)
        && !value.encode_wide().any(|unit| unit == 0)
}

#[cfg(unix)]
/// Returns true if the path begins with `./` or `../`
fn is_cwd_relative_path<P: AsRef<Path>>(p: P) -> bool {
    matches!(
        p.as_ref().components().next(),
        Some(Component::CurDir | Component::ParentDir)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn test_cwd_relative() {
        assert!(is_cwd_relative_path("."));
        assert!(is_cwd_relative_path("./foo"));
        assert!(is_cwd_relative_path("../foo"));
        assert!(!is_cwd_relative_path("foo"));
        assert!(!is_cwd_relative_path("/foo"));
    }

    #[test]
    fn test_env() {
        let mut cmd = CommandBuilder::new("dummy");
        let package_authors = cmd.get_env("CARGO_PKG_AUTHORS");
        println!("package_authors: {:?}", package_authors);
        assert!(package_authors == Some(OsStr::new("Wez Furlong")));

        cmd.env("foo key", "foo value");
        cmd.env("bar key", "bar value");

        let iterated_envs = cmd.iter_extra_env_as_str().collect::<Vec<_>>();
        println!("iterated_envs: {:?}", iterated_envs);
        assert!(iterated_envs == vec![("bar key", "bar value"), ("foo key", "foo value")]);

        {
            let mut cmd = cmd.clone();
            cmd.env_remove("foo key");

            let iterated_envs = cmd.iter_extra_env_as_str().collect::<Vec<_>>();
            println!("iterated_envs: {:?}", iterated_envs);
            assert!(iterated_envs == vec![("bar key", "bar value")]);
        }

        {
            let mut cmd = cmd.clone();
            cmd.env_remove("bar key");

            let iterated_envs = cmd.iter_extra_env_as_str().collect::<Vec<_>>();
            println!("iterated_envs: {:?}", iterated_envs);
            assert!(iterated_envs == vec![("foo key", "foo value")]);
        }

        {
            let mut cmd = cmd.clone();
            cmd.env_clear();

            let iterated_envs = cmd.iter_extra_env_as_str().collect::<Vec<_>>();
            println!("iterated_envs: {:?}", iterated_envs);
            assert!(iterated_envs.is_empty());
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_env_case_insensitive_override() {
        let mut cmd = CommandBuilder::new("dummy");
        cmd.env("Cargo_Pkg_Authors", "Not Wez");
        assert!(cmd.get_env("cargo_pkg_authors") == Some(OsStr::new("Not Wez")));

        cmd.env_remove("cARGO_pKG_aUTHORS");
        assert!(cmd.get_env("CARGO_PKG_AUTHORS").is_none());
    }
}
