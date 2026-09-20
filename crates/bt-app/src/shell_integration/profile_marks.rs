//! The account's profile marks. Schema and extension contract:
//! `docs/shell-integration-marks.md`. All content decisions take injected bytes.

use super::*;
use crate::i18n::Text;
use serde::{Deserialize, Serialize};
use std::{fs, io};

pub const RECORD_FILE: &str = "integration-marks.json";
pub const LEGACY_LINE: &str = r#". "$env:APPDATA\Folio\shell-integration\folio.ps1""#;
pub const MANAGED_LINE: &str = r#"if (Test-Path -LiteralPath "$env:APPDATA\Folio\shell-integration\folio.ps1" -PathType Leaf) { . "$env:APPDATA\Folio\shell-integration\folio.ps1" } # Folio shell integration v1"#;

/// Exact spellings only. Literal legacy forms are generated from known script
/// locations, never parsed out of arbitrary user code mentioning folio.ps1.
pub struct Forms {
    legacy: Vec<String>,
}

impl Forms {
    pub fn new(scripts: &[PathBuf]) -> Self {
        let mut legacy = vec![
            LEGACY_LINE.to_owned(),
            r#". "$env:APPDATA\BetterTerminal\shell-integration\folio.ps1""#.to_owned(),
        ];
        for script in scripts {
            let literal = script.display().to_string();
            legacy.push(format!(". \"{literal}\""));
            let quoted = format!("'{}'", literal.replace('\'', "''"));
            legacy.push(format!(". {quoted}"));
        }
        Self { legacy }
    }

    pub fn owns(&self, line: &str) -> bool {
        let line = line.trim();
        line == MANAGED_LINE || self.legacy.iter().any(|known| known == line)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Migrate,
    Remove,
}

#[derive(Clone, Copy)]
enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
}

pub struct Decoded {
    pub text: String,
    encoding: Encoding,
}

impl Decoded {
    pub fn read(bytes: &[u8]) -> io::Result<Self> {
        let (text, encoding) = if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
            if body.len() % 2 != 0 {
                return Err(invalid_encoding());
            }
            let units: Vec<_> = body
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect();
            (
                String::from_utf16(&units).map_err(|_| invalid_encoding())?,
                Encoding::Utf16Le,
            )
        } else {
            let (body, encoding) = bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .map_or((bytes, Encoding::Utf8), |b| (b, Encoding::Utf8Bom));
            (
                std::str::from_utf8(body)
                    .map_err(|_| invalid_encoding())?
                    .to_owned(),
                encoding,
            )
        };
        // NUL in a BOM-less file is usually unmarked UTF-16. Never guess.
        if text.contains('\0') {
            return Err(invalid_encoding());
        }
        Ok(Self { text, encoding })
    }

    pub fn encode(&self, text: &str) -> Vec<u8> {
        match self.encoding {
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Utf8Bom => [b"\xef\xbb\xbf".as_slice(), text.as_bytes()].concat(),
            Encoding::Utf16Le => [
                vec![0xff, 0xfe],
                text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            ]
            .concat(),
        }
    }
}

fn invalid_encoding() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        Text::ShellProfileEncoding.text(),
    )
}

/// None means byte-identical. Each line keeps its own terminator; removal
/// consumes only the owned line's terminator, never a neighbouring blank line.
pub fn rewrite(bytes: &[u8], forms: &Forms, action: Action) -> io::Result<Option<Vec<u8>>> {
    let decoded = Decoded::read(bytes)?;
    let mut output = String::new();
    for raw in decoded.text.split_inclusive('\n') {
        let body = raw.strip_suffix('\n').unwrap_or(raw);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let trimmed = body.trim();
        if !forms.owns(trimmed) {
            output.push_str(raw);
            continue;
        }
        if action == Action::Remove {
            continue;
        }
        if !forms.legacy.iter().any(|line| line == trimmed) {
            output.push_str(raw);
            continue;
        }
        let leading = body.len() - body.trim_start().len();
        output.push_str(&body[..leading]);
        output.push_str(MANAGED_LINE);
        output.push_str(&body[body.trim_end().len()..]);
        output.push_str(&raw[body.len()..]);
    }
    let output = decoded.encode(&output);
    Ok((output != bytes).then_some(output))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRoots {
    pub claude: Vec<PathBuf>,
    pub codex: Vec<PathBuf>,
    pub copilot: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marks {
    pub version: u32,
    pub powershell_profiles: Vec<PathBuf>,
    pub powershell_scripts: Vec<PathBuf>,
    pub psreadline_module_roots: Vec<PathBuf>,
    pub agent_config_roots: AgentRoots,
    pub profile_refusals: Vec<Refusal>,
}

impl Default for Marks {
    fn default() -> Self {
        Self {
            version: 1,
            powershell_profiles: Vec::new(),
            powershell_scripts: Vec::new(),
            psreadline_module_roots: Vec::new(),
            agent_config_roots: AgentRoots::default(),
            profile_refusals: Vec::new(),
        }
    }
}

impl Marks {
    pub fn read(data: &Path) -> io::Result<Self> {
        let path = data.join(RECORD_FILE);
        super::refuse_profile_path(&path)?;
        match fs::read(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
            Ok(bytes) => {
                let marks: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                if marks.version != 1 {
                    return Err(io::Error::other(Text::ShellMarksVersion.text()));
                }
                if marks
                    .powershell_profiles
                    .iter()
                    .chain(&marks.powershell_scripts)
                    .chain(&marks.psreadline_module_roots)
                    .chain(&marks.agent_config_roots.claude)
                    .chain(&marks.agent_config_roots.codex)
                    .chain(&marks.agent_config_roots.copilot)
                    .chain(marks.profile_refusals.iter().map(|r| &r.path))
                    .any(|p| !p.is_absolute())
                {
                    return Err(io::Error::other(Text::ShellMarksPath.text()));
                }
                Ok(marks)
            }
        }
    }

    pub fn write(&self, data: &Path) -> io::Result<()> {
        let path = data.join(RECORD_FILE);
        super::refuse_profile_path(&path)?;
        fs::create_dir_all(data)?;
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        bt_persist::atomic_write(&path, &bytes).map_err(io::Error::other)
    }

    pub fn remember(&mut self, profile: &Path, script: &Path) {
        for (list, path) in [
            (&mut self.powershell_profiles, profile),
            (&mut self.powershell_scripts, script),
        ] {
            if !list.iter().any(|p| p == path) {
                list.push(path.to_path_buf());
            }
        }
    }
}

/// Hold across read/modify/write AND the corresponding profile operation.
/// OS lock is released on drop/crash; the empty lock file is not a mark.
pub fn lock(data: &Path) -> io::Result<fs::File> {
    fs::create_dir_all(data)?;
    let path = data.join("integration-marks.lock");
    super::refuse_profile_path(&path)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock().map_err(io::Error::other)?;
    Ok(file)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fate {
    Unchanged,
    Migrated,
    Removed,
    Refused(String),
}

#[derive(Clone, Debug)]
pub struct FileReport {
    pub path: PathBuf,
    pub fate: Fate,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub files: Vec<FileReport>,
}

impl Report {
    pub fn exit_code(&self) -> i32 {
        i32::from(
            self.files
                .iter()
                .any(|f| matches!(f.fate, Fate::Refused(_))),
        )
    }
    pub fn refusals(&self) -> Vec<Refusal> {
        self.files
            .iter()
            .filter_map(|f| match &f.fate {
                Fate::Refused(reason) => Some(Refusal {
                    path: f.path.clone(),
                    reason: reason.clone(),
                }),
                _ => None,
            })
            .collect()
    }
    pub fn text(&self, refused: bool) -> String {
        self.files
            .iter()
            .filter(|f| matches!(f.fate, Fate::Refused(_)) == refused)
            .map(|f| {
                let (label, reason) = match &f.fate {
                    Fate::Unchanged => (Text::ShellProfileUnchanged, ""),
                    Fate::Migrated => (Text::ShellProfileMigrated, ""),
                    Fate::Removed => (Text::ShellProfileRemoved, ""),
                    Fate::Refused(reason) => (Text::ShellProfileRefused, reason.as_str()),
                };
                format!(
                    "{}: {}{}{}\n",
                    label.text(),
                    f.path.display(),
                    if reason.is_empty() { "" } else { ": " },
                    reason
                )
            })
            .collect()
    }
}

/// Pure multi-file plan, including injected refusals. A refusal never hides
/// another file's decision. The applier below consumes these same decisions.
pub fn plan(
    inputs: Vec<(PathBuf, Result<Vec<u8>, String>)>,
    forms: &Forms,
    action: Action,
) -> Vec<(FileReport, Option<Vec<u8>>)> {
    inputs
        .into_iter()
        .map(|(path, input)| {
            let result =
                input.and_then(|bytes| rewrite(&bytes, forms, action).map_err(|e| e.to_string()));
            let (fate, bytes) = match result {
                Err(reason) => (Fate::Refused(reason), None),
                Ok(None) => (Fate::Unchanged, None),
                Ok(Some(bytes)) => (
                    if action == Action::Remove {
                        Fate::Removed
                    } else {
                        Fate::Migrated
                    },
                    Some(bytes),
                ),
            };
            (FileReport { path, fate }, bytes)
        })
        .collect()
}

pub fn apply(paths: &[PathBuf], forms: &Forms, action: Action) -> Report {
    let mut report = Report::default();
    for path in paths {
        let before = super::read_profile_for_edit(path);
        let input = before
            .as_ref()
            .map(|b| b.clone().unwrap_or_default())
            .map_err(ToString::to_string);
        let (mut file, replacement) = plan(vec![(path.clone(), input)], forms, action)
            .pop()
            .unwrap();
        if let Some(bytes) = replacement {
            let original = before.unwrap().unwrap_or_default();
            let result =
                super::replace_profile(path, &original, &bytes, std::time::SystemTime::now());
            if let Err(e) = result {
                file.fate = Fate::Refused(e.to_string());
            }
        }
        report.files.push(file);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_integration_rewrite_table_preserves_every_other_byte() {
        let forms = Forms::new(&[]);
        for encoding in [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le] {
            let codec = Decoded {
                text: String::new(),
                encoding,
            };
            for (prefix, suffix) in [
                ("", ""),
                ("", "\n"),
                ("", "\r\n"),
                ("", "\n# tail\r\n"),
                ("# café\r\n\n", "\n# tail\r\n"),
                ("# before\n", ""),
                ("\r\n", "\r\n\n# mixed\n"),
            ] {
                let legacy = format!("{prefix}  {LEGACY_LINE}\t{suffix}");
                let managed = format!("{prefix}  {}\t{suffix}", MANAGED_LINE);
                let before = codec.encode(&legacy);
                let after = rewrite(&before, &forms, Action::Migrate).unwrap().unwrap();
                assert_eq!(after, codec.encode(&managed));
                assert_eq!(rewrite(&after, &forms, Action::Migrate).unwrap(), None);
                let trailing = suffix
                    .strip_prefix("\r\n")
                    .or_else(|| suffix.strip_prefix('\n'))
                    .unwrap_or(suffix);
                let removed = rewrite(&after, &forms, Action::Remove).unwrap().unwrap();
                assert_eq!(removed, codec.encode(&format!("{prefix}{trailing}")));
                assert_eq!(rewrite(&removed, &forms, Action::Remove).unwrap(), None);
                assert_eq!(
                    rewrite(&before, &forms, Action::Remove).unwrap(),
                    Some(removed)
                );
            }
        }
    }

    #[test]
    fn shell_integration_user_lines_and_nonexact_forms_survive() {
        let forms = Forms::new(&[]);
        for line in [
            "# my note about folio.ps1",
            r". D:\tools\folio.ps1",
            r". 'D:\tools\folio.ps1'",
            r#"Write-Host 'folio.ps1'"#,
            ". \"$env:APPDATA\\Folio\\shell-integration\\folio.ps1\" # mine",
        ] {
            for action in [Action::Remove, Action::Migrate] {
                assert_eq!(
                    rewrite(line.as_bytes(), &forms, action).unwrap(),
                    None,
                    "{line}"
                );
            }
        }
    }

    #[test]
    fn shell_integration_partial_refusal_and_encoding_refusal_are_explicit() {
        let forms = Forms::new(&[]);
        let result = plan(
            vec![
                (PathBuf::from("first"), Ok(LEGACY_LINE.as_bytes().to_vec())),
                (PathBuf::from("second"), Err("locked".into())),
            ],
            &forms,
            Action::Remove,
        );
        assert_eq!(result[0].0.fate, Fate::Removed);
        assert_eq!(result[0].1, Some(Vec::new()));
        assert_eq!(result[1].0.fate, Fate::Refused("locked".into()));
        for bytes in [
            vec![0xfe, 0xff, 0, 65],
            vec![0xff],
            vec![0xff, 0xfe, 65],
            vec![0xff, 0xfe, 0, 0xd8],
            vec![65, 0],
        ] {
            assert!(rewrite(&bytes, &forms, Action::Remove).is_err());
        }
    }

    #[test]
    fn shell_integration_all_historical_literal_spellings_are_exact() {
        let path = PathBuf::from(r"D:\old data\Folio\shell-integration\folio.ps1");
        let forms = Forms::new(&[path]);
        for line in &forms.legacy {
            assert_eq!(
                rewrite(line.as_bytes(), &forms, Action::Migrate).unwrap(),
                Some(MANAGED_LINE.as_bytes().to_vec())
            );
        }
        assert!(!forms.owns(r#". "D:\someone else\folio.ps1""#));
    }
}
