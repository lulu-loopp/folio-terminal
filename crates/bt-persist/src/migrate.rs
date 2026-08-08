//! Read fallback chain and the forward-migration scaffold —
//! docs/M2-persistence-schema-v1.md §1.3 and §5.4.
//!
//! §1.3's rules, implemented here exactly as numbered:
//! 1. "只向前迁移,不支持降级" — [`migrate_value`] walks `from..to` applying one
//!    registered step per version, never backwards.
//! 2. "读到 `schema_version > current`…不尝试部分解析" — [`read_with_fallback`]
//!    checks this *before* attempting to parse anything beyond the version
//!    envelope, and falls back to defaults wholesale.
//! 3. "迁移函数只做结构升级,不做语义修复" — migration steps operate on raw
//!    `serde_json::Value`s and are never handed the per-leaf degradation
//!    logic in `layout.rs`/`session.rs`; that runs afterward, once, on the
//!    fully-migrated-and-parsed value (see `SessionV1::degrade_in_place`).
//! 4. "未识别字段…静默丢弃" — this falls out of plain `serde` semantics: none
//!    of the types in this crate set `#[serde(deny_unknown_fields)]`, so
//!    unrecognized keys are ignored during `Deserialize` and never
//!    reappear on the next `Serialize`. No code here does that dropping on
//!    purpose; it is default behavior being *relied on*, not implemented.

use std::io;
use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// One migration step: transforms a JSON value at schema_version `N` (the
/// key it is registered under) into schema_version `N+1`. Structural only —
/// see this module's doc comment, rule 3. `settings.json` is still v1;
/// `session.json` uses this scaffold for its registered v1-to-v2 theme migration.
pub type MigrationStep = fn(Value) -> Value;

/// Migration table for `settings.json`. Empty: v1 is the only version that
/// has ever existed.
pub const SETTINGS_MIGRATIONS: &[(u32, MigrationStep)] = &[];
/// Migration table for `session.json`. Schema v2 adds the runtime theme and maps every v1 session
/// to the historical dark default.
pub const SESSION_MIGRATIONS: &[(u32, MigrationStep)] =
    &[(1, migrate_session_v1_to_v2), (2, migrate_session_v2_to_v3)];

fn migrate_session_v1_to_v2(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(2));
        object.insert("theme".to_owned(), Value::from("dark"));
    }
    value
}

fn migrate_session_v2_to_v3(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(3));
        object.insert("cursor_style".to_owned(), Value::from("bar"));
    }
    value
}

/// Why a read fell back to defaults — surfaced so the caller can build the
/// "显式告警,绝不假装成功" (§5.3/§5.4) message. Never constructed for the
/// "file simply doesn't exist yet" case — see [`ReadReport::NotFound`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackReason {
    /// The file exists but could not even be opened/read (permission
    /// denied, locked by another process, etc.) — distinct from a parse
    /// failure because the bytes were never obtained.
    Io(String),
    /// The bytes were read but are not valid JSON, or do not match the
    /// expected shape at the schema version they claim.
    ParseError(String),
    /// §1.3 rule 2: the file's `schema_version` is newer than this build
    /// understands. Never partially parsed — see this module's doc comment.
    FutureSchemaVersion { found: u32, current: u32 },
    /// The file's `schema_version` is older than current, but no migration
    /// step is registered to bridge from it — this build cannot have
    /// written such a file (nothing precedes v1), so this only fires for
    /// hand-edited or foreign files.
    NoMigrationPath { found: u32 },
}

/// Outcome of a `read_settings`/`read_session` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadReport {
    /// No file was there. §5.4 case 1: the normal first-run state, not an
    /// error — callers must not alert on this.
    NotFound,
    /// Parsed successfully (after migration, if any) at the current schema
    /// version.
    Loaded,
    /// The file existed and was read, but could not be used as-is; the
    /// returned value is `T::default()`. §5.4 case 2: callers must alert,
    /// naming the file and `reason`. The original file is never touched by
    /// the read path — only a later write can replace it.
    FellBackToDefaults { reason: FallbackReason },
}

#[derive(Deserialize)]
struct VersionEnvelope {
    schema_version: u32,
}

/// Applies `read_with_fallback`'s §5.4 chain for one file: missing → silent
/// default; unparseable/future-version → default + explicit reason;
/// otherwise the deserialized value (after migration, if the file was
/// behind current). Never panics — every branch returns a value.
pub(crate) fn read_with_fallback<T>(
    path: &Path,
    current_version: u32,
    migrations: &[(u32, MigrationStep)],
) -> (T, ReadReport)
where
    T: DeserializeOwned + Default,
{
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return (T::default(), ReadReport::NotFound);
        }
        Err(e) => {
            return (
                T::default(),
                ReadReport::FellBackToDefaults {
                    reason: FallbackReason::Io(e.to_string()),
                },
            );
        }
    };

    let envelope: VersionEnvelope = match serde_json::from_slice(&bytes) {
        Ok(env) => env,
        Err(e) => {
            return (
                T::default(),
                ReadReport::FellBackToDefaults {
                    reason: FallbackReason::ParseError(e.to_string()),
                },
            );
        }
    };

    if envelope.schema_version > current_version {
        return (
            T::default(),
            ReadReport::FellBackToDefaults {
                reason: FallbackReason::FutureSchemaVersion {
                    found: envelope.schema_version,
                    current: current_version,
                },
            },
        );
    }

    if envelope.schema_version < current_version {
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => {
                return (
                    T::default(),
                    ReadReport::FellBackToDefaults {
                        reason: FallbackReason::ParseError(e.to_string()),
                    },
                );
            }
        };
        let migrated =
            match migrate_value(value, envelope.schema_version, current_version, migrations) {
                Ok(v) => v,
                Err(found) => {
                    return (
                        T::default(),
                        ReadReport::FellBackToDefaults {
                            reason: FallbackReason::NoMigrationPath { found },
                        },
                    );
                }
            };
        return match serde_json::from_value::<T>(migrated) {
            Ok(v) => (v, ReadReport::Loaded),
            Err(e) => (
                T::default(),
                ReadReport::FellBackToDefaults {
                    reason: FallbackReason::ParseError(e.to_string()),
                },
            ),
        };
    }

    match serde_json::from_slice::<T>(&bytes) {
        Ok(v) => (v, ReadReport::Loaded),
        Err(e) => (
            T::default(),
            ReadReport::FellBackToDefaults {
                reason: FallbackReason::ParseError(e.to_string()),
            },
        ),
    }
}

/// Walks `from..to`, applying the registered step for each version in turn.
/// Returns `Err(v)` naming the first version for which no step is
/// registered — §1.3 rule 1, "只向前迁移": there is no backward path and no
/// guessing across a gap.
fn migrate_value(
    mut value: Value,
    from: u32,
    to: u32,
    migrations: &[(u32, MigrationStep)],
) -> Result<Value, u32> {
    let mut v = from;
    while v < to {
        let step = migrations
            .iter()
            .find(|(ver, _)| *ver == v)
            .map(|(_, f)| *f)
            .ok_or(v)?;
        value = step(value);
        v += 1;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use serde_json::json;

    #[test]
    fn empty_chain_is_identity_when_already_current() {
        let value = json!({"schema_version": 1, "x": 1});
        let migrated = migrate_value(value.clone(), 1, 1, SESSION_MIGRATIONS).unwrap();
        assert_eq!(migrated, value);
    }

    #[test]
    fn missing_step_reports_the_first_unreachable_version() {
        let err = migrate_value(json!({"schema_version": 1}), 1, 3, &[]).unwrap_err();
        assert_eq!(err, 1);
    }

    /// Proves the scaffold can apply more than the one production step by using a synthetic
    /// two-step chain: registering steps 1->2 and 2->3 must apply both in order.
    #[test]
    fn scaffold_applies_multiple_registered_steps_in_order() {
        fn v1_to_v2(v: Value) -> Value {
            let mut obj = v.as_object().cloned().unwrap();
            obj.insert("schema_version".to_string(), json!(2));
            obj.insert("added_in_v2".to_string(), json!("present"));
            Value::Object(obj)
        }
        fn v2_to_v3(v: Value) -> Value {
            let mut obj = v.as_object().cloned().unwrap();
            obj.insert("schema_version".to_string(), json!(3));
            obj.insert("added_in_v3".to_string(), json!(true));
            Value::Object(obj)
        }
        let migrations: &[(u32, MigrationStep)] = &[(1, v1_to_v2), (2, v2_to_v3)];
        let migrated = migrate_value(json!({"schema_version": 1}), 1, 3, migrations).unwrap();
        assert_eq!(migrated["schema_version"], json!(3));
        assert_eq!(migrated["added_in_v2"], json!("present"));
        assert_eq!(migrated["added_in_v3"], json!(true));
    }

    #[test]
    fn real_session_v1_to_v2_migration_adds_the_dark_default() {
        let migrated = migrate_value(
            json!({"schema_version": 1, "window": {}}),
            1,
            2,
            SESSION_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(2));
        assert_eq!(migrated["theme"], json!("dark"));
    }

    #[test]
    fn real_session_v2_to_v3_migration_adds_the_bar_cursor_default() {
        let migrated = migrate_value(
            json!({"schema_version": 2, "theme": "light", "window": {}}),
            2,
            3,
            SESSION_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(3));
        assert_eq!(migrated["theme"], json!("light"));
        assert_eq!(migrated["cursor_style"], json!("bar"));
    }

    #[derive(Debug, Default, PartialEq, Deserialize, Serialize)]
    struct Fixture {
        schema_version: u32,
        value: String,
    }

    fn write_fixture(dir: &Path, name: &str, contents: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "bt-persist-migrate-{tag}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_is_silent_default() {
        let dir = unique_dir("missing");
        let path = dir.join("does-not-exist.json");
        let (value, report) = read_with_fallback::<Fixture>(&path, 1, &[]);
        assert_eq!(value, Fixture::default());
        assert_eq!(report, ReadReport::NotFound);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn syntax_error_falls_back_with_parse_error_reason() {
        let dir = unique_dir("syntax");
        let path = write_fixture(&dir, "broken.json", "{ not json");
        let (value, report) = read_with_fallback::<Fixture>(&path, 1, &[]);
        assert_eq!(value, Fixture::default());
        assert!(matches!(
            report,
            ReadReport::FellBackToDefaults {
                reason: FallbackReason::ParseError(_)
            }
        ));
        // The corrupt file itself must survive the read untouched (§5.4: "原文件不覆盖、不删除").
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn future_schema_version_refuses_without_partial_parse() {
        let dir = unique_dir("future");
        let path = write_fixture(
            &dir,
            "future.json",
            r#"{"schema_version": 99, "value": "from the future"}"#,
        );
        let (value, report) = read_with_fallback::<Fixture>(&path, 1, &[]);
        assert_eq!(value, Fixture::default());
        assert_eq!(
            report,
            ReadReport::FellBackToDefaults {
                reason: FallbackReason::FutureSchemaVersion {
                    found: 99,
                    current: 1
                }
            }
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn older_version_without_a_registered_migration_falls_back() {
        let dir = unique_dir("nopath");
        let path = write_fixture(
            &dir,
            "old.json",
            r#"{"schema_version": 0, "value": "prehistoric"}"#,
        );
        let (value, report) = read_with_fallback::<Fixture>(&path, 1, &[]);
        assert_eq!(value, Fixture::default());
        assert_eq!(
            report,
            ReadReport::FellBackToDefaults {
                reason: FallbackReason::NoMigrationPath { found: 0 }
            }
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn current_version_loads_normally() {
        let dir = unique_dir("ok");
        let path = write_fixture(
            &dir,
            "ok.json",
            r#"{"schema_version": 1, "value": "hello"}"#,
        );
        let (value, report) = read_with_fallback::<Fixture>(&path, 1, &[]);
        assert_eq!(
            value,
            Fixture {
                schema_version: 1,
                value: "hello".to_string()
            }
        );
        assert_eq!(report, ReadReport::Loaded);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_fields_are_silently_dropped() {
        let dir = unique_dir("unknown-fields");
        let path = write_fixture(
            &dir,
            "extra.json",
            r#"{"schema_version": 1, "value": "hello", "from_a_third_party_tool": 42}"#,
        );
        let (value, report) = read_with_fallback::<Fixture>(&path, 1, &[]);
        assert_eq!(
            value,
            Fixture {
                schema_version: 1,
                value: "hello".to_string()
            }
        );
        assert_eq!(report, ReadReport::Loaded);
        // Re-serializing must not resurrect the unknown field.
        let round_tripped = serde_json::to_string(&value).unwrap();
        assert!(!round_tripped.contains("from_a_third_party_tool"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
