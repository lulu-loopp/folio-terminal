//! **The one picture encoding this platform offers that no Rust decoder in this
//! tree can read** (§7.61).
//!
//! `public.tiff` is what an application that copies a picture through AppKit —
//! Preview, Safari, the Finder — puts on the pasteboard, and often the only
//! thing it puts there. The workspace's `image` crate is built without its TIFF
//! feature and turning that feature on is a package this lock file does not
//! have, so the decoder used here is the system's own: `NSBitmapImageRep` reads
//! the data and writes it back out as PNG, which is one object and two messages.
//!
//! # Why this is not on the window thread
//!
//! It is the encode, and a screenshot's encode is tens of milliseconds. The
//! caller is `bt_app`'s picture worker, and this file's claim to be callable
//! from it is `macos_player.rs`'s, in the same words: the *Thread Safety
//! Summary* in Apple's Cocoa Multithreading Programming Guide does not name
//! `NSBitmapImageRep` among the classes that belong to the main thread, and says
//! of everything it does not name that "in most cases, you can use these classes
//! from any thread as long as you use them from only one thread at a time". One
//! thread at a time is exactly what this is: the representation is born inside
//! this call, never leaves it, and dies in it.
//!
//! The pool is this function's own for the same reason the player's pump has
//! one per turn: everything AppKit autoreleases on a thread `bt_app` started
//! would otherwise be held until that thread ends, and the thread ends when the
//! process does.

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep};
use objc2_foundation::{NSData, NSDictionary, NSString};

/// **TIFF bytes in, PNG bytes out** — or a sentence saying which half refused.
///
/// The reasons carry no picture and no name: a clipboard diagnostic that
/// repeated what was on the clipboard would be this process writing the
/// reader's own screenshot into a log.
pub fn png_from_tiff(tiff: &[u8]) -> Result<Vec<u8>, String> {
    autoreleasepool(|_pool| {
        let data = NSData::with_bytes(tiff);
        let representation = NSBitmapImageRep::imageRepWithData(&data).ok_or_else(|| {
            "the clipboard picture is not a picture this machine reads".to_owned()
        })?;
        // An empty properties dictionary asks for the format's own defaults, which for
        // PNG is lossless and uninterlaced. There is no quality to choose.
        let properties: Retained<NSDictionary<NSString, AnyObject>> =
            // SAFETY: the dictionary is empty, so the generic parameters it is being
            // given describe nothing that is in it.
            unsafe { Retained::cast_unchecked(NSDictionary::<AnyObject, AnyObject>::new()) };
        // SAFETY: the keys this dictionary would hold are `NSString`s, which is the
        // type the parameter is declared in; it is empty, and it outlives the call.
        let png = unsafe {
            representation
                .representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        }
        .ok_or_else(|| "the clipboard picture could not be written as PNG".to_owned())?;
        let bytes = png.to_vec();
        if bytes.is_empty() {
            return Err("the clipboard picture encoded to nothing".to_owned());
        }
        Ok(bytes)
    })
}
