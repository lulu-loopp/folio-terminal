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
    (3, migrate_settings_v3_to_v4),
    (4, migrate_settings_v4_to_v5),
    (5, migrate_settings_v5_to_v6),
    (6, migrate_settings_v6_to_v7),
    (7, migrate_settings_v7_to_v8),
    (8, migrate_settings_v8_to_v9),
    (9, migrate_settings_v9_to_v10),
    (10, migrate_settings_v10_to_v11),
    (11, migrate_settings_v11_to_v12),
    (12, migrate_settings_v12_to_v13),
    (13, migrate_settings_v13_to_v14),
    (14, migrate_settings_v14_to_v15),
    (15, migrate_settings_v15_to_v16),
    (16, migrate_settings_v16_to_v17),
    (17, migrate_settings_v17_to_v18),
    (18, migrate_settings_v18_to_v19),
    (19, migrate_settings_v19_to_v20),
    (20, migrate_settings_v20_to_v21),
    (21, migrate_settings_v21_to_v22),
    (22, migrate_settings_v22_to_v23),
    (23, migrate_settings_v23_to_v24),
    (24, migrate_settings_v24_to_v25),
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

/// v3 -> v4: which profile a new tab starts from, defaulted to **unchosen**.
///
/// The third shape these three steps have taken, and it is neither of the first
/// two. `v1_to_v2` carried a behaviour forward and `v2_to_v3` took the product's
/// default for a feature shipping new; this one writes the *absence* of a choice,
/// because a v3 user was never offered the question. Writing `"pwsh"` here would
/// look identical today — PowerShell is what the reader falls back to — and would
/// differ the moment the fallback moved: every migrated file would be pinning a
/// decision its owner never made, indistinguishable from one they had.
fn migrate_settings_v3_to_v4(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(4));
        object.insert(
            "default_profile".to_owned(),
            Value::from(crate::settings::DEFAULT_PROFILE_UNSET),
        );
    }
    value
}

/// v4 -> v5: the Git panel's master switch, defaulted **on**.
///
/// The second step to take the product's default for a feature shipping new, and
/// it takes it for `migrate_settings_v2_to_v3`'s reason rather than
/// `v1_to_v2`'s: a v4 build had no Git page at all, so there is no behaviour to
/// carry forward and "off" would freeze an absence rather than preserve a status
/// quo. The one thing worth saying about writing `true` here is what it does *not*
/// do — the panel still reads nothing until a Git page is actually looked at, so
/// a migrated user's first session after this step spawns exactly as many `git`
/// processes as their last one did: none.
fn migrate_settings_v4_to_v5(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(5));
        object.insert("git_panel".to_owned(), Value::from(true));
    }
    value
}

/// v5 -> v6: which way a direction-less split cuts, defaulted to **`Auto`**.
///
/// The first of these five steps to be a plain `v1_to_v2`: a behaviour carried
/// forward, not a product default taken for a feature shipping new. Every v5
/// build cut a direction-less split across the pane's longer side — the
/// duplicate chord, and later the pane head's `⊞` — and it did so with no way to
/// ask for anything else. `Auto` is therefore not this step choosing on the
/// user's behalf; it is the answer their build has been giving them all along,
/// written down at the moment it became a question.
fn migrate_settings_v5_to_v6(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(6));
        object.insert("split_direction".to_owned(), Value::from("Auto"));
    }
    value
}

/// v6 -> v7: which language the interface is written in, defaulted to
/// **`System`**.
///
/// A third shape again, and the closest of the six to `v1_to_v2`: not a
/// behaviour carried forward exactly, but the *only* answer that has ever been
/// given. Every build before this one drew its own words in English and read
/// nothing to decide that; `System` is what that silence turns out to have
/// meant on an English Windows, and it is the answer a Chinese-Windows user
/// would have wanted all along. Writing `English` here would look identical on
/// the machine the migration runs on and would differ on the next one — it would
/// pin a decision its owner never made, indistinguishable from one they had,
/// which is exactly what `v3_to_v4` refused to do with `"pwsh"`.
fn migrate_settings_v6_to_v7(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(7));
        object.insert("language".to_owned(), Value::from("System"));
    }
    value
}

/// v7 -> v8: the grid's face, its size, and how far the PSReadLine invitation
/// has got — all three carrying **what the build already did** forward.
///
/// The first step here to insert three keys at once, and that is the ruling
/// rather than a shortcut: all three arrive with their readers in one change, so
/// splitting them across three versions would be three migrations describing one
/// moment. What matters is that none of the three values is a choice made on the
/// user's behalf.
///
/// - `""` for the family is `v3_to_v4`'s answer exactly — a v7 build drew
///   Consolas because that was the only face it had, not because anyone picked
///   it, and writing `"Consolas"` would pin a decision its owner never made
///   *and* freeze the default the day it moves.
/// - `16` for the size is `v5_to_v6`'s answer: it is the number every v7 build
///   drew at, written down at the moment it became a question.
/// - `"NotAsked"` is the literal truth. A v7 build had no invitation, so this
///   user has not been offered anything and is owed the offer once.
fn migrate_settings_v7_to_v8(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(8));
        object.insert("terminal_font_family".to_owned(), Value::from(""));
        object.insert("terminal_font_size".to_owned(), Value::from(16));
        object.insert("psreadline_invite".to_owned(), Value::from("NotAsked"));
    }
    value
}

/// v8 -> v9: which colour scheme is painted on each side of the theme, both
/// left **unnamed**.
///
/// `v3_to_v4`'s answer for the third time, and the third table it applies to:
/// a shell, then a face, now a palette. A v8 build painted one light palette
/// and one dark one because those were the only two it had, not because anyone
/// chose them, and writing this build's two default names here would look
/// identical on the machine the migration runs on while pinning a decision its
/// owner never made — indistinguishable, ever after, from a user who opened the
/// list and picked those two out of it. The day the built-in palette is renamed
/// or improved, a migrated file would still be asking for the old name and the
/// user would never see the new one.
///
/// Two keys in one step for [`crate::SETTINGS_SCHEMA_VERSION`]'s reason: the
/// two are one decision's two halves. Filling in one side and leaving the other
/// absent would leave a hole for the reader to guess at — and it is a hole the
/// user would fall into on the first day their Windows flipped the other way.
fn migrate_settings_v8_to_v9(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(9));
        object.insert(
            "light_scheme".to_owned(),
            Value::from(crate::settings::DEFAULT_LIGHT_SCHEME),
        );
        object.insert(
            "dark_scheme".to_owned(),
            Value::from(crate::settings::DEFAULT_DARK_SCHEME),
        );
    }
    value
}

/// v9 -> v10: the window's ground (a picture, its fit, two percentages) and two
/// window postures, every one of them writing down **what a v9 build already
/// did**.
///
/// This is `v5_to_v6`'s kind of step and not `v3_to_v4`'s, and the difference is
/// worth stating because six keys appearing at once looks like the other kind. A
/// v9 build drew no picture, at an opaque ground, with no system backdrop and no
/// topmost bit — not because nobody had been asked, but because those were the
/// only behaviours it had. So the values written here are not defaults chosen on
/// the user's behalf; they are the behaviour already in force, recorded. The
/// test of that distinction is the one `v3_to_v4` fails: a user who never opens
/// this page must see no change whatsoever after the migration, and here they
/// do not.
///
/// Six keys in one step for [`crate::SETTINGS_SCHEMA_VERSION`]'s reason. The
/// four ground keys in particular cannot be split: `background_fit` and the two
/// percentages are meaningless without `background_image`, and a file carrying
/// one of them and not the others would describe a picture nobody chose.
fn migrate_settings_v9_to_v10(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(10));
        object.insert(
            "background_image".to_owned(),
            Value::from(crate::settings::DEFAULT_BACKGROUND_IMAGE),
        );
        // Serialised through the enum rather than as a bare `"Fill"` literal, so
        // that renaming the variant is a compile error here instead of a silent
        // migration onto a value no reader answers to.
        object.insert(
            "background_fit".to_owned(),
            serde_json::to_value(crate::settings::BackgroundFitV1::default())
                .expect("a fieldless enum serialises to a JSON string"),
        );
        object.insert(
            "background_image_opacity".to_owned(),
            Value::from(crate::settings::DEFAULT_BACKGROUND_IMAGE_OPACITY),
        );
        object.insert(
            "background_opacity".to_owned(),
            Value::from(crate::settings::DEFAULT_BACKGROUND_OPACITY),
        );
        object.insert("acrylic".to_owned(), Value::from(false));
        object.insert("always_on_top".to_owned(), Value::from(false));
    }
    value
}

/// One key, and it carries no behaviour forward because there is none to carry:
/// every v10 file was written by a build with no Advanced group at all, so the
/// honest reading of one is "no page has been opened", which is also the
/// default. See `SettingsV1::advanced_open`.
fn migrate_settings_v10_to_v11(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(11));
        object.insert("advanced_open".to_owned(), Value::Array(Vec::new()));
    }
    value
}

/// One key, and it carries a behaviour forward rather than choosing a default — the v1→v2 shape,
/// not the v2→v3 one. Every build that wrote a v11 file rendered no tables at all, so there is no
/// preference in such a file to preserve; what there *is* is the product's answer to a question
/// that file was never asked, and the product's answer is the same `true` a fresh install gets.
/// Writing `false` would be inventing a refusal on the reader's behalf; leaving the key out would
/// leave the next reader to guess. See `SettingsV1::tables`.
fn migrate_settings_v11_to_v12(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(12));
        object.insert("tables".to_owned(), Value::from(true));
    }
    value
}

/// One key, and it carries a behaviour forward for the third time running. Every build that
/// wrote a v12 file drew every rendered block at its full height, whatever that was, because
/// there was no control that could have said otherwise; `0` is that behaviour written down, and
/// it is the same `0` a fresh install gets. Choosing a cap here would be capping blocks on a
/// reader who never asked for it, on the strength of a control they have not seen yet. See
/// `SettingsV1::block_max_height`.
fn migrate_settings_v12_to_v13(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(13));
        object.insert(
            "block_max_height".to_owned(),
            Value::from(crate::DEFAULT_BLOCK_MAX_HEIGHT),
        );
    }
    value
}

/// One key, and it carries a behaviour forward for the fourth time running — with the one
/// difference that here the behaviour was a written number rather than an absence. Every build
/// that wrote a v13 file kept exactly 100,000 frozen lines a pane, because `M0_FROZEN_LINE_QUOTA`
/// said so and there was no control that could say otherwise; this step writes that number into
/// the file rather than choosing a new one, so a reader who has never opened the row keeps the
/// history they already had. Shrinking here would delete a stranger's output on the strength of
/// a control they have not seen; growing here would spend their memory the same way. See
/// `SettingsV1::scrollback_lines`.
fn migrate_settings_v13_to_v14(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(14));
        object.insert(
            "scrollback_lines".to_owned(),
            Value::from(crate::DEFAULT_SCROLLBACK_LINES),
        );
    }
    value
}

/// One key, a fifth time, and here the behaviour being carried forward is the plainest kind
/// there is: no build that wrote a v14 file had a focus mode at all, so the answer every one of
/// them gave is `false` and this step writes it down. The temptation a settings key like this
/// invites — shipping the new mode *on* because it is the reason the version moved — is the
/// same mistake `migrate_settings_v13_to_v14` refuses about scrollback: a reader who has never
/// seen the row must open the window they opened yesterday. See `SettingsV1::focus_mode`.
fn migrate_settings_v14_to_v15(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(15));
        object.insert("focus_mode".to_owned(), Value::from(false));
    }
    value
}

/// One key, a sixth time, and this one is the strongest case in the table for the rule the
/// five before it follow. Every build that wrote a v15 file drew each cell in exactly the
/// colour the program named, so `"Off"` is the behaviour being carried forward — but unlike
/// `focus_mode`, choosing otherwise here would not merely open a different window: it would
/// **re-ink output the reader has already read**, on the strength of a row they have never
/// seen, in a scheme they deliberately chose. The floor is a repair somebody asks for. See
/// `SettingsV1::minimum_contrast`.
fn migrate_settings_v15_to_v16(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(16));
        object.insert("minimum_contrast".to_owned(), Value::from("Off"));
    }
    value
}

/// v16 -> v17: whether a program may put a message on the desktop, defaulted **on**.
///
/// One key a seventh time, and the first of the seven to take the product's default rather than the
/// answer earlier builds gave — because they gave none. No build that wrote a v16 file could
/// raise a desktop notification at all, so `false` here would not be preserving anybody's status
/// quo, only freezing an absence; that is exactly `migrate_settings_v2_to_v3`'s distinction, and
/// it reaches the opposite conclusion from `migrate_settings_v14_to_v15` for the opposite reason.
/// The difference between the two cases is whether the feature *replaces* something the reader
/// was already living with. Focus mode changes the window they open every morning. This changes
/// nothing they can see until a program asks for it, and a program that asks has been told to.
/// See `SettingsV1::terminal_notifications`.
fn migrate_settings_v16_to_v17(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(17));
        object.insert("terminal_notifications".to_owned(), Value::from(true));
    }
    value
}

/// v17 -> v18: whether a PowerShell pane with no integration is offered one, defaulted **on**.
///
/// One key an eighth time, and the second to take the product's default rather than the answer
/// earlier builds gave — for `migrate_settings_v16_to_v17`'s reason exactly: no build that could
/// write a v17 file offered anything, so `false` would freeze an absence rather than preserve a
/// status quo. What separates this from `migrate_settings_v14_to_v15` is the same test: focus
/// mode changes the window a reader opens every morning, and this changes nothing at all in a
/// window whose PowerShell already loads the script — which is most of the ways a reader can
/// arrive here, because a reader who installed it by hand is exactly the reader the criterion
/// recognises. See `SettingsV1::powershell_integration_offer`.
fn migrate_settings_v17_to_v18(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(18));
        object.insert("powershell_integration_offer".to_owned(), Value::from(true));
    }
    value
}

/// v18 -> v19: how tall a focus card's body stands, defaulted to **the height it already was**.
///
/// One key a ninth time, and it lands the way v13–v16 did rather than the way v17 and v18 did: this
/// feature does not arrive with the row, it has been on screen since 2026-08-20 at exactly one
/// height, so `160` is a behaviour being carried forward and not a product default being chosen.
/// A migration that wrote a taller card here would change the shape of a column somebody has been
/// living in, on the strength of a row they have never seen — `migrate_settings_v15_to_v16`'s
/// sentence, on a surface instead of a colour.
///
/// See `SettingsV1::focus_card_height`, and that field's note on why this step stamps the number
/// it does: it is the one it was handed second.
fn migrate_settings_v18_to_v19(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(19));
        object.insert(
            "focus_card_height".to_owned(),
            Value::from(crate::settings::DEFAULT_FOCUS_CARD_HEIGHT),
        );
    }
    value
}

/// v19 -> v20: which engine a web preview's address field hands a non-address to
/// (`docs/DESIGN.md` §7.7 ②, W2 slice ④).
///
/// One key a tenth time, and it lands the way v17 and v18 did rather than the way v13–v16 did:
/// there was no behaviour here to carry forward. Before this key there was no address field and
/// no way at all to type a word into a web preview, so the migration is not choosing between a
/// habit and a product default — it is writing the default the feature ships with, which is the
/// one `SearchEngineV1` argues for.
fn migrate_settings_v19_to_v20(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(20));
        object.insert("search_engine".to_owned(), Value::from("DuckDuckGo"));
    }
    value
}

/// v20 -> v21: whether a line too long for the pane wraps, defaulted to **the thing it has always
/// done** (`docs/plans/horizontal-scroll/plan.md` §5.7, ladder one level two).
///
/// One key an eleventh time, and it lands the way v13–v16 and v19 did rather than the way v17–v20
/// did. The distinction those steps are written under is whether there is a habit to carry or only
/// a default to choose, and here there is nothing but habit: every terminal this product has ever
/// drawn wrapped, so `true` preserves the document on somebody's screen rather than choosing a
/// side. Writing `false` here would flatten every pane in the world on the strength of a row its
/// owner has never seen — `migrate_settings_v15_to_v16`'s sentence, applied to a line instead of a
/// colour.
///
/// See `SettingsV1::line_wrapping`, and `bt_doc::LayoutKey::line_wrapping` for why this answer is
/// part of a layout's identity and not a render flag.
fn migrate_settings_v20_to_v21(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(21));
        object.insert("line_wrapping".to_owned(), Value::from(true));
    }
    value
}

/// v21 -> v22: whether a held modifier raises the card that lists what it starts, defaulted **on**
/// (`docs/DESIGN.md` §7.1.5e′).
///
/// One key a twelfth time, and it lands the way `migrate_settings_v16_to_v17` did rather than the
/// way `migrate_settings_v20_to_v21` did. The distinction those steps are written under is whether
/// there is a habit to carry or only a default to choose, and here there is no habit at all: no
/// build that could write a v22 file ever drew this card, so there is no answer to carry forward
/// and `false` would freeze an absence. What is being defaulted on is an **offer** — a surface a
/// hand has to deliberately stop for, that takes no key and leaves the moment one is pressed — and
/// a reader who does not want it has one press that ends the offering for good.
///
/// See `SettingsV1::key_hints`.
fn migrate_settings_v21_to_v22(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(22));
        object.insert("key_hints".to_owned(), Value::from(true));
    }
    value
}

/// v22 -> v23: whether the end of a turn may reach the desktop, defaulted **on**
/// (`docs/plans/attention/plan.md` §11.7, user ruling 2026-08-25).
///
/// One key a thirteenth time, and it lands the way `migrate_settings_v21_to_v22` did rather than
/// the way `migrate_settings_v20_to_v21` did. The distinction those steps are written under is
/// whether there is a habit to carry or only a default to choose, and here there is no habit: no
/// build that could write a v23 file ever flashed a taskbar button or raised a toast when an agent
/// stopped talking, because until this slice nothing in the product knew a turn had ended. `false`
/// would freeze an absence rather than preserve a status quo.
///
/// What is being defaulted on is the *quietest* of the three answers the lane can give — a flash
/// on a taskbar button of a window the reader is not looking at, and nothing at all while they
/// are. The loud one, a desktop toast, is reached only by a window that is minimised or on another
/// virtual desktop, which is the one case where nothing inside the window can be seen.
///
/// See `SettingsV1::turn_end_notification`.
fn migrate_settings_v22_to_v23(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(23));
        object.insert("turn_end_notification".to_owned(), Value::from(true));
    }
    value
}

/// **v23 → v24.** Whether entering Cards still owes the reader the `Alt`+wheel bubble.
///
/// `true` for a file written before this build, and that is the step's one real decision: an
/// existing reader has *not* been shown the sentence, because there was no sentence to be shown.
/// Migrating them in as already-spent would use the schema step to hide the feature from exactly
/// the readers who have been living without it longest.
///
/// See `SettingsV1::cards_gesture_hint_offer`.
fn migrate_settings_v23_to_v24(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(24));
        object.insert("cards_gesture_hint_offer".to_owned(), Value::from(true));
    }
    value
}

/// v24 -> v25: whether dragging a selection writes it to the clipboard, defaulted **on**
/// (`docs/plans/ui-style/invisible-gestures-2026-08-26.md`, 丙4).
///
/// One key a fifteenth time, and it lands the way `migrate_settings_v20_to_v21` did rather than
/// the way the two steps above it did. The distinction every step in this ladder is written under
/// is whether there is a habit to carry or only a default to choose, and the three of us are a
/// clean set of examples: v23 chose a default for a lane that had never existed, v24 chose one for
/// a sentence nobody had ever been shown, and this one **carries a habit and nothing else**. Every
/// build that could write a file this step reads already wrote a drag's selection to the clipboard
/// the instant it was let go — silently, and with no row to name it by. `false` here would not
/// preserve a status quo, it would end one: every reader this step ever runs for has been living
/// with the write since before the key existed, and taking it away on the strength of an upgrade
/// would be the migration changing their terminal's behaviour rather than describing it.
///
/// See `SettingsV1::copy_on_select`.
fn migrate_settings_v24_to_v25(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(25));
        object.insert("copy_on_select".to_owned(), Value::from(true));
    }
    value
}

/// Migration table for `keybindings.json`. Empty, and it will stay empty for as
/// long as the file's *shape* holds: a schema step is owed when the document
/// changes, and adding, renaming or retiring a shortcut row does not change this
/// document at all — an id this build cannot resolve is one row degrading to its
/// default (§5.4 逐叶降级), which the reader already does, per line, without a
/// version bump.
pub const KEYBINDINGS_MIGRATIONS: &[(u32, MigrationStep)] = &[];

/// Migration table for `profiles.json`. Empty for the same reason
/// [`KEYBINDINGS_MIGRATIONS`] is, and for one more of its own: this document is
/// **a list of departures**, so a build that ships a sixth profile, retires a
/// fifth or retunes a fourth's arguments changes nothing here — an entry naming
/// an id this build has never heard of is a row that degrades, and a built-in
/// this file never mentions is a row the reader appends. Neither is a version.
pub const PROFILES_MIGRATIONS: &[(u32, MigrationStep)] = &[];

/// Migration table for `pins.json`. Empty, and it starts empty for the reason
/// the two above are empty and one of its own: this document's shape is *a list
/// of rows with a category and a target*, and a build that adds a fourth
/// category changes nothing about that shape — an entry naming a category this
/// build has never heard of is a row that is carried and not offered, which
/// `PinEntryV1` already does per row without a version. A step is owed only if
/// the row itself stops being `{kind, target}`.
pub const PINS_MIGRATIONS: &[(u32, MigrationStep)] = &[];

/// Migration table for `session.json`. Schema v2 adds the runtime theme and maps every v1 session
/// to the historical dark default.
pub const SESSION_MIGRATIONS: &[(u32, MigrationStep)] = &[
    (1, migrate_session_v1_to_v2),
    (2, migrate_session_v2_to_v3),
    (3, migrate_session_v3_to_v4),
    (4, migrate_session_v4_to_v5),
    (5, migrate_session_v5_to_v6),
    (6, migrate_session_v6_to_v7),
    (7, migrate_session_v7_to_v8),
    (8, migrate_session_v8_to_v9),
    (9, migrate_session_v9_to_v10),
    (10, migrate_session_v10_to_v11),
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

/// v6 -> v7: which page each Files column was on (R1).
///
/// The second step to walk below the top-level object, and unlike
/// `v5_to_v6` it walks the tabs' trees **and not `recent`**. That is not an
/// omission: a vault entry's seed is a [`RecentSeedV1`](crate::RecentSeedV1),
/// which for a column is `{ root }` and nothing else — the whole of what a closed
/// tab can be rebuilt from — so there is no column state there to give a page to.
/// Inserting a key into it would write a field the schema does not have, and the
/// reader would drop it on the next save.
///
/// Every document written before this one was written by a build with a single
/// page, so `"files"` is not a default being imposed — it is the state those
/// columns were provably in.
fn migrate_session_v6_to_v7(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(7));
        if let Some(tabs) = object.get_mut("tabs").and_then(Value::as_array_mut) {
            for tab in tabs {
                if let Some(root) = tab.get_mut("root") {
                    migrate_files_views_in_tree(root);
                }
            }
        }
    }
    value
}

/// **v7 → v8 — the version alone, and that is the whole step.**
///
/// v8 exists because [`crate::RecentSeedV1`] gained a third shape (`preview`)
/// for the tab shape §7.1.6h added. Nothing in a v7 document acquires a field, a
/// meaning or a default: no v7 build could write a `preview` seed, so every
/// entry in an old vault is already exactly what it should be at v8 and a step
/// that touched one would be inventing a fact rather than upgrading a structure
/// (rule 3, this file's own header).
///
/// The version still has to move, and the reason is the reader on the *other*
/// side: `RecentSeedV1` has no `#[serde(other)]` arm, so a v7 build handed a
/// document containing a `preview` seed would fail to parse it altogether. With
/// the version bumped it refuses for the right reason instead — §5.4's
/// future-version refusal, which is a sentence the user can act on ("this file
/// was written by a newer Folio") rather than a corruption report about a file
/// that is perfectly well formed.
fn migrate_session_v7_to_v8(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(8));
    }
    value
}

/// **v8 → v9 — `window` becomes `windows[]`, and the old document is window
/// zero.**
///
/// The step `docs/M2-persistence-schema-v1.md` §3.1 wrote down in v1 and left
/// unpaid for seven versions: "多窗口落地时用 schema_version bump 把 `window`
/// 升格为 `windows[]`". Multiwindow slice D is that day.
///
/// **Four keys move and nothing else happens.** `window`, `tab_layout`,
/// `sidebar_mode`, `tabs` and `active_tab` were all sentences about the one
/// window a v8 document could describe, so they go inside the one entry a v8
/// document becomes; `theme`, `cursor_style` and `recent` stay where they are,
/// because they were never about a window at all — they are the process's
/// (`docs/DESIGN.md` §2.4's own question, asked of a file). No value is
/// invented, reinterpreted or dropped: a reader who upgrades and looks at their
/// window sees the window they left, in the place they left it, wearing the rail
/// they left it wearing.
///
/// **The wrap is unconditional**, even for a document whose `tabs` is empty.
/// Deciding here that an empty tab list "means no window" would be this step
/// answering a product question (rule 3: 迁移函数只做结构升级,不做语义修复) —
/// the reader already has that rule and applies it to every entry, old and new
/// alike, rather than only to the one that came through here.
///
/// A key that is somehow missing is left missing rather than defaulted, for the
/// same reason: [`SessionWindowV1`](crate::SessionWindowV1) carries `#[serde(default)]`
/// on exactly the fields whose absence has a right answer, and a migration that
/// wrote those defaults in would be making a claim the reader is better placed
/// to make.
fn migrate_session_v8_to_v9(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(9));
        let mut window = serde_json::Map::new();
        for (was, becomes) in [
            ("window", "placement"),
            ("tab_layout", "tab_layout"),
            ("sidebar_mode", "sidebar_mode"),
            ("tabs", "tabs"),
            ("active_tab", "active_tab"),
        ] {
            if let Some(moved) = object.remove(was) {
                window.insert(becomes.to_owned(), moved);
            }
        }
        object.insert(
            "windows".to_owned(),
            Value::Array(vec![Value::Object(window)]),
        );
    }
    value
}

/// v9 -> v10: where each pane's focus card aims its window (§7.1.6b′, user ruling 2026-08-21).
///
/// `v6_to_v7`'s shape on the other kind of leaf, and it walks the same two places for the same
/// reason: **the tabs' trees and not `recent`**. A vault entry's seed for a shell is
/// `{ profile_id, cwd, manual_name }` — the whole of what a closed tab can be rebuilt from — and a
/// card's aim is not part of rebuilding one; inserting a key there would write a field the schema
/// does not have.
///
/// It walks `windows[]` rather than a top-level `tabs`, which is the one difference from
/// `v6_to_v7`: v9 moved the tab list inside a window object, and a step written against the older
/// shape would silently touch nothing at all.
///
/// The answer written is `0` — the tail — because that is where every card has looked since F2.
fn migrate_session_v9_to_v10(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(10));
        if let Some(windows) = object.get_mut("windows").and_then(Value::as_array_mut) {
            for window in windows {
                let Some(tabs) = window.get_mut("tabs").and_then(Value::as_array_mut) else {
                    continue;
                };
                for tab in tabs {
                    if let Some(root) = tab.get_mut("root") {
                        migrate_card_skips_in_tree(root);
                    }
                }
            }
        }
    }
    value
}

/// Walks a persisted layout tree, giving every `term` leaf its aim.
///
/// Structurally recursive over `children` for [`migrate_profile_ids_in_tree`]'s reason.
fn migrate_card_skips_in_tree(node: &mut Value) {
    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            migrate_card_skips_in_tree(child);
        }
    }
    migrate_card_skip_in_leaf(node);
}

/// The insert itself, on one `term`-shaped object. Gated on `kind` for
/// [`migrate_profile_id_in_leaf`]'s reason, and it leaves a `card_skip` that is somehow already
/// there alone: a key this step did not write is a key some other writer meant.
fn migrate_card_skip_in_leaf(leaf: &mut Value) {
    let Some(object) = leaf.as_object_mut() else {
        return;
    };
    if object.get("kind").and_then(Value::as_str) != Some("term") {
        return;
    }
    if !object.contains_key("card_skip") {
        object.insert("card_skip".to_owned(), Value::from(0));
    }
}

/// **v10 → v11 — a preview row may name a page, and no v10 row does** (Web 预览块 W2 片③, user
/// ruling 2026-08-22).
///
/// `v7_to_v8`'s step exactly, and for its sentence: the bump exists for a distinction no v10
/// document can draw. Every string a v10 build ever wrote into a pane's `cur`, a pool row's `path`
/// or a `preview` vault seed was a path on a disk, which is precisely what
/// [`PreviewSourceV1`](crate::PreviewSourceV1)'s default says — so the honest step writes no field
/// and invents no key (rule 3: 迁移函数只做结构升级,不做语义修复).
///
/// **The version is still owed.** Nothing in this crate refuses an unknown key, so a v10 build
/// handed a v11 document would read `http://localhost:5173/app` out of `cur` as a *path* and hand
/// it to a filesystem — a silent misreading rather than a refusal. The version number is what turns
/// that into §5.4's future-version refusal, which is a sentence the reader can act on ("this file
/// was written by a newer Folio").
fn migrate_session_v10_to_v11(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema_version".to_owned(), Value::from(11));
    }
    value
}

/// Walks a persisted layout tree, giving every `files` leaf its page.
///
/// Structurally recursive over `children` for the reason
/// [`migrate_profile_ids_in_tree`] gives: a node shape this build does not
/// recognize is a node whose children it must still not lose.
fn migrate_files_views_in_tree(node: &mut Value) {
    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            migrate_files_views_in_tree(child);
        }
    }
    migrate_files_view_in_leaf(node);
}

/// The insert itself, on one `files`-shaped object. Gated on `kind` for
/// [`migrate_profile_id_in_leaf`]'s reason, and it leaves a `view` that is
/// somehow already there alone: a key this step did not write is a key some other
/// writer meant.
fn migrate_files_view_in_leaf(leaf: &mut Value) {
    let Some(object) = leaf.as_object_mut() else {
        return;
    };
    if object.get("kind").and_then(Value::as_str) != Some("files") {
        return;
    }
    if !object.contains_key("view") {
        object.insert("view".to_owned(), Value::from("files"));
    }
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
/// **The two PowerShells map to two slugs**, and this reverses an earlier reading of the same
/// ruling. While the application offered a single `PowerShell` profile whose resolution order ran
/// `BT_SHELL` → PowerShell 7 → Windows PowerShell, which end a machine landed on was a fact about
/// the machine rather than a choice, and one slug was right. The application now offers the two as
/// separate profiles (Windows Terminal's arrangement, and the one a person with both installed
/// expects), so which of them a pane was running is again something the user picked — and the
/// executable a v1–v5 document recorded is the only surviving record of it. Folding both onto
/// `"pwsh"` would spend that record: a pane that ran 5.1 would come back as PowerShell 7 wherever
/// one is installed, which is a different shell with a different language version, and no later
/// build could recover what it had been.
///
/// **An unrecognized value is returned verbatim**, and is not mapped to the default profile. The
/// two are not the same promise: mapping here would erase, permanently and at the next write, a
/// value that a newer build or a hand edit may well understand, whereas keeping it costs only that
/// *this* build shows the pane under the default profile — which is §5.4's 逐叶降级 ("未知
/// profile → 默认") already doing its job at read time, every launch, reversibly. A migration is
/// forever; a degradation is for as long as the build lacks the profile.
///
/// This table is a **historical mapping and not a registry**. It names the executables that
/// Folio actually wrote into v1–v5 documents, plus the slugs those become; the live list
/// of profiles is the application's, and this crate deliberately still has no opinion about which
/// profiles exist (see [`crate::layout::TermLeafV1`]).
fn profile_slug(current: &str) -> &str {
    let file_name = current.rsplit(['\\', '/']).next().unwrap_or(current).trim();
    match () {
        // The two PowerShells are two profiles and always were two programs;
        // this build is simply the first one that has a name for the second.
        // A v1–v5 document recorded the executable that *actually ran*, so the
        // path is a record of which of them it was, and mapping both onto one
        // slug would spend that record to save a line — a pane that ran
        // `powershell.exe` would come back as PowerShell 7 on a machine that has
        // one, which is a different shell with a different language version.
        () if file_name.eq_ignore_ascii_case("pwsh.exe") => "pwsh",
        () if file_name.eq_ignore_ascii_case("powershell.exe") => "winps",
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

    /// PIN — v5 -> v6 writes `Auto` and leaves every sibling exactly as it found
    /// it (rule 3, "迁移函数只做结构升级").
    ///
    /// The value is asserted as the *string* `"Auto"` rather than through the
    /// typed enum, because that is what this layer actually writes: a step is a
    /// `Value -> Value` and the one way it can be wrong at this level is by
    /// spelling the word differently from the `Serialize` impl that has to read
    /// it back. `settings_v5_fixture_migrates_to_v6_…` in `tests/round_trip.rs`
    /// is the other half — it reads the same step's output through the typed API,
    /// so the two spellings are pinned to agree.
    #[test]
    fn real_settings_v5_to_v6_migration_adds_the_longer_edge_default() {
        let migrated = migrate_value(
            json!({
                "schema_version": 5,
                "theme_mode": "Light",
                "display_formulas": false,
                "inline_formulas": true,
                "default_profile": "gitbash",
                "git_panel": false
            }),
            5,
            6,
            SETTINGS_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(6));
        assert_eq!(migrated["split_direction"], json!("Auto"));
        assert_eq!(migrated["theme_mode"], json!("Light"));
        assert_eq!(migrated["display_formulas"], json!(false));
        assert_eq!(migrated["inline_formulas"], json!(true));
        assert_eq!(migrated["default_profile"], json!("gitbash"));
        assert_eq!(migrated["git_panel"], json!(false));
    }

    /// PIN — v6 -> v7 writes `System` and leaves every sibling exactly as it
    /// found it (rule 3, "迁移函数只做结构升级").
    ///
    /// The fixture is non-default in all six older fields, `split_direction`
    /// deliberately among them: a step that reset a sibling to its default while
    /// inserting its own field is the one failure this shape of test exists to
    /// catch, and the most recently added sibling is the one a copy-paste of the
    /// step above is most likely to clobber.
    #[test]
    fn real_settings_v6_to_v7_migration_adds_the_follow_the_system_default() {
        let migrated = migrate_value(
            json!({
                "schema_version": 6,
                "theme_mode": "Light",
                "display_formulas": false,
                "inline_formulas": true,
                "default_profile": "gitbash",
                "git_panel": false,
                "split_direction": "Down"
            }),
            6,
            7,
            SETTINGS_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(7));
        assert_eq!(migrated["language"], json!("System"));
        assert_eq!(migrated["theme_mode"], json!("Light"));
        assert_eq!(migrated["display_formulas"], json!(false));
        assert_eq!(migrated["inline_formulas"], json!(true));
        assert_eq!(migrated["default_profile"], json!("gitbash"));
        assert_eq!(migrated["git_panel"], json!(false));
        assert_eq!(migrated["split_direction"], json!("Down"));
    }

    /// PIN — v7 -> v8 writes the unnamed family, 16, and `NotAsked`, and leaves
    /// every sibling exactly as it found it (rule 3, "迁移函数只做结构升级").
    ///
    /// The fixture is non-default in all seven older fields, `language`
    /// deliberately among them for the reason `split_direction` was above: it is
    /// the sibling added one version ago and therefore the one a copy-paste of
    /// the step above would most plausibly reset.
    ///
    /// `""` and not `"Consolas"` is the assertion with teeth. A step that wrote
    /// the build's current default face would pass every test on the machine it
    /// ran on and pin a decision its owner never made into every file on disk.
    #[test]
    fn real_settings_v7_to_v8_migration_adds_an_unnamed_face_and_an_unasked_invitation() {
        let migrated = migrate_value(
            json!({
                "schema_version": 7,
                "theme_mode": "Light",
                "display_formulas": false,
                "inline_formulas": true,
                "default_profile": "gitbash",
                "git_panel": false,
                "split_direction": "Down",
                "language": "Chinese"
            }),
            7,
            8,
            SETTINGS_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(8));
        assert_eq!(migrated["terminal_font_family"], json!(""));
        assert_eq!(migrated["terminal_font_size"], json!(16));
        assert_eq!(migrated["psreadline_invite"], json!("NotAsked"));
        assert_eq!(migrated["theme_mode"], json!("Light"));
        assert_eq!(migrated["display_formulas"], json!(false));
        assert_eq!(migrated["inline_formulas"], json!(true));
        assert_eq!(migrated["default_profile"], json!("gitbash"));
        assert_eq!(migrated["git_panel"], json!(false));
        assert_eq!(migrated["split_direction"], json!("Down"));
        assert_eq!(migrated["language"], json!("Chinese"));
    }

    /// PIN — v8 -> v9 writes two empty strings and leaves every sibling exactly
    /// as it found it (rule 3, "迁移函数只做结构升级").
    ///
    /// The fixture is non-default in all ten older fields, the three v8 keys
    /// deliberately among them: they are the siblings added one version ago and
    /// therefore the ones a copy-paste of the step above would most plausibly
    /// reset while inserting its own pair.
    ///
    /// `""` twice, and not this build's two default palette names, is the
    /// assertion with teeth — `v7_to_v8`'s `""` for the family, reappearing. A
    /// step that wrote the names it happens to ship with would pass on the
    /// machine it ran on and pin a choice its owner never made into every file
    /// on disk. That *both* are empty is the second half: filling in one side
    /// only would leave the other for the reader to guess.
    #[test]
    fn real_settings_v8_to_v9_migration_adds_two_unnamed_schemes() {
        let migrated = migrate_value(
            json!({
                "schema_version": 8,
                "theme_mode": "Light",
                "display_formulas": false,
                "inline_formulas": true,
                "default_profile": "gitbash",
                "git_panel": false,
                "split_direction": "Down",
                "language": "Chinese",
                "terminal_font_family": "Cascadia Mono",
                "terminal_font_size": 20,
                "psreadline_invite": "Installed"
            }),
            8,
            9,
            SETTINGS_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(9));
        assert_eq!(migrated["light_scheme"], json!(""));
        assert_eq!(migrated["dark_scheme"], json!(""));
        assert_eq!(migrated["theme_mode"], json!("Light"));
        assert_eq!(migrated["display_formulas"], json!(false));
        assert_eq!(migrated["inline_formulas"], json!(true));
        assert_eq!(migrated["default_profile"], json!("gitbash"));
        assert_eq!(migrated["git_panel"], json!(false));
        assert_eq!(migrated["split_direction"], json!("Down"));
        assert_eq!(migrated["language"], json!("Chinese"));
        assert_eq!(migrated["terminal_font_family"], json!("Cascadia Mono"));
        assert_eq!(migrated["terminal_font_size"], json!(20));
        assert_eq!(migrated["psreadline_invite"], json!("Installed"));
    }

    /// PIN — v9 -> v10 writes down the ground a v9 build already drew, and
    /// leaves every one of the twelve older fields exactly as it found them.
    ///
    /// The six values are asserted as literals rather than against the
    /// constants they came from, and that is the whole point of this test: a
    /// constant compared against itself proves nothing, while `100`, `100`,
    /// `false`, `false`, `""` and `"Fill"` written out here are a second,
    /// independent statement of what "no visible change for a user who never
    /// opens the page" means. Change any default and this test says so.
    ///
    /// `"Fill"` is the one value here that is a *choice* rather than a record,
    /// because a v9 build had no fit at all. It is safe to choose because it is
    /// unreachable until a picture is named: the migration is not answering a
    /// question the user was asked, it is filling the field that the next
    /// question will need.
    #[test]
    fn real_settings_v9_to_v10_migration_writes_down_the_ground_v9_already_drew() {
        let migrated = migrate_value(
            json!({
                "schema_version": 9,
                "theme_mode": "Light",
                "display_formulas": false,
                "inline_formulas": true,
                "default_profile": "gitbash",
                "git_panel": false,
                "split_direction": "Down",
                "language": "Chinese",
                "terminal_font_family": "Cascadia Mono",
                "terminal_font_size": 20,
                "psreadline_invite": "Installed",
                "light_scheme": "Solarized Light",
                "dark_scheme": "Nord"
            }),
            9,
            10,
            SETTINGS_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(10));
        assert_eq!(
            migrated["background_image"],
            json!(""),
            "no build before this one drew a picture, so there is none to name"
        );
        assert_eq!(migrated["background_fit"], json!("Fill"));
        assert_eq!(migrated["background_image_opacity"], json!(100));
        assert_eq!(
            migrated["background_opacity"],
            json!(100),
            "an opaque window is what v9 drew; the migration records it rather \
             than making somebody's terminal see-through overnight"
        );
        assert_eq!(migrated["acrylic"], json!(false));
        assert_eq!(migrated["always_on_top"], json!(false));
        assert_eq!(migrated["theme_mode"], json!("Light"));
        assert_eq!(migrated["display_formulas"], json!(false));
        assert_eq!(migrated["inline_formulas"], json!(true));
        assert_eq!(migrated["default_profile"], json!("gitbash"));
        assert_eq!(migrated["git_panel"], json!(false));
        assert_eq!(migrated["split_direction"], json!("Down"));
        assert_eq!(migrated["language"], json!("Chinese"));
        assert_eq!(migrated["terminal_font_family"], json!("Cascadia Mono"));
        assert_eq!(migrated["terminal_font_size"], json!(20));
        assert_eq!(migrated["psreadline_invite"], json!("Installed"));
        assert_eq!(migrated["light_scheme"], json!("Solarized Light"));
        assert_eq!(migrated["dark_scheme"], json!("Nord"));
    }

    /// PIN — v20 -> v21 writes down the thing every build before it did, and leaves every one of
    /// its siblings exactly as it found them (rule 3, "迁移函数只做结构升级").
    ///
    /// `true` is asserted as a literal and not against a constant, for
    /// `real_settings_v9_to_v10_…`'s reason: a constant compared with itself proves nothing, while
    /// the word written out here is a second, independent statement that "no visible change for a
    /// reader who never opens the page" means *wrapping*. A step that wrote `false` would take
    /// three quarters of every long line in every scrollback on every machine off the right-hand
    /// edge, and this is the line that says so.
    #[test]
    fn real_settings_v20_to_v21_migration_keeps_every_pane_wrapping() {
        let migrated = migrate_value(
            json!({
                "schema_version": 20,
                "theme_mode": "Light",
                "display_formulas": false,
                "scrollback_lines": 25000,
                "focus_card_height": 320,
                "search_engine": "Google"
            }),
            20,
            21,
            SETTINGS_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(21));
        assert_eq!(migrated["line_wrapping"], json!(true));
        assert_eq!(migrated["theme_mode"], json!("Light"));
        assert_eq!(migrated["display_formulas"], json!(false));
        assert_eq!(migrated["scrollback_lines"], json!(25000));
        assert_eq!(migrated["focus_card_height"], json!(320));
        assert_eq!(
            migrated["search_engine"],
            json!("Google"),
            "the sibling added one version ago is the one a copy-paste of the \
             step above would most plausibly reset"
        );
    }

    /// PIN §7.1.5e′ — **v21 -> v22 turns the hint card on for a file that predates it**, and
    /// leaves every one of its siblings exactly as it found them (rule 3, "迁移函数只做结构升级").
    ///
    /// `true` written out as a literal, for the test above's reason: a constant compared with
    /// itself proves nothing, and the word here is the second, independent statement that a
    /// default-on offer is what this step decided. A step that wrote `false` would ship the
    /// feature switched off for every reader who has ever opened this product before today —
    /// which is every reader — and this is the line that says so.
    #[test]
    fn real_settings_v21_to_v22_migration_turns_the_hint_card_on() {
        let migrated = migrate_value(
            json!({
                "schema_version": 21,
                "theme_mode": "Light",
                "line_wrapping": false,
                "scrollback_lines": 25000,
                "search_engine": "Google"
            }),
            21,
            22,
            SETTINGS_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(22));
        assert_eq!(migrated["key_hints"], json!(true));
        assert_eq!(migrated["theme_mode"], json!("Light"));
        assert_eq!(migrated["scrollback_lines"], json!(25000));
        assert_eq!(
            migrated["line_wrapping"],
            json!(false),
            "the sibling added one version ago is the one a copy-paste of the \
             step above would most plausibly reset"
        );
    }

    /// PIN (`docs/plans/ui-style/invisible-gestures-2026-08-26.md` 丙4) — **v24 -> v25 turns
    /// `copy_on_select` on for a file that predates it**, and leaves every one of its siblings
    /// exactly as it found them (rule 3, "迁移函数只做结构升级").
    ///
    /// `true` written out as a literal, for the test above's reason: a constant compared with
    /// itself proves nothing, and the word here is the second, independent statement that carrying
    /// the habit forward is what this step decided. A step that wrote `false` would silently stop
    /// copying a drag's selection for every reader who has ever run this product before today —
    /// which is every reader — and this is the line that says so.
    ///
    /// **It was written as v23 -> v24 and became v24 -> v25 on 2026-08-27**, when the Cards
    /// bubble's own key landed on `main` and claimed that number first. Two steps cannot share
    /// one number, and the ladder is `migrate_value`'s only map of the road: the later slice takes
    /// the later rung, which is the whole of the rule and needs no judgement about which key
    /// mattered more.
    #[test]
    fn real_settings_v24_to_v25_migration_turns_copy_on_select_on() {
        let migrated = migrate_value(
            json!({
                "schema_version": 24,
                "theme_mode": "Light",
                "cards_gesture_hint_offer": false,
                "scrollback_lines": 25000,
                "search_engine": "Google"
            }),
            24,
            25,
            SETTINGS_MIGRATIONS,
        )
        .unwrap();
        assert_eq!(migrated["schema_version"], json!(25));
        assert_eq!(migrated["copy_on_select"], json!(true));
        assert_eq!(migrated["theme_mode"], json!("Light"));
        assert_eq!(migrated["scrollback_lines"], json!(25000));
        assert_eq!(
            migrated["cards_gesture_hint_offer"],
            json!(false),
            "the sibling added one version ago is the one a copy-paste of the \
             step above would most plausibly reset"
        );
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
            json!("winps"),
            "and so is one nested two splits deep — and it arrives at Windows PowerShell rather \
             than at PowerShell 7, because the path is the record of which of the two actually \
             ran and this build has a profile for each"
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

    /// PIN — the mapping table itself: every spelling Folio ever wrote, the slugs it
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
            // Two PowerShells, two slugs: the path is the record of which one
            // actually ran, and this build has a profile for each.
            (
                r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe",
                "winps",
            ),
            ("powershell.exe", "winps"),
            ("winps", "winps"),
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
