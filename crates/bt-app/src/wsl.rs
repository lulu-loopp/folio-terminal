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
//! first question is answered here, synchronously, out of `HKCU\…\Lxss`.
//!
//! **The second question is not asked here at all any more** (2026-09-07). It
//! was, from the pane spawn and with nothing waiting for the answer — which is
//! precisely why the *first* WSL pane of every run was started without an init
//! file: it composed its command line before the answer existed
//! (`docs/plans/shell-matrix-2026-09-07.md` T-2, §7.40 ③'s booked cost). A
//! question about a Linux user account is now put by the pane that needs the
//! answer, inside the distribution it is about, in the same command line as the
//! shell it decides — see `shell_integration::WSL_LOGIN_SHELL`. So this module
//! reads the registry and starts nothing, which is a stronger form of the same
//! ruling: a machine that merely has WSL installed boots no virtual machine for
//! Folio, and neither does one whose reader opens a WSL pane.

use std::{ffi::OsStr, sync::OnceLock};

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
    /// The one `wsl.exe` starts when nothing names another.
    default: Option<DefaultDistribution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DefaultDistribution {
    name: String,
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

    /// The distribution `wsl.exe` starts when nothing names another — **whether or not there is a
    /// choice to disambiguate**, which is the whole difference between this and
    /// [`Self::title_qualifier`].
    ///
    /// A title says a name only when the name is load-bearing for a reader. A path needs the name
    /// whenever it needs one at all: `/etc/hosts` in the one and only installed distribution is
    /// `\\wsl.localhost\<that one>\etc\hosts`, and a window that declined to say which distribution
    /// it meant would have no share to open (`profiles::printed_path_namespace`, §7.30
    /// 2026-09-07).
    #[must_use]
    pub fn default_distribution(&self) -> Option<&str> {
        self.default.as_ref().map(|default| default.name.as_str())
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
            facts.default = Some(DefaultDistribution { name: name.clone() });
        }
        facts.distributions.push(name);
    }
    facts.distributions.sort();
    facts
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
    let installation = if program.is_some() {
        read_installation(&CurrentUser)
    } else {
        WslFacts::default()
    };
    let _ = INSTALLATION.set(installation);
}

/// What the machine says, right now, without waiting for anything.
///
/// **There is no blocking twin to tell this apart from** (§7.40 ④), which is why
/// it is not called `try_facts`: the installation is already in hand — it was
/// read from the registry before the window existed — and there is no second
/// half that might not have arrived. Nothing in this module can make a caller
/// wait, and since 2026-09-07 nothing in it can start a process either.
///
/// Owned rather than borrowed because it is a handful of short strings, read
/// once per rebuild of the profile titles, and every reader of it is a place a
/// profile is *named*.
#[must_use]
pub fn facts() -> WslFacts {
    INSTALLATION.get().cloned().unwrap_or_default()
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

    /// RED — **this module boots no distribution, on any path.**
    ///
    /// The structural half of the 2026-09-07 fix, and the reason it is a source
    /// gate rather than an assertion about behaviour: the defect it retires was
    /// not a wrong answer but a *timing* — a `wsl.exe` started beside the pane
    /// spawn whose answer arrived after the command line it was for had already
    /// gone out (`docs/plans/shell-matrix-2026-09-07.md` T-2). Any repair that
    /// keeps a process here keeps the race, whether it is waited for (a frame
    /// that stops for a virtual machine, §7.40 ②④) or not (the first pane goes
    /// out unintegrated again). Neither failure is visible from a unit test of
    /// the answer, and both are visible here.
    ///
    /// MUTATION: put `ask_login_shell` back, however it is spelled, and this is
    /// red on the word that starts the child.
    ///
    /// The needles are split with `concat!` — the same trick
    /// `bt_platform::quiet_door_tests` uses — because a test that spelled them
    /// whole would be the thing it is looking for.
    #[test]
    fn nothing_in_this_module_boots_a_distribution() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/wsl.rs");
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} is in the repository: {error}", path.display()));
        for needle in [
            concat!("quiet_", "command"),
            concat!("Command", "::new"),
            concat!("spawn_at_", "priority"),
        ] {
            assert!(
                !source.contains(needle),
                "a fact about a Linux user account is asked by the pane that needs it, inside \
                 the distribution, not by a second process started from here: {needle}"
            );
        }
    }

    /// RED — **nothing on the way to a frame waits for the distribution.**
    ///
    /// Red gate for §7.40 ④, and it is written the way
    /// `git::tests::a_child_that_will_not_finish_is_killed_and_reported` is,
    /// for that test's reason: the failure this guards is *never returns*, not
    /// *slow*, so the read happens on a thread of its own and the answer comes
    /// back down a channel. A build that joined a probe would fail this in a
    /// minute instead of hanging the suite for ever. **The minute is not a
    /// budget** — it is the difference between "came back" and "did not", which
    /// is the only difference this test is about.
    ///
    /// It reads what the launch reads — the facts themselves, the profile titles
    /// the opening window's own title comes out of — and what the *spawn* reads,
    /// which is the WSL profile's whole command line. That third read is new
    /// with the 2026-09-07 fix: the command line is now composed without
    /// consulting anything that could be outstanding, and a repair that put a
    /// blocking ask back on the spawn path would be caught here rather than in
    /// the real window.
    ///
    /// It leaves no state behind for the rest of the suite: `INSTALLATION` is
    /// never set — no test calls [`start`] — so `facts()` answers the empty
    /// installation here as it does everywhere else.
    #[test]
    fn the_first_frame_and_the_first_pane_do_not_wait_for_a_distribution() {
        /// Long enough that only a read which never returns can reach it.
        const NEVER: Duration = Duration::from_secs(60);

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let facts = facts();
            let _ = crate::profiles::title(0);
            let command = crate::shell_integration::shell_command(
                &crate::profiles::row(crate::profiles::index_of_id("wsl"))
                    .expect("the shipped WSL row"),
                &[std::ffi::OsString::from("--cd"), "~".into()],
                crate::shell_integration::Scripts {
                    bash: Some(std::path::Path::new(r"C:\Folio\folio.bash")),
                    zdotdir: Some(std::path::Path::new(r"C:\Folio\zdotdir")),
                },
                &bt_pty::SystemShellEnvironment,
            );
            let _ = tx.send((facts.distributions, command.arguments.len()));
        });
        let (distributions, arguments) = rx.recv_timeout(NEVER).expect(
            "the launch's reads and the WSL spawn's own command line came back without a \
             distribution having been asked anything",
        );
        assert!(
            distributions.is_empty(),
            "no test starts the module, so the installation it reports is the empty one"
        );
        assert_eq!(
            arguments, 9,
            "`--cd ~ -e sh -c <question> folio <script> <zdotdir>`: the place, then the question \
             the pane puts to its own distribution, then a door for each shell that has one"
        );
    }
}
