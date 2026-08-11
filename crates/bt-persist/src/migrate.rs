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
/// see this module's doc comment, rule 3. Both `settings.json` and
/// `session.json` use this scaffold for their registered forward migrations.
pub type MigrationStep = fn(Value) -> Value;

/// Migration table for `settings.json`. v2 adds the display-formula render
/// switch; every v1 file was written by a build that always drew formulas, so
/// the step carries that behaviour forward rather than imposing a new one.
/// v3 adds the inline-formula switch — see [`migrate_settings_v2_to_v3`] for why
/// that one does *not* carry its predecessor's behaviour forward.
pub const SETTINGS_MIGRATIONS: &[(u32, MigrationStep)] = &[
    (1, migrate_settings_v1_to_v2),
    (2, migrate_settings_v2_to_v3),
];

fn migrate_settings_v1_to_v2(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(2));
        object.insert("display_formulas".to_owned(), Value::from(true));
    }
    value
}

/// v2 -> v3: the inline-formula switch, defaulted **on** (user ruling).
///
/// This deliberately breaks the symmetry with `migrate_settings_v1_to_v2`, and
/// the difference is worth stating because the two steps look alike. That step
/// carried a behaviour forward: a v1 user had been watching `$$…$$` blocks get
/// typeset for as long as they had used the product, and a migration that
/// silently stopped doing it would be taking something away. There is no such
/// behaviour to carry here — a v2 build never rendered an inline `$…$` at all,
/// because the detector was disabled outright pending a sound disambiguator, so
/// "off" would not be preserving a user's status quo, only freezing an absence.
/// A feature shipping for the first time takes the product's default, and the
/// ruling sets that default to on.
fn migrate_settings_v2_to_v3(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(3));
        object.insert("inline_formulas".to_owned(), Value::from(true));
    }
    value
}
/// Migration table for `session.json`. Schema v2 adds the runtime theme and maps every v1 session
/// to the historical dark default.
pub const SESSION_MIGRATIONS: &[(u32, MigrationStep)] = &[
    (1, migrate_session_v1_to_v2),
    (2, migrate_session_v2_to_v3),
    (3, migrate_session_v3_to_v4),
    (4, migrate_session_v4_to_v5),
    (5, migrate_session_v5_to_v6),
];

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

fn migrate_session_v3_to_v4(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(4));
    }
    value
}

fn migrate_session_v4_to_v5(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(5));
        object.insert("tab_layout".to_owned(), Value::from("horizontal"));
        object.insert("sidebar_mode".to_owned(), Value::from("expanded"));
    }
    value
}

/// v5 -> v6: `profile_id` becomes a **stable profile slug** everywhere it appears.
///
/// The migration `docs/M2-persistence-schema-v1.md` §3.3 predicted in as many words:
/// "profile 系统落地后,这个字段的语义升级为真正的 `ProfileId`,是一次 `schema_version`
/// bump 加一次一次性迁移(把可执行路径映射到新建的默认 profile)". The profile system has
/// landed, so this is that migration.
///
/// **What was on disk.** Two conventions at once, which is the whole reason a migration is
/// owed rather than a read-time guess. §3.3 specified the v1 transitional value as the
/// normalized path of the shell that started the pane (`C:\Program Files\PowerShell\7\pwsh.exe`),
/// and the fixtures in this crate carry exactly that; the application, meanwhile, has always
/// written the short id `"pwsh"`. A field with two spellings is a field that cannot be compared,
/// and `recent`'s dedup key is `profile_id + cwd + manual_name` — so the same shell in the same
/// folder was already capable of occupying two Recent rows.
///
/// **Why the slug won** (ruling 2026-08-10): a path is neither stable nor an identity. `pwsh.exe`
/// moves between `%ProgramFiles%`, the Store alias and `PATH` without the user doing anything,
/// `BT_SHELL` moves it anywhere at all, and two profiles may legitimately run one binary — which
/// is precisely what "PowerShell" and a future "PowerShell with different arguments" would do.
///
/// **Rule 3 ("迁移函数只做结构升级,不做语义修复") is not bent here.** This is not a repair of a
/// damaged value; it is the schema's own vocabulary being restated, exactly as §3.3 scheduled.
/// The step rewrites nothing else, and every `profile_id` it does not recognize is **left
/// verbatim** — see [`profile_slug`].
///
/// It is also the first step to walk *below* the top-level object. Every earlier one inserted a
/// key beside `schema_version`; this one has to reach every `term` leaf of every tab's tree and
/// every `term` seed in `recent`, because that is where the field it renames actually lives.
///
/// **`recent[].key` is deliberately left alone**, and that is not an oversight even though the
/// key is documented as `profile_id + cwd + manual_name` and therefore now disagrees with the
/// seed beside it. The key is write-only state: `SeedVault::from_persisted` reads only `seed` and
/// `timestamp` and never the key, and `to_persisted` recomputes it from the seed on every save —
/// so a stale key is inert on disk and is gone the first time the vault is written. Rewriting it
/// here would mean this crate reconstructing a key format §3.5 explicitly says it neither
/// computes nor validates, in order to fix something nothing reads.
fn migrate_session_v5_to_v6(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(6));
        if let Some(tabs) = object.get_mut("tabs").and_then(Value::as_array_mut) {
            for tab in tabs {
                if let Some(root) = tab.get_mut("root") {
                    migrate_profile_ids_in_tree(root);
                }
            }
        }
        if let Some(recent) = object.get_mut("recent").and_then(Value::as_array_mut) {
            for entry in recent {
                if let Some(seed) = entry.get_mut("seed") {
                    migrate_profile_id_in_leaf(seed);
                }
            }
        }
    }
    value
}

/// Walks a persisted layout tree, migrating the `profile_id` of every `term` leaf.
///
/// Structural recursion over `children` rather than over the two node types this build happens to
/// know: a migration reads documents written by *older* builds, and a node shape it does not
/// recognize is a node whose children it must still not lose. Anything without `children` and
/// without a `term` kind is passed over untouched.
fn migrate_profile_ids_in_tree(node: &mut Value) {
    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            migrate_profile_ids_in_tree(child);
        }
    }
    migrate_profile_id_in_leaf(node);
}

/// The rewrite itself, on one `term`-shaped object.
///
/// Gated on `kind == "term"` rather than on the mere presence of a `profile_id` key, so a future
/// leaf kind that happens to carry a field of that name is not quietly rewritten by a step that
/// knows nothing about it.
fn migrate_profile_id_in_leaf(leaf: &mut Value) {
    let Some(object) = leaf.as_object_mut() else {
        return;
    };
    if object.get("kind").and_then(Value::as_str) != Some("term") {
        return;
    }
    let Some(current) = object.get("profile_id").and_then(Value::as_str) else {
        return;
    };
    let slug = profile_slug(current).to_owned();
    object.insert("profile_id".to_owned(), Value::from(slug));
}

/// The v6 slug for a v5 `profile_id`, which may be a slug already, an executable path, or
/// something this build has never seen.
///
/// The table is keyed on the executable's **file name**, case-insensitively, because that is the
/// only part of a Windows shell path that carries the identity: the directories differ between an
/// MSI install, a `winget` install and a Store alias, and none of those differences means the user
/// picked a different profile.
///
/// Both PowerShells map to one slug, and that is the point of the ruling rather than a loss of
/// information — `pwsh.exe` and `powershell.exe` were never two profiles, they were one profile's
/// resolution order (`BT_SHELL`, then PowerShell 7, then Windows PowerShell), and which end of it
/// a given machine landed on is a fact about the machine and not about what the user chose.
///
/// **An unrecognized value is returned verbatim**, and is not mapped to the default profile. The
/// two are not the same promise: mapping here would erase, permanently and at the next write, a
/// value that a newer build or a hand edit may well understand, whereas keeping it costs only that
/// *this* build shows the pane under the default profile — which is §5.4's 逐叶降级 ("未知
/// profile → 默认") already doing its job at read time, every launch, reversibly. A migration is
/// forever; a degradation is for as long as the build lacks the profile.
///
/// This table is a **historical mapping and not a registry**. It names the executables that
/// BetterTerminal actually wrote into v1–v5 documents, plus the slugs those become; the live list
/// of profiles is the application's, and this crate deliberately still has no opinion about which
/// profiles exist (see [`crate::layout::TermLeafV1`]).
fn profile_slug(current: &str) -> &str {
    let file_name = current.rsplit(['\\', '/']).next().unwrap_or(current).trim();
    match () {
        () if file_name.eq_ignore_ascii_case("pwsh.exe")
            || file_name.eq_ignore_ascii_case("powershell.exe") =>
        {
            "pwsh"
        }
        () if file_name.eq_ignore_ascii_case("wsl.exe") => "wsl",
        () if file_name.eq_ignore_ascii_case("cmd.exe") => "cmd",
        () if file_name.eq_ignore_ascii_case("bash.exe")
            || file_name.eq_ignore_ascii_case("git-bash.exe") =>
        {
            "gitbash"
        }
        () => current,
    }
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

    #[test]
    fn real_session_v3_to_v4_migration_preserves_explicit_theme_modes() {
        for theme in ["dark", "light"] {
            let migrated = migrate_value(
                json!({"schema_version": 3, "theme": theme, "cursor_style": "bar"}),
                3,
                4,
                SESSION_MIGRATIONS,
            )
            .unwrap();
            assert_eq!(migrated["schema_version"], json!(4));
            assert_eq!(migrated["theme"], json!(theme));
            assert_eq!(migrated["cursor_style"], json!("bar"));
        }
    }

    #[test]
    fn real_session_v4_to_v5_migration_adds_the_horizontal_expanded_defaults() {
        let migrated = migrate_value(
            json!({
                "schema_version": 4,
                "theme": "system",
                "cursor_style": "underline",
                "active_tab": 3,
                "window": {}
            }),
            4,
            5,
            SESSION_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(5));
        assert_eq!(migrated["tab_layout"], json!("horizontal"));
        assert_eq!(migrated["sidebar_mode"], json!("expanded"));
        // Rule 3 ("迁移函数只做结构升级"): everything the v4 document already
        // carried must come through byte-identical, including fields this step
        // has no opinion about.
        assert_eq!(migrated["theme"], json!("system"));
        assert_eq!(migrated["cursor_style"], json!("underline"));
        assert_eq!(migrated["active_tab"], json!(3));
        assert_eq!(migrated["window"], json!({}));
    }

    /// PIN — v5 -> v6 maps every executable path on the disk to its profile slug, in every
    /// place `profile_id` is written: a tab's tree, a nested split's leaves, and `recent`.
    ///
    /// Red gate, and it is the reason this test enumerates places rather than values: every
    /// migration before this one wrote a single key beside `schema_version`, so a step that
    /// touched only the top-level object would have looked exactly like its four predecessors
    /// and passed any test that checked `schema_version` and one field. The field this step
    /// renames is not at the top level at all — it is once per pane, arbitrarily deep in a
    /// split tree, and once per Recent row. A `recent` list left unmigrated is the failure that
    /// would survive longest unnoticed: the tabs would come back correctly and only the menu's
    /// marks would be wrong.
    #[test]
    fn real_session_v5_to_v6_migration_rewrites_profile_ids_everywhere_they_are_written() {
        let migrated = migrate_value(
            json!({
                "schema_version": 5,
                "theme": "system",
                "tabs": [
                    {
                        "root": {
                            "dir": "row",
                            "ratio": 400_000,
                            "children": [
                                {
                                    "kind": "term",
                                    "profile_id": r"C:\Program Files\PowerShell\7\pwsh.exe",
                                    "cwd": r"C:\work",
                                    "manual_name": null
                                },
                                {
                                    "dir": "col",
                                    "ratio": 500_000,
                                    "children": [
                                        {
                                            "kind": "term",
                                            "profile_id":
                                                r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe",
                                            "cwd": r"C:\deep",
                                            "manual_name": "deep"
                                        },
                                        {"kind": "files", "root": r"C:\tree", "open": [], "sel": null, "width": 260}
                                    ]
                                }
                            ]
                        },
                        "pinned": true,
                        "focused_leaf": "leaf-0"
                    }
                ],
                "active_tab": 0,
                "recent": [
                    {
                        "key": "k1",
                        "seed": {"kind": "term", "profile_id": r"C:\WINDOWS\System32\wsl.exe", "cwd": "~", "manual_name": null},
                        "timestamp": "2026-08-10T00:00:00Z"
                    },
                    {
                        "key": "k2",
                        "seed": {"kind": "files", "root": r"D:\notes"},
                        "timestamp": "2026-08-10T00:01:00Z"
                    }
                ],
                "window": {}
            }),
            5,
            6,
            SESSION_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(6));

        let root = &migrated["tabs"][0]["root"];
        assert_eq!(
            root["children"][0]["profile_id"],
            json!("pwsh"),
            "a first-level term leaf is migrated"
        );
        assert_eq!(
            root["children"][1]["children"][0]["profile_id"],
            json!("pwsh"),
            "and so is one nested two splits deep — Windows PowerShell is the same profile as \
             PowerShell 7, because they were always one profile's resolution order"
        );
        assert_eq!(
            migrated["recent"][0]["seed"]["profile_id"],
            json!("wsl"),
            "a Recent seed carries the same field and must be migrated with it"
        );

        // Rule 3: nothing else moved. The files leaf, the ratios, the pin, the cwds and the
        // Recent entry that has no profile at all come through untouched.
        assert_eq!(
            root["children"][1]["children"][1]["root"],
            json!(r"C:\tree")
        );
        assert_eq!(root["ratio"], json!(400_000));
        assert_eq!(migrated["tabs"][0]["pinned"], json!(true));
        assert_eq!(root["children"][0]["cwd"], json!(r"C:\work"));
        assert_eq!(
            root["children"][1]["children"][0]["manual_name"],
            json!("deep")
        );
        assert_eq!(
            migrated["recent"][1]["seed"],
            json!({"kind": "files", "root": r"D:\notes"})
        );
        assert_eq!(migrated["theme"], json!("system"));
    }

    /// PIN — the mapping table itself: every spelling BetterTerminal ever wrote, the slugs it
    /// already wrote, and the one case that must **not** be rewritten.
    ///
    /// Red gate for the last of those. Folding an unrecognized value into the default profile
    /// would look harmless — §5.4 already degrades an unknown profile to the default at read
    /// time — and would be the one irreversible act in this file: the read-time degradation
    /// lasts only as long as the build lacks that profile, while a migration that overwrote the
    /// value has destroyed it at the next save.
    #[test]
    fn the_profile_slug_table_maps_what_was_written_and_preserves_what_it_cannot_place() {
        for (written, expected) in [
            // The executable paths §3.3 specified as the v1 transitional value.
            (r"C:\Program Files\PowerShell\7\pwsh.exe", "pwsh"),
            (
                r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe",
                "pwsh",
            ),
            (r"C:\WINDOWS\System32\wsl.exe", "wsl"),
            (r"C:\WINDOWS\System32\cmd.exe", "cmd"),
            (r"C:\Program Files\Git\bin\bash.exe", "gitbash"),
            (r"C:\Program Files\Git\git-bash.exe", "gitbash"),
            // A bare name, which is what a `PATH`-resolved shell was written as.
            ("pwsh.exe", "pwsh"),
            // Windows paths are case-insensitive, so the file name comparison is too.
            (r"c:\windows\system32\CMD.EXE", "cmd"),
            // Forward slashes: `BT_SHELL` accepts them and so does the OS.
            ("C:/Program Files/Git/bin/bash.exe", "gitbash"),
            // The slugs the application already wrote are already v6 values.
            ("pwsh", "pwsh"),
            ("wsl", "wsl"),
            ("gitbash", "gitbash"),
            ("cmd", "cmd"),
            // And what this build cannot place is kept, letter for letter.
            ("wsl-ubuntu", "wsl-ubuntu"),
            (r"C:\Tools\my-own-shell.exe", r"C:\Tools\my-own-shell.exe"),
            ("", ""),
        ] {
            assert_eq!(
                profile_slug(written),
                expected,
                "profile_id {written:?} must migrate to {expected:?}"
            );
        }
    }

    /// PIN — a v1 document walks the whole chain and arrives at v6 with its panes intact.
    ///
    /// The chain is what a real user's oldest file actually travels, and each step is written
    /// against the shape its immediate predecessor leaves. Testing only the last hop would miss
    /// a step that stopped composing — which for this one is a live risk, because it is the
    /// first step that reads a *nested* structure rather than the top-level object.
    #[test]
    fn a_v1_document_migrates_all_the_way_to_a_slugged_v6() {
        let migrated = migrate_value(
            json!({
                "schema_version": 1,
                "tabs": [{
                    "root": {"kind": "term", "profile_id": "pwsh.exe", "cwd": r"C:\x", "manual_name": null},
                    "pinned": false,
                    "focused_leaf": "leaf-0"
                }],
                "active_tab": 0,
                "recent": [],
                "window": {}
            }),
            1,
            6,
            SESSION_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(6));
        assert_eq!(migrated["tabs"][0]["root"]["profile_id"], json!("pwsh"));
        // The four earlier steps still did their own work along the way.
        assert_eq!(migrated["theme"], json!("dark"));
        assert_eq!(migrated["cursor_style"], json!("bar"));
        assert_eq!(migrated["tab_layout"], json!("horizontal"));
        assert_eq!(migrated["sidebar_mode"], json!("expanded"));
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
