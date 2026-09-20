//! AppKit offers no exclusion interval; a changed changeCount discards the acquired result.

use objc2::rc::Retained;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
};

use crate::clipboard::{
    Candidate, ClipboardPayload, ClipboardPort, ClipboardTypes, MACOS_PICTURE_ORDER,
    MAX_PICTURE_BYTES, PictureBytes, PictureEncoding, PictureSource, first_offered_picture,
    read_payload,
};

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
                // Both of AppKit's picture types, because a source that copies a
                // picture through AppKit rather than through the screenshot key
                // offers `public.tiff` and nothing else (§7.61).
                "public.png" | "public.tiff" => found.picture = true,
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
    /// **The better of the two shapes, and only that one**, copied and not
    /// decoded.
    ///
    /// PNG before TIFF, for the reason the Windows arm asks for its registered
    /// `PNG` first: a source that offers it has already done the encode this
    /// paste would otherwise have to do, and every screenshot on this platform
    /// offers it. The order is [`MACOS_PICTURE_ORDER`] and the walk that stops at
    /// the first answer is [`first_offered_picture`] — the same two pieces the
    /// Windows arm uses, because the choice is the same choice.
    ///
    /// A TIFF of a 4K screen is about 33 MB, and copying it behind a PNG that
    /// was going to win was 33 MB the window thread spent for nothing.
    ///
    /// A type that is advertised and then hands back nothing is not an error on
    /// its own — a pasteboard item may promise a representation it declines to
    /// render — so the walk moves to the next shape, and so it does for a
    /// `public.png` whose bytes are not a PNG. The rung answers `Absent` only
    /// when neither shape answered at all, letting the payload fall to silence
    /// rather than raising a card about a picture nobody asked for.
    fn picture(&mut self) -> Candidate<Vec<PictureBytes>> {
        first_offered_picture(&MACOS_PICTURE_ORDER, self)
    }
    fn finish(&mut self) -> Result<(), String> {
        if self.pasteboard.changeCount() == self.before {
            Ok(())
        } else {
            Err("pasteboard changed during read".into())
        }
    }
}

impl PictureSource for MacClipboard {
    /// AppKit has no question that separates "is this shape here" from "hand it
    /// over" the way `IsClipboardFormatAvailable` does: `dataForType` is both,
    /// and it answers `None` for a type the board does not carry. So the default
    /// `true` stands and [`read`](PictureSource::read) says everything.
    fn read(&mut self, encoding: PictureEncoding) -> Option<Vec<u8>> {
        // SAFETY: the two names are AppKit's own constants, and the general pasteboard is
        // retained through acquisition; the answer is copied before it is dropped.
        let kind = unsafe {
            match encoding {
                PictureEncoding::Png => NSPasteboardTypePNG,
                PictureEncoding::Tiff => NSPasteboardTypeTIFF,
                // No pasteboard type carries a Windows DIB.
                PictureEncoding::DibV5 | PictureEncoding::Dib => return None,
            }
        };
        let data = self.pasteboard.dataForType(kind)?;
        // Asked before it is copied (review X-4): `length` is the
        // representation's own size and reading it costs nothing, where
        // `to_vec` is the allocation this ceiling exists to refuse.
        if data.is_empty() || data.len() > MAX_PICTURE_BYTES {
            return None;
        }
        let bytes = data.to_vec();
        if bytes.is_empty() { None } else { Some(bytes) }
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
