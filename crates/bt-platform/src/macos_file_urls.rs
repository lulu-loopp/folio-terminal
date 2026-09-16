//! The Service and terminal clipboard share acquisition without sharing the Service's logging.

use objc2::ClassType;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSArray, NSURL};

use crate::clipboard::{Candidate, file_urls};

pub(crate) fn paths_on(pasteboard: &NSPasteboard) -> Candidate<Vec<std::path::PathBuf>> {
    match urls_on(pasteboard) {
        Ok(urls) => file_urls(urls),
        Err(reason) => Candidate::Unreadable(reason),
    }
}

/// Retain no AppKit objects beyond the acquisition: both callers receive the same absolute URLs.
pub(crate) fn urls_on(pasteboard: &NSPasteboard) -> Result<Vec<Option<String>>, String> {
    let classes = NSArray::from_slice(&[NSURL::class()]);
    // SAFETY: NSURL conforms to NSPasteboardReading; the live class array outlives this send.
    let Some(objects) = (unsafe { pasteboard.readObjectsForClasses_options(&classes, None) })
    else {
        return Err("pasteboard file URL acquisition failed".into());
    };
    Ok(objects
        .iter()
        .map(|object| {
            object
                .downcast_ref::<NSURL>()
                .and_then(|url| url.absoluteString())
                .map(|url| url.to_string())
        })
        .collect())
}
