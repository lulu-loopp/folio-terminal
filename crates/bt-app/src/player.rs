//! **The page this window writes so that a video can be played inside one**
//! (user ruling 2026-08-27, route A; `docs/DESIGN.md` §7.23 ⑩).
//!
//! # Why there is a page here at all
//!
//! §7.16 measured the alternative and it does not exist. A top-level navigation
//! to `file:///…/clip.mp4` is not a document to WebView2 — the engine has no
//! viewer for a bare media response, so it turns the navigation into a
//! **download**, and this product cancels every download unconditionally. What
//! arrives on the glass is `ConnectionAborted` under a "did not respond" card.
//! That was measured twice, on `.mp4` and on `.webm`, on this repository's
//! release build with an isolated profile.
//!
//! The same measurement found the one thing that *does* work, and found it on
//! the same seat with the same file: **an ordinary local page with a
//! `<video controls>` in it plays**, with a scrubber and a running time. So the
//! only route to playing a video here is to write that page, and this module is
//! the one place it is written.
//!
//! # Why it is a file on the disk and not a string handed to the engine
//!
//! Three reasons, and any one of them settles it.
//!
//! 1. **`NavigateToString` gives the document an opaque origin**, and an opaque
//!    origin cannot load a `file:` subresource. The video would be the one thing
//!    the shell exists to show and the one thing it could not fetch. The same
//!    sentence rules out a `data:` URL shell.
//! 2. **The gate has no room for it and should not be given any.**
//!    `webnav::check` refuses `data:` unconditionally
//!    ([`webnav::Refusal::ScriptOrInlineScheme`]), and a mint is a note about a
//!    *URL*; a string navigation has no URL to mint, so admitting one would mean
//!    a hole in the one door every navigation in this product goes through.
//! 3. **A file is the form that was measured.** The page §7.16 played the video
//!    in was a file on that disk reached by a `file:` URL, and this module
//!    writes the same shape of page and reaches it the same way. Choosing the
//!    unmeasured form over the measured one, to save writing seven hundred
//!    bytes, would be trading the one piece of evidence this slice has.
//!
//! # Where it lives, and who clears it
//!
//! `%LOCALAPPDATA%\Folio\player`, beside — and deliberately not inside — the
//! `WebView2` user-data folder the engine owns. `%LOCALAPPDATA%` for
//! [`crate::webhost::user_data_folder_in`]'s reason: it is a cache, and a
//! roaming profile would carry it between machines. Reading it from the
//! environment rather than assembling it is what lets a test or a taking of
//! evidence isolate the whole of this window's disk with one variable.
//!
//! **Nobody clears it, on purpose, and the leak is bounded by construction.**
//! The shell's name is a hash of the video's own URL, so it is one file per
//! distinct recording ever played — about seven hundred bytes each — and playing
//! the same file twice rewrites the same name. A per-seat name would have needed
//! a rule for the shell of a process that crashed, and a sweep would have needed
//! a rule for the shell of the *other* Folio running against the same
//! `%LOCALAPPDATA%`. Content-addressing has neither problem: two windows playing
//! one recording write identical bytes to one path, and a crash leaves behind a
//! file the next play will simply overwrite. It goes when the profile goes.
//!
//! # What is in it, and what is deliberately not
//!
//! One `<video controls autoplay preload="metadata">` on the window's own ground
//! colour, and nothing else. **No message bridge**: `IsWebMessageEnabled` and
//! `AreHostObjectsAllowed` stay shut, which is the discipline §7.7 wrote and
//! which this slice had no reason to spend. The progress, the duration, the
//! volume and the scrubber are the engine's own controls — this window does not
//! read them, does not print them and does not need them to, because the shell
//! is the player's face and not a document the host is reporting on. The day a
//! reading of the position is wanted for something (a resume, a thumbnail
//! strip), it will be worth a door; today it would be a door with nothing behind
//! it.

use std::path::{Path, PathBuf};

use crate::webnav::{Mint, Refusal};

/// The folder the shells are written into, under a given `%LOCALAPPDATA%`.
///
/// Split from [`shell_folder`] exactly as [`crate::webhost::user_data_folder_in`]
/// is split from its own environment reader, and for the same reason: the shape
/// of the path is a fact a test can assert, and where `%LOCALAPPDATA%` is is not.
#[must_use]
pub fn shell_folder_in(local_appdata: &Path) -> PathBuf {
    local_appdata.join("Folio").join("player")
}

/// The same, on this machine.
#[must_use]
pub fn shell_folder() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(|base| shell_folder_in(Path::new(&base)))
}

/// **The shell's file name for one video URL** — a hash and nothing else.
///
/// Not the recording's own name: two files called `capture.mp4` in two folders
/// are two recordings, and a name that collided would put one reader in front of
/// another's video. Not the recording's path spelled out either — a path is
/// longer than a file name may be, and it would put the whole of somebody's
/// directory tree into a cache folder's listing.
///
/// The hash is over the **minted URL** rather than over the `Path`, so that the
/// key and the thing the page actually points at are the same string: a shell is
/// correct for a video exactly when it was written from that video's URL.
#[must_use]
fn shell_name_of(video_url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    video_url.hash(&mut hasher);
    format!("play-{:016x}.html", hasher.finish())
}

/// One string, safe to sit inside a double-quoted HTML attribute.
///
/// **All five and not the two that a Windows path can hold**, because the thing
/// being escaped is a URL and a URL is not only a path: `&` and `'` are ordinary
/// in a Windows file name, `<`, `>` and `"` are not — and an escaper that knew
/// which of its inputs were which would be an escaper somebody would later reach
/// for with a different input.
#[must_use]
fn attribute_escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// The page, as text.
///
/// `ground` is the window's own `--termbg` — the colour a preview body is
/// painted in — so that the letterboxing around a recording that is not the
/// pane's shape is the pane's own ground rather than the browser's white. It is
/// baked in at the moment the shell is written, which means a theme changed
/// while a video is playing does not repaint the letterbox until the next play.
/// That is the honest cost of a page this window does not talk to, and it is
/// cheap: the bars are the only part of the shell a theme can reach.
///
/// **`autoplay` because the reader already pressed play.** The gesture happened
/// on this window's own button; the page cannot see it, and the shell is being
/// navigated to *because* of it. A shell that then sat waiting to be pressed a
/// second time would be asking the reader to say the same thing twice.
///
/// The `Content-Security-Policy` says out loud what the shell is: it may load a
/// `file:` for its media and it may lay out its own inline style, and there is
/// no source at all for a script, a frame, an image or a connection. A page that
/// cannot fetch is a page that cannot leak the path it was given.
///
/// **It was measured, and it admits the recording** (2026-08-27, on this
/// repository's release build with an isolated profile): the `<video>` sized
/// itself to the fixture's own 160×120, which is a `loadedmetadata` and
/// therefore a media load that `media-src file:` let through. It was briefly
/// suspected of blocking one — the page came up black, at what looked like a
/// default size — and the real answer was Chromium's autoplay policy plus a
/// fixture whose first frames are deliberately black. The suspicion is recorded
/// because the next reader will have it too.
#[must_use]
pub fn shell_html(video_url: &str, ground: [u8; 3]) -> String {
    let [red, green, blue] = ground;
    let source = attribute_escaped(video_url);
    format!(
        concat!(
            "<!doctype html>\n",
            "<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n",
            "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; ",
            "media-src file:; style-src 'unsafe-inline'\">\n",
            "<title>Folio player</title>\n",
            "<style>\n",
            "html,body{{margin:0;padding:0;width:100%;height:100%;background:#{r:02x}{g:02x}{b:02x};",
            "overflow:hidden}}\n",
            "body{{display:flex;align-items:center;justify-content:center}}\n",
            "video{{max-width:100%;max-height:100%;width:auto;height:auto;",
            "background:#{r:02x}{g:02x}{b:02x};outline:none}}\n",
            "</style>\n</head>\n<body>\n",
            "<video controls autoplay preload=\"metadata\" src=\"{source}\"></video>\n",
            "</body>\n</html>\n",
        ),
        r = red,
        g = green,
        b = blue,
        source = source,
    )
}

/// **Write the shell for one video and mint the URL of it** — the whole of what
/// the play verb has to do before it can ask for a navigation.
///
/// **Two spellings of one file, and they are not interchangeable** (found on the
/// machine, 2026-08-27).
///
/// * `canonical` is what the disk says — `std::fs::canonicalize`'s answer, which
///   on Windows is a **verbatim** path (`\\?\D:\…`). It is what the URL inside
///   the shell is minted from, exactly as `Runtime::open_preview_web_file` mints
///   from a canonicalised path: the disk is what says what a path is, and the URL
///   has to be the one a reader's `↗` would resolve back to.
/// * `named` is what this window calls the file — the spelling on the pane's own
///   `PreviewImageState`, which is the string a reader typed, double-clicked or
///   dropped. It is what the mint *records*, because every surface that reads it
///   back compares it against that same pane.
///
/// **The first build passed the canonical path for both and the player closed
/// itself inside a frame.** `a_page_was_replaced` compares the recording the
/// mint names against the path the pane is showing; `\\?\D:\…\clip.mp4` and
/// `D:\…\clip.mp4` are the same file and different strings, so the pane looked
/// like a pane something else had landed on, the orphan rule retired the browser
/// on the next turn, and what a reader saw was a page's chrome flashing up and
/// the first frame coming back. Two parameters rather than one, because the two
/// jobs are genuinely two: one names a file to the operating system and the
/// other names it to this window.
///
/// The video's own URL is made by [`Mint::file`] and not by a second encoder
/// here. That is the point of asking it: `notes#1.mp4` is a file and not a
/// fragment, and there is one place in this product that knows so.
///
/// Errors are a [`Refusal`] rather than an `io::Error` because the caller has
/// exactly one thing to do with either — decline to play and say why — and
/// because a network path has to be refused here for `Mint::file`'s own reason,
/// which is not an I/O failure at all.
pub fn mint_player_shell(named: &Path, canonical: &Path, ground: [u8; 3]) -> Result<Mint, Refusal> {
    let video_url = match Mint::file(canonical)? {
        Mint::File(url) => url,
        // Unreachable by construction: `Mint::file` returns `File` or an error,
        // and it has done so since the door was written. Spelled as a refusal
        // rather than as a panic because a mint that changed shape is a thing to
        // decline, not a thing to crash a terminal over.
        _ => return Err(Refusal::NotMinted),
    };
    let folder = shell_folder().ok_or(Refusal::NotMinted)?;
    std::fs::create_dir_all(&folder).map_err(|_| Refusal::NotMinted)?;
    let shell = folder.join(shell_name_of(&video_url));
    std::fs::write(&shell, shell_html(&video_url, ground)).map_err(|_| Refusal::NotMinted)?;
    let Mint::File(url) = Mint::file(&shell)? else {
        return Err(Refusal::NotMinted);
    };
    Ok(Mint::VideoShell {
        url,
        video: named.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RED — **the shell points at the video and at nothing else, and it is the
    /// mint's own spelling of it** (user ruling 2026-08-27; §7.23 ⑩).
    ///
    /// Four claims about one string, and every one of them is a way the page has
    /// been got wrong before by somebody: the element that plays is a `<video>`
    /// with the engine's own controls; it is told to start, because the reader
    /// already pressed play on this window's button; its source is the URL
    /// [`Mint::file`] writes, character for character, so that a `#` in a file
    /// name is a `#` in a file name; and there is no script tag and no bridge,
    /// which is the discipline §7.7 wrote and this slice did not spend.
    ///
    /// RED GATE: put the path in raw instead of through `Mint::file` and the
    /// third assertion fails on the `#`, which is the exact defect that would
    /// otherwise be found by a reader whose recording is called `take#2.mp4`.
    #[test]
    fn the_shell_plays_one_video_with_the_engines_own_controls_and_no_bridge() {
        let video = Path::new(r"D:\shots\take#2.mp4");
        let Mint::File(url) = Mint::file(video).expect("a drive path mints") else {
            unreachable!("Mint::file answers File")
        };
        let html = shell_html(&url, [0x1b, 0x1b, 0x1b]);
        assert!(html.contains("<video controls autoplay"), "{html}");
        assert!(
            html.contains(r#"src="file:///D:/shots/take%232.mp4""#),
            "{html}"
        );
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("chrome.webview"), "{html}");
        // And the policy that says the page may load its media and nothing
        // else — measured to admit the recording; see [`shell_html`].
        assert!(html.contains("media-src file:"), "{html}");
        assert!(html.contains("default-src 'none'"), "{html}");
        // The ground is the window's, spelled the way CSS spells one.
        assert!(html.contains("#1b1b1b"), "{html}");
    }

    /// RED — **the mint records the name this window knows, and mints the URL
    /// from the name the disk knows** (found on the machine, 2026-08-27;
    /// `docs/DESIGN.md` §7.23 ⑩).
    ///
    /// The defect this is the gate for, in full: the first build handed the
    /// canonicalised path to both halves. On Windows `std::fs::canonicalize`
    /// answers a **verbatim** path — `\\?\D:\shots\clip.mp4` — while the pane's
    /// `PreviewImageState` holds `D:\shots\clip.mp4`, the spelling the reader
    /// arrived with. Those are one file and two strings, so `a_page_was_replaced`
    /// read the pane as one something else had landed on, the orphan rule retired
    /// the browser on the next turn, and what the machine showed was a page's
    /// chrome appearing for a frame and the first frame coming back.
    ///
    /// RED GATE: pass `canonical` for both and the first assertion fails on the
    /// prefix — which is exactly the frame that shipped and was caught.
    #[test]
    fn the_mint_records_the_name_and_mints_the_url_from_the_disk() {
        let named = Path::new(r"D:\shots\clip.mp4");
        let canonical = Path::new(r"\\?\D:\shots\clip.mp4");
        let mint =
            mint_player_shell(named, canonical, [0, 0, 0]).expect("a drive path mints a shell");
        assert_eq!(
            mint.video_behind_the_shell(),
            Some(named),
            "the window's own spelling is what every surface compares against"
        );
        // And the shell it wrote points at the disk's spelling, with the verbatim
        // prefix stripped by the one encoder that knows how — `Mint::file`.
        let shell = mint.target().expect("a shell mint names its URL");
        let path = Mint::path_and_tail_of_file_url(shell)
            .expect("this door minted it")
            .0;
        let html = std::fs::read_to_string(&path).expect("the shell is on the disk");
        assert!(
            html.contains("src=\"file:///D:/shots/clip.mp4\""),
            "the recording is reached by the path the disk agreed on:\n{html}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// RED — **an attribute cannot be left by a file name** (2026-08-27).
    ///
    /// A Windows path may hold `&` and `'`, and a URL that carried either into a
    /// double-quoted attribute unescaped would end the attribute or start an
    /// entity. `<`, `>` and `"` cannot occur in a Windows path and are escaped
    /// anyway, because this function's contract is about HTML and not about
    /// NTFS — the day it is handed a string from somewhere else, it must still
    /// be true.
    #[test]
    fn no_file_name_can_end_the_attribute_it_is_written_into() {
        assert_eq!(
            attribute_escaped(r#"a&b'c<d>e"f"#),
            "a&amp;b&#39;c&lt;d&gt;e&quot;f"
        );
        let html = shell_html(r"file:///D:/a&b/o'clock.mp4", [0, 0, 0]);
        assert!(html.contains("a&amp;b/o&#39;clock.mp4"), "{html}");
        assert!(!html.contains("a&b/"), "{html}");
    }

    /// RED — **one recording is one shell, and two recordings are two**
    /// (2026-08-27).
    ///
    /// The whole of the "who clears it" answer rests on this: the name is a
    /// function of the video's URL, so playing one file a hundred times writes
    /// one file, and two files that happen to share a name in two folders do not
    /// collide. It also pins the shape — a `.html`, because the engine is being
    /// asked for a document and an extension it does not know is how a page
    /// becomes a download again.
    #[test]
    fn a_shell_is_named_after_the_one_recording_it_is_for() {
        let one = shell_name_of("file:///D:/a/capture.mp4");
        let two = shell_name_of("file:///D:/b/capture.mp4");
        assert_ne!(one, two, "two folders are two recordings");
        assert_eq!(one, shell_name_of("file:///D:/a/capture.mp4"));
        assert!(one.ends_with(".html"), "{one}");
        assert!(one.starts_with("play-"), "{one}");
    }

    /// RED — **`%LOCALAPPDATA%`, beside the engine's folder and not inside it**
    /// (2026-08-27).
    ///
    /// Inside it would be writing into a directory another process owns and
    /// rewrites; `%APPDATA%` would roam a cache between machines, which is the
    /// mistake `webhost::user_data_folder_in` already has a test against.
    #[test]
    fn the_shells_live_beside_the_engines_profile_and_not_in_it() {
        let base = Path::new(r"C:\Users\x\AppData\Local");
        assert_eq!(
            shell_folder_in(base),
            PathBuf::from(r"C:\Users\x\AppData\Local\Folio\player")
        );
        assert_ne!(
            shell_folder_in(base),
            crate::webhost::user_data_folder_in(base)
        );
        assert!(!shell_folder_in(base).starts_with(crate::webhost::user_data_folder_in(base)));
    }
}
