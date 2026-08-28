//! What this machine's WSL installation says about itself.
//!
//! Two questions, and they are not the same **kind** of question — which is the
//! whole of this module's shape (§7.40 ② and ③):
//!
//! * **which distributions are installed, and which one the `WSL` profile
//!   actually starts**, so that the picker can name it when there is more than
//!   one to choose between. `wsl.exe` with no arguments starts the user's
//!   *default* distribution, which on this machine is Ubuntu and on the next is
//!   Debian or Alpine — printing one over a command that will start the other is
//!   chrome saying something it did not check. This is a fact about **Windows**,
//!   it is written in the registry, and reading it costs microseconds and starts
//!   nothing;
//! * **which shell that distribution logs the user into**, because the
//!   integration script is a bash script and handing `--init-file` to a shell
//!   that is not bash would replace the user's shell with one they did not
//!   choose. Asking is the difference between an integration and a substitution.
//!   This is a fact about a **Linux user account**, the only thing that holds it
//!   is the distribution's own password database, and reading it means booting a
//!   virtual machine.
//!
//! Both used to be asked at launch, by two `wsl.exe` invocations on a worker
//! thread, and both used to be waited for by the opening window's own title. That
//! cost this product a Windows Terminal window at every launch and several
//! seconds before the first frame; §7.40 is the ruling that took both apart. The
//! first question is now answered here, synchronously, out of
//! `HKCU\…\Lxss`. The second is not asked until a WSL pane is actually opened,
//! and nothing ever waits for it.

use std::{
    ffi::{OsStr, OsString},
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

/// The shell a distribution has to log into before this terminal will hand it an
/// init file.
///
/// `bash` and nothing else, because `folio.bash` is a bash script:
/// `--init-file` is bash's own flag, the `PROMPT_COMMAND`/`DEBUG` pair it
/// installs is bash's own mechanism, and zsh and fish spell every part of this
/// differently. A distribution logging into one of those keeps its shell and
/// goes without markers, which is the documented degradation
/// (`docs/shell-integration.md` §111-115) rather than a broken shell.
const INTEGRATED_SHELL: &str = "bash";

/// Where Windows writes down this user's WSL installation, below
/// `HKEY_CURRENT_USER`.
///
/// This is `wsl.exe --list`'s own source, and not a guess at one: the launcher
/// reads these keys to find out which distributions exist and which of them it
/// starts when nothing names another. Every subkey is one distribution, named by
/// the GUID Windows minted for it.
const LXSS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";

/// The value under [`LXSS_KEY`] holding the **GUID** — not the name — of the
/// distribution `wsl.exe` starts with no arguments.
const DEFAULT_DISTRIBUTION_VALUE: &str = "DefaultDistribution";

/// The value under each distribution's own key holding the name a human types.
///
/// Its presence is also what makes a subkey a *distribution*: `Lxss` holds other
/// things (an installer cache, for one), and a subkey that does not name a
/// distribution is not one.
const DISTRIBUTION_NAME_VALUE: &str = "DistributionName";

/// The part of the registry this module reads, as a thing that can be stood in
/// for.
///
/// A trait rather than two free functions because the alternative is a module
/// whose only test is "run it on the machine you happen to be holding": the
/// answer would be Ubuntu here, Debian on the next desk and nothing in CI, and
/// none of the three would be a statement about the parsing. With this, the
/// suite hands over a registry it wrote — including the shapes a real one takes
/// and a developer's own machine may not have, like a default GUID naming a key
/// that is gone.
trait Registry {
    /// One `REG_SZ`, or `None` for "the key is not there, the value is not
    /// there, or what is there is not a string".
    fn string(&self, key: &str, name: &str) -> Option<String>;
    /// Every immediate subkey of `key`, in whatever order the registry gives
    /// them.
    fn subkeys(&self, key: &str) -> Vec<String>;
}

/// The machine's own registry, below `HKEY_CURRENT_USER`.
struct CurrentUser;

#[cfg(windows)]
impl Registry for CurrentUser {
    fn string(&self, key: &str, name: &str) -> Option<String> {
        bt_platform::current_user_registry_string(key, name)
    }

    fn subkeys(&self, key: &str) -> Vec<String> {
        bt_platform::current_user_registry_subkeys(key)
    }
}

#[cfg(not(windows))]
impl Registry for CurrentUser {
    fn string(&self, _key: &str, _name: &str) -> Option<String> {
        None
    }

    fn subkeys(&self, _key: &str) -> Vec<String> {
        Vec::new()
    }
}

/// What this machine's WSL installation is. Empty is a valid, meaningful answer:
/// a machine with `wsl.exe` present but no distribution installed reaches
/// exactly this, and so does one whose registry says nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WslFacts {
    /// Every installed distribution, by name, in alphabetical order.
    ///
    /// **Sorted here rather than left as the registry gave them**, and the order
    /// is not a claim about anything: the registry enumerates subkeys by their
    /// GUIDs, which is an order nobody chose and nobody can predict, so passing
    /// it on would be presenting an arbitrary sequence as though it meant
    /// something. Alphabetical is the one order this list can be given that says
    /// only what it is.
    distributions: Vec<String>,
    /// The one `wsl.exe` starts when nothing names another, and the shell it
    /// logs into.
    default: Option<DefaultDistribution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DefaultDistribution {
    name: String,
    /// The login shell out of `/etc/passwd`, or `None` when the distribution has
    /// not been asked **yet** — which at launch, and until the first WSL pane of
    /// the run is opened, is always.
    login_shell: Option<String>,
}

impl WslFacts {
    /// The word after `·` in the profile's title, or `None` when the title is
    /// complete without one.
    ///
    /// **Only when there is a choice to disambiguate.** One distribution needs no
    /// qualifier: "WSL" already names it uniquely, and `WSL · Ubuntu-24.04`
    /// spends a third of the strip's width restating what the mark already says.
    /// Two or more, and the bare title is an unanswered question — which one? —
    /// so the name earns its space the moment it is load-bearing and not before.
    /// This is the mock-up's own `WSL · Ubuntu` (line 2598) with the discovery
    /// claim behind it actually made.
    #[must_use]
    pub fn title_qualifier(&self) -> Option<&str> {
        if self.distributions.len() < 2 {
            return None;
        }
        self.default.as_ref().map(|default| default.name.as_str())
    }

    /// The absolute path of the shell to start, when this machine's default
    /// distribution logs into a bash and can therefore be given an init file.
    ///
    /// `None` covers four different machines with one answer, and the answer is
    /// the same for all four because the consequence is: no distribution, a
    /// distribution that would not answer, a distribution whose user logs into
    /// zsh, and — new with §7.40 ③ — a distribution **nobody has asked yet**. In
    /// each case the profile starts `wsl.exe` exactly as it did before this
    /// module existed.
    #[must_use]
    pub fn integrated_login_shell(&self) -> Option<&str> {
        let shell = self.default.as_ref()?.login_shell.as_deref()?;
        (Path::new(shell).file_name()? == OsStr::new(INTEGRATED_SHELL)).then_some(shell)
    }
}

/// The installation, read out of the registry.
///
/// Three reads and no processes. The default is stored as a **GUID** and the
/// names live one key down, so the two have to be joined here; a `DefaultDistribution`
/// naming a key that is no longer there — which is what a half-finished
/// `wsl --unregister` leaves — yields distributions and no default, which is
/// exactly the honest answer and the one `title_qualifier` already knows how to
/// say nothing about.
fn read_installation(registry: &dyn Registry) -> WslFacts {
    let default_guid = registry.string(LXSS_KEY, DEFAULT_DISTRIBUTION_VALUE);
    let mut facts = WslFacts::default();
    for guid in registry.subkeys(LXSS_KEY) {
        // A subkey with no name is not a distribution — `Lxss` holds an
        // installer cache beside them — and this is the criterion rather than
        // "the subkey looks like a GUID", because what makes an entry a
        // distribution is that it *has* a name, not how Windows spelled its key.
        let Some(name) = registry.string(&format!("{LXSS_KEY}\\{guid}"), DISTRIBUTION_NAME_VALUE)
        else {
            continue;
        };
        // Case-insensitively, because registry key names are: the value and the
        // subkey are two spellings of one GUID and Windows does not promise they
        // agree on case.
        if default_guid
            .as_deref()
            .is_some_and(|default| default.eq_ignore_ascii_case(&guid))
        {
            facts.default = Some(DefaultDistribution {
                name: name.clone(),
                login_shell: None,
            });
        }
        facts.distributions.push(name);
    }
    facts.distributions.sort();
    facts
}

/// The login shell `/etc/passwd` gives this user, as the distribution itself
/// reports it.
///
/// `getent passwd` and not `$SHELL`: `wsl.exe -e` does not start a login shell,
/// so `SHELL` is either unset or — worse — inherited from the Windows side,
/// where this process may well have been started by something that set it. The
/// password database is the same source the distribution's own login would read.
fn parse_login_shell(stdout: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stdout);
    let shell = text.lines().next()?.trim();
    (!shell.is_empty()).then(|| shell.to_owned())
}

/// Boot the distribution and ask it.
///
/// **The expensive half of this module, and the whole reason for the split.**
/// `wsl.exe -e` on a machine whose virtual machine is not already up costs
/// seconds — the WSL2 utility VM has to start and the distribution has to be
/// mounted — and it does that on any machine that merely *has* WSL installed,
/// including one whose owner never opens a WSL pane. That is what it used to do
/// at every launch of Folio.
///
/// Through [`bt_platform::quiet_command`], which is where the console this child
/// must not be given is refused. Without it Windows allocates the child a
/// console of its own, and on Windows 11 the registered host for a new console
/// is Windows Terminal — so this call, and only this call, was opening a second
/// terminal emulator's window in front of Folio at every launch, tab-titled
/// `C:\WINDOWS\System32\wsl.exe`.
fn ask_login_shell(program: &OsStr) -> Option<String> {
    let output = bt_platform::quiet_command(program)
        .args([
            "-e",
            "sh",
            "-c",
            r#"getent passwd "$(id -u)" | cut -d: -f7"#,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_login_shell(&output.stdout)
}

/// One machine, one answer — and one place it is kept.
///
/// Process-wide rather than owned by `Runtime`, for the reason
/// `bt_term::local_host_name` is: a machine does not install a WSL distribution
/// *inside* one terminal session, and the readers are the places a profile is
/// **named** — a menu row, a tooltip, a settings option — which are scattered
/// through the chrome and would otherwise each need this threaded down to them
/// through layout code that has no other reason to know what WSL is.
///
/// Untouched by any test: [`facts`] answers `WslFacts::default()` until [`start`]
/// is called, which nothing but `main` does, so a unit test of anything that
/// names a profile gets the bare titles deterministically rather than whatever
/// the machine running the test happens to have installed.
static INSTALLATION: OnceLock<WslFacts> = OnceLock::new();

/// The `wsl.exe` this machine resolved to, kept so that the deferred probe can
/// be armed later by a caller that has no reason to know where it is.
///
/// `None` inside the `Some` is a machine with no WSL, and it is the second of
/// [`begin_login_shell_probe`]'s two guards: the spawn path already asks whether
/// this pane's profile lives in WSL's namespace, and this asks whether the
/// machine has a `wsl.exe` to put the question to. That is the same guard
/// [`start`] used to apply to the whole probe, kept where it still means
/// something.
static WSL_PROGRAM: OnceLock<Option<OsString>> = OnceLock::new();

/// What the distribution answered, once it has. Set exactly once per run.
static LOGIN_SHELL: OnceLock<Option<String>> = OnceLock::new();

/// Whether the asking has already begun, so that the fourth WSL pane of a run
/// does not start a fourth `wsl.exe`.
static LOGIN_SHELL_ASKED: AtomicBool = AtomicBool::new(false);

fn asking_started() -> bool {
    LOGIN_SHELL_ASKED.swap(true, Ordering::SeqCst)
}

/// Read what Windows knows, once. `program` is what
/// [`crate::profiles::ProfilePrograms`] resolved the WSL profile to, so a
/// machine without WSL is one that reads nothing.
///
/// **Synchronous, and that is the point** (§7.40 ②). This used to spawn a worker
/// that ran two `wsl.exe` invocations, and the opening window's own title then
/// joined that worker — so the launch waited for a virtual machine to boot
/// before it could draw a window. Three registry reads take microseconds; there
/// is nothing here worth a thread, and nothing left for a frame to wait on.
pub fn start(program: Option<&OsStr>) {
    let _ = WSL_PROGRAM.set(program.map(OsStr::to_os_string));
    let installation = if program.is_some() {
        read_installation(&CurrentUser)
    } else {
        WslFacts::default()
    };
    let _ = INSTALLATION.set(installation);
}

/// **Ask the default distribution what shell it logs into** — starting the ask
/// if this is the first WSL session of the run, and never waiting for it.
///
/// Called from the pane spawn and from nowhere else, beside
/// `psreadline::begin_probe` and for its argument: the question is only worth a
/// process on a machine where somebody is actually opening this kind of shell,
/// and on every other machine it is a virtual machine booted for nothing.
///
/// **Idempotent.** The second WSL pane of a run finds the ask already begun and
/// starts nothing; a run that opens none never asks at all.
pub fn begin_login_shell_probe() {
    let Some(program) = WSL_PROGRAM.get().cloned().flatten() else {
        return;
    };
    begin_login_shell_probe_with(move || ask_login_shell(&program));
}

/// The mechanism under [`begin_login_shell_probe`], with the asking handed in.
///
/// Split for one caller: `the_first_frame_does_not_wait_for_the_wsl_probe` needs
/// a distribution that never answers, and there is no machine on which a real
/// one behaves that way on demand.
fn begin_login_shell_probe_with(ask: impl FnOnce() -> Option<String> + Send + 'static) {
    if LOGIN_SHELL.get().is_some() || asking_started() {
        return;
    }
    // In the workers' band: this boots a virtual machine to ask a question about
    // a password file, and it must never be the reason a frame was late.
    bt_platform::spawn_at_priority(
        "bt-wsl-login-shell",
        bt_platform::ThreadPriority::BelowNormal,
        move || {
            let _ = LOGIN_SHELL.set(ask());
        },
    )
    .ok();
}

/// What the machine says, right now, without waiting for anything.
///
/// **There is no blocking twin to tell this apart from** (§7.40 ④), which is why
/// it is not called `try_facts`: the installation is already in hand — it was
/// read from the registry before the window existed — and the login shell either
/// has been answered or has not. A reader that arrives before the answer gets
/// `None` for the shell and the complete list of distributions, and that is the
/// whole of the state space. Nothing in this module can make a caller wait.
///
/// Owned rather than borrowed because the two halves come from two places and
/// are joined here. It is a handful of short strings, read once per pane spawn
/// and once per rebuild of the profile titles.
#[must_use]
pub fn facts() -> WslFacts {
    let mut facts = INSTALLATION.get().cloned().unwrap_or_default();
    if let Some(default) = facts.default.as_mut() {
        default.login_shell = LOGIN_SHELL.get().cloned().flatten();
    }
    facts
}

/// An answer a probe could have returned, for the tests of the things that read
/// one. Not a fixture shared between assertions — a constructor, so that each
/// caller states the machine it means.
#[cfg(test)]
#[must_use]
pub fn test_facts(default: &str, login_shell: Option<&str>) -> WslFacts {
    WslFacts {
        distributions: vec![default.to_owned()],
        default: Some(DefaultDistribution {
            name: default.to_owned(),
            login_shell: login_shell.map(str::to_owned),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::mpsc, time::Duration};

    /// A registry somebody wrote down, shaped exactly like the one Windows keeps.
    ///
    /// Keys are compared case-insensitively, as the real one does, so a fixture
    /// is free to spell a GUID the way `DefaultDistribution` spells it and the
    /// subkey the way `RegEnumKeyEx` returns it.
    #[derive(Default)]
    struct FakeRegistry {
        subkeys: BTreeMap<String, Vec<String>>,
        strings: BTreeMap<(String, String), String>,
    }

    impl FakeRegistry {
        /// `default` is the GUID in `DefaultDistribution`, or `None` for a value
        /// that is not there at all. Each entry of `subkeys` is a key below
        /// `Lxss` and the `DistributionName` sitting in it, where `None` is a
        /// subkey that names no distribution.
        fn machine(default: Option<&str>, subkeys: &[(&str, Option<&str>)]) -> Self {
            let mut registry = Self::default();
            if let Some(default) = default {
                registry.strings.insert(
                    (
                        LXSS_KEY.to_ascii_lowercase(),
                        DEFAULT_DISTRIBUTION_VALUE.to_owned(),
                    ),
                    default.to_owned(),
                );
            }
            registry.subkeys.insert(
                LXSS_KEY.to_ascii_lowercase(),
                subkeys.iter().map(|(guid, _)| (*guid).to_owned()).collect(),
            );
            for (guid, name) in subkeys {
                if let Some(name) = name {
                    registry.strings.insert(
                        (
                            format!("{LXSS_KEY}\\{guid}").to_ascii_lowercase(),
                            DISTRIBUTION_NAME_VALUE.to_owned(),
                        ),
                        (*name).to_owned(),
                    );
                }
            }
            registry
        }
    }

    impl Registry for FakeRegistry {
        fn string(&self, key: &str, name: &str) -> Option<String> {
            self.strings
                .get(&(key.to_ascii_lowercase(), name.to_owned()))
                .cloned()
        }

        fn subkeys(&self, key: &str) -> Vec<String> {
            self.subkeys
                .get(&key.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default()
        }
    }

    /// RED — **the installation is read out of the registry, and the default is
    /// found by joining a GUID to a name.**
    ///
    /// Red gate for §7.40 ②. This machine's own `Lxss` is the fixture: three
    /// subkeys, `DefaultDistribution` holding the GUID of the third, and one
    /// extra subkey that names no distribution because a real `Lxss` has those.
    ///
    /// MUTATIONS:
    /// ① read `DefaultDistribution` as a *name* rather than as a GUID and the
    ///    default is `None` on every real machine, so the qualifier silently
    ///    never appears — which looks exactly like a machine with one
    ///    distribution;
    /// ② count every subkey rather than every subkey that names a distribution
    ///    and a single-distribution machine with an installer cache beside it
    ///    grows a qualifier it has no second distribution to disambiguate from.
    #[test]
    fn the_installation_is_read_from_the_registry_and_the_default_is_a_guid() {
        let facts = read_installation(&FakeRegistry::machine(
            Some("{ee591c83-9346-4f59-a665-04d63bd8e127}"),
            &[
                (
                    "{0a68a8ba-c307-4413-b758-f5683e4c7161}",
                    Some("Ubuntu-22.04"),
                ),
                (
                    "{8a50e0f3-c08c-4419-aeeb-81037cd1ec3a}",
                    Some("docker-desktop"),
                ),
                (
                    "{ee591c83-9346-4f59-a665-04d63bd8e127}",
                    Some("Ubuntu-24.04"),
                ),
                ("AppxInstallerCache", None),
            ],
        ));
        assert_eq!(
            facts.distributions,
            ["Ubuntu-22.04", "Ubuntu-24.04", "docker-desktop"],
            "every subkey that names a distribution, and no subkey that does not"
        );
        assert_eq!(
            facts.title_qualifier(),
            Some("Ubuntu-24.04"),
            "the GUID in `DefaultDistribution` names the key whose name is the answer"
        );
        // The registry does not promise the two spellings agree on case, and it
        // compares key names without it.
        let uppercase = read_installation(&FakeRegistry::machine(
            Some("{EE591C83-9346-4F59-A665-04D63BD8E127}"),
            &[
                ("{0a68a8ba-c307-4413-b758-f5683e4c7161}", Some("Debian")),
                ("{ee591c83-9346-4f59-a665-04d63bd8e127}", Some("Ubuntu")),
            ],
        ));
        assert_eq!(uppercase.title_qualifier(), Some("Ubuntu"));
    }

    /// PIN — the qualifier appears exactly when it is answering a question.
    #[test]
    fn one_distribution_needs_no_qualifier_and_none_at_all_needs_no_wsl() {
        let one = read_installation(&FakeRegistry::machine(
            Some("{aaa}"),
            &[("{aaa}", Some("Ubuntu"))],
        ));
        assert_eq!(one.distributions, ["Ubuntu"]);
        assert_eq!(
            one.title_qualifier(),
            None,
            "`WSL` already names it uniquely; the name would only take up room"
        );
        // A machine with no `Lxss` at all — WSL's launcher present, no
        // distribution ever installed.
        assert_eq!(
            read_installation(&FakeRegistry::default()),
            WslFacts::default()
        );
        assert_eq!(WslFacts::default().title_qualifier(), None);
        // A `DefaultDistribution` naming a key that is gone — what a
        // half-finished `wsl --unregister` leaves — cannot say which one the
        // profile starts, so it says nothing.
        let dangling = read_installation(&FakeRegistry::machine(
            Some("{ccc}"),
            &[("{aaa}", Some("Ubuntu")), ("{bbb}", Some("Debian"))],
        ));
        assert_eq!(dangling.distributions.len(), 2);
        assert_eq!(dangling.title_qualifier(), None);
    }

    /// PIN — an init file is offered to bash and to nothing else.
    ///
    /// Red gate: handing `--init-file` to whatever shell the distribution logs
    /// into replaces a zsh user's shell with bash every time they open a tab,
    /// and the symptom — "my prompt is gone" — names neither this terminal nor
    /// the flag that did it.
    #[test]
    fn only_a_distribution_that_logs_into_bash_is_handed_an_init_file() {
        let mut facts = read_installation(&FakeRegistry::machine(
            Some("{aaa}"),
            &[("{aaa}", Some("Ubuntu"))],
        ));
        assert_eq!(
            facts.integrated_login_shell(),
            None,
            "a distribution that has not been asked yet is not assumed to run bash"
        );
        for (shell, integrated) in [
            ("/bin/bash", Some("/bin/bash")),
            ("/usr/bin/bash", Some("/usr/bin/bash")),
            ("/usr/bin/zsh", None),
            ("/usr/bin/fish", None),
            ("/sbin/nologin", None),
            ("", None),
        ] {
            facts.default.as_mut().unwrap().login_shell =
                (!shell.is_empty()).then(|| shell.to_owned());
            assert_eq!(facts.integrated_login_shell(), integrated, "{shell:?}");
        }
    }

    /// PIN — the password entry is read at the field the shell lives in.
    #[test]
    fn the_login_shell_is_the_last_field_of_the_password_entry() {
        assert_eq!(
            parse_login_shell(b"/bin/bash\n"),
            Some("/bin/bash".to_owned())
        );
        assert_eq!(parse_login_shell(b"\n"), None);
        assert_eq!(parse_login_shell(b""), None);
    }

    /// RED — **nothing on the way to a frame waits for the distribution.**
    ///
    /// Red gate for §7.40 ④, and it is written the way
    /// `git::tests::a_child_that_will_not_finish_is_killed_and_reported` is,
    /// for that test's reason: the failure this guards is *never returns*, not
    /// *slow*, so the read happens on a thread of its own and the answer comes
    /// back down a channel. A build that joined the probe would fail this in a
    /// minute instead of hanging the suite for ever. **The minute is not a
    /// budget** — it is the difference between "came back" and "did not", which
    /// is the only difference this test is about.
    ///
    /// The distribution here never answers while the test is running, which is
    /// exactly the machine the old code could not survive: `WslProbe::facts`
    /// joined the probe thread, `profiles::title` called it to compose the
    /// opening window's title, and `main` composed that title before it created
    /// the window.
    ///
    /// It leaves no state behind for the rest of the suite. `INSTALLATION` is
    /// never set — no test calls [`start`] — so `facts()` answers the empty
    /// installation here as it does everywhere else, and the ask this arms
    /// resolves to `None`, which is what an unasked machine already reads as.
    #[test]
    fn the_first_frame_does_not_wait_for_the_wsl_probe() {
        /// Long enough that only a read which never returns can reach it.
        const NEVER: Duration = Duration::from_secs(60);

        // A distribution that will not answer until this test lets go of the
        // sender, which it does when it ends.
        let (release, held) = mpsc::channel::<()>();
        begin_login_shell_probe_with(move || {
            let _ = held.recv();
            None
        });

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            // Both reads the launch makes: the facts themselves, and the profile
            // titles that the opening window's own title comes out of.
            let facts = facts();
            let _ = crate::profiles::title(0);
            let _ = tx.send((
                facts.integrated_login_shell().is_none(),
                facts.distributions,
            ));
        });
        let (no_shell, distributions) = rx.recv_timeout(NEVER).expect(
            "the launch's reads came back while the distribution is still out: a build that \
             joined the probe would be waiting still",
        );
        assert!(
            no_shell,
            "an unanswered distribution is not assumed to log into bash"
        );
        assert!(
            distributions.is_empty(),
            "no test starts the module, so the installation it reports is the empty one"
        );

        drop(release);
    }
}
