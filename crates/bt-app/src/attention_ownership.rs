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
    let text = literal_absolute_path(exe)?;
    let normalized = text.replace('\\', "/");
    if normalized.starts_with("/private/var/folders/") && normalized.contains("/AppTranslocation/")
    {
        return Err(crate::i18n::Text::AgentHooksTranslocated.text());
    }
    Ok(exe)
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
pub(crate) fn other_live(owner: &Path, exe: &Path) -> Result<bool, &'static str> {
    // An old translocated operand may be dead and cleanable. Translocation
    // prohibits creating a new mark from there, not removing an obsolete mark.
    literal_absolute_path(owner)?;
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

    #[test]
    #[ignore = "requires Windows symlink privilege; shared injected path-refusal tests run normally"]
    fn attention_symlinked_configs_are_refused_and_executable_aliases_are_owned() {
        for (family, file, apply) in adapters() {
            let root = root(&format!("symlink-{family}"));
            let exe = root.join("folio.exe");
            fs::write(&exe, b"exe").unwrap();
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let target = root.join("real-config");
            let _ = fs::remove_file(&path);
            fs::write(&target, b"user content").unwrap();
            #[cfg(windows)]
            let linked = std::os::windows::fs::symlink_file(&target, &path);
            #[cfg(unix)]
            let linked = std::os::unix::fs::symlink(&target, &path);
            linked.expect("symlink tests require developer mode on Windows");
            for decision in [Decision::Install, Decision::Remove] {
                assert!(matches!(
                    apply(&path, decision, &exe, &root.join("data")),
                    Outcome::Refused(_)
                ));
                assert_eq!(fs::read(&target).unwrap(), b"user content");
                assert!(
                    fs::symlink_metadata(&path)
                        .unwrap()
                        .file_type()
                        .is_symlink()
                );
            }
            fs::remove_file(&path).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&exe, &path).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&exe, &path).unwrap();
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
