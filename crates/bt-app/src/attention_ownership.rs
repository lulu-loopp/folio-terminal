//! Shared policy for per-copy agent marks. Adapters alone decode their schemas.
use crate::shell_integration::profile_marks::{self, Marks};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Install,
    Remove,
    /// Consent applies only to the owners shown by the preceding refusal.
    TakeOver(Vec<PathBuf>),
}

impl Decision {
    pub fn installs(&self) -> bool {
        !matches!(self, Self::Remove)
    }
}

impl From<bool> for Decision {
    fn from(install: bool) -> Self {
        if install { Self::Install } else { Self::Remove }
    }
}

pub(crate) struct Pending {
    config: PathBuf,
    owners: Vec<PathBuf>,
    shown: std::time::Instant,
}

impl Pending {
    pub fn new(config: Option<PathBuf>, owners: Vec<PathBuf>) -> Option<Self> {
        Some(Self {
            config: config?,
            owners,
            shown: std::time::Instant::now(),
        })
    }
}

pub(crate) fn next_decision(
    pending: &mut Option<Pending>,
    install: bool,
    config: Option<&Path>,
) -> Decision {
    let consent = pending.take();
    if install
        && let Some(consent) = consent
        && config == Some(consent.config.as_path())
        && consent.shown.elapsed() < std::time::Duration::from_secs(30)
    {
        return Decision::TakeOver(consent.owners);
    }
    install.into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Installed,
    Removed,
    Unchanged,
    TakeOverRequired(Vec<PathBuf>),
    LeftOther(Vec<PathBuf>),
    Refused(&'static str),
}

/// The single gate for writing per-copy marks; no PATH-relative fallback.
pub(crate) fn stable_executable(exe: Option<&Path>) -> Result<&Path, &'static str> {
    let exe = exe.ok_or(crate::i18n::Text::AgentHooksExeUnknown.text())?;
    // **Before the absolute-path gate**, so that the quarantine is named on every platform rather
    // than only where macOS's spelling of a path happens to satisfy `is_absolute`.
    if translocated(exe) {
        return Err(crate::i18n::Text::AgentHooksTranslocated.text());
    }
    literal_absolute_path(exe)?;
    Ok(exe)
}

/// **Whether a path stands in macOS's App Translocation quarantine.**
///
/// The `AppTranslocation` component is the fact and the leading directory was a spelling:
/// `current_exe()` on macOS hands back `_NSGetExecutablePath`'s string uncanonicalised, and `/var`
/// is a symbolic link to `/private/var`, so the same executable is reported as
/// `/private/var/folders/…/AppTranslocation/…` or `/var/folders/…/AppTranslocation/…`. A prefix
/// test passes the second one through and installs hooks naming a copy macOS deletes when the
/// quarantine is released (closure review R7).
///
/// A pure predicate over a path, so the rule is testable on machines that have no such directory.
#[must_use]
pub(crate) fn translocated(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "AppTranslocation")
}

fn literal_absolute_path(path: &Path) -> Result<&str, &'static str> {
    let text = path
        .to_str()
        .ok_or(crate::i18n::Text::AgentHooksExeUnstable.text())?;
    // Claude exec form still substitutes its reserved ${...} placeholders.
    if text.contains("${") {
        return Err(crate::i18n::Text::AgentHooksPathPlaceholder.text());
    }
    if !path.is_absolute() || text.contains(['\0', '\n', '\r']) {
        return Err(crate::i18n::Text::AgentHooksExeUnstable.text());
    }
    Ok(text)
}

/// Errors are not evidence of death. Path comparison has the Explorer owner's semantics.
///
/// **An operand that does not literally name a file names no copy**, and no copy is nobody: it is
/// not another live Folio, so the mark is removable as one "naming a file that does not exist"
/// (design §6.2). 0.4.2 wrote a bare `folio.exe` whenever `current_exe()` failed, and a machine
/// carrying one could neither install nor uninstall for ever — the operand was an error rather
/// than an answer (closure review R3). The gate that keeps such an operand from being *written*
/// is [`stable_executable`], and it stays exactly where it is.
pub(crate) fn other_live(owner: &Path, exe: &Path) -> Result<bool, &'static str> {
    // An old translocated operand may be dead and cleanable. Translocation
    // prohibits creating a new mark from there, not removing an obsolete mark.
    if literal_absolute_path(owner).is_err() {
        return Ok(false);
    }
    if crate::explorer_menu::same_path(owner, exe) {
        return Ok(false);
    }
    match std::fs::metadata(owner) {
        Ok(meta) if meta.is_file() => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        _ => Err(crate::i18n::Text::AgentHooksOwnerUnknown.text()),
    }
}

pub(crate) fn check(
    owners: &[PathBuf],
    exe: &Path,
    decision: &Decision,
) -> Result<Vec<PathBuf>, Outcome> {
    stable_executable(Some(exe)).map_err(Outcome::Refused)?;
    let mut others = Vec::new();
    for owner in owners {
        if other_live(owner, exe).map_err(Outcome::Refused)? && !others.contains(owner) {
            others.push(owner.clone());
        }
    }
    if decision.installs() && !others.is_empty() {
        let granted = matches!(decision, Decision::TakeOver(consent) if consent == &others);
        if !granted {
            return Err(Outcome::TakeOverRequired(others));
        }
    }
    Ok(others)
}

/// Strictly decode the literal operand of Folio's old templates, never a shell program.
pub(crate) fn legacy_path(line: &str, family: &str) -> Result<Option<PathBuf>, &'static str> {
    let marker = format!(" attention {family}:");
    let Some((quoted, tail)) = line.rsplit_once(&marker) else {
        return Ok(None);
    };
    let event = tail.strip_suffix(" --json -").unwrap_or(tail);
    if event.is_empty()
        || !event
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    }
    let path = if let Some(inner) = quoted
        .strip_prefix("& '")
        .and_then(|s| s.strip_suffix('\''))
    {
        let decoded = inner.replace("''", "'");
        if format!("& '{}'", decoded.replace('\'', "''")) != quoted {
            return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
        }
        decoded
    } else if let Some(inner) = quoted.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        let decoded = inner.replace("'\\''", "'");
        if format!("'{}'", decoded.replace('\'', "'\\''")) != quoted {
            return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
        }
        decoded
    } else if let Some(inner) = quoted.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        if inner.contains('"') {
            return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
        }
        inner.to_owned()
    } else {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    };
    Ok(Some(PathBuf::from(path)))
}

pub(crate) fn direct_path(
    program: &str,
    args: &serde_json::Value,
    family: &str,
) -> Result<Option<PathBuf>, &'static str> {
    let Some(args) = args.as_array() else {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    };
    let words: Option<Vec<_>> = args.iter().map(serde_json::Value::as_str).collect();
    let words = words.ok_or(crate::i18n::Text::AgentHooksSchemaUnknown.text())?;
    if words.first() != Some(&"attention")
        || !words
            .get(1)
            .is_some_and(|s| s.starts_with(&format!("{family}:")))
    {
        return Ok(None);
    }
    if !matches!(words.len(), 2 | 4) || (words.len() == 4 && words[2..] != ["--json", "-"]) {
        return Err(crate::i18n::Text::AgentHooksSchemaUnknown.text());
    }
    Ok(Some(PathBuf::from(program)))
}

/// Intent precedes the foreign-file write. The lock is held through that write.
pub(crate) fn record(
    data: &Path,
    root: &Path,
    family: &str,
) -> Result<std::fs::File, &'static str> {
    if !root.is_absolute() {
        return Err(crate::i18n::Text::AgentHooksRootUnstable.text());
    }
    let lock =
        profile_marks::lock(data).map_err(|_| crate::i18n::Text::AgentHooksRecordFailed.text())?;
    let mut marks =
        Marks::read(data).map_err(|_| crate::i18n::Text::AgentHooksRecordFailed.text())?;
    let roots = match family {
        "claude" => &mut marks.agent_config_roots.claude,
        "codex" => &mut marks.agent_config_roots.codex,
        "copilot" => &mut marks.agent_config_roots.copilot,
        _ => unreachable!("adapter family"),
    };
    if !roots
        .iter()
        .any(|p| crate::explorer_menu::same_path(p, root))
    {
        roots.push(root.to_path_buf());
        marks
            .write(data)
            .map_err(|_| crate::i18n::Text::AgentHooksRecordFailed.text())?;
    }
    Ok(lock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    type Apply = fn(&Path, Decision, &Path, &Path) -> Outcome;
    fn adapters() -> [(&'static str, &'static str, Apply); 3] {
        [
            ("claude", "settings.json", crate::attention_hooks::apply_at),
            ("codex", "config.toml", crate::attention_codex::apply_at),
            (
                "copilot",
                "hooks/folio.json",
                crate::attention_copilot::apply_at,
            ),
        ]
    }

    fn root(name: &str) -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tb-tests")
            .join(name);
        fs::create_dir_all(&path).unwrap();
        fs::canonicalize(path).unwrap()
    }

    #[test]
    fn attention_takeover_cleanup_dead_moved_owner_and_recorded_roots() {
        for (family, file, apply) in adapters() {
            let root = root(&format!("lifecycle-{family}"));
            let a = root.join("A space $ ` ' 中文.exe");
            let b = root.join("B.exe");
            fs::write(&a, b"A").unwrap();
            fs::write(&b, b"B").unwrap();
            let config = root.join("config");
            let path = config.join(file);
            let data = root.join("data");
            let _ = fs::remove_file(&path);
            assert_eq!(
                apply(&path, Decision::Install, &a, &data),
                Outcome::Installed
            );
            let before = fs::read(&path).unwrap();
            assert_eq!(
                apply(&path, Decision::Install, &a, &data),
                Outcome::Unchanged
            );
            assert_eq!(fs::read(&path).unwrap(), before);
            assert_eq!(
                apply(&path, Decision::Install, &b, &data),
                Outcome::TakeOverRequired(vec![a.clone()])
            );
            assert_eq!(fs::read(&path).unwrap(), before);
            assert_eq!(
                apply(&path, Decision::TakeOver(vec![a.clone()]), &b, &data),
                Outcome::Installed
            );
            let from_b = fs::read(&path).unwrap();
            assert_eq!(
                apply(&path, Decision::Remove, &a, &data),
                Outcome::LeftOther(vec![b.clone()])
            );
            assert_eq!(fs::read(&path).unwrap(), from_b);
            // A moved executable leaves a dead old operand. The new copy can replace it.
            let moved = root.join("B moved.exe");
            let _ = fs::remove_file(&moved);
            fs::rename(&b, &moved).unwrap();
            assert_eq!(
                apply(&path, Decision::Install, &moved, &data),
                Outcome::Installed
            );
            fs::remove_file(&moved).unwrap();
            assert_eq!(apply(&path, Decision::Remove, &a, &data), Outcome::Removed);
            assert_eq!(
                apply(&path, Decision::Remove, &a, &data),
                Outcome::Unchanged
            );
            let second = root.join("different-config").join(file);
            let _ = fs::remove_file(&second);
            assert_eq!(
                apply(&second, Decision::Install, &a, &data),
                Outcome::Installed
            );
            let marks = Marks::read(&data).unwrap();
            let roots = match family {
                "claude" => marks.agent_config_roots.claude,
                "codex" => marks.agent_config_roots.codex,
                _ => marks.agent_config_roots.copilot,
            };
            assert!(roots.contains(&config));
            assert!(roots.contains(&root.join("different-config")));
        }
    }

    #[test]
    fn attention_untrusted_executable_and_bad_record_refuse_without_config_changes() {
        assert!(stable_executable(None).is_err());
        for (family, file, apply) in adapters() {
            let root = root(&format!("refusal-{family}"));
            let exe = root.join("folio.exe");
            let path = root.join("config").join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let data = root.join("data");
            fs::create_dir_all(&data).unwrap();
            let original = if family == "codex" {
                "# user's config\n"
            } else if family == "copilot" {
                ""
            } else {
                "{}\n"
            };
            fs::write(&path, original).unwrap();
            let placeholder = root.join("${CLAUDE_PROJECT_DIR}/folio.exe");
            for bad in [
                Path::new("folio.exe"),
                placeholder.as_path(),
                Path::new(
                    "/private/var/folders/a/b/AppTranslocation/c/Folio.app/Contents/MacOS/folio",
                ),
            ] {
                assert!(matches!(
                    apply(&path, Decision::Install, bad, &data),
                    Outcome::Refused(_)
                ));
                assert_eq!(fs::read_to_string(&path).unwrap(), original);
            }
            fs::write(data.join(profile_marks::RECORD_FILE), b"{\"version\":999}").unwrap();
            assert!(matches!(
                apply(&path, Decision::Install, &exe, &data),
                Outcome::Refused(_)
            ));
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn attention_malformed_future_schema_and_readonly_are_byte_identical() {
        for (family, file, apply) in adapters() {
            let root = root(&format!("bad-config-{family}"));
            let exe = root.join("folio.exe");
            let path = root.join("config").join(file);
            let data = root.join("data");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let future = match family {
                "claude" => "{\"hooks\":{\"Stop\":{\"future\":true}}}",
                "codex" => "notify = { future = true }",
                _ => "{\"version\":999,\"hooks\":{}}",
            };
            for text in ["{broken", future] {
                fs::write(&path, text).unwrap();
                for decision in [Decision::Install, Decision::Remove] {
                    assert!(
                        matches!(apply(&path, decision, &exe, &data), Outcome::Refused(_)),
                        "{family}: {text}"
                    );
                    assert_eq!(fs::read_to_string(&path).unwrap(), text);
                }
            }
            fs::remove_file(&path).unwrap();
            assert_eq!(
                apply(&path, Decision::Install, &exe, &data),
                Outcome::Installed
            );
            let bytes = fs::read(&path).unwrap();
            let original_permissions = fs::metadata(&path).unwrap().permissions();
            let mut readonly = original_permissions.clone();
            readonly.set_readonly(true);
            fs::set_permissions(&path, readonly).unwrap();
            for decision in [Decision::Install, Decision::Remove] {
                assert!(matches!(
                    apply(&path, decision, &exe, &data),
                    Outcome::Refused(_)
                ));
                assert_eq!(fs::read(&path).unwrap(), bytes);
            }
            fs::set_permissions(&path, original_permissions).unwrap();
        }
    }

    #[test]
    fn attention_takeover_consent_is_specific_and_expires() {
        let root = root("consent");
        let owner = root.join("a.exe");
        let here = root.join("b.exe");
        fs::write(&owner, b"a").unwrap();
        let owners = vec![owner.clone()];
        assert_eq!(
            check(&owners, &here, &Decision::TakeOver(vec![])),
            Err(Outcome::TakeOverRequired(owners.clone()))
        );
        let mut pending = Pending::new(Some(root.clone()), owners.clone());
        assert_eq!(
            next_decision(&mut pending, true, Some(&root)),
            Decision::TakeOver(owners.clone())
        );
        assert_eq!(
            next_decision(&mut pending, true, Some(&root)),
            Decision::Install
        );
        pending = Pending::new(Some(root.clone()), owners.clone());
        assert_eq!(
            next_decision(&mut pending, true, Some(&owner)),
            Decision::Install
        );
        pending = Pending::new(Some(root.clone()), owners);
        pending.as_mut().unwrap().shown -= std::time::Duration::from_secs(31);
        assert_eq!(
            next_decision(&mut pending, true, Some(&root)),
            Decision::Install
        );
    }

    #[test]
    fn attention_legacy_migration_keeps_user_rows_and_literal_paths() {
        for family in ["claude", "copilot"] {
            let root = root(&format!("legacy-{family}"));
            let exe = root.join("space $ ` ' 中文.exe");
            let other = root.join("other.exe");
            fs::write(&exe, b"ours").unwrap();
            fs::write(&other, b"other").unwrap();
            let path = root.join(if family == "claude" {
                "settings.json"
            } else {
                "hooks/folio.json"
            });
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let data = root.join("data");
            let apply = if family == "claude" {
                crate::attention_hooks::apply_at
            } else {
                crate::attention_copilot::apply_at
            };
            for shell in ["windows", "unix"] {
                let literal = exe.to_str().unwrap();
                let command = match (family, shell) {
                    ("claude", "windows") => {
                        format!("\"{literal}\" attention claude-code:Stop --json -")
                    }
                    ("claude", _) => format!(
                        "'{}' attention claude-code:Stop --json -",
                        literal.replace('\'', "'\\''")
                    ),
                    (_, "windows") => format!(
                        "& '{}' attention copilot:agentStop",
                        literal.replace('\'', "''")
                    ),
                    _ => format!(
                        "'{}' attention copilot:agentStop",
                        literal.replace('\'', "'\\''")
                    ),
                };
                let value = if family == "claude" {
                    serde_json::json!({"hooks":{"Stop":[{"hooks":[
                        {"type":"command","command":command,"async":true},
                        {"type":"command","command":"echo user-hook"}
                    ]}]},"model":"user-choice"})
                } else {
                    let column = if shell == "windows" {
                        "powershell"
                    } else {
                        "bash"
                    };
                    serde_json::json!({"version":1,"hooks":{"agentStop":[{"type":"command",column:command,"timeoutSec":5}]}})
                };
                let bytes = serde_json::to_vec(&value).unwrap();
                fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    apply(&path, Decision::Install, &other, &data),
                    Outcome::TakeOverRequired(vec![exe.clone()])
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                assert_eq!(
                    apply(&path, Decision::Install, &exe, &data),
                    Outcome::Installed
                );
                let written: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                let entry = if family == "claude" {
                    assert_eq!(written["model"], "user-choice");
                    assert_eq!(
                        written["hooks"]["Stop"][0]["hooks"][0]["command"],
                        "echo user-hook"
                    );
                    assert_eq!(written["hooks"]["Stop"][1]["hooks"][0]["async"], true);
                    &written["hooks"]["Stop"][1]["hooks"][0]
                } else {
                    &written["hooks"]["agentStop"][0]
                };
                assert_eq!(
                    entry[if family == "claude" {
                        "command"
                    } else {
                        "exec"
                    }],
                    literal
                );
                assert!(entry["args"].is_array());
                assert!(entry.get("powershell").is_none());
                assert!(entry.get("bash").is_none());
            }
        }
    }

    #[test]
    fn attention_codex_takeover_never_replaces_a_user_notifier() {
        let root = root("user-notifier");
        let exe = root.join("folio.exe");
        let path = root.join("config.toml");
        let original = "# keep this\nnotify = ['my-notifier', '--user-option']\n";
        fs::write(&path, original).unwrap();
        assert_eq!(
            crate::attention_codex::apply_at(
                &path,
                Decision::TakeOver(vec![]),
                &exe,
                &root.join("data")
            ),
            Outcome::Refused("codex already runs a notify program of your own")
        );
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn attention_cleanup_preserves_live_other_entries_in_a_mixed_file() {
        for family in ["claude", "copilot"] {
            let root = root(&format!("mixed-{family}"));
            let a = root.join("a.exe");
            let b = root.join("b.exe");
            fs::write(&a, b"a").unwrap();
            fs::write(&b, b"b").unwrap();
            let path = root.join("mixed.json");
            let entry = |exe: &Path| {
                if family == "claude" {
                    serde_json::json!({"type":"command","command":exe,"args":["attention","claude-code:Stop"],"async":true})
                } else {
                    serde_json::json!({"type":"command","exec":exe,"args":["attention","copilot:agentStop"]})
                }
            };
            let original = if family == "claude" {
                serde_json::json!({"hooks":{"Stop":[{"hooks":[entry(&a),entry(&b)]}]}})
            } else {
                serde_json::json!({"version":1,"hooks":{"agentStop":[entry(&a),entry(&b)]}})
            };
            fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
            let apply = if family == "claude" {
                crate::attention_hooks::apply_at
            } else {
                crate::attention_copilot::apply_at
            };
            assert_eq!(
                apply(&path, Decision::Remove, &a, &root.join("data")),
                Outcome::LeftOther(vec![b.clone()])
            );
            let bytes = fs::read(&path).unwrap();
            let written: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let retained = if family == "claude" {
                &written["hooks"]["Stop"][0]["hooks"]
            } else {
                &written["hooks"]["agentStop"]
            };
            assert_eq!(retained, &serde_json::json!([entry(&b)]));
            assert_eq!(
                apply(&path, Decision::Remove, &a, &root.join("data")),
                Outcome::LeftOther(vec![b])
            );
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }

    /// The exact entry 0.4.2 wrote whenever `current_exe()` failed, per family.
    fn relative_operand_fixture(family: &str) -> String {
        match family {
            "claude" => serde_json::to_string(&serde_json::json!({"hooks":{"Stop":[{"hooks":[
                {"type":"command","command":"\"folio.exe\" attention claude-code:Stop --json -","async":true}
            ]}]}}))
            .unwrap(),
            "codex" => {
                "notify = [\"folio.exe\", \"attention\", \"codex:agent-turn-complete\", \"--json\"]\n"
                    .to_owned()
            }
            _ => serde_json::to_string(&serde_json::json!({"version":1,"hooks":{"agentStop":[
                {"type":"command","exec":"folio.exe","args":["attention","copilot:agentStop"],"timeoutSec":5}
            ]}}))
            .unwrap(),
        }
    }

    /// RED (closure review R3) — **a relative operand names no copy, so it is nobody's.**
    ///
    /// `"folio.exe" attention claude-code:Stop --json -` is what 0.4.2 wrote when `current_exe()`
    /// failed. It cannot be resolved to a file on this machine, so it is not another live Folio;
    /// a mark that names no file is removable (design §6.2), and a machine carrying one must not
    /// be frozen out of installing or uninstalling for ever.
    ///
    /// RED GATE: put `literal_absolute_path(owner)?` back at the top of [`other_live`] and every
    /// assertion here becomes `Refused`.
    #[test]
    fn attention_relative_operand_names_nobody_and_is_removable() {
        assert_eq!(
            other_live(Path::new("folio.exe"), Path::new("/opt/folio/folio")),
            Ok(false)
        );
        for (family, file, apply) in adapters() {
            let root = root(&format!("relative-{family}"));
            // Not `folio.exe`: this copy's own name must not be able to satisfy the assertion
            // below that the operand naming nobody is gone.
            let exe = root.join("this-copy.exe");
            fs::write(&exe, b"exe").unwrap();
            let data = root.join("data");
            let fixture = relative_operand_fixture(family);
            for (name, decision, expected) in [
                ("remove", Decision::Remove, Outcome::Removed),
                ("install", Decision::Install, Outcome::Installed),
            ] {
                let path = root.join(name).join(file);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, &fixture).unwrap();
                assert_eq!(apply(&path, decision, &exe, &data), expected, "{family}");
            }
            // And the entry that named nobody is gone rather than kept beside ours.
            let after = fs::read_to_string(root.join("install").join(file)).unwrap();
            assert!(!after.contains("folio.exe"), "{family}: {after}");
        }
    }

    /// RED (closure review R4) — **one entry Folio cannot read does not speak for the others.**
    ///
    /// A hand-written `"mytool attention claude-code:Stop"` wears Folio's verb without Folio's
    /// quoting, so no copy can be decoded from it. It is therefore not Folio's, it is left exactly
    /// where it is, and it has no say over the well-formed entry beside it.
    ///
    /// RED GATE: make `owners()` propagate a per-entry decode error again and removal of the
    /// neighbour becomes `Refused`.
    #[test]
    fn attention_an_unparseable_entry_does_not_veto_its_neighbours() {
        let root = root("stranger-claude");
        let exe = root.join("folio.exe");
        fs::write(&exe, b"exe").unwrap();
        let path = root.join("settings.json");
        let stranger = serde_json::json!({
            "type": "command",
            "command": "mytool attention claude-code:Stop"
        });
        let ours = serde_json::json!({
            "type": "command",
            "command": exe,
            "args": ["attention", "claude-code:Stop"],
            "async": true
        });
        let original = serde_json::json!({"hooks":{"Stop":[{"hooks":[stranger, ours]}]}});
        fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        assert_eq!(
            crate::attention_hooks::apply_at(&path, Decision::Remove, &exe, &root.join("data")),
            Outcome::Removed
        );
        let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            written["hooks"]["Stop"][0]["hooks"],
            serde_json::json!([stranger])
        );
    }

    /// RED (closure review R7) — **App Translocation is a path component, not a prefix.**
    ///
    /// `current_exe()` on macOS hands back `_NSGetExecutablePath`'s string uncanonicalised, so the
    /// same file is reported as `/var/folders/…` or `/private/var/folders/…` — `/var` is a link to
    /// `/private/var`. The component is the fact; the prefix was a spelling.
    ///
    /// RED GATE: require the `/private/var/folders/` prefix again and the second spelling installs
    /// hooks from a copy macOS will delete.
    #[test]
    fn attention_translocation_is_a_path_component_on_every_platform() {
        for spelling in [
            "/private/var/folders/qx/T/AppTranslocation/00-UUID/d/Folio.app/Contents/MacOS/folio",
            "/var/folders/qx/T/AppTranslocation/00-UUID/d/Folio.app/Contents/MacOS/folio",
        ] {
            assert_eq!(
                stable_executable(Some(Path::new(spelling))),
                Err(crate::i18n::Text::AgentHooksTranslocated.text()),
                "{spelling}"
            );
            assert!(translocated(Path::new(spelling)));
        }
        // A folder whose name merely begins with the word is not the quarantine, and a copy that
        // is nowhere near one installs.
        assert!(!translocated(Path::new(
            "/Users/alice/AppTranslocationNotes/Folio.app/Contents/MacOS/folio"
        )));
        assert!(!translocated(Path::new(
            "/Applications/Folio.app/Contents/MacOS/folio"
        )));
    }

    /// RED (closure review R1) — **a file Folio will not edit is named, not flattened.**
    ///
    /// "this build cannot read it" is not true of a file that is perfectly readable and merely
    /// protected, and a row that says it sends the reader looking for a corrupt document. The
    /// refusal the filesystem predicate gave is the one the reader is shown.
    ///
    /// RED GATE: send the predicate's answer back through `Standing::Unreadable` and every
    /// sentence here becomes "not one this build can read".
    #[test]
    fn attention_a_protected_config_names_its_own_reason() {
        let unreadable = [
            "the settings file is not one this build can read",
            "the codex configuration file is not one this build can read",
            "the copilot hook file is not one this build can read",
        ];
        for (family, file, apply) in adapters() {
            let root = root(&format!("protected-{family}"));
            let exe = root.join("folio.exe");
            fs::write(&exe, b"exe").unwrap();
            let data = root.join("data");
            let path = root.join("config").join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let _ = fs::remove_file(&path);
            assert_eq!(
                apply(&path, Decision::Install, &exe, &data),
                Outcome::Installed
            );
            let bytes = fs::read(&path).unwrap();
            let writable = fs::metadata(&path).unwrap().permissions();
            let mut readonly = writable.clone();
            readonly.set_readonly(true);
            fs::set_permissions(&path, readonly).unwrap();
            let refused = apply(&path, Decision::Remove, &exe, &data);
            let after = fs::read(&path).unwrap();
            // Restored before the assertions, not after them: a panic in between would leave the
            // fixture read-only and every later run of this test dead on its `write` (review R14).
            fs::set_permissions(&path, writable).unwrap();
            assert_eq!(after, bytes, "{family}");
            let Outcome::Refused(reason) = refused else {
                panic!("{family}: a read-only file is refused, not {refused:?}");
            };
            assert!(!unreadable.contains(&reason), "{family}: {reason}");
            assert_eq!(
                reason,
                crate::i18n::Text::AgentConfigReadOnly.text(),
                "{family}"
            );
        }
    }

    /// **Which file a linked configuration path names** — the decision, over the filesystem's
    /// answers rather than over a filesystem, so that the rule this machine cannot demonstrate
    /// (creating a link needs a privilege this account may not have) is still pinned here.
    ///
    /// The one question: is the thing at the end of the link a regular file inside the directory
    /// the configuration path names? A dotfiles machine whose whole `~/.claude` is junctioned onto
    /// a managed folder answers yes and is editable, which is the state 0.4.2 installed into and
    /// 0.4.3 must be able to leave (closure review R1).
    #[test]
    fn attention_a_link_is_followed_only_inside_the_agents_own_folder() {
        use crate::attention_hooks::{Resolution, linked_target};
        let refused = Err(crate::i18n::Text::AgentConfigLink.text());
        let inside = PathBuf::from("/dotfiles/.claude/settings.json");
        assert_eq!(
            linked_target(&Resolution {
                root: Some(Path::new("/dotfiles/.claude")),
                resolved: Some(&inside),
                regular: true,
            }),
            Ok(inside.clone())
        );
        // Out of the folder: a link Folio will not write through, whatever stands at the far end.
        assert_eq!(
            linked_target(&Resolution {
                root: Some(Path::new("/home/alice/.claude")),
                resolved: Some(Path::new("/etc/passwd")),
                regular: true,
            }),
            refused
        );
        // Inside it, but not a file: a directory, a device, a pipe.
        assert_eq!(
            linked_target(&Resolution {
                root: Some(Path::new("/dotfiles/.claude")),
                resolved: Some(&inside),
                regular: false,
            }),
            refused
        );
        // A link to nothing, and a directory that does not resolve at all.
        assert_eq!(
            linked_target(&Resolution {
                root: Some(Path::new("/dotfiles/.claude")),
                resolved: None,
                regular: false,
            }),
            refused
        );
        assert_eq!(
            linked_target(&Resolution {
                root: None,
                resolved: Some(&inside),
                regular: true,
            }),
            refused
        );
    }

    /// **The same decision against a real link**, for the machine that has the privilege to make
    /// one (closure review R1).
    ///
    /// Two links per family and one rule between them: the one whose target stands in the
    /// directory the configuration path names is resolved once and *edited at its target*, so the
    /// dotfiles machine can install and — the reason this ticket exists — uninstall; the one
    /// pointing out of that directory is refused, and both the link and its target come through
    /// byte-identical.
    #[test]
    #[ignore = "requires Windows symlink privilege; the injected-facts decision test runs everywhere"]
    fn attention_a_linked_config_is_edited_at_its_target_inside_the_agents_folder() {
        for (family, file, apply) in adapters() {
            let root = root(&format!("symlink-{family}"));
            let exe = root.join("folio.exe");
            fs::write(&exe, b"exe").unwrap();
            let data = root.join("data");
            let path = root.join(file);
            let folder = path.parent().unwrap().to_path_buf();
            fs::create_dir_all(&folder).unwrap();
            let link = |target: &Path, at: &Path| {
                let _ = fs::remove_file(at);
                #[cfg(windows)]
                let made = std::os::windows::fs::symlink_file(target, at);
                #[cfg(unix)]
                let made = std::os::unix::fs::symlink(target, at);
                made.expect("symlink tests require developer mode on Windows");
            };
            let is_link = |at: &Path| fs::symlink_metadata(at).unwrap().file_type().is_symlink();

            // ① A dotfile manager's own copy, beside the file it stands in for.
            let managed = folder.join("managed-config");
            fs::write(&managed, b"").unwrap();
            link(&managed, &path);
            assert_eq!(
                apply(&path, Decision::Install, &exe, &data),
                Outcome::Installed,
                "{family}"
            );
            assert!(
                fs::read_to_string(&managed).unwrap().contains("attention"),
                "{family}: the target is what was written"
            );
            assert!(is_link(&path), "{family}: the link itself is untouched");
            assert_eq!(
                apply(&path, Decision::Remove, &exe, &data),
                Outcome::Removed
            );
            // Copilot's whole file goes, under both of its names; the other two hand the document
            // back. Either way nothing of Folio's is left behind to fire.
            assert!(
                fs::read_to_string(&managed)
                    .map(|text| !text.contains("attention"))
                    .unwrap_or(true),
                "{family}: the hooks left with the removal"
            );

            // ② A target outside the folder the configuration path names. Never written through.
            let outside = root.join("elsewhere");
            fs::create_dir_all(&outside).unwrap();
            let theirs = outside.join("somebody-elses-config");
            fs::write(&theirs, b"user content").unwrap();
            link(&theirs, &path);
            for decision in [Decision::Install, Decision::Remove] {
                assert_eq!(
                    apply(&path, decision, &exe, &data),
                    Outcome::Refused(crate::i18n::Text::AgentConfigLink.text()),
                    "{family}"
                );
                assert_eq!(fs::read(&theirs).unwrap(), b"user content");
                assert!(is_link(&path));
            }

            // And a link is still compared as the file it names when it is an *executable*
            // operand: an alias of this copy is this copy.
            link(&exe, &path);
            assert_eq!(other_live(&path, &exe), Ok(false));
            fs::remove_file(&path).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn attention_locked_configs_are_refused_without_changes() {
        use std::os::windows::fs::OpenOptionsExt;
        for (family, file, apply) in adapters() {
            let root = root(&format!("locked-{family}"));
            let exe = root.join("folio.exe");
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let data = root.join("data");
            let _ = fs::remove_file(&path);
            assert_eq!(
                apply(&path, Decision::Install, &exe, &data),
                Outcome::Installed
            );
            let bytes = fs::read(&path).unwrap();
            let lock = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&path)
                .unwrap();
            for decision in [Decision::Install, Decision::Remove] {
                assert!(matches!(
                    apply(&path, decision, &exe, &data),
                    Outcome::Refused(_)
                ));
            }
            drop(lock);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
