//! The Bring back on the storage worker (0.4.10 plan row 68d, R7; review N2): what it does when
//! the older build's write cannot be kept.

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

use bt_platform::admission::WorkerCtx;

use super::copy_back;
use crate::copy_back::{CopyBackDone, CopyBackJob};

/// `body` on a thread of its own, with the worker's context the thread door hands it.
fn on_a_bring_back_worker(body: impl FnOnce(&WorkerCtx) + Send + 'static) {
    match bt_platform::spawn_at_priority(
        "bt-bring-back-test",
        bt_platform::ThreadPriority::BelowNormal,
        body,
    )
    .expect("the thread door starts a thread")
    .join()
    {
        Ok(()) => {}
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// A scratch folder of the test's own, removed with it.
struct BringBackScratch(PathBuf);

impl Drop for BringBackScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The names in `folder`, sorted.
fn names_beside(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// RED (0.4.10 plan row 68d, R7; review N2) — **when the older build's write cannot be kept,
/// nothing is written**: the answer is that it could not be kept, the file holds the older
/// build's write byte for byte, and no copy appeared beside it.
///
/// MUTATION: a keep that fails is passed over and the copy written anyway
/// (`plant-68d-writes-when-the-keep-failed.py`).
#[test]
fn a_bring_back_whose_keep_failed_writes_nothing() {
    on_a_bring_back_worker(|worker| {
        let root = bt_testpath::temp_path("bring-back-keep-failed");
        std::fs::create_dir_all(&root).unwrap();
        let _scratch = BringBackScratch(root.clone());
        let which = root.join(crate::persist::KEYBINDINGS_FILE_NAME);
        let older: &[u8] = b"{\"schema_version\":1,\"bindings\":[]}";
        std::fs::write(&which, older).unwrap();
        let (answer, _answered) = mpsc::channel();
        let job = CopyBackJob {
            which: which.clone(),
            expected: Some(Arc::from(older)),
            bytes: Arc::from(&b"{\"schema_version\":2,\"bindings\":[]}"[..]),
            answer,
        };
        let done = copy_back(worker, &job, &|_, _, _| None);
        assert_eq!(
            done,
            CopyBackDone::NotBrought(crate::i18n::Text::CopyBackNotKept.text().to_owned())
        );
        assert_eq!(
            std::fs::read(&which).unwrap(),
            older,
            "the file is as it was"
        );
        assert_eq!(
            names_beside(&root),
            [crate::persist::KEYBINDINGS_FILE_NAME],
            "and nothing appeared beside it"
        );
    });
}
