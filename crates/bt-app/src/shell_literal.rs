//! Path insertion at a fresh argument boundary (paste-paths design §§2, 5.2).
//! The row configures an encoder; spelling supplies its string. Neither stage queries a live shell.

use std::path::{Path, PathBuf};

use bt_transcript::paths::PrintedPathNamespace;

use crate::i18n::Text;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellGrammar {
    PowerShell,
    Cmd,
    Posix,
    Fish,
    Nushell,
    Agent,
}

impl ShellGrammar {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "powershell" => Self::PowerShell,
            "cmd" => Self::Cmd,
            "posix" => Self::Posix,
            "fish" => Self::Fish,
            "nu" => Self::Nushell,
            "agent" => Self::Agent,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathSpelling {
    Windows,
    WindowsSlash,
    Wsl,
    Msys,
}

impl PathSpelling {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "windows" => Self::Windows,
            "windows-slash" => Self::WindowsSlash,
            "wsl" => Self::Wsl,
            "msys" => Self::Msys,
            // Cygwin belongs to T-PASTE-CYG, including explicit opt-in.
            _ => return None,
        })
    }
}

/// PROBE 2 may narrow this set only after both PowerShell versions were measured.
const POWERSHELL_QUOTES: [char; 5] = ['\'', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Encoder {
    pub grammar: ShellGrammar,
    /// Review 5 correction 3: CRT defaults do not inherit cmd.exe interpretation.
    pub named_cmd: bool,
    pub delayed_expansion: bool,
    /// Empty in production until PROBE 2; pure fixtures select measured policy explicitly.
    pub powershell_doubled_quotes: &'static [char],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    Encoding,
    Control,
    PowerShellQuote,
    DoubleQuote,
    CmdPercent,
    CmdDelayedExpansion,
    NushellUnmeasured,
}

impl Refusal {
    pub fn text(self) -> Text {
        match self {
            Self::Encoding => Text::PastePathEncoding,
            Self::Control => Text::PastePathControl,
            Self::PowerShellQuote => Text::PastePathPowerShellQuote,
            Self::DoubleQuote => Text::PastePathDoubleQuote,
            Self::CmdPercent => Text::PastePathCmdPercent,
            Self::CmdDelayedExpansion => Text::PastePathCmdExpansion,
            Self::NushellUnmeasured => Text::PastePathNushell,
        }
    }
}

pub fn representable(path: &Path) -> Result<&str, Refusal> {
    let text = path.to_str().ok_or(Refusal::Encoding)?;
    if text
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err(Refusal::Control);
    }
    Ok(text)
}

fn windows_path(text: &str) -> bool {
    let b = text.as_bytes();
    text.starts_with("\\\\")
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

fn posix(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

impl Encoder {
    pub fn literal(self, text: &str) -> Result<String, Refusal> {
        match self.grammar {
            ShellGrammar::PowerShell => {
                if text.chars().any(|c| {
                    POWERSHELL_QUOTES.contains(&c) && !self.powershell_doubled_quotes.contains(&c)
                }) {
                    return Err(Refusal::PowerShellQuote);
                }
                let mut quoted = String::from("'");
                for c in text.chars() {
                    quoted.push(c);
                    if POWERSHELL_QUOTES.contains(&c) {
                        quoted.push(c);
                    }
                }
                quoted.push('\'');
                Ok(quoted)
            }
            ShellGrammar::Cmd => {
                if text.contains('"') {
                    return Err(Refusal::DoubleQuote);
                }
                if self.named_cmd && text.contains('%') {
                    return Err(Refusal::CmdPercent);
                }
                if self.named_cmd && self.delayed_expansion && text.contains('!') {
                    return Err(Refusal::CmdDelayedExpansion);
                }
                let trailing = text.bytes().rev().take_while(|b| *b == b'\\').count();
                Ok(format!("\"{text}{}\"", "\\".repeat(trailing)))
            }
            ShellGrammar::Posix => Ok(posix(text)),
            ShellGrammar::Fish => Ok(format!(
                "'{}'",
                text.replace('\\', "\\\\").replace('\'', "\\'")
            )),
            ShellGrammar::Nushell => Err(Refusal::NushellUnmeasured),
            ShellGrammar::Agent if windows_path(text) => {
                if text.contains('"') {
                    return Err(Refusal::DoubleQuote);
                }
                Ok(format!("\"{text}\""))
            }
            ShellGrammar::Agent => Ok(posix(text)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Recipient {
    pub encoder: Encoder,
    pub namespace: PrintedPathNamespace,
    pub spelling: Option<PathSpelling>,
    /// Also populated for a non-WSL row with an explicit WSL spelling.
    pub wsl_distribution: Option<String>,
}

impl Recipient {
    pub fn literal(&self, path: &Path) -> Result<String, Refusal> {
        let host = representable(path)?;
        let spelled = match self.spelling {
            None => self.namespace.to_pane_spelling(host),
            Some(PathSpelling::Windows) => host.to_owned(),
            Some(PathSpelling::WindowsSlash) => host.replace('\\', "/"),
            Some(PathSpelling::Wsl) => PrintedPathNamespace::Wsl {
                distro: self.wsl_distribution.clone(),
                home: None,
            }
            .to_pane_spelling(host),
            Some(PathSpelling::Msys) => {
                PrintedPathNamespace::Msys { home: None }.to_pane_spelling(host)
            }
        };
        self.encoder.literal(&spelled)
    }
}

#[derive(Debug, Default)]
pub struct PathInsertion {
    pub text: String,
    pub refused: Vec<(PathBuf, Refusal)>,
}

pub fn paths_text(paths: &[PathBuf], recipient: &Recipient, leading_space: bool) -> PathInsertion {
    let mut result = PathInsertion::default();
    let mut literals = Vec::new();
    for path in paths {
        match recipient.literal(path) {
            Ok(literal) => literals.push(literal),
            Err(reason) => result.refused.push((path.clone(), reason)),
        }
    }
    if !literals.is_empty() {
        result.text = format!(
            "{}{} ",
            if leading_space { " " } else { "" },
            literals.join(" ")
        );
    }
    result
}

/// The name is displayed only in the toast, never logged. Debug spelling preserves invalid native
/// units and escapes controls, so reporting a refused name cannot insert a control or invent U+FFFD.
pub fn refusal_notice(refused: &[(PathBuf, Refusal)]) -> Option<String> {
    if refused.is_empty() {
        return None;
    }
    Some(
        refused
            .iter()
            .map(|(path, reason)| format!("{:?}: {}", path.as_os_str(), reason.text().text()))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// args belong to the row even when the grammar was explicitly named on a wrapper.
pub fn delayed_expansion(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg.eq_ignore_ascii_case("/v:on") || arg.eq_ignore_ascii_case("/v on"))
        || args
            .windows(2)
            .any(|pair| pair[0].eq_ignore_ascii_case("/v") && pair[1].eq_ignore_ascii_case("on"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoder(grammar: ShellGrammar) -> Encoder {
        Encoder {
            grammar,
            named_cmd: false,
            delayed_expansion: false,
            powershell_doubled_quotes: &[],
        }
    }
    fn recipient(grammar: ShellGrammar) -> Recipient {
        Recipient {
            encoder: encoder(grammar),
            namespace: PrintedPathNamespace::Windows,
            spelling: None,
            wsl_distribution: Some("Ubuntu".into()),
        }
    }

    #[test]
    fn representability_refuses_every_transport_control_and_line_terminator() {
        for c in (0..=0x9f)
            .filter_map(char::from_u32)
            .filter(|c| c.is_control())
            .chain(['\u{2028}', '\u{2029}'])
        {
            assert_eq!(
                representable(Path::new(&format!("/name{c}file"))),
                Err(Refusal::Control),
                "{c:?}"
            );
        }
        for text in ["/中文/📎", "/non\u{a0}breaking space", "/a’b", "/literal~"] {
            assert_eq!(representable(Path::new(text)), Ok(text));
        }
    }

    #[test]
    fn native_invalid_name_is_refused_without_a_lossy_transport() {
        #[cfg(windows)]
        let invalid = {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(&[0x44, 0x3a, 0x5c, 0xd800])
        };
        #[cfg(unix)]
        let invalid = {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(b"/bad\xff".to_vec())
        };
        assert_eq!(representable(Path::new(&invalid)), Err(Refusal::Encoding));
    }

    const NAMES: &[&str] = &[
        " ",
        "'",
        "a' b",
        "\\",
        "\\\\",
        "a\\' b",
        "\"",
        "$",
        "`",
        "%",
        "%NAME%",
        "!",
        "#",
        "&",
        ";",
        "(",
        "[",
        "~",
        "~a/b~",
        "中文",
        "📎",
        "non\u{a0}breaking",
        "a‘b",
        "a’b",
        "a‚b",
        "a‛b",
        "D:\\",
    ];

    #[test]
    fn posix_and_agent_posix_are_one_independently_lexed_token() {
        for name in NAMES {
            let path = format!("/folder/{name}");
            for grammar in [ShellGrammar::Posix, ShellGrammar::Agent] {
                let literal = encoder(grammar).literal(&path).unwrap();
                assert_eq!(
                    shlex::split(&literal),
                    Some(vec![path.clone()]),
                    "{grammar:?}: {path:?}"
                );
            }
        }
    }

    #[test]
    fn powershell_unrun_refuses_quote_class_and_measured_fixture_doubles_it() {
        for c in POWERSHELL_QUOTES {
            let path = format!("/a{c};Write-Host PASTE_PROBE;#.txt");
            assert_eq!(
                encoder(ShellGrammar::PowerShell).literal(&path),
                Err(Refusal::PowerShellQuote)
            );
            let measured = Encoder {
                powershell_doubled_quotes: &POWERSHELL_QUOTES,
                ..encoder(ShellGrammar::PowerShell)
            };
            assert_eq!(
                measured.literal(&path).unwrap(),
                format!("'/a{c}{c};Write-Host PASTE_PROBE;#.txt'")
            );
        }
        for name in NAMES
            .iter()
            .filter(|name| !name.chars().any(|c| POWERSHELL_QUOTES.contains(&c)))
        {
            assert_eq!(
                encoder(ShellGrammar::PowerShell).literal(name).unwrap(),
                format!("'{name}'")
            );
        }
    }

    #[test]
    fn cmd_origin_and_delayed_expansion_are_policy_not_path_properties() {
        let crt = encoder(ShellGrammar::Cmd);
        let named = Encoder {
            named_cmd: true,
            ..crt
        };
        let delayed = Encoder {
            delayed_expansion: true,
            ..named
        };
        for path in ["%", r"D:\Data\%NAME%\r.csv", r"D:\Data\100%\r.csv"] {
            assert_eq!(crt.literal(path).unwrap(), format!("\"{path}\""));
            assert_eq!(named.literal(path), Err(Refusal::CmdPercent));
        }
        for path in ["!", r"D:\!NAME!\a"] {
            assert!(crt.literal(path).is_ok());
            assert!(
                Encoder {
                    delayed_expansion: true,
                    ..crt
                }
                .literal(path)
                .is_ok()
            );
            assert!(named.literal(path).is_ok());
            assert_eq!(delayed.literal(path), Err(Refusal::CmdDelayedExpansion));
        }
        for n in 1..=3 {
            let path = format!("D:\\root{}", "\\".repeat(n));
            assert_eq!(
                crt.literal(&path).unwrap(),
                format!("\"D:\\root{}\"", "\\".repeat(2 * n))
            );
        }
        assert_eq!(named.literal("a^b").unwrap(), "\"a^b\"");
        for policy in [crt, named, delayed] {
            assert_eq!(policy.literal("a\"b"), Err(Refusal::DoubleQuote));
        }
        for args in [
            vec!["/v:on"],
            vec!["/V:ON"],
            vec!["/v", "on"],
            vec!["/v on"],
        ] {
            assert!(delayed_expansion(
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
            ));
        }
        assert!(!delayed_expansion(&["/v:off".into()]));
    }

    /// Native acceptance, not a shell paste experiment. Build the defined MSVC wmain fixture
    /// separately and name its absolute path in BT_PASTE_CRT_CONSUMER. Command on Windows calls
    /// CreateProcessW with the program token first; raw_arg appends our literal without re-quoting
    /// it. This preserves review 5 correction 4's argv[0]/argv[1] distinction.
    #[cfg(windows)]
    #[test]
    #[ignore = "requires the separately compiled MSVC CRT consumer and coordinator native acceptance lane"]
    fn direct_crt_receives_the_exact_literal_after_the_program_token() {
        use std::os::windows::process::CommandExt;
        let consumer = std::env::var_os("BT_PASTE_CRT_CONSUMER")
            .expect("set BT_PASTE_CRT_CONSUMER to the precompiled paste_paths_crt.exe fixture");
        let consumer = PathBuf::from(consumer).canonicalize().unwrap();
        let mut paths = vec![
            r"D:\space here\a.txt".to_owned(),
            r"D:\John's Archive\a.txt".to_owned(),
            r"D:\Data\100%\r.csv".to_owned(),
            r"D:\Data\%NAME%\r.csv".to_owned(),
            r"D:\中文\📎.txt".to_owned(),
        ];
        paths.extend((1..=3).map(|n| format!("D:\\root{}", "\\".repeat(n))));
        let crt = encoder(ShellGrammar::Cmd);
        for path in paths {
            let literal = crt.literal(&path).unwrap();
            let output = std::process::Command::new(&consumer)
                .raw_arg(&literal)
                // CREATE_NO_WINDOW: output is captured through pipes, never a console window.
                .creation_flags(0x0800_0000)
                .output()
                .unwrap();
            assert!(output.status.success(), "{path:?}: {:?}", output.status);
            let stdout = String::from_utf8(output.stdout).unwrap();
            let expected_units = path
                .encode_utf16()
                .map(|unit| format!("{unit:04X}"))
                .collect::<String>();
            assert_eq!(
                stdout.lines().collect::<Vec<_>>(),
                ["argc=2".to_owned(), format!("argv[1]={expected_units}")],
                "{path:?} -> {literal:?}"
            );
        }
    }

    #[test]
    fn cmd_and_agent_windows_cover_the_punctuation_and_unicode_table() {
        let crt = encoder(ShellGrammar::Cmd);
        let cmd = Encoder {
            named_cmd: true,
            ..crt
        };
        let agent = encoder(ShellGrammar::Agent);
        // A final filename suffix leaves the backslashes inside the token. The independent
        // root/trailing-backslash fixtures cover the distinct closing-quote boundary.
        for name in NAMES {
            let path = format!("D:\\folder\\{name}.txt");
            let expected = format!("\"{path}\"");
            if name.contains('"') {
                for policy in [crt, cmd, agent] {
                    assert_eq!(policy.literal(&path), Err(Refusal::DoubleQuote));
                }
            } else {
                assert_eq!(crt.literal(&path), Ok(expected.clone()));
                assert_eq!(agent.literal(&path), Ok(expected.clone()));
                if name.contains('%') {
                    assert_eq!(cmd.literal(&path), Err(Refusal::CmdPercent));
                } else {
                    assert_eq!(cmd.literal(&path), Ok(expected));
                }
            }
        }
    }

    #[test]
    fn fish_escapes_backslashes_and_apostrophes_together() {
        assert_eq!(
            encoder(ShellGrammar::Fish).literal("a\\\\' b\\").unwrap(),
            "'a\\\\\\\\\\' b\\\\'"
        );
        for name in NAMES {
            let literal = encoder(ShellGrammar::Fish).literal(name).unwrap();
            // Independent reader of fish's two escapes inside a single-quoted token.
            let mut chars = literal[1..literal.len() - 1].chars();
            let mut decoded = String::new();
            while let Some(c) = chars.next() {
                decoded.push(if c == '\\' { chars.next().unwrap() } else { c });
            }
            assert_eq!(&decoded, name);
        }
    }

    #[test]
    fn nushell_has_no_enabled_literal_before_a_measured_baseline() {
        for path in NAMES {
            assert_eq!(
                encoder(ShellGrammar::Nushell).literal(path),
                Err(Refusal::NushellUnmeasured)
            );
        }
        assert_eq!(
            encoder(ShellGrammar::Nushell).literal("a'###b"),
            Err(Refusal::NushellUnmeasured)
        );
    }

    // Models the pinned recipient's branch order; Windows outputs are deliberately not shlex inputs.
    fn agent_read(pasted: &str) -> Option<String> {
        let unquoted = pasted
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| pasted.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(pasted);
        if unquoted.starts_with("file:") {
            return bt_platform::path_from_file_url(unquoted)
                .ok()?
                .to_str()
                .map(str::to_owned);
        }
        if windows_path(unquoted) {
            return Some(unquoted.to_owned());
        }
        let parts = shlex::split(pasted)?;
        (parts.len() == 1).then(|| parts[0].clone())
    }

    #[test]
    fn agent_windows_roots_apostrophe_and_multiple_paths_follow_recipient_order() {
        for path in ["C:\\", "\\\\server\\share\\", "C:\\a' b.png", "D:/a b.png"] {
            let literal = encoder(ShellGrammar::Agent).literal(path).unwrap();
            assert_eq!(agent_read(&literal).as_deref(), Some(path));
        }
        assert_eq!(agent_read("'file:///a%20b'"), Some("/a b".into()));
        let multiple = "\"C:\\a.png\" \"C:\\b.png\"";
        assert_eq!(agent_read(multiple), Some("C:\\a.png\" \"C:\\b.png".into()));
        assert!(agent_read("'/a.png' '/b.png'").is_none());
    }

    #[test]
    fn ordered_batch_envelope_skips_bad_entries_and_reports_their_names_once() {
        let paths = vec![
            "/one".into(),
            "/bad\nname".into(),
            "/three space".into(),
            "/four".into(),
        ];
        for leading in [false, true] {
            let result = paths_text(&paths, &recipient(ShellGrammar::Posix), leading);
            assert_eq!(
                result.text,
                format!(
                    "{}'/one' '/three space' '/four' ",
                    if leading { " " } else { "" }
                )
            );
            assert_eq!(result.refused.len(), 1);
            let notice = refusal_notice(&result.refused).unwrap();
            assert!(notice.contains("bad\\nname"));
            assert!(!notice.contains('\n'));
            assert_eq!(
                crate::input::paste_bytes(&result.text, false),
                result.text.as_bytes()
            );
        }
        assert!(
            paths_text(
                &["/bad\tname".into()],
                &recipient(ShellGrammar::Posix),
                true
            )
            .text
            .is_empty()
        );
    }

    #[cfg(windows)]
    #[test]
    fn all_enabled_grammar_spelling_cells_encode_the_spelled_string() {
        let mut row = recipient(ShellGrammar::Posix);
        for (spelling, path) in [
            (PathSpelling::Windows, r"D:\Demo space\a.txt"),
            (PathSpelling::WindowsSlash, "D:/Demo space/a.txt"),
            (PathSpelling::Wsl, "/mnt/d/Demo space/a.txt"),
            (PathSpelling::Msys, "/d/Demo space/a.txt"),
        ] {
            row.spelling = Some(spelling);
            for grammar in [
                ShellGrammar::PowerShell,
                ShellGrammar::Cmd,
                ShellGrammar::Posix,
                ShellGrammar::Fish,
                ShellGrammar::Nushell,
                ShellGrammar::Agent,
            ] {
                row.encoder = encoder(grammar);
                let expected = match grammar {
                    ShellGrammar::Nushell => Err(Refusal::NushellUnmeasured),
                    ShellGrammar::Cmd => Ok(format!("\"{path}\"")),
                    ShellGrammar::Agent
                        if matches!(
                            spelling,
                            PathSpelling::Windows | PathSpelling::WindowsSlash
                        ) =>
                    {
                        Ok(format!("\"{path}\""))
                    }
                    ShellGrammar::Fish => Ok(format!("'{}'", path.replace('\\', "\\\\"))),
                    _ => Ok(format!("'{path}'")),
                };
                assert_eq!(
                    row.literal(Path::new(r"D:\Demo space\a.txt")),
                    expected,
                    "{grammar:?} × {spelling:?}"
                );
            }
        }
        assert_eq!(PathSpelling::from_name("cygwin"), None);
    }

    #[cfg(windows)]
    #[test]
    fn grammar_spelling_product_covers_fallback_and_probe_policy() {
        let mut row = recipient(ShellGrammar::PowerShell);
        row.spelling = Some(PathSpelling::Wsl);
        let apostrophe = Path::new(r"D:\John's Archive\a.txt");
        assert_eq!(row.literal(apostrophe), Err(Refusal::PowerShellQuote));
        row.encoder.powershell_doubled_quotes = &['\''];
        assert_eq!(
            row.literal(apostrophe).unwrap(),
            "'/mnt/d/John''s Archive/a.txt'"
        );
        row.encoder = encoder(ShellGrammar::Cmd);
        for (spelling, expected) in [
            (PathSpelling::Windows, "\"D:\\\\\""),
            (PathSpelling::WindowsSlash, "\"D:/\""),
            (PathSpelling::Wsl, "\"/mnt/d/\""),
            (PathSpelling::Msys, "\"/d/\""),
        ] {
            row.spelling = Some(spelling);
            assert_eq!(row.literal(Path::new("D:\\")).unwrap(), expected);
        }
        row.encoder = encoder(ShellGrammar::Agent);
        row.spelling = Some(PathSpelling::Windows);
        assert_eq!(row.literal(Path::new("D:\\")).unwrap(), "\"D:\\\"");
        row.spelling = Some(PathSpelling::Wsl);
        assert_eq!(
            row.literal(Path::new(r"\\server\share\a.txt")).unwrap(),
            r#""\\server\share\a.txt""#
        );
        let quoted = Path::new(r#"\\wsl.localhost\Ubuntu\home\ann\a"b.txt"#);
        for spelling in [PathSpelling::Msys, PathSpelling::Wsl] {
            row.spelling = Some(spelling);
            for grammar in [ShellGrammar::Posix, ShellGrammar::Cmd, ShellGrammar::Agent] {
                row.encoder = encoder(grammar);
                let result = row.literal(quoted);
                assert_eq!(
                    result.is_ok(),
                    grammar == ShellGrammar::Posix
                        || (grammar == ShellGrammar::Agent && spelling == PathSpelling::Wsl)
                );
            }
        }
        row.spelling = Some(PathSpelling::Wsl);
        row.wsl_distribution = None;
        row.encoder = encoder(ShellGrammar::Agent);
        assert_eq!(row.literal(quoted), Err(Refusal::DoubleQuote));
        assert_eq!(
            row.literal(Path::new(r"D:\Demo\a.txt")).unwrap(),
            "'/mnt/d/Demo/a.txt'"
        );
    }
}
