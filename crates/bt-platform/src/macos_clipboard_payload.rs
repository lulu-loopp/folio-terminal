//! AppKit offers no exclusion interval; a changed changeCount discards the acquired result.

use objc2::rc::Retained;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};

use crate::clipboard::{Candidate, ClipboardPayload, ClipboardPort, ClipboardTypes, read_payload};

struct MacClipboard {
    pasteboard: Retained<NSPasteboard>,
    before: isize,
}

impl ClipboardPort for MacClipboard {
    fn begin(&mut self) -> Result<(), String> {
        self.before = self.pasteboard.changeCount();
        Ok(())
    }
    fn survey(&mut self) -> Result<ClipboardTypes, String> {
        let Some(types) = self.pasteboard.types() else {
            return Ok(ClipboardTypes::default());
        };
        let mut found = ClipboardTypes::default();
        for kind in types.iter() {
            match kind.to_string().as_str() {
                "public.file-url" | "public.url" | "NSURLPboardType" | "NSFilenamesPboardType" => {
                    found.files = true
                }
                "public.utf8-plain-text" => found.text = true,
                "public.png" => found.picture = true,
                "NSFilesPromisePboardType" | "com.apple.NSFilePromiseItemMetaData" => {
                    found.promise = true
                }
                _ => {}
            }
        }
        Ok(found)
    }
    fn files(&mut self) -> Candidate<Vec<std::path::PathBuf>> {
        crate::macos_file_urls::paths_on(&self.pasteboard)
    }
    fn text(&mut self) -> Candidate<String> {
        // SAFETY: the general pasteboard is retained through acquisition; the returned string is copied.
        match unsafe { self.pasteboard.stringForType(NSPasteboardTypeString) } {
            Some(text) => Candidate::Present(text.to_string()),
            None => Candidate::Unreadable("pasteboard text acquisition failed".into()),
        }
    }
    fn finish(&mut self) -> Result<(), String> {
        if self.pasteboard.changeCount() == self.before {
            Ok(())
        } else {
            Err("pasteboard changed during read".into())
        }
    }
}

pub fn clipboard_payload() -> Result<ClipboardPayload, String> {
    if objc2::MainThreadMarker::new().is_none() {
        return Err("clipboard read requires the event-loop thread".into());
    }
    read_payload(&mut MacClipboard {
        pasteboard: NSPasteboard::generalPasteboard(),
        before: 0,
    })
}
