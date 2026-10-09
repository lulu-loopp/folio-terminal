//! **Which adapter a copy's update takes, and whether its road is built**
//! (0.4.7 ticket U-41a1; `docs/plans/design/managed-update-2026-09-29.md`
//! §1.1 R1–R2, §1.5).
//!
//! One road updates every copy (R1): `update_txn`'s transaction, whose three
//! interface points — **Prepare** (`Allocated → Prepared`, on the job's
//! worker), **Activate** forward and back (at the phase boundaries the layout
//! records: after `Moving` is durable and before `TrialBegan`, and after
//! `RollbackIntent` is durable) and **Prove / Recover** (which set is live,
//! read from the disk, at every lock holder's step) — are called through the
//! adapter the journal names ([`Adapter`], chosen here from the channel at the
//! press and recorded at `Allocated`, R2). Each road declares its points as a
//! trait beside the code that calls them: `update_prepare_windows::PreparePoint`
//! and `update_apply_windows::ApplyPoints` on Windows,
//! `update_prepare_macos::PreparePoint` and `update_apply_macos::ApplyPoints`
//! on macOS; each road's `Ours` is Folio's own layout, and a road finds the
//! adapter a journal names through [`Layouts`].
//!
//! **`Ours` and Homebrew are built** (Homebrew since 0.4.8 D1, U-41b: the
//! bundle's swap at the app target Homebrew recorded, with the cask's marks
//! carried). scoop is named so a journal can record it, and its road stays
//! off until its ticket turns it on (U-41c); winget's is off by design
//! (design (a), U-41d). A copy whose adapter is not built is never offered
//! the road — the update job's eligibility asks [`built_on`] before the card,
//! and each Prepare's road check asks it again before `Allocated` — so it
//! keeps the row with its manager's command and **Copy**, and has no journal.

use std::fmt;
use std::sync::Arc;

use bt_platform::HostPlatform;

use crate::install_channel::{Channel, Manager};
use crate::update_txn::Adapter;

/// **Homebrew's road** (U-41b, 0.4.8 D1): on, at the app target Homebrew
/// recorded and with the cask's marks carried (managed-update §2.2); a
/// Homebrew copy anywhere else keeps `brew upgrade`'s row.
pub(crate) const HOMEBREW_ROAD: bool = true;

/// **scoop's road** (U-41c turns it on with the version folder and the
/// junction; until then a scoop copy keeps `scoop update`'s row).
pub(crate) const SCOOP_ROAD: bool = false;

/// **winget's road: off in 0.4.7** (design (a), U-41d): a winget copy keeps
/// `winget upgrade`'s row with **Copy**; no precondition, no pin read and no
/// winget process. A winget road is a later ticket of its own.
pub(crate) const WINGET_ROAD: bool = false;

/// **The adapter a copy installed as `channel` takes**: its own road for a
/// copy of this account's, its manager's for a managed one, and none for a
/// copy of another account's or one whose installation is not known.
#[must_use]
pub(crate) const fn of_channel(channel: Channel) -> Option<Adapter> {
    match channel {
        Channel::Ours => Some(Adapter::Ours),
        Channel::Managed { manager, .. } => Some(of_manager(manager)),
        Channel::NotOurs | Channel::Unknown => None,
    }
}

/// The adapter of the manager that owns a copy.
#[must_use]
pub(crate) const fn of_manager(manager: Manager) -> Adapter {
    match manager {
        Manager::Homebrew => Adapter::Homebrew,
        Manager::Scoop => Adapter::Scoop,
        Manager::Winget => Adapter::Winget,
    }
}

/// **Whether `adapter`'s road is built on `platform`** (§1.5): `Ours` on
/// every platform with a road, Homebrew on macOS, scoop and winget on
/// Windows — each behind its constant.
#[must_use]
pub(crate) const fn built_on(adapter: Adapter, platform: HostPlatform) -> bool {
    match adapter {
        Adapter::Ours => true,
        Adapter::Homebrew => HOMEBREW_ROAD && matches!(platform, HostPlatform::MacOs),
        Adapter::Scoop => SCOOP_ROAD && matches!(platform, HostPlatform::Windows),
        Adapter::Winget => WINGET_ROAD && matches!(platform, HostPlatform::Windows),
    }
}

/// **A road asked for an adapter this build has no layout for.**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NotBuilt(pub(crate) Adapter);

impl fmt::Display for NotBuilt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the journal names the {:?} adapter, which this build has not built",
            self.0
        )
    }
}

/// **The layouts one road can call, by the adapter a journal names** — the
/// road's points `L` (one of the four traits the module header names).
/// Each road holds one; the product's has the road's own `Ours` and, on the
/// macOS road, its Homebrew layout; a test hands the road a layout of its own
/// in either place.
pub(crate) struct Layouts<L: ?Sized> {
    ours: Arc<L>,
    homebrew: Option<Arc<L>>,
}

impl<L: ?Sized> Layouts<L> {
    /// The layouts of a road whose own layout is `ours`, and no other.
    #[must_use]
    pub(crate) fn of(ours: Arc<L>) -> Self {
        Self {
            ours,
            homebrew: None,
        }
    }

    /// These layouts with `homebrew` as the Homebrew adapter's.
    #[must_use]
    pub(crate) fn with_homebrew(self, homebrew: Arc<L>) -> Self {
        Self {
            homebrew: Some(homebrew),
            ..self
        }
    }

    /// **The layout `adapter` names.**
    ///
    /// # Errors
    /// [`NotBuilt`] for an adapter this road holds no layout for.
    pub(crate) fn named(&self, adapter: Adapter) -> Result<Arc<L>, NotBuilt> {
        match (adapter, &self.homebrew) {
            (Adapter::Ours, _) => Ok(Arc::clone(&self.ours)),
            (Adapter::Homebrew, Some(homebrew)) => Ok(Arc::clone(homebrew)),
            (other, _) => Err(NotBuilt(other)),
        }
    }
}

impl<L: ?Sized> Clone for Layouts<L> {
    fn clone(&self) -> Self {
        Self {
            ours: Arc::clone(&self.ours),
            homebrew: self.homebrew.as_ref().map(Arc::clone),
        }
    }
}
