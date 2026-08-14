//! What this machine's WSL installation says about itself.
//!
//! Two questions, both of which only `wsl.exe` can answer and neither of which
//! this product may assume:
//!
//! * **which distribution the `WSL` profile actually starts**, so that the
//!   picker can name it when there is more than one to choose between. `wsl.exe`
//!   with no arguments starts the user's *default* distribution, which on this
//!   machine is Ubuntu and on the next is Debian or Alpine — printing one over a
//!   command that will start the other is chrome saying something it did not
//!   check;
//! * **which shell that distribution logs the user into**, because the
//!   integration script is a bash script and handing `--init-file` to a shell
//!   that is not bash would replace the user's shell with one they did not
//!   choose. Asking is the difference between an integration and a substitution.
//!
//! Both are probed once, off the main thread, and only on a machine that has
//! `wsl.exe` at all — see [`WslProbe`].

use std::{
    ffi::OsStr,
    path::Path,
    process::Command,
    sync::{Mutex, OnceLock},
    thread::JoinHandle,
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

/// What one probe of `wsl.exe` learned. Empty is a valid, meaningful answer: a
/// machine with `wsl.exe` present but no distribution installed reaches exactly
/// this, and so does one whose `wsl.exe` refused to answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WslFacts {
    /// Every installed distribution, in the order `wsl.exe` lists them.
    distributions: Vec<String>,
    /// The one `wsl.exe` starts when nothing names another, and the shell it
    /// logs into.
    default: Option<DefaultDistribution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DefaultDistribution {
    name: String,
    /// The login shell out of `/etc/passwd`, or `None` when the distribution
    /// could not be asked.
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
    /// `None` covers three different machines with one answer, and the answer is
    /// the same for all three because the consequence is: no distribution, a
    /// distribution that would not answer, and a distribution whose user logs
    /// into zsh. In each case the profile starts `wsl.exe` exactly as it did
    /// before this module existed.
    #[must_use]
    pub fn integrated_login_shell(&self) -> Option<&str> {
        let shell = self.default.as_ref()?.login_shell.as_deref()?;
        (Path::new(shell).file_name()? == OsStr::new(INTEGRATED_SHELL)).then_some(shell)
    }
}

/// The list `wsl --list --verbose` prints, decoded.
///
/// `--verbose` rather than `--quiet`, and the extra column is the whole reason:
/// `-q` prints names and nothing else, so the **default** — the one this profile
/// will actually start — is not identifiable from it. `-v` marks that row with a
/// `*` in the first column, which is a glyph rather than a word and so is the
/// one part of this table that is not translated on a non-English Windows.
///
/// The output is UTF-16LE, as every `wsl.exe` control command's is, and decoding
/// it as UTF-8 yields a string of interleaved NULs that matches no distribution
/// name — a failure that looks like "you have no distributions" rather than like
/// a decoding bug.
///
/// The first line is the header and is dropped positionally, because that is the
/// only property of it that is not localised: on a Chinese Windows it reads
/// `名称   状态   版本`, which has the same shape as a distribution row and
/// cannot be told from one by its content.
fn parse_distributions(utf16le: &[u8]) -> WslFacts {
    let text: String = char::decode_utf16(
        utf16le
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
    )
    .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
    .collect();
    let mut facts = WslFacts::default();
    for line in text.lines().skip(1) {
        let is_default = line.starts_with('*');
        let Some(name) = line.trim_start_matches('*').split_whitespace().next() else {
            continue;
        };
        if is_default {
            facts.default = Some(DefaultDistribution {
                name: name.to_owned(),
                login_shell: None,
            });
        }
        facts.distributions.push(name.to_owned());
    }
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

fn probe(program: &OsStr) -> WslFacts {
    let Ok(listed) = Command::new(program).args(["--list", "--verbose"]).output() else {
        return WslFacts::default();
    };
    if !listed.status.success() {
        return WslFacts::default();
    }
    let mut facts = parse_distributions(&listed.stdout);
    let Some(default) = facts.default.as_mut() else {
        return facts;
    };
    // Asked of the distribution rather than of the registry, because the answer
    // is a fact about a Linux user account and the only thing that holds it is
    // the distribution's own password database.
    if let Ok(shell) = Command::new(program)
        .args([
            "-e",
            "sh",
            "-c",
            r#"getent passwd "$(id -u)" | cut -d: -f7"#,
        ])
        .output()
        && shell.status.success()
    {
        default.login_shell = parse_login_shell(&shell.stdout);
    }
    facts
}

/// The probe, started at launch and read whenever the answer is first wanted.
///
/// Off the main thread because it is two `wsl.exe` invocations — roughly 200ms
/// on the machine this was written on, against a `ProfilePrograms` probe next to
/// it that costs microseconds. On the thread, it would be 200ms of a window that
/// is not on screen yet; lazily on first read, it would be 200ms of a menu that
/// has already been clicked open. Started early and joined late, it is neither:
/// by the time any chrome asks, the answer has been sitting there for the whole
/// of window creation.
///
/// A machine with no `wsl.exe` starts no thread and asks nothing.
#[derive(Debug, Default)]
struct WslProbe {
    answer: OnceLock<WslFacts>,
    pending: Mutex<Option<JoinHandle<WslFacts>>>,
}

impl WslProbe {
    fn facts(&self) -> &WslFacts {
        if let Some(facts) = self.answer.get() {
            return facts;
        }
        let joined = self
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.take())
            .and_then(|handle| handle.join().ok());
        if let Some(joined) = joined {
            let _ = self.answer.set(joined);
        }
        self.answer.get_or_init(WslFacts::default)
    }
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
static PROBE: OnceLock<WslProbe> = OnceLock::new();

/// Begin asking, once. `program` is what [`crate::profiles::ProfilePrograms`]
/// resolved the WSL profile to, so a machine without WSL is one that never asks.
pub fn start(program: Option<&OsStr>) {
    let probe = match program.map(OsStr::to_os_string) {
        Some(program) => WslProbe {
            answer: OnceLock::new(),
            pending: Mutex::new(Some(std::thread::spawn(move || probe(&program)))),
        },
        None => {
            let idle = WslProbe::default();
            let _ = idle.answer.set(WslFacts::default());
            idle
        }
    };
    let _ = PROBE.set(probe);
}

/// What the machine said, waiting for the probe if it is still running and
/// answering "nothing" if it was never started.
pub fn facts() -> &'static WslFacts {
    static NEVER_ASKED: OnceLock<WslFacts> = OnceLock::new();
    PROBE.get().map_or_else(
        || NEVER_ASKED.get_or_init(WslFacts::default),
        WslProbe::facts,
    )
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

    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// PIN — the list `wsl.exe` prints, read the way `wsl.exe` writes it.
    ///
    /// Red gate, and it is the one this ticket was warned about by name: read the
    /// same bytes as UTF-8 and every name comes back as `U\0b\0u\0…`, which
    /// matches nothing, so the qualifier silently never appears and the failure
    /// looks exactly like a machine with one distribution.
    #[test]
    fn the_distribution_list_is_read_as_the_utf16_wsl_writes() {
        let facts = parse_distributions(&utf16le(concat!(
            "  NAME              STATE           VERSION\r\n",
            "* Ubuntu-24.04      Stopped         2\r\n",
            "  Ubuntu-22.04      Stopped         2\r\n",
            "  docker-desktop    Stopped         2\r\n"
        )));
        assert_eq!(
            facts.distributions,
            ["Ubuntu-24.04", "Ubuntu-22.04", "docker-desktop"],
            "every installed distribution, in the order it was listed"
        );
        assert_eq!(
            facts.title_qualifier(),
            Some("Ubuntu-24.04"),
            "the `*` row is the one `wsl.exe` with no arguments will start"
        );
        // The header is dropped by position and not by its words, so a Windows
        // that translates it does not gain a distribution named `名称`.
        let localised = parse_distributions(&utf16le(concat!(
            "  名称            状态            版本\r\n",
            "* Ubuntu          Stopped         2\r\n",
            "  Debian          Stopped         2\r\n"
        )));
        assert_eq!(localised.distributions, ["Ubuntu", "Debian"]);
        assert_eq!(localised.title_qualifier(), Some("Ubuntu"));
    }

    /// PIN — the qualifier appears exactly when it is answering a question.
    #[test]
    fn one_distribution_needs_no_qualifier_and_none_at_all_needs_no_wsl() {
        let one = parse_distributions(&utf16le(
            "  NAME    STATE     VERSION\r\n* Ubuntu  Stopped   2\r\n",
        ));
        assert_eq!(one.distributions, ["Ubuntu"]);
        assert_eq!(
            one.title_qualifier(),
            None,
            "`WSL` already names it uniquely; the name would only take up room"
        );
        assert_eq!(WslFacts::default().title_qualifier(), None);
        // A machine whose `wsl.exe` lists distributions but marks none of them
        // as default cannot say which one the profile starts, so it says
        // nothing — the bare title is the honest one.
        let unmarked = parse_distributions(&utf16le(
            "  NAME    STATE     VERSION\r\n  Ubuntu  Stopped   2\r\n  Debian  Stopped   2\r\n",
        ));
        assert_eq!(unmarked.distributions.len(), 2);
        assert_eq!(unmarked.title_qualifier(), None);
    }

    /// PIN — an init file is offered to bash and to nothing else.
    ///
    /// Red gate: handing `--init-file` to whatever shell the distribution logs
    /// into replaces a zsh user's shell with bash every time they open a tab,
    /// and the symptom — "my prompt is gone" — names neither this terminal nor
    /// the flag that did it.
    #[test]
    fn only_a_distribution_that_logs_into_bash_is_handed_an_init_file() {
        let mut facts = parse_distributions(&utf16le(
            "  NAME    STATE     VERSION\r\n* Ubuntu  Stopped   2\r\n",
        ));
        assert_eq!(
            facts.integrated_login_shell(),
            None,
            "a distribution that was never asked is not assumed to run bash"
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
}
