//! One index per process per universe, and nothing else kept.
//!
//! §5's arithmetic is the reason this exists: the test harness runs many
//! processes, and within a process many threads, and a lowering that ran once
//! per *thread* would multiply a six-second build by the thread count for no
//! gain at all. The index is [`Send`] and [`Sync`] precisely so that one of them
//! can serve all of them.
//!
//! **What the cache holds is the lowered index and the universe that keys it.**
//! No parse tree, no file handle, no builder. A universe is its own key because
//! it is a value: two readers that declare the same roots, scopes and vendor
//! answer are asking the same question, however each of them wrote it down.
//!
//! The map's lock is never held across a build. The entry taken under it is an
//! empty [`OnceLock`]; the build happens after the lock is dropped, so a second
//! thread asking for a *different* universe does not queue behind it, and a
//! second thread asking for the *same* one waits and gets the same object rather
//! than building a duplicate.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};

use crate::index::Index;
use crate::reject::Rejection;
use crate::universe::Universe;

/// The answer for one universe, computed at most once.
type Slot = Arc<OnceLock<Result<Arc<Index>, Vec<Rejection>>>>;

static INDEXES: LazyLock<Mutex<HashMap<Universe, Slot>>> = LazyLock::new(Mutex::default);

pub(crate) fn shared(universe: &Universe) -> Result<Arc<Index>, Vec<Rejection>> {
    let slot = {
        // A poisoned map is a map some other test panicked beside, not a map
        // with a half-written entry in it: every value is either an untouched
        // `OnceLock` or a finished answer.
        let mut held = INDEXES.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(held.entry(universe.clone()).or_default())
    };
    slot.get_or_init(|| Index::build(universe).map(Arc::new))
        .clone()
}
