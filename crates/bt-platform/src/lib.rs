//! Audited native bridges that are not exposed by winit's safe APIs.

use std::num::NonZeroIsize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelScrollAmount {
    Lines(u32),
    Page,
}

/// Geometry consumed by the pure half of the Win32 `WM_NCHITTEST` bridge.
/// All values are physical pixels; callers derive them from the live window DPI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustomFrameMetrics {
    pub width: i32,
    pub height: i32,
    pub title_bar_height: i32,
    pub tab_strip_right_px: i32,
    pub caption_button_width: i32,
    pub caption_button_count: i32,
    pub resize_border: i32,
    pub resizable: bool,
}

/// The self-drawn frame's two logical measurements, handed in at install time
/// rather than restated here.
///
/// # One number for the bar that is painted and the bar that is clicked
///
/// This crate used to keep its own `TITLE_BAR_LOGICAL_PX = 40` and
/// `CAPTION_BUTTON_LOGICAL_PX = 46` for the hit test while `bt-render` exported
/// `WINDOW_TITLE_BAR_LOGICAL_PX` and `WINDOW_CAPTION_BUTTON_LOGICAL_PX` for the
/// painting — two copies of the same design decision, in two crates, agreeing
/// by coincidence. The multiwindow spike (Q5, item 2) called it in: what is
/// drawn and what is clicked must be the same number, and the number belongs to
/// the side that draws it. So it arrives as an argument, and this crate no
/// longer has an opinion about how tall a title bar is.
///
/// Logical pixels at Win32's 96-DPI baseline; every read scales them by the
/// window's live DPI, so one window at 1.5x and another at 2.0x are two
/// different physical bars from one pair of numbers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustomFrameGeometry {
    pub title_bar_logical_px: u32,
    pub caption_button_logical_px: u32,
}

/// Win32 non-client regions expressed without Win32 constants so their mapping
/// can be pinned on every host used by the workspace tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustomFrameHit {
    Client,
    Caption,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Map one client-coordinate point to the native non-client region it represents.
/// Resize edges win over the title bar, and the complete settings/caption-button
/// run stays `Client` so the application can paint and handle those buttons.
#[must_use]
pub fn custom_frame_hit_test(metrics: CustomFrameMetrics, x: i32, y: i32) -> CustomFrameHit {
    let border = metrics.resize_border.max(0);
    let left = metrics.resizable && x >= 0 && x < border;
    let right = metrics.resizable && x >= metrics.width.saturating_sub(border) && x < metrics.width;
    let top = metrics.resizable && y >= 0 && y < border;
    let bottom =
        metrics.resizable && y >= metrics.height.saturating_sub(border) && y < metrics.height;

    match (left, right, top, bottom) {
        (true, _, true, _) => return CustomFrameHit::TopLeft,
        (_, true, true, _) => return CustomFrameHit::TopRight,
        (true, _, _, true) => return CustomFrameHit::BottomLeft,
        (_, true, _, true) => return CustomFrameHit::BottomRight,
        (true, _, _, _) => return CustomFrameHit::Left,
        (_, true, _, _) => return CustomFrameHit::Right,
        (_, _, true, _) => return CustomFrameHit::Top,
        (_, _, _, true) => return CustomFrameHit::Bottom,
        _ => {}
    }

    let buttons_width = metrics
        .caption_button_width
        .max(0)
        .saturating_mul(metrics.caption_button_count.max(0));
    let buttons_left = metrics.width.saturating_sub(buttons_width);
    if y < border || y >= metrics.title_bar_height.max(border) {
        return CustomFrameHit::Client;
    }
    if x < metrics.tab_strip_right_px.max(0) {
        return CustomFrameHit::Client;
    }
    if x < buttons_left || x >= metrics.width {
        CustomFrameHit::Caption
    } else {
        CustomFrameHit::Client
    }
}

/// Scale a logical chrome measurement using Win32's 96-DPI baseline.
#[must_use]
pub fn logical_px_for_dpi(logical_px: u32, dpi: u32) -> i32 {
    let scaled = u64::from(logical_px)
        .saturating_mul(u64::from(dpi.max(1)))
        .saturating_add(48)
        / 96;
    scaled.min(i32::MAX as u64) as i32
}

/// The offset a [`Compositor`] gives its GPU visual, in the physical pixels
/// every rectangle in this program is already expressed in.
///
/// # A physical pixel is already a physical pixel
///
/// DirectComposition takes `f32` where the rest of this program carries `i32`,
/// and the whole of the conversion is that cast — which is exactly the point of
/// naming it. A visual's offset is in **physical** pixels (WebView2 spike Q4:
/// measured on a 1.5x monitor and correct to the pixel), and every rectangle
/// `bt-layout` produces is already physical, so the one mistake available here
/// is to helpfully multiply by a scale factor on the way past. There is nothing
/// to multiply by; there is a cast.
///
/// The cast is exact for every coordinate a desktop can produce: `f32` carries
/// every integer up to 2^24, and a virtual desktop spanning four 8K monitors
/// does not reach one ten-thousandth of that.
#[must_use]
pub fn composition_visual_offset(x: i32, y: i32) -> (f32, f32) {
    (x as f32, y as f32)
}

/// **The name of one page's visual** inside one window's composition tree.
///
/// Two numbers and not one, and the second one is the whole of this type. A
/// window holds one visual per hosted page ([`Compositor::attach_web_visual`]),
/// and until F1b′ that visual was filed under the seat number alone. A seat
/// number is only unique inside its tab — every tab a pane is torn out into
/// starts its numbering again at one — so two tabs of one window could each
/// claim seat 1, and the second page then composed into the first page's box
/// while the first page's teardown pulled the second page's visual out of the
/// tree. That is the same failure W2 slice ③ measured when a window held a
/// single slot, arriving one level up.
///
/// The application layer is where a tab number comes from, so this type carries
/// the two halves as plain integers rather than the caller's own id types: what
/// the compositor requires is only that two pages standing in one window at one
/// time never hand it the same pair. `bt_app::LeafId` is the one thing that
/// mints them, and `TabId` is minted once per process and never reused, so the
/// pair is unique across every window as well.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PageVisual {
    /// The tab the page's pane stands in.
    pub tab: u64,
    /// The seat inside that tab.
    pub seat: u64,
}

/// Which end of a DirectComposition child list a new visual is added at.
///
/// # The argument does not read like what it does
///
/// `IDCompositionVisual::AddVisual(visual, insertAbove, referenceVisual)` with a
/// **NULL** reference does not mean "above nothing" or "below nothing" the way
/// the parameter's name suggests. With no reference visual the flag chooses an
/// end of the child list: `TRUE` inserts at the **beginning** of the list and
/// `FALSE` appends at the end — and a DirectComposition child list is painted
/// front to back in list order, so **the beginning of the list is the bottom of
/// the stack**.
///
/// This is not a reading of the documentation, it is a photograph. The W0′
/// probe built its tree with `TRUE` for both children and got the wgpu overlay
/// composed *underneath* the web page: a floating panel sliced off at the seat's
/// left edge, looking for all the world like the airspace failure of a child
/// HWND — which it was not (`docs/plans/web-preview/plan.md` §0 ④, and
/// `docs/plans/web-preview/w0p-evidence/evidence.md` §1 gate 1, where the same
/// ordering is reproduced on the second run).
///
/// So the web preview's visual is added [`VisualLayer::Bottom`] and everything
/// Folio draws itself stays above it, permanently and by construction, rather
/// than by whoever adds a visual next remembering to append.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisualLayer {
    /// Under every sibling already in the list. The web preview's visual, and
    /// the only place it may ever be.
    Bottom,
    /// Over every sibling already in the list.
    Top,
}

impl VisualLayer {
    /// The `insertAbove` argument to pass `AddVisual` **when the reference
    /// visual is NULL**, which is the only form this program calls it in.
    #[must_use]
    pub const fn insert_above_with_null_reference(self) -> bool {
        match self {
            Self::Bottom => true,
            Self::Top => false,
        }
    }
}

/// **The four switches a locally minted page is hosted behind** (W2 slice 5;
/// `docs/plans/web-preview/plan.md` section 3, the controlled file entry).
///
/// Source pins, and they have to be: three of the four are calls into a COM
/// object that only exists once a browser is running, and the fourth is an
/// argument this product deliberately never passes - there is no API anywhere
/// that reports "no additional browser arguments were given". What a machine
/// can hold is that the calls are in the file and spelled the way the ruling
/// spells them, which is the same kind of claim
/// `the_compositor_adds_the_web_visual_at_the_bottom_and_nowhere_else` makes
/// one screen up and for the same reason.
///
/// The switches are unconditional rather than thrown for `file:` pages: a
/// setting that depends on where the page came from is a setting somebody has
/// to remember to change, and this host has exactly one way to be configured.
///
/// RED GATE: delete any of the four lines. `SetAreHostObjectsAllowed` was the
/// one actually missing on 2026-08-22, which is what this test was written red
/// against.
#[cfg(test)]
mod web_security_tests {
    /// `webview.rs` with its whitespace removed, so that rustfmt's line breaks
    /// are not part of any claim below.
    fn source() -> String {
        include_str!("webview.rs")
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect()
    }

    /// The bridge is shut in both directions, and permission requests are
    /// refused before anybody is asked.
    #[test]
    fn the_page_gets_no_bridge_and_no_permissions() {
        let source = source();
        for switch in [
            concat!("SetIsWebMessage", "Enabled(false)"),
            concat!("SetAreHostObjects", "Allowed(false)"),
        ] {
            assert_eq!(
                source.matches(switch).count(),
                1,
                "exactly one call throws this switch, and it throws it off: {switch}"
            );
        }
        // The permission handler exists and its body is the denial. Written as
        // one needle rather than two facts, because a handler that is installed
        // and answers something else is worse than no handler at all.
        assert!(
            source.contains(concat!(
                "args.SetState(COREWEBVIEW2_PERMISSION",
                "_STATE_DENY)?;"
            )),
            "every permission request is refused by the handler itself"
        );
    }

    /// **No additional browser arguments at all**, which is how
    /// `--allow-file-access-from-files` is not passed.
    ///
    /// The stronger claim than "that one flag is absent", and the reason it is
    /// worth making: the environment is created with no
    /// `ICoreWebView2EnvironmentOptions`, so there is no place for *any*
    /// command line to be added. A future slice that needs one has to come
    /// through here and past this test.
    #[test]
    fn the_environment_is_created_with_no_browser_arguments() {
        let source = source();
        assert!(
            source.contains(concat!("None::<&ICoreWebView2", "EnvironmentOptions>,")),
            "the environment takes no options, so it takes no command line"
        );
        for absent in [
            concat!("Additional", "BrowserArguments"),
            concat!("allow-file-access", "-from-files"),
        ] {
            assert!(
                !source.contains(absent),
                "this host does not pass browser arguments: {absent}"
            );
        }
    }
}

#[cfg(test)]
mod visual_layer_tests {
    use super::VisualLayer;

    /// RED ①, the one the handoff names by hand: the WebView's visual is added
    /// with `AddVisual(insertAbove = TRUE, NULL)` and therefore lands at the
    /// bottom of its container — under every pixel Folio draws itself.
    #[test]
    fn the_bottom_of_a_child_list_is_reached_with_insert_above_true() {
        assert!(
            VisualLayer::Bottom.insert_above_with_null_reference(),
            "TRUE with a NULL reference inserts at the beginning of the child \
             list, and the beginning of the list is the bottom of the stack"
        );
        assert!(!VisualLayer::Top.insert_above_with_null_reference());
    }

    /// RED ②: and the compositor spells it that way at the one call site.
    ///
    /// A source pin because DirectComposition has no way to ask a visual what
    /// its children are, in any order — the stacking is observable only as
    /// pixels, and the pixels are photographed once by the probe (gate 1) and
    /// once by this slice's own acceptance shots. What a test *can* hold is that
    /// the product's one web `AddVisual` is the call the photograph was taken
    /// of.
    #[test]
    fn the_compositor_adds_the_web_visual_at_the_bottom_and_nowhere_else() {
        // Whitespace out, because rustfmt decides where the line breaks in a
        // three-argument call go and the claim is about the arguments. Every
        // needle is spelled in pieces so that this test's own source is not one
        // of the things it finds.
        let source: String = include_str!("lib.rs")
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        let opens_with = concat!("AddVisual", "(&web,");
        assert_eq!(
            source.matches(opens_with).count(),
            1,
            "exactly one call adds the web visual"
        );
        let call = &source[source
            .find(opens_with)
            .expect("the call this test just counted")..];
        assert!(
            call.starts_with(concat!(
                "AddVisual",
                "(&web,VisualLayer::Bottom.insert_above_with_null_reference(),"
            )),
            "the web visual's insertion must name the layer it is claiming \
             rather than a bare boolean: {}",
            &call[..call.len().min(120)]
        );
        assert!(
            !source.contains(concat!("AddVisual", "(&web,VisualLayer::Top")),
            "nothing may add the web visual above anything"
        );
    }

    /// **One visual per page, and every operation on one names the page**
    /// (W2 slice ③; re-keyed by F1b′).
    ///
    /// A source pin for the reason the test above is one: DirectComposition
    /// cannot be asked what is in a tree, so what a machine can hold is the shape
    /// of the calls. The bug it forbids was measured on the real window — a
    /// window that held a single slot pointed the second page's controller at the
    /// first page's visual, and then the first page's teardown took that visual
    /// out of the tree about three hundred milliseconds after the second page
    /// arrived, leaving a hole with nothing behind it.
    ///
    /// The key is a [`super::PageVisual`] and not a bare seat number, because a
    /// seat number restarts at one in every tab: a window with the same page open
    /// in two tabs of its own hands the map one name twice, which is the single
    /// slot again with two extra steps.
    ///
    /// Red gate: put the slot back to `RefCell<Option<IDCompositionVisual>>` and
    /// the first assertion fails; key it by `u64` again and it fails too; drop
    /// the `page` argument from any of the four entry points and the crate stops
    /// compiling, which is the other half.
    #[test]
    fn every_web_visual_belongs_to_one_page_and_is_reached_by_naming_it() {
        let source: String = include_str!("lib.rs")
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        assert!(
            source.contains(concat!(
                "web:RefCell<BTreeMap<PageVisual,",
                "IDCompositionVisual",
                ">>"
            )),
            "the window's web visuals are a map keyed by the whole page name, \
             neither one slot nor a bare seat number"
        );
        for entry in [
            concat!("fnattach_web", "_visual(&self,page:PageVisual)"),
            concat!("fndetach_web", "_visual(&self,page:PageVisual)"),
            concat!("fnweb_", "visual(&self,page:PageVisual)"),
            concat!("fnplace_web", "_visual(&self,page:PageVisual,"),
        ] {
            assert!(
                source.contains(entry),
                "every door onto a web visual takes the page it belongs to: {entry}"
            );
        }
    }

    /// **Two tabs numbering their seats from one are two names** (F1b′).
    ///
    /// The value half of the pin above: the map's key type itself has to tell
    /// the two apart, because everything under it — the controller's root visual
    /// target, the placement, the teardown — is reached by that key alone.
    ///
    /// MUTATION: make [`super::PageVisual`] a one-field struct over `seat` and
    /// the map holds one entry: the second tab's page composes into the first
    /// tab's box, and the first tab closing takes the live page's visual with it.
    #[test]
    fn one_seat_number_in_two_tabs_is_two_page_visuals() {
        use super::PageVisual;
        let first = PageVisual { tab: 1, seat: 1 };
        let second = PageVisual { tab: 2, seat: 1 };
        assert_ne!(
            first, second,
            "a seat number is only unique inside its tab, so the tab is part of \
             the name and not decoration on it"
        );
        let tree: std::collections::BTreeMap<PageVisual, &str> = [
            (first, "the first tab's page"),
            (second, "the second tab's page"),
        ]
        .into_iter()
        .collect();
        assert_eq!(tree.len(), 2, "two pages, two visuals");
        assert_eq!(tree.get(&second), Some(&"the second tab's page"));
    }

    /// The third angle, on a real DirectComposition device: the argument pair
    /// the two tests above are *about* is one the API accepts.
    ///
    /// It cannot be asked what order the children ended up in — no
    /// `IDCompositionVisual` tells anyone that, in any order — so what this
    /// holds is the half a machine can answer: `AddVisual(child, TRUE, NULL)`
    /// followed by `AddVisual(child, FALSE, NULL)` on one parent is legal, and
    /// a clip and an offset on the child that arrived first are legal too. The
    /// order itself is a photograph, taken twice by the probe (gate 1) and once
    /// per acceptance run by the product's own shots.
    #[cfg(windows)]
    #[test]
    fn a_real_composition_device_accepts_the_arguments_the_web_visual_is_added_with() {
        use windows::Win32::Graphics::DirectComposition::{
            DCompositionCreateDevice3, IDCompositionDesktopDevice, IDCompositionDevice3,
            IDCompositionVisual,
        };
        use windows::core::{IUnknown, Interface as _};

        let device: IDCompositionDesktopDevice =
            unsafe { DCompositionCreateDevice3(None::<&IUnknown>) }.expect("a composition device");
        let device3: IDCompositionDevice3 = device.cast().expect("the Device3 face");
        let visual = || -> IDCompositionVisual {
            unsafe { device.CreateVisual() }
                .expect("a visual")
                .cast()
                .expect("the base interface")
        };
        // Named `above` and `below` rather than `gpu` and `web`: the pin above
        // counts the calls that add *the* web visual, and there is one of those.
        let (root, above, below) = (visual(), visual(), visual());
        unsafe { root.AddVisual(&above, true, None::<&IDCompositionVisual>) }
            .expect("the first child");
        unsafe {
            root.AddVisual(
                &below,
                VisualLayer::Bottom.insert_above_with_null_reference(),
                None::<&IDCompositionVisual>,
            )
        }
        .expect("the second child, at the bottom");
        unsafe { below.SetOffsetX2(12.0) }.expect("an offset");
        let clip = unsafe { device3.CreateRectangleClip() }.expect("a rectangle clip");
        unsafe { clip.SetRight2(320.0) }.expect("a clip edge");
        unsafe { below.SetClip(&clip) }.expect("the clip on the bottom visual");
        unsafe { device.Commit() }.expect("a commit");
    }
}

/// The parameters `explorer.exe` is handed to **reveal** a path (user ruling,
/// 2026-08-13).
///
/// # 「Show me where this is」, not 「open the folder it is in」
///
/// Every foot in this window carries a path and offers to take you to it. What
/// that used to mean was `ShellExecute("open", <the parent folder>)` — Explorer
/// opened on a directory and the file the foot was actually naming was one of
/// two hundred rows, indistinguishable from the rest. `/select` is the verb
/// that means what the foot says: the folder opens **with that item
/// highlighted**.
///
/// A directory keeps the old answer, and that is a judgement rather than a
/// limitation: `/select` on a folder opens its *parent* with the folder
/// highlighted, which is one level further out than a foot pointing at a root
/// is offering. Looking *inside* it is the natural reading of a tree's own
/// root, so a directory is opened and a file is selected.
///
/// Pure, because the one thing that can be wrong here is the string — Explorer
/// parses `/select,<path>` as a single token and wants the path quoted, and a
/// command line that is a quote out is a command line that silently opens
/// `Documents` instead.
#[must_use]
pub fn reveal_arguments(path: &std::path::Path, is_directory: bool) -> std::ffi::OsString {
    let mut arguments = std::ffi::OsString::new();
    if !is_directory {
        arguments.push("/select,");
    }
    arguments.push("\"");
    arguments.push(path.as_os_str());
    arguments.push("\"");
    arguments
}

/// The Windows path one `file:` URI names, or nothing when it names none.
///
/// # Why this is here and why it is pure
///
/// A `file:` URI arriving out of an OSC 8 hyperlink is a *string a program
/// printed*, and every door this crate opens onto the shell takes a `Path`. One
/// of the two has to become the other, and the translation is the only place the
/// whole thing can go wrong: a `%20` left undecoded opens nothing, a `%2e%2e`
/// decoded twice opens the wrong thing, and a lenient reading of a stray `%`
/// would be exactly the guessing `CONVENTIONS.md` §1 forbids. So it is written
/// once, next to [`reveal_arguments`] and for its reason — the one thing that can
/// be wrong here is the string — and it is a pure function with its own tests
/// rather than a step inside a bridge that can only be exercised by launching
/// something.
///
/// It answers only what this machine could actually name. Nothing here checks
/// that the path *exists*, and nothing here decides what to do with it: the
/// caller's own policy — preview, Explorer, refuse — is a separate question and
/// stays where the rest of that policy lives.
///
/// # The reading
///
/// * The scheme is matched case-insensitively; anything but `file` is not ours.
/// * A fragment (`#…`) and a query (`?…`) are cut. They are URI syntax, not part
///   of any filename.
/// * `file://<host>/…` with an empty host or `localhost` is this machine;
///   any other host is the UNC share `\\<host>\…` it plainly means (RFC 8089
///   §2). `file:/C:/…` and `file:C:/…`, which emitters do produce, are read as
///   the same local path with no authority at all.
/// * Percent-escapes decode to bytes and the bytes must be UTF-8 — which is what
///   `%E4%B8%AD` being a Chinese character rather than three broken ones depends
///   on. A `%` that is not followed by two hex digits makes the whole URI
///   unreadable rather than a literal percent: a filename holding a real `%` is
///   `%25` in a URI, and treating the malformed case as text is how a decoder
///   starts opening files nobody named.
/// * Forward slashes become backslashes, and the leading slash in front of a
///   drive letter goes away, so `file:///C:/a%20b.md` is `C:\a b.md`.
/// * The result must be drive-rooted or UNC. A POSIX URI such as
///   `file:///etc/passwd` names nothing on this machine and is refused rather
///   than turned into a relative path that would resolve against whatever the
///   process's current directory happens to be.
/// * A NUL or an ASCII control character anywhere — before decoding or after —
///   is a refusal. Raw non-ASCII is kept as itself, which is the IRI a browser
///   would accept.
#[must_use]
pub fn file_uri_to_path(uri: &str) -> Option<std::path::PathBuf> {
    let (scheme, rest) = uri.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    // Fragment before query: `?` inside a fragment is fragment text, and cutting
    // the other way round would leave it behind.
    let rest = rest.split_once('#').map_or(rest, |(head, _)| head);
    let rest = rest.split_once('?').map_or(rest, |(head, _)| head);
    if rest
        .chars()
        .any(|character| character <= ' ' || character == '\u{7f}')
    {
        return None;
    }
    let (host, path) = match rest.strip_prefix("//") {
        // The authority runs to the next separator; without one there is a host
        // and no path, which names a machine rather than a file on it.
        Some(authority) => match authority.find(['/', '\\']) {
            Some(cut) => (&authority[..cut], &authority[cut..]),
            None => return None,
        },
        None => ("", rest),
    };
    let host = percent_decode_utf8(host)?;
    let path = percent_decode_utf8(path)?;
    if path.is_empty() {
        return None;
    }
    let local = host.is_empty() || host.eq_ignore_ascii_case("localhost");
    let mut text = if local {
        // `/C:/x` → `C:/x`. Only in front of a drive letter: a lone leading
        // slash anywhere else is a POSIX path, which the root check below
        // refuses on purpose.
        let bytes = path.as_bytes();
        if bytes.len() >= 3
            && matches!(bytes[0], b'/' | b'\\')
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b':'
        {
            path[1..].to_owned()
        } else {
            path
        }
    } else {
        // `path` still carries the separator that ended the authority, so this
        // is `\\host` + `\share\…` and never `\\hostshare`.
        format!(r"\\{host}{path}")
    };
    text = text.replace('/', "\\");
    if text.contains('\0') || text.chars().any(|character| character.is_control()) {
        return None;
    }
    let bytes = text.as_bytes();
    let drive_rooted =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    // The same shape `validate_openable_path` asks of every path this crate will
    // hand to the shell, asked one step earlier so a URI can never become a
    // relative path in the first place.
    let unc = text.starts_with(r"\\") && text.len() > 2;
    (drive_rooted || unc).then(|| std::path::PathBuf::from(text))
}

/// Percent-escapes to bytes to UTF-8, strictly: a `%` without two hex digits
/// after it, or a byte sequence that is not UTF-8, makes the whole string
/// unreadable rather than partly guessed.
fn percent_decode_utf8(text: &str) -> Option<String> {
    if !text.contains('%') {
        return Some(text.to_owned());
    }
    let source = text.as_bytes();
    let mut decoded = Vec::with_capacity(source.len());
    let mut at = 0;
    while at < source.len() {
        if source[at] == b'%' {
            let high = hex_digit(*source.get(at + 1)?)?;
            let low = hex_digit(*source.get(at + 2)?)?;
            decoded.push(high << 4 | low);
            at += 3;
        } else {
            decoded.push(source[at]);
            at += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// ── Explorer's context menu (Windows landing block, slice 2) ───────────────
//
// The three values one classic shell verb is made of, the trees it is written
// into, and the arithmetic that decides whether what is on the machine is what
// this build would write. Everything in this section is **pure**: it is a
// description of a registry, not a registry. The reads and the writes are
// `windows_impl`'s [`read_context_menu`], [`install_context_menu`] and
// [`remove_context_menu`], which do nothing but carry these strings across.
//
// That division is not tidiness. A test that had to *write* to decide whether
// the shape was right would be a test writing into the reader's own Explorer
// menu, and the only shapes worth pinning — the quoting of a path with a space
// in it, the `,0` on the icon, what counts as "not what we would write now" —
// are all decided before any key is opened.

/// The three `REG_SZ` values one Explorer verb is made of.
///
/// `label` is the words in the menu, `icon` is `"<path>,<index>"` — the shell's
/// own spelling for "the n-th icon in that file" — and `command` is the literal
/// command line the shell runs, `%V` and all.
///
/// Held together in one struct because they are written together, read back
/// together and **compared together**: see [`context_menu_verdict`], where a
/// verb whose command still points at yesterday's folder and a verb whose label
/// is in the language the reader has since left are the same finding.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextMenuShape {
    /// The menu label.
    pub label: String,
    /// `Icon`, as `"<absolute path to the exe>,0"`.
    pub icon: String,
    /// The `command` subkey's own default value.
    pub command: String,
}

/// The key each verb is written under, one level below its tree.
///
/// The product's own name and not a GUID: a classic verb's key name is what
/// `ShellExecuteEx`'s `lpVerb` addresses it by, it is visible to anyone who
/// opens `regedit`, and it is the thing a user who wants this gone by hand goes
/// looking for.
pub const CONTEXT_MENU_VERB_KEY: &str = "Folio";

/// Where per-user class registrations live, under `HKEY_CURRENT_USER`.
///
/// **`HKCU` and never `HKLM`.** Everything this product writes to the registry
/// belongs to the account that ran it, needs no elevation to write or to remove,
/// and is invisible to every other user of the machine — which is the only
/// honest shape for a program that ships as a bare `.exe` with no installer
/// behind it (`docs/spikes/spike-win-landing.md` §6).
pub const CONTEXT_MENU_CLASSES: &str = r"Software\Classes";

/// The shell trees this verb is written into, relative to [`CONTEXT_MENU_CLASSES`].
///
/// Two, because two different things get right-clicked: a folder's own icon in a
/// listing, and the empty space *inside* a folder that is already open. They are
/// separate trees in the registry and a verb in one of them is invisible from
/// the other.
///
/// **`Drive\shell` is the third the spike measured and is deliberately not here**
/// (`spike-win-landing.md` §4). A drive root is not a `Directory` as far as the
/// shell is concerned, so right-clicking `C:` in *This PC* finds no verb today.
/// Adding it is one entry in this array and nothing else — every other part of
/// this section, the install, the read-back, the verdict and the prune, is
/// written over the array rather than over the number two.
pub const CONTEXT_MENU_TREES: [&str; 2] = [r"Directory\shell", r"Directory\Background\shell"];

/// The three values this build would write for `exe`, given the menu's words.
///
/// **`%V` for both trees, and `%1` for neither.** `%1` works for `Directory` and
/// would silently do nothing for `Directory\Background`, where the clicked
/// object is the folder's window rather than a folder; `%V` is the substitution
/// that means "the thing that was right-clicked" in all of them, so it is the
/// uniform choice and there is no per-tree command.
///
/// The path is quoted and the `%V` is quoted, and both matter: `C:\Program
/// Files\…` unquoted is two arguments, and a folder with a space in its name is
/// the ordinary case rather than the exotic one.
#[must_use]
pub fn context_menu_shape(exe: &std::path::Path, label: &str) -> ContextMenuShape {
    let exe = exe.display();
    ContextMenuShape {
        label: label.to_owned(),
        // `,0` — the first icon in the executable's own resources, which is the
        // application icon. An `.ico` beside the exe would be a second file that
        // has to be shipped, found and kept in step with the binary; the icon
        // Explorer already draws for `folio.exe` is by construction the right
        // one.
        icon: format!("{exe},0"),
        command: format!("\"{exe}\" --cwd \"%V\""),
    }
}

// ── Desktop notifications (Windows landing block, slice 3) ────────────────
//
// The identity Windows files a notification under, and the one document a toast
// is. Everything in this section is **pure**, on the Explorer verb's own
// division: the strings are decided here and `windows_impl::Notifier` does
// nothing but carry them across. What can go wrong in a toast is an unescaped
// ampersand in a shell's error message, and that is settled before any COM
// object exists.

/// The AppUserModelID every Folio notification is sent under.
///
/// **This string is an identity and must never change.** Windows files the
/// user's own per-app notification preferences — banners on or off, sounds,
/// priority in Focus Assist — under this exact text, at
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\<AUMID>`.
/// Renaming it would not rename those settings; it would abandon them and start
/// a second app beside the first in the reader's Settings list.
///
/// Reverse-DNS-ish and without spaces, which is the convention the platform's
/// own entries follow.
pub const NOTIFICATION_AUMID: &str = "Folio.Terminal";

/// Where that identity is declared, as a path below `HKEY_CURRENT_USER`.
///
/// **`HKCU` for [`CONTEXT_MENU_CLASSES`]'s reason** and one of its own: this is
/// per-user state by construction — it is the sender name shown to *this*
/// account, beside the notification settings *this* account chose.
#[must_use]
pub fn notification_aumid_key() -> String {
    format!(r"Software\Classes\AppUserModelId\{NOTIFICATION_AUMID}")
}

/// The name shown on the toast and in Windows' own notification settings.
///
/// The product name and not the window title: what this names is the *sender*,
/// and the sender is Folio however many windows it has open.
pub const NOTIFICATION_DISPLAY_NAME: &str = "Folio";

/// One toast, as the XML the notification platform parses.
///
/// `ToastGeneric` with one or two text lines — the template every unpackaged
/// desktop app gets, and the only one that needs no image assets. The second
/// line is omitted rather than left empty when there is no body, because an
/// empty `<text>` is a blank line the platform still lays out.
///
/// `launch` is handed back verbatim as the activation argument when the toast is
/// clicked; it is this product's only channel for saying *which pane* a
/// notification came from. `activationType="foreground"` is what makes the click
/// an activation at all rather than a dismissal.
///
/// **Every one of the three is escaped**, and that is the whole reason this is a
/// function and not a `format!` at the call site: a body is arbitrary text from
/// the far end of a pipe, `make: *** [all] Error 2 && exit` is an ordinary thing
/// for a shell to print, and an unescaped `&` is a document the platform refuses
/// to parse — which would turn one hostile line of output into a terminal that
/// silently stops notifying.
#[must_use]
pub fn toast_xml(title: &str, body: &str, launch: &str) -> String {
    let mut xml = format!(
        "<toast launch=\"{launch}\" activationType=\"foreground\">\
<visual><binding template=\"ToastGeneric\"><text>{title}</text>",
        launch = xml_escape(launch),
        title = xml_escape(title),
    );
    if !body.is_empty() {
        xml.push_str(&format!("<text>{}</text>", xml_escape(body)));
    }
    xml.push_str("</binding></visual></toast>");
    xml
}

/// XML's five predefined entities, and nothing else.
///
/// `'` and `"` are escaped even though only one of the three call sites is an
/// attribute: a function that escaped by position would have to be told which
/// position, and the entity is legal in element content as well.
fn xml_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

/// What the machine's registry says about this verb, against what this build
/// would write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenuState {
    /// No tree carries the verb. The row reads `Off` and there is nothing to
    /// re-assert.
    Absent,
    /// Every tree carries exactly the shape this build would write now.
    Current,
    /// At least one tree carries the verb, and what is written is not what this
    /// build would write now.
    ///
    /// The case this exists for is a **moved `folio.exe`**: the `command` value
    /// holds an absolute path, so a binary dragged to another folder leaves a
    /// menu entry that opens nothing at all, and there is no installer to notice.
    /// It is also the case of a verb written in a language the reader has since
    /// left, and of a set of trees somebody deleted half of by hand. All three
    /// have the same repair — write the shape again — so they are one answer.
    Stale,
}

/// Read the trees' answers as one verdict.
///
/// `found` is parallel to [`CONTEXT_MENU_TREES`]: `None` where the tree carries
/// no verb, `Some` with whatever was actually read where it does.
///
/// **A partial set is `Stale` and not `Absent`.** The difference is what happens
/// next: `Absent` means the switch reads `Off` and the launch leaves the
/// registry alone, and a machine whose folder verb survived while its background
/// verb was deleted would then have a switch reading `Off` over a menu entry
/// that is still there.
#[must_use]
pub fn context_menu_verdict(
    found: &[Option<ContextMenuShape>],
    desired: &ContextMenuShape,
) -> ContextMenuState {
    if found.iter().all(Option::is_none) {
        return ContextMenuState::Absent;
    }
    if found.len() == CONTEXT_MENU_TREES.len()
        && found
            .iter()
            .all(|shape| shape.as_ref().is_some_and(|shape| shape == desired))
    {
        return ContextMenuState::Current;
    }
    ContextMenuState::Stale
}

/// The extensions this product will decode a picture from, lower case.
///
/// **One list, three readers.** It is the file chooser's filter
/// ([`image_file_filter_spec`]), it is `validate_local_image_path`'s admission
/// gate, and it is the same set `bt_term::has_admissible_image_extension`
/// answers with — a chooser that offered a format the decoder refuses is a
/// dialog that lets you pick a file and then says no, which is worse than not
/// offering it. `svg` is on the list and does not go through the `image` crate;
/// it is rasterised by `bt_math`, and that is the decoder's business rather than
/// this list's.
pub const IMAGE_FILE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "svg"];

/// [`IMAGE_FILE_EXTENSIONS`] as a common-dialog filter — `*.png;*.jpg;…`.
///
/// Built rather than written out, so the filter cannot fall behind the list the
/// decoder actually honours. One spec and not one per format: a chooser with
/// six entries in its type dropdown asks the user to classify their own
/// wallpaper before they can see it.
#[must_use]
pub fn image_file_filter_spec() -> String {
    IMAGE_FILE_EXTENSIONS
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// One monospaced family this machine has, named and located.
///
/// **Both halves, and that is why the type exists.** A font picker needs the
/// name — it is what goes in the list, what is written to `settings.json`, and
/// what a shaper is later asked to resolve. But a name alone cannot be honoured:
/// `bt-render`'s font database is deliberately not a system-wide enumeration
/// (see its `terminal_font_system`), so a family it has never loaded is a family
/// `Family::Name("…")` falls back out of. Handing back the files alongside the
/// name is what lets the chosen family actually be loaded, and it is why this
/// goes through DirectWrite rather than through GDI's cheaper
/// `EnumFontFamiliesExW`, which answers only the first half.
///
/// `files` may hold several paths — a family's regular, bold, italic and bold
/// italic are four faces and usually four files — and may be *empty*, which is
/// not a failure: see [`DEFAULT_MONOSPACE_FAMILY`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonospaceFamily {
    /// The family name as DirectWrite reports it, preferring the machine's own
    /// UI language and falling back to the first localized name the family has.
    pub name: String,
    /// Every file the family's faces live in, de-duplicated, in the order the
    /// faces were reported.
    pub files: Vec<std::path::PathBuf>,
}

/// **The page Windows installs fonts on**, and the door
/// `open_system_fonts_page` knocks on first (user ruling 2026-08-19).
///
/// Named here rather than spelled at the call site so that the one URI this
/// build hands the shell is a constant a reader can find by searching for it.
pub const FONT_SETTINGS_URI: &str = "ms-settings:fonts";

/// The folder that page is a view of — where `open_system_fonts_page` goes when
/// the URI has no handler.
///
/// `%WINDIR%\Fonts`, with the documented default when the variable is unset:
/// this is a machine-wide location, not a user one, and a terminal that guessed
/// `C:` because an environment variable had been cleared would be opening a
/// folder on the wrong drive on exactly the machines that clear it.
#[must_use]
pub fn fonts_folder() -> std::path::PathBuf {
    let root = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
    std::path::PathBuf::from(root).join("Fonts")
}

/// The family the renderer draws when a settings file names none.
///
/// It is a constant here rather than a lookup because [`monospace_font_families`]
/// promises the list contains it: a picker whose list can come back without the
/// entry that is currently selected is a picker that shows a blank row on the
/// one machine where DirectWrite refuses.
pub const DEFAULT_MONOSPACE_FAMILY: &str = "Consolas";

/// Sort the enumerated families into the order a list draws them in, and
/// guarantee the default is among them.
///
/// Split out from the enumeration itself so the ordering promise can be tested
/// on any host: the DirectWrite half needs a Windows font collection, this half
/// needs nothing, and the property worth pinning — *the list is deterministic
/// and always contains the default* — lives entirely here.
///
/// Case-insensitive because "consolas" and "Consolas" are the same family to
/// every user who reads the list, and because DirectWrite's own ordering is the
/// collection's internal one, which is not stable between machines or between
/// font installs on one machine.
///
/// The default, if the collection did not report it, is inserted with **no
/// files**, and that is the honest value rather than a placeholder: the
/// renderer already has Consolas loaded from its fixed startup list, so there is
/// nothing to load — an empty `files` says "this family needs no loading", which
/// is exactly true of every face the startup list already covers.
#[must_use]
pub fn order_monospace_families(mut families: Vec<MonospaceFamily>) -> Vec<MonospaceFamily> {
    families.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    families.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    if !families
        .iter()
        .any(|family| family.name.eq_ignore_ascii_case(DEFAULT_MONOSPACE_FAMILY))
    {
        let at = families
            .iter()
            .position(|family| family.name.to_lowercase() > DEFAULT_MONOSPACE_FAMILY.to_lowercase())
            .unwrap_or(families.len());
        families.insert(
            at,
            MonospaceFamily {
                name: DEFAULT_MONOSPACE_FAMILY.to_owned(),
                files: Vec::new(),
            },
        );
    }
    families
}

/// WebView2 in composition hosting — the web preview block's engine (slice ①).
///
/// A file of its own rather than another region of [`windows_impl`], because it
/// is a second unsafe boundary against a second SDK: everything in
/// `windows_impl` is Win32 through the `windows` crate, and everything here is
/// WebView2 through `webview2-com`. They share this crate's exemption from
/// `unsafe_code = "deny"` and nothing else.
#[cfg(windows)]
mod webview;

#[cfg(windows)]
pub use webview::{
    REHOST_SEQUENCE, RehostCompensation, RehostOutcome, RehostSide, RehostStep, WebChord,
    WebDpiOwnership, WebEvent, WebHost, WebKey, WebMouseEvent, WebNavigationVerdict,
    forget_web_environment, rehost_compensation, web_mouse_buttons, webview2_runtime_version,
};

#[cfg(windows)]
mod windows_impl {
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        ffi::c_void,
        os::windows::ffi::OsStrExt,
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex, OnceLock,
            atomic::{AtomicI32, Ordering},
        },
    };
    use windows::core::{HRESULT, HSTRING, IUnknown, Interface, PCWSTR, PWSTR};

    // The notification centre's own three namespaces: the document a toast is,
    // the event a click arrives on, and the objects that carry both. See
    // `Notifier`.
    use windows::{
        Data::Xml::Dom::XmlDocument,
        Foundation::TypedEventHandler,
        UI::Notifications::{
            ToastActivatedEventArgs, ToastNotification, ToastNotificationManager, ToastNotifier,
        },
    };

    use windows::Win32::{
        Foundation::{
            COLORREF, CloseHandle, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND, GENERIC_WRITE,
            GetLastError, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT,
            RPC_E_CHANGED_MODE, SetLastError, WAIT_EVENT, WAIT_OBJECT_0, WIN32_ERROR, WPARAM,
        },
        Globalization::{GetUserDefaultUILanguage, GetUserPreferredUILanguages, MUI_LANGUAGE_NAME},
        Graphics::DirectComposition::{
            DCompositionCreateDevice3, IDCompositionDesktopDevice, IDCompositionDevice3,
            IDCompositionTarget, IDCompositionVisual,
        },
        Graphics::DirectWrite::{
            DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory, IDWriteFont1,
            IDWriteFontCollection, IDWriteFontFace, IDWriteFontFile, IDWriteLocalFontFileLoader,
            IDWriteLocalizedStrings,
        },
        Graphics::Dwm::{
            DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE, DWMSBT_AUTO, DWMSBT_NONE,
            DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
            DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
        },
        Graphics::Gdi::{
            CreateSolidBrush, DeleteObject, GetMonitorInfoW, HGDIOBJ, MONITOR_DEFAULTTONEAREST,
            MONITORINFO, MonitorFromWindow,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
            FILE_FLAGS_AND_ATTRIBUTES, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE,
            FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME,
            FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileVersionInfoSizeW,
            GetFileVersionInfoW, OPEN_EXISTING, ReadDirectoryChangesW, VerQueryValueW,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
            },
            // The front door's console half. `folio.exe --help` has to reach
            // whoever typed it, and on Windows that is a console this process may
            // not own — see `write_to_console`.
            Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetConsoleWindow, WriteConsoleW},
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, SetClipboardData,
            },
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            // Explorer's context menu is a handful of `REG_SZ` values under
            // `HKEY_CURRENT_USER`, and nothing else — see
            // `install_context_menu`.
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
                REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW,
                RegQueryValueExW, RegSetValueExW,
            },
            Threading::{
                CreateEventW, GetCurrentThread, GetThreadPriority, INFINITE, ResetEvent, SetEvent,
                SetThreadPriority, THREAD_PRIORITY, THREAD_PRIORITY_ABOVE_NORMAL,
                THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL, WaitForMultipleObjects,
            },
        },
        UI::{
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            Input::KeyboardAndMouse::{GetKeyboardLayout, SetFocus, VkKeyScanW},
            Shell::{
                Common::COMDLG_FILTERSPEC, DefSubclassProc, FO_DELETE, FOF_ALLOWUNDO,
                FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
                FOLDERID_Documents, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
                FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, ITaskbarList3,
                KF_FLAG_DEFAULT, RemoveWindowSubclass, SHCreateItemFromParsingName,
                SHFILEOPSTRUCTW, SHFileOperationW, SHGetKnownFolderPath, SIGDN_FILESYSPATH,
                SetWindowSubclass, ShellExecuteW, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS,
                TBPF_NORMAL, TBPF_PAUSED, TaskbarList,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreateCaret, CreatePopupMenu, DestroyCaret, DestroyMenu,
                GCLP_HBRBACKGROUND, GetClientRect, GetCursorPos, GetWindowRect, HTBOTTOM,
                HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP,
                HTTOPLEFT, HTTOPRIGHT, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsZoomed,
                MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MF_STRING, MINMAXINFO, MessageBoxW,
                NCCALCSIZE_PARAMS, PostMessageW, RegisterWindowMessageW, SM_CXFRAME,
                SM_CXPADDEDBORDER, SPI_GETCLIENTAREAANIMATION, SPI_GETWHEELSCROLLLINES,
                SW_SHOWNORMAL, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                SWP_NOZORDER, SetCaretPos, SetClassLongPtrW, SetWindowPos, SystemParametersInfoW,
                TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CLOSE, WM_GETMINMAXINFO,
                WM_NCCALCSIZE, WM_NCHITTEST,
            },
        },
    };

    use super::{
        CustomFrameGeometry, CustomFrameHit, CustomFrameMetrics, NonZeroIsize, PageVisual,
        TaskbarProgress, TaskbarProgressState, ThreadPriority, VisualLayer, WheelScrollAmount,
        WindowRect, composition_visual_offset, custom_frame_hit_test, logical_px_for_dpi,
    };

    /// GDI brush currently owned by this process and installed on winit's shared window class.
    /// The class itself outlives individual windows; theme switches replace this handle in place.
    ///
    /// **One brush for the process, by ruling** (`docs/DESIGN.md` §2.8, multiwindow slice E1). This
    /// is the static the spike named as the one that would hit a wall under a per-window theme
    /// (spike-multiwindow Q5 item 1): every winit window in a process shares one window class, so
    /// a second theme needs a second registered class — its own slice, and nobody has asked for it.
    /// Until somebody does, "the theme is the application's" is what makes one handle here correct
    /// rather than a simplification, and this static stays where it is.
    static WINDOW_CLASS_BACKGROUND: OnceLock<Mutex<Option<isize>>> = OnceLock::new();
    const CF_UNICODETEXT: u32 = 13;
    const CLIPBOARD_OPEN_RETRY_DELAYS: [std::time::Duration; 4] = [
        std::time::Duration::from_millis(5),
        std::time::Duration::from_millis(10),
        std::time::Duration::from_millis(20),
        std::time::Duration::from_millis(40),
    ];
    const WHEEL_PAGESCROLL: u32 = u32::MAX;
    const DEFERRED_MATH_MENU_MESSAGE: u32 = WM_APP + 0x4b7;
    const DEFERRED_FOLDER_PICKER_MESSAGE: u32 = WM_APP + 0x4b8;
    const DEFERRED_IMAGE_PICKER_MESSAGE: u32 = WM_APP + 0x4b9;
    const MATH_MENU_SUBCLASS_ID: usize = 0x4254_4d4d;
    const FOLDER_PICKER_SUBCLASS_ID: usize = 0x4254_4650;
    const IMAGE_PICKER_SUBCLASS_ID: usize = 0x4254_4950;
    const CUSTOM_FRAME_SUBCLASS_ID: usize = 0x4254_4346;
    const TASKBAR_SUBCLASS_ID: usize = 0x4254_5442;
    const CAPTION_BUTTON_COUNT: i32 = 4;

    /// The shell's `TaskbarButtonCreated` broadcast, registered once per process.
    ///
    /// `RegisterWindowMessageW` answers the same number to every process that
    /// asks for the same string, which is how a message the shell sends and a
    /// message this program listens for are the same message. It returns `0` on
    /// failure, and `0` is `WM_NULL` — a real message — so every comparison
    /// against this value has to exclude zero or a failed registration would
    /// turn every idle-time `WM_NULL` into a taskbar re-apply.
    static TASKBAR_BUTTON_CREATED: OnceLock<u32> = OnceLock::new();

    fn taskbar_button_created_message() -> u32 {
        *TASKBAR_BUTTON_CREATED.get_or_init(|| {
            let name = wide_null("TaskbarButtonCreated");
            // SAFETY: `RegisterWindowMessageW` reads one NUL-terminated string and
            // returns an atom; `name` outlives the call.
            unsafe { RegisterWindowMessageW(PCWSTR(name.as_ptr())) }
        })
    }

    /// The DirectComposition visual tree one window presents through.
    ///
    /// # Why the picture stopped going straight to the HWND
    ///
    /// It is the same picture, drawn by the same renderer, and on screen it is
    /// the same pixels. What changed is the door: wgpu's dx12 backend offers
    /// [`PreMultiplied`][premultiplied] composite alpha **only** to a visual
    /// target (`wgpu-hal-30.0.0/src/dx12/adapter.rs:1364` answers a
    /// `WndHandle` target with `vec![Opaque]` and nothing else), and a window
    /// that cannot be configured `PreMultiplied` can never have a hole cut in
    /// it for a web preview to show through. So the ground moves first, alone,
    /// with nothing above it: this slice changes where the swapchain hangs and
    /// changes nothing about what is drawn into it. (WebView2 spike, 切片建议
    /// item 1.)
    ///
    /// # The tree
    ///
    /// ```text
    /// target  ← CreateTargetForHwnd(hwnd, topmost = true)
    ///  └─ root                    ← IDCompositionTarget::SetRoot
    ///      └─ gpu                 ← the swapchain wgpu hangs here
    /// ```
    ///
    /// `topmost = true` is load-bearing rather than decorative: the whole tree
    /// is composed **above** the window's own painting, so anywhere the visuals
    /// do not cover shows the window class background brush
    /// ([`install_window_class_background`]) and not the desktop behind it.
    /// That is the same brush that has always been under the swapchain, so a
    /// frame that has not been presented yet looks exactly as it did before.
    ///
    /// The web preview's own visual becomes a second child of `root` in a later
    /// slice; the shape above is already the shape that takes it.
    ///
    /// # Failure is failure
    ///
    /// There is no fallback to an HWND swapchain. DirectComposition is present
    /// on every Windows this product supports, and a second presentation path
    /// kept alive "just in case" would be a second set of alpha semantics, a
    /// second resize behaviour and a second thing to photograph at acceptance —
    /// carried permanently against a failure mode that does not exist. If this
    /// constructor fails, the window fails to open and says why.
    ///
    /// [premultiplied]: https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ne-dxgi1_2-dxgi_alpha_mode
    pub struct Compositor {
        /// The one object that commits. See [`Compositor::commit`].
        device: IDCompositionDesktopDevice,
        /// The same device, narrowed once: `CreateRectangleClip` is only on the
        /// `Device3` face, and the web visual's clip is the one caller.
        device3: IDCompositionDevice3,
        /// Held for its lifetime and read by nothing: this handle **is** the
        /// binding between the tree and the HWND, and dropping it unbinds it.
        _target: IDCompositionTarget,
        /// The root, reached by [`Compositor::attach_web_visual`] and by nothing
        /// else — the web preview's visual is the only sibling `gpu` will ever
        /// have.
        root: IDCompositionVisual,
        gpu: IDCompositionVisual,
        /// **One visual per page**, each keyed by the [`PageVisual`] that asked
        /// for it.
        ///
        /// A single slot until W2 slice ③ made a window able to hold a page per
        /// seat, and the plural is not tidiness: a controller is pointed at one
        /// visual for its whole life (`SetRootVisualTarget`), so two pages sharing
        /// a slot means the second page composes into the first page's box and the
        /// first page's teardown takes the second page's visual out of the tree.
        /// Measured on the real window — a page reopened from Recent while the
        /// closed one was still waiting for its browser to go appeared as a hole
        /// with nothing behind it, about three hundred milliseconds after it
        /// arrived.
        ///
        /// Keyed by the seat *and* the tab since F1b′, for the same measurement
        /// one window over: seat numbers restart at one in every tab, so two tabs
        /// of one window sharing a bare seat number is the same single slot again
        /// — see [`PageVisual`].
        ///
        /// `RefCell` because a web seat opens and closes while the window is
        /// running, and every other operation on this type takes `&self` — the
        /// present funnel holds the compositor by shared reference and always
        /// has.
        web: RefCell<BTreeMap<PageVisual, IDCompositionVisual>>,
    }

    impl Compositor {
        /// Build the tree for one window. The window must already exist.
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            // A null rendering device is the documented way to ask for a
            // composition device that only arranges visuals: this one never
            // rasterizes anything itself, because the only content it will ever
            // hold is a swapchain wgpu made on its own D3D12 device.
            let device: IDCompositionDesktopDevice =
                unsafe { DCompositionCreateDevice3(None::<&IUnknown>) }
                    .map_err(|error| compositor_failure("DCompositionCreateDevice3", &error))?;
            let target = unsafe { device.CreateTargetForHwnd(hwnd, true) }.map_err(|error| {
                compositor_failure("IDCompositionDesktopDevice::CreateTargetForHwnd", &error)
            })?;
            let device3: IDCompositionDevice3 = device
                .cast()
                .map_err(|error| compositor_failure("IDCompositionDesktopDevice::cast", &error))?;
            let root = Self::create_visual(&device, "root")?;
            let gpu = Self::create_visual(&device, "gpu")?;
            unsafe { root.AddVisual(&gpu, true, None::<&IDCompositionVisual>) }
                .map_err(|error| compositor_failure("IDCompositionVisual::AddVisual", &error))?;
            unsafe { target.SetRoot(&root) }
                .map_err(|error| compositor_failure("IDCompositionTarget::SetRoot", &error))?;
            let compositor = Self {
                device,
                device3,
                _target: target,
                root,
                gpu,
                web: RefCell::new(BTreeMap::new()),
            };
            // The tree exists on screen only once it is committed, and this one
            // is empty until wgpu sets the swapchain on `gpu` — so what this
            // commit publishes is "a tree with nothing in it", which looks
            // exactly like the window did a moment ago. Committing here anyway
            // keeps the invariant simple: at no point does this type hold
            // uncommitted structure it is relying on someone else to publish.
            compositor.commit()?;
            Ok(compositor)
        }

        fn create_visual(
            device: &IDCompositionDesktopDevice,
            role: &str,
        ) -> Result<IDCompositionVisual, String> {
            let visual = unsafe { device.CreateVisual() }.map_err(|error| {
                compositor_failure(
                    &format!("IDCompositionDevice2::CreateVisual({role})"),
                    &error,
                )
            })?;
            // `CreateVisual` hands back an `IDCompositionVisual2`; the base
            // interface is what carries the offset and what wgpu expects on the
            // other side of `gpu_visual_ptr`, so the narrowing happens once,
            // here, rather than at every call.
            visual.cast::<IDCompositionVisual>().map_err(|error| {
                compositor_failure(&format!("IDCompositionVisual2::cast({role})"), &error)
            })
        }

        /// The raw `IDCompositionVisual` wgpu builds its surface upon.
        ///
        /// No ownership travels with it. wgpu increments the visual's COM
        /// refcount when it takes the pointer and holds that reference for as
        /// long as the surface lives (`wgpu-hal-30.0.0/src/dx12/mod.rs:551`), so
        /// the surface cannot outlive its content even if this `Compositor` is
        /// dropped first.
        #[must_use]
        pub fn gpu_visual_ptr(&self) -> *mut c_void {
            self.gpu.as_raw()
        }

        /// Move the GPU visual inside the window, in **physical** pixels.
        ///
        /// The main window's is `(0, 0)` and stays there — the swapchain covers
        /// the whole client area. The parameter exists because a window whose
        /// swapchain is one child among several is the shape this tree was built
        /// for, and because putting the conversion in one place is what stops
        /// someone scaling a physical rectangle by a scale factor a second time
        /// (see [`composition_visual_offset`]).
        ///
        /// Takes effect on the next [`Compositor::commit`], with that frame,
        /// atomically — which is the whole reason the WebView2 spike measured
        /// zero seam here and 4–10px of tearing on the child-window path.
        pub fn set_gpu_offset(&self, x: i32, y: i32) -> Result<(), String> {
            let (x, y) = composition_visual_offset(x, y);
            unsafe { self.gpu.SetOffsetX2(x) }
                .map_err(|error| compositor_failure("IDCompositionVisual::SetOffsetX", &error))?;
            unsafe { self.gpu.SetOffsetY2(y) }
                .map_err(|error| compositor_failure("IDCompositionVisual::SetOffsetY", &error))
        }

        /// Build the web preview's visual and put it **under** everything Folio
        /// draws. Idempotent: a second call on a window that already has one
        /// changes nothing.
        ///
        /// # Where it lands, and why that is the whole of this method
        ///
        /// ```text
        /// target  ← CreateTargetForHwnd(hwnd, topmost = true)
        ///  └─ root                    ← IDCompositionTarget::SetRoot
        ///      ├─ web                 ← the WebView2's own visual tree hangs here
        ///      └─ gpu                 ← the swapchain wgpu hangs here
        /// ```
        ///
        /// The order is not a preference and it is not achieved by adding the
        /// two in the right sequence — `gpu` was added when the window opened
        /// and `web` arrives whenever a person opens a page. It is achieved by
        /// [`VisualLayer::Bottom`]: `AddVisual(insertAbove = TRUE, NULL)` puts a
        /// child at the *beginning* of the child list, and the beginning of a
        /// DirectComposition child list is the **bottom** of the stack. So the
        /// page can only ever be under Folio's own painting, and a later slice
        /// that adds a third visual cannot get between them by forgetting
        /// something.
        ///
        /// Folio still has to *stop painting* where the page is — see
        /// `bt_render::WindowRenderer::set_web_holes`. This method decides who
        /// is on top; the hole decides where the top is transparent.
        pub fn attach_web_visual(&self, page: PageVisual) -> Result<(), String> {
            if self.web.borrow().contains_key(&page) {
                return Ok(());
            }
            let web = Self::create_visual(&self.device, "web")?;
            unsafe {
                self.root.AddVisual(
                    &web,
                    VisualLayer::Bottom.insert_above_with_null_reference(),
                    None::<&IDCompositionVisual>,
                )
            }
            .map_err(|error| compositor_failure("IDCompositionVisual::AddVisual(web)", &error))?;
            self.web.borrow_mut().insert(page, web);
            Ok(())
        }

        /// Take the web preview's visual back out of the tree.
        ///
        /// Called when the last page in this window goes away. Leaving an empty
        /// visual behind would cost nothing on screen and would still be a lie
        /// in the tree, which is the thing this file is for.
        pub fn detach_web_visual(&self, page: PageVisual) -> Result<(), String> {
            let Some(web) = self.web.borrow_mut().remove(&page) else {
                return Ok(());
            };
            unsafe { self.root.RemoveVisual(&web) }
                .map_err(|error| compositor_failure("IDCompositionVisual::RemoveVisual", &error))
        }

        /// The `IUnknown` a `ICoreWebView2CompositionController` is handed as its
        /// root visual target.
        pub(crate) fn web_visual(&self, page: PageVisual) -> Option<IUnknown> {
            self.web
                .borrow()
                .get(&page)
                .and_then(|visual| visual.cast::<IUnknown>().ok())
        }

        /// Move and crop the web preview's visual, in **physical** pixels.
        ///
        /// Two rectangles and not one, for the reason the preview picture takes
        /// two (`WindowRenderer::place_preview_image`): `offset` is where the
        /// page is laid out and `clip` is the box it may appear in. At rest they
        /// are the same rectangle; mid-FLIP they are not, and a page drawn
        /// without the second would overhang the pane it is flying into.
        ///
        /// `clip` is in the visual's own coordinates — i.e. relative to
        /// `offset` — which is what the WebView2 side also believes, because its
        /// bounds are set at the origin (`COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS`
        /// with a zero-based rectangle) and the visual carries the placement.
        pub fn place_web_visual(
            &self,
            page: PageVisual,
            offset: (i32, i32),
            clip: (f32, f32, f32, f32),
        ) -> Result<(), String> {
            let borrowed = self.web.borrow();
            let Some(web) = borrowed.get(&page) else {
                return Ok(());
            };
            let (x, y) = composition_visual_offset(offset.0, offset.1);
            unsafe { web.SetOffsetX2(x) }.map_err(|error| {
                compositor_failure("IDCompositionVisual::SetOffsetX(web)", &error)
            })?;
            unsafe { web.SetOffsetY2(y) }.map_err(|error| {
                compositor_failure("IDCompositionVisual::SetOffsetY(web)", &error)
            })?;
            let rectangle = unsafe { self.device3.CreateRectangleClip() }.map_err(|error| {
                compositor_failure("IDCompositionDevice3::CreateRectangleClip", &error)
            })?;
            unsafe {
                rectangle
                    .SetLeft2(clip.0)
                    .and_then(|()| rectangle.SetTop2(clip.1))
                    .and_then(|()| rectangle.SetRight2(clip.2))
                    .and_then(|()| rectangle.SetBottom2(clip.3))
            }
            .map_err(|error| compositor_failure("IDCompositionRectangleClip::Set*", &error))?;
            unsafe { web.SetClip(&rectangle) }
                .map_err(|error| compositor_failure("IDCompositionVisual::SetClip", &error))
        }

        /// Publish this frame. **Call once per presented frame.**
        ///
        /// # wgpu does not commit, and nothing else will
        ///
        /// When wgpu creates or resizes the swapchain it calls `SetContent` on
        /// our visual and stops there — `wgpu-hal-30.0.0/src/dx12/mod.rs:1619`,
        /// the `SurfaceTarget::Visual` arm, which is deliberately *not* the
        /// `VisualFromWndHandle` arm right above it where wgpu owns the
        /// composition device and does commit. Whoever owns the DirectComposition
        /// device owns the commit, and that is this type. Miss it and the picture
        /// does not move — not a dropped frame, not a stutter: nothing on screen
        /// ever changes again, while every trace in the program reports frames
        /// being presented normally.
        ///
        /// The app therefore calls this at exactly one place, immediately after
        /// the one call that presents a frame — see `Runtime::present_seats_and_commit`
        /// in `crates/bt-app/src/main.rs`, which is the only caller of
        /// `Renderer::present_seats` in the program.
        pub fn commit(&self) -> Result<(), String> {
            unsafe { self.device.Commit() }
                .map_err(|error| compositor_failure("IDCompositionDevice2::Commit", &error))
        }
    }

    /// One shape for every DirectComposition refusal: the call that refused, in
    /// its own name, and the `HRESULT` in both of the forms a search will be
    /// run on — Windows' sentence and the hex code the documentation indexes.
    ///
    /// Pure, and separated from the call sites for the same reason every other
    /// bridge in this crate separates its pure half: it is the part that can be
    /// wrong without a window, and therefore the part a test can hold.
    fn compositor_failure(step: &str, error: &windows::core::Error) -> String {
        format!(
            "{step} failed: {} (0x{:08X})",
            error.message(),
            error.code().0 as u32
        )
    }

    /// Keeps winit's ordinary overlapped-window styles (and therefore native
    /// snap, resize borders, minimize animation and system-menu semantics) while
    /// extending the client area through the system caption.
    pub struct CustomWindowFrame {
        hwnd: HWND,
        state: Box<CustomFrameState>,
    }

    /// Everything the subclass procedure reads out of the owning
    /// `CustomWindowFrame`. It is reached through the subclass reference data, so
    /// it must be a single stable allocation the frame keeps alive.
    struct CustomFrameState {
        /// This window's frame measurements, as the caller stated them. Plain
        /// values rather than atomics because they are settled at install and
        /// nothing may move them afterwards — a title bar that changed height
        /// mid-session would be painted at one number and clicked at another,
        /// which is the very thing this field exists to prevent.
        geometry: CustomFrameGeometry,
        tab_strip_right_px: AtomicI32,
        /// Smallest client size the window may be dragged to, in logical pixels,
        /// or `(0, 0)` for "no minimum". Logical rather than physical so the
        /// constraint survives a DPI change without anyone recomputing it.
        min_client_logical_width: AtomicI32,
        min_client_logical_height: AtomicI32,
    }

    impl CustomWindowFrame {
        pub fn install(hwnd: NonZeroIsize, geometry: CustomFrameGeometry) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Box::new(CustomFrameState {
                geometry,
                tab_strip_right_px: AtomicI32::new(0),
                min_client_logical_width: AtomicI32::new(0),
                min_client_logical_height: AtomicI32::new(0),
            });
            let reference_data = (&*state as *const CustomFrameState) as usize;
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(custom_frame_subclass),
                    CUSTOM_FRAME_SUBCLASS_ID,
                    reference_data,
                )
            };
            if !installed.as_bool() {
                return Err(format!(
                    "SetWindowSubclass(custom frame) failed: {}",
                    unsafe { GetLastError().0 }
                ));
            }
            // Re-run non-client calculation now that the subclass owns it. No
            // position, size or z-order changes are requested.
            if let Err(error) = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            } {
                let _ = unsafe {
                    RemoveWindowSubclass(
                        hwnd,
                        Some(custom_frame_subclass),
                        CUSTOM_FRAME_SUBCLASS_ID,
                    )
                };
                return Err(format!("SetWindowPos(SWP_FRAMECHANGED) failed: {error}"));
            }
            // Claiming the caption does not surrender the Windows 11 rounded
            // corners every ordinary window wears; state the preference
            // explicitly so DWM keeps clipping the frame like the system
            // default. Best-effort: Windows 10 has no such attribute and is
            // square everywhere, which is its default too.
            let preference = DWMWCP_ROUND;
            let _ = unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &preference as *const DWM_WINDOW_CORNER_PREFERENCE as *const c_void,
                    std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
                )
            };
            Ok(Self { hwnd, state })
        }

        pub fn set_tab_strip_right_px(&self, tab_strip_right_px: i32) {
            self.state
                .tab_strip_right_px
                .store(tab_strip_right_px.max(0), Ordering::Relaxed);
        }

        /// Constrain how far the window may be resized, in logical pixels of
        /// *client* area. `None` lifts the constraint.
        ///
        /// This is the self-drawn frame's replacement for winit's
        /// `set_min_inner_size`, and it exists because winit's setter cannot be
        /// used on a window whose `WM_NCCALCSIZE` has made the client area the
        /// entire outer rect: winit implements it by re-requesting the current
        /// inner size, and re-requesting runs `AdjustWindowRectExForDpi`, which
        /// adds a frame margin this window does not have. The window grew by that
        /// margin on every call. Owning `WM_GETMINMAXINFO` states the same
        /// constraint to the same OS with no size request at all.
        pub fn set_min_client_size(&self, logical: Option<(u32, u32)>) -> Result<(), String> {
            let (width, height) = logical.unwrap_or((0, 0));
            let width = i32::try_from(width).unwrap_or(i32::MAX);
            let height = i32::try_from(height).unwrap_or(i32::MAX);
            self.state
                .min_client_logical_width
                .store(width, Ordering::Relaxed);
            self.state
                .min_client_logical_height
                .store(height, Ordering::Relaxed);
            if width <= 0 || height <= 0 {
                return Ok(());
            }
            // `WM_GETMINMAXINFO` only governs future sizing, so a window that is
            // already smaller than a freshly raised minimum has to be grown here.
            // A maximized window is not user-resizable and its rect belongs to the
            // monitor, so it is left alone.
            // SAFETY: `self.hwnd` is the live window this frame is installed on.
            if unsafe { IsZoomed(self.hwnd) }.as_bool() {
                return Ok(());
            }
            // SAFETY: as above; both queries are read-only and `rect` is
            // exclusively borrowed for the call.
            let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
            let mut rect = RECT::default();
            unsafe { GetWindowRect(self.hwnd, &mut rect) }
                .map_err(|error| format!("GetWindowRect failed: {error}"))?;
            let current_width = rect.right.saturating_sub(rect.left);
            let current_height = rect.bottom.saturating_sub(rect.top);
            let target_width = current_width.max(logical_px_for_dpi(width as u32, dpi));
            let target_height = current_height.max(logical_px_for_dpi(height as u32, dpi));
            if target_width == current_width && target_height == current_height {
                return Ok(());
            }
            // SAFETY: `self.hwnd` is live; no insert-after handle is passed, so
            // the null `hwndinsertafter` is inert under `SWP_NOZORDER`.
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    target_width,
                    target_height,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
            .map_err(|error| format!("SetWindowPos(grow to minimum) failed: {error}"))
        }
    }

    impl Drop for CustomWindowFrame {
        fn drop(&mut self) {
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(custom_frame_subclass),
                    CUSTOM_FRAME_SUBCLASS_ID,
                )
            };
        }
    }

    unsafe extern "system" fn custom_frame_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        match message {
            WM_NCCALCSIZE => {
                // A zoomed overlapped window deliberately extends its outer
                // resize frame beyond the monitor. With the entire outer rect
                // made client, those pixels would clip our titlebar/content.
                // Keep that invisible native frame as a maximized inset while
                // still removing the ordinary system caption everywhere else.
                if wparam.0 != 0 && lparam.0 != 0 && unsafe { IsZoomed(hwnd) }.as_bool() {
                    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                    let border = native_resize_border(dpi);
                    let params = unsafe { &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS) };
                    params.rgrc[0].left = params.rgrc[0].left.saturating_add(border);
                    params.rgrc[0].top = params.rgrc[0].top.saturating_add(border);
                    params.rgrc[0].right = params.rgrc[0].right.saturating_sub(border);
                    params.rgrc[0].bottom = params.rgrc[0].bottom.saturating_sub(border);
                }
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                if lparam.0 == 0 {
                    return result;
                }
                // SAFETY: `CustomWindowFrame` owns this allocation and removes the subclass
                // before dropping it, so the reference-data pointer is live for every callback.
                let state = unsafe { &*(reference_data as *const CustomFrameState) };
                let width = state.min_client_logical_width.load(Ordering::Relaxed);
                let height = state.min_client_logical_height.load(Ordering::Relaxed);
                if width <= 0 || height <= 0 {
                    return result;
                }
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                // `ptMinTrackSize` is an *outer* rect bound. Under this frame the
                // client area is the outer rect, so a minimum client size is that
                // bound byte for byte, with no non-client margin to add.
                // SAFETY: for WM_GETMINMAXINFO the OS passes a live, writable
                // MINMAXINFO in `lparam`, checked non-null above.
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                info.ptMinTrackSize.x = logical_px_for_dpi(width as u32, dpi);
                info.ptMinTrackSize.y = logical_px_for_dpi(height as u32, dpi);
                result
            }
            WM_NCHITTEST => {
                let mut client = RECT::default();
                if unsafe { GetClientRect(hwnd, &mut client) }.is_err() {
                    return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                }
                let mut point = POINT {
                    x: low_word_signed(lparam.0),
                    y: high_word_signed(lparam.0),
                };
                if !unsafe { windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut point) }
                    .as_bool()
                {
                    return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
                }
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                let resize_border = if unsafe { IsZoomed(hwnd) }.as_bool() {
                    0
                } else {
                    native_resize_border(dpi)
                };
                // SAFETY: `CustomWindowFrame` owns this allocation and removes the subclass
                // before dropping it, so the reference-data pointer is live for every callback.
                let state = unsafe { &*(reference_data as *const CustomFrameState) };
                let tab_strip_right_px = state.tab_strip_right_px.load(Ordering::Relaxed);
                let hit = custom_frame_hit_test(
                    CustomFrameMetrics {
                        width: client.right.saturating_sub(client.left),
                        height: client.bottom.saturating_sub(client.top),
                        title_bar_height: logical_px_for_dpi(
                            state.geometry.title_bar_logical_px,
                            dpi,
                        ),
                        tab_strip_right_px,
                        caption_button_width: logical_px_for_dpi(
                            state.geometry.caption_button_logical_px,
                            dpi,
                        ),
                        caption_button_count: CAPTION_BUTTON_COUNT,
                        resize_border,
                        resizable: resize_border > 0,
                    },
                    point.x,
                    point.y,
                );
                LRESULT(match hit {
                    CustomFrameHit::Client => HTCLIENT,
                    CustomFrameHit::Caption => HTCAPTION,
                    CustomFrameHit::Left => HTLEFT,
                    CustomFrameHit::Right => HTRIGHT,
                    CustomFrameHit::Top => HTTOP,
                    CustomFrameHit::Bottom => HTBOTTOM,
                    CustomFrameHit::TopLeft => HTTOPLEFT,
                    CustomFrameHit::TopRight => HTTOPRIGHT,
                    CustomFrameHit::BottomLeft => HTBOTTOMLEFT,
                    CustomFrameHit::BottomRight => HTBOTTOMRIGHT,
                } as isize)
            }
            _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
        }
    }

    /// Everything the taskbar subclass reads, kept in one stable allocation the
    /// owning [`Taskbar`] holds for as long as the subclass is installed.
    ///
    /// The reading is a [`Cell`] and not an atomic because both writers are the
    /// same thread: `set_progress` is called from the event loop, and the
    /// subclass procedure runs on the thread that owns the HWND, which is that
    /// same event loop. An atomic here would be a claim about sharing that is
    /// not true.
    struct TaskbarState {
        list: ITaskbarList3,
        /// What the button has been told, and what it will be told again the
        /// next time the shell announces it. **Not** a cache of what to skip —
        /// that gate lives with the caller, where the aggregate is computed.
        showing: Cell<TaskbarProgress>,
    }

    impl TaskbarState {
        fn apply(&self, hwnd: HWND) -> Result<(), String> {
            let showing = self.showing.get();
            let flag = match showing.state {
                TaskbarProgressState::None => TBPF_NOPROGRESS,
                TaskbarProgressState::Normal => TBPF_NORMAL,
                TaskbarProgressState::Error => TBPF_ERROR,
                TaskbarProgressState::Paused => TBPF_PAUSED,
                TaskbarProgressState::Indeterminate => TBPF_INDETERMINATE,
            };
            // SAFETY: both calls are ordinary in-proc COM on the interface this
            // object created and owns, made on the thread whose apartment
            // created it, against an HWND this process owns.
            unsafe {
                self.list
                    .SetProgressState(hwnd, flag)
                    .map_err(|error| format!("ITaskbarList3::SetProgressState: {error}"))?;
                // The state goes first on purpose: `SetProgressValue` is
                // documented to do nothing at all while the button is
                // `TBPF_NOPROGRESS` or `TBPF_INDETERMINATE`, so a value sent
                // before the state that admits it is a value thrown away.
                if let Some((completed, total)) = showing.value {
                    self.list
                        .SetProgressValue(hwnd, completed, total)
                        .map_err(|error| format!("ITaskbarList3::SetProgressValue: {error}"))?;
                }
            }
            Ok(())
        }
    }

    /// One window's taskbar button, as a place to put a reading (Windows landing
    /// block slice 1).
    ///
    /// # Two things this owns that are easy to get wrong
    ///
    /// **The apartment.** `ITaskbarList3` is apartment-threaded and this object
    /// keeps it alive across many calls, so — unlike the shell pickers, which
    /// initialise and balance COM inside the one call that shows a dialog — the
    /// apartment reference has to last exactly as long as the interface does. It
    /// is taken in [`Taskbar::new`] and returned in `Drop`, after the interface
    /// has been released, which is the only order in which releasing an object
    /// into an apartment that still exists is guaranteed. A thread already in a
    /// multi-threaded apartment is reported rather than forced, on the pickers'
    /// reasoning: a window with no progress bar is a fine outcome and not one
    /// worth breaking somebody's apartment model over.
    ///
    /// **`TaskbarButtonCreated`.** The shell announces each top-level window's
    /// button by broadcasting a registered message, and every call made before
    /// that announcement silently does nothing. It is broadcast **again** every
    /// time `explorer.exe` restarts, and the button that comes back is blank. So
    /// this object listens for it and **re-applies** the reading it is carrying,
    /// rather than initialising once and trusting the shell to remember: without
    /// that, an explorer restart during a long build would leave the button
    /// empty until the build's *next* state change, which for the last stretch
    /// of a build is never.
    pub struct Taskbar {
        hwnd: HWND,
        /// `Option` for one reason: `Drop::drop` runs before a struct's fields
        /// are dropped, so releasing the interface before `CoUninitialize` needs
        /// a `take` and cannot be left to field order.
        state: Option<Box<TaskbarState>>,
        /// Whether this object's own `CoInitializeEx` counted, and therefore
        /// owes a `CoUninitialize`. `S_FALSE` — "already initialised, and this
        /// call counted" — owes one exactly as `S_OK` does, which is why the
        /// balance is read off `is_ok` and not off `== S_OK`.
        com_balance: bool,
    }

    impl Taskbar {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            // SAFETY: this runs on the window's own event-loop thread, which is
            // the thread that owns the HWND and the apartment. Every failure
            // path below releases what it took, in the order it took it.
            unsafe {
                let apartment =
                    CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
                if apartment == RPC_E_CHANGED_MODE {
                    return Err(
                        "the event-loop thread is not a single-threaded apartment".to_owned()
                    );
                }
                let com_balance = apartment.is_ok();
                let created = (|| {
                    let list: ITaskbarList3 =
                        CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)
                            .map_err(|error| format!("CoCreateInstance(TaskbarList): {error}"))?;
                    // `HrInit` is what tells the shell this process intends to
                    // talk to the button; nothing else on the interface works
                    // until it has returned.
                    list.HrInit()
                        .map_err(|error| format!("ITaskbarList3::HrInit: {error}"))?;
                    Ok(list)
                })();
                let list = match created {
                    Ok(list) => list,
                    Err(error) => {
                        if com_balance {
                            CoUninitialize();
                        }
                        return Err(error);
                    }
                };
                let state = Box::new(TaskbarState {
                    list,
                    showing: Cell::new(TaskbarProgress::CLEARED),
                });
                let reference_data = (&*state as *const TaskbarState) as usize;
                let installed = SetWindowSubclass(
                    hwnd,
                    Some(taskbar_subclass),
                    TASKBAR_SUBCLASS_ID,
                    reference_data,
                );
                if !installed.as_bool() {
                    let failure =
                        format!("SetWindowSubclass(taskbar) failed: {}", GetLastError().0);
                    // The interface goes before the apartment it lives in.
                    drop(state);
                    if com_balance {
                        CoUninitialize();
                    }
                    return Err(failure);
                }
                Ok(Self {
                    hwnd,
                    state: Some(state),
                    com_balance,
                })
            }
        }

        /// Put `progress` on this window's button, and remember it as the reading
        /// to restore the next time the shell hands the button back.
        pub fn set_progress(&self, progress: TaskbarProgress) -> Result<(), String> {
            let state = self
                .state
                .as_ref()
                .expect("the state is taken only while dropping");
            state.showing.set(progress);
            state.apply(self.hwnd)
        }
    }

    impl Drop for Taskbar {
        fn drop(&mut self) {
            // SAFETY: dropped on the same event-loop thread that installed the
            // subclass and initialised the apartment. The subclass is removed
            // first so nothing can reach the state after it is freed, the
            // interface is released next, and only then is the apartment let go.
            unsafe {
                let _ =
                    RemoveWindowSubclass(self.hwnd, Some(taskbar_subclass), TASKBAR_SUBCLASS_ID);
                drop(self.state.take());
                if self.com_balance {
                    CoUninitialize();
                }
            }
        }
    }

    unsafe extern "system" fn taskbar_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        let announced = taskbar_button_created_message();
        if announced != 0 && message == announced {
            let state = reference_data as *const TaskbarState;
            if !state.is_null() {
                // SAFETY: the owning `Taskbar` holds this box for the whole
                // installed interval and removes the subclass before freeing it,
                // on this same thread.
                let _ = unsafe { (*state).apply(hwnd) };
            }
        }
        // Forwarded either way: a broadcast this program answers is still a
        // broadcast every other subclass in the chain is entitled to see.
        // SAFETY: forwarding untouched messages is the required subclass contract.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    /// How many shown toasts are kept alive so their clicks can still be heard.
    ///
    /// A click comes back through the `Activated` handler registered on the
    /// **object** that was shown, so the object is what has to be held. Thirty-two
    /// is the depth of "notifications a person might still scroll back to in the
    /// notification centre"; past that the object is released and clicking it
    /// does nothing, which is also what clicking any of them does once Folio has
    /// exited (see [`Notifier`]).
    const NOTIFICATION_HISTORY: usize = 32;

    /// What the activation callback and the event loop share.
    ///
    /// Two mutexes and not one, because they are locked by different threads for
    /// different reasons and holding either while the other is wanted is the
    /// shape a deadlock has.
    struct NotifierShared {
        /// Launch strings from toasts that have been clicked and not yet routed.
        activations: Mutex<Vec<String>>,
        /// How the owning loop is told there is something in the queue.
        wake: Mutex<Box<dyn Fn() + Send>>,
    }

    /// This process's voice in the Windows notification centre (Windows landing
    /// block slice 3).
    ///
    /// # What it registers, and what it deliberately does not
    ///
    /// **One string in one key**, `DisplayName` under
    /// `HKCU\Software\Classes\AppUserModelId\Folio.Terminal`. That is the whole
    /// registry footprint, and it was measured rather than assumed — see
    /// `docs/DESIGN.md` §7.6 for the probe. In particular there is **no
    /// `CustomActivator`, no `CLSID` tree, no `LocalServer32`, no start-menu
    /// shortcut and no call to `SetCurrentProcessExplicitAppUserModelID`**: with
    /// only that one value written, `Show` displays the toast and clicking it
    /// delivers `ToastNotification::Activated` **into this running process**,
    /// which is the only case a terminal has. Microsoft's own (archived) Win32
    /// toast guide still says a shortcut is mandatory; on Windows 11 26200 it is
    /// not.
    ///
    /// The one thing that footprint gives up is *cold* activation: a toast still
    /// sitting in the notification centre after Folio has exited does nothing
    /// when clicked, because there is no COM server to launch. That is the
    /// honest outcome rather than a gap — the pane it named is gone with the
    /// process, so there is nothing for the click to land on.
    ///
    /// **Registration is not removed when notifications are switched off.** The
    /// key is the name the user's own choices about Folio's notifications are
    /// filed under; deleting it would throw those away and re-registering would
    /// look like a different app. Off means no toast is raised, which is the
    /// whole of what off can honestly promise.
    ///
    /// **If a toast is ever attributed to `Folio.Terminal` rather than `Folio`,
    /// the machine has a stale name cached and the code is fine.** The
    /// notification platform remembers a display name per AUMID and does not
    /// re-read this value once it has one; a machine that met this identity
    /// before it had a `DisplayName` — a developer's, running an earlier probe —
    /// keeps answering with the raw id. `ToastNotificationManager.History.Clear`
    /// for the AUMID clears it, and so does restarting Explorer. Measured both
    /// ways; see `docs/DESIGN.md` §7.6.
    pub struct Notifier {
        notifier: ToastNotifier,
        shared: Arc<NotifierShared>,
        /// The toasts still able to route a click, newest last.
        live: std::collections::VecDeque<ToastNotification>,
        /// Whether this object's own `CoInitializeEx` counted — `Taskbar`'s
        /// balance rule, and for the same reason.
        com_balance: bool,
    }

    impl Notifier {
        /// Claim the identity and open the channel, or say why not.
        ///
        /// `wake` is called from whatever thread the platform delivers a click
        /// on. It must do nothing but nudge the event loop — the launch string
        /// is already in the queue by then, and [`Self::take_activations`] is
        /// where it is read, on the loop's own thread.
        pub fn new(wake: Box<dyn Fn() + Send>) -> Result<Self, String> {
            write_registry_string(
                &super::notification_aumid_key(),
                "DisplayName",
                super::NOTIFICATION_DISPLAY_NAME,
            )?;
            // SAFETY: called on the event-loop thread, which is the apartment
            // the notifier and every toast object below live in. The balance is
            // returned in `Drop`, after the interface is released.
            let apartment =
                unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
            if apartment == RPC_E_CHANGED_MODE {
                return Err("the event-loop thread is not a single-threaded apartment".to_owned());
            }
            let com_balance = apartment.is_ok();
            let notifier = match ToastNotificationManager::CreateToastNotifierWithId(
                &HSTRING::from(super::NOTIFICATION_AUMID),
            ) {
                Ok(notifier) => notifier,
                Err(error) => {
                    if com_balance {
                        // SAFETY: balancing the call made immediately above.
                        unsafe { CoUninitialize() };
                    }
                    return Err(format!("CreateToastNotifierWithId: {error}"));
                }
            };
            // `notifier.Setting()` is deliberately **not** consulted. On the very
            // first notification of a fresh identity it answers
            // `0x80070490 "Element not found"` — the platform has not made its
            // own settings entry yet — and `Show` succeeds regardless. Reading it
            // as "notifications are off" would silence the first toast on every
            // machine, once, and only on machines where nothing was wrong.
            Ok(Self {
                notifier,
                shared: Arc::new(NotifierShared {
                    activations: Mutex::new(Vec::new()),
                    wake: Mutex::new(wake),
                }),
                live: std::collections::VecDeque::new(),
                com_balance,
            })
        }

        /// Raise one toast. `launch` comes back verbatim if it is clicked.
        pub fn show(&mut self, title: &str, body: &str, launch: &str) -> Result<(), String> {
            let document = XmlDocument::new().map_err(|error| format!("XmlDocument: {error}"))?;
            document
                .LoadXml(&HSTRING::from(super::toast_xml(title, body, launch)))
                .map_err(|error| format!("XmlDocument::LoadXml: {error}"))?;
            let toast = ToastNotification::CreateToastNotification(&document)
                .map_err(|error| format!("CreateToastNotification: {error}"))?;
            let shared = Arc::clone(&self.shared);
            toast
                .Activated(&TypedEventHandler::<
                    ToastNotification,
                    windows::core::IInspectable,
                >::new(move |_sender, arguments| {
                    let launched = arguments
                        .as_ref()
                        .and_then(|arguments| arguments.cast::<ToastActivatedEventArgs>().ok())
                        .and_then(|arguments| arguments.Arguments().ok())
                        .map(|arguments| arguments.to_string_lossy())
                        .unwrap_or_default();
                    // Poisoned locks are ignored rather than unwrapped: a panic
                    // in the loop must not turn every later click into a second
                    // panic on a platform thread, where there is nobody to catch
                    // it.
                    if let Ok(mut queue) = shared.activations.lock() {
                        queue.push(launched);
                    }
                    if let Ok(wake) = shared.wake.lock() {
                        wake();
                    }
                    Ok(())
                }))
                .map_err(|error| format!("ToastNotification::Activated: {error}"))?;
            self.notifier
                .Show(&toast)
                .map_err(|error| format!("ToastNotifier::Show: {error}"))?;
            self.live.push_back(toast);
            while self.live.len() > NOTIFICATION_HISTORY {
                self.live.pop_front();
            }
            Ok(())
        }

        /// Every clicked toast's launch string since the last call, oldest first.
        pub fn take_activations(&self) -> Vec<String> {
            self.shared
                .activations
                .lock()
                .map(|mut queue| std::mem::take(&mut *queue))
                .unwrap_or_default()
        }
    }

    impl Drop for Notifier {
        fn drop(&mut self) {
            // SAFETY: dropped on the thread that initialised the apartment. The
            // toasts and the notifier are released before the apartment they
            // live in is let go, which is `Taskbar::drop`'s order and the only
            // one that is defined.
            self.live.clear();
            if self.com_balance {
                unsafe { CoUninitialize() };
            }
        }
    }

    fn low_word_signed(value: isize) -> i32 {
        (value as u16 as i16) as i32
    }

    fn high_word_signed(value: isize) -> i32 {
        ((value as usize >> 16) as u16 as i16) as i32
    }

    fn native_resize_border(dpi: u32) -> i32 {
        unsafe {
            GetSystemMetricsForDpi(SM_CXFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        }
        .max(1)
    }

    pub fn request_window_close(hwnd: NonZeroIsize) -> Result<(), String> {
        unsafe {
            PostMessageW(
                Some(HWND(hwnd.get() as *mut c_void)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .map_err(|error| format!("PostMessageW(WM_CLOSE) failed: {error}"))
    }

    /// Ask Windows to open one already-policy-checked target with its registered default handler.
    /// Scheme allowlisting deliberately belongs to the caller; this bridge only supplies the
    /// audited UTF-16 and ShellExecuteW boundary.
    pub fn shell_execute(hwnd: NonZeroIsize, target: &str) -> Result<(), String> {
        if target.contains('\0') {
            return Err("ShellExecuteW target contains an embedded NUL".to_owned());
        }
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut target = target.encode_utf16().collect::<Vec<_>>();
        target.push(0);
        // SAFETY: both strings are live, NUL-terminated UTF-16 buffers for the duration of the
        // synchronous call. No parameters or working directory are supplied, so the URI is never
        // reparsed as a command line. `hwnd` is winit's live top-level window.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// Open one worker-validated local image with its registered default handler.
    ///
    /// The caller must obtain `path` from a successful image decode record, never directly from
    /// terminal text. This bridge independently enforces the slice's immutable syntax policy
    /// (drive-rooted, supported extension, no embedded NUL) and supplies no parameters or working
    /// directory, preventing command-line reinterpretation. It performs no event-thread file I/O.
    pub fn open_local_file(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        validate_local_image_path(path)?;
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut target = path.as_os_str().encode_wide().collect::<Vec<_>>();
        target.push(0);
        // SAFETY: the worker-success capability supplied an existing decoded image path, and both
        // buffers remain live and NUL-terminated for this synchronous call. Parameters and working
        // directory are null, so Windows never receives a command line to reparse.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// Open one file the user picked out of a directory listing with its
    /// registered default handler — and never run a program.
    ///
    /// **Why a second bridge rather than widening the first.** [`open_local_file`]
    /// serves paths *scraped out of terminal text*, where the only defence
    /// against a hostile line of output is that the syntax policy is narrow
    /// enough to be immutable. A row of the files tree has the opposite
    /// provenance: the user chose the root, this process enumerated the
    /// directory, and the user pressed the row. Making the two share one
    /// validator would mean either the tree can open nothing but pictures or
    /// terminal output can open anything.
    ///
    /// **Why programs are refused.** Not as a hedge — as the product rule
    /// `DESIGN.md` §7.1.3 already implies by making activation mean *open the
    /// preview*: the tree is a way of looking at files, and the thing next to it
    /// that runs programs is the terminal, where running one is a line you typed
    /// and can see. A double click that silently starts an executable is a verb
    /// this pane does not have, and the fact that the pane sits half an inch
    /// from a shell prompt is exactly why it should not acquire it by accident.
    pub fn open_local_path(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        validate_openable_path(path)?;
        if names_a_program(path, std::env::var("PATHEXT").unwrap_or_default().as_str()) {
            return Err(PROGRAM_REFUSED.to_owned());
        }
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut target = path.as_os_str().encode_wide().collect::<Vec<_>>();
        target.push(0);
        // SAFETY: the path was enumerated by this process from a directory the
        // user rooted a column at, both buffers stay live and NUL-terminated for
        // this synchronous call, and parameters and working directory are null,
        // so Windows never receives a command line to reparse.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// Open Explorer on a path, with a file **highlighted** inside its folder
    /// (user ruling, 2026-08-13).
    ///
    /// **A third bridge, and deliberately not a widening of the second.**
    /// [`open_local_path`] hands a path to *whatever the machine has registered
    /// for it* — which is why it reads `PATHEXT` and refuses programs, since
    /// the whole risk there is that opening a thing runs it. This one hands the
    /// path to `explorer.exe` **as text to look at**, and never executes the
    /// target at all: a `.exe` revealed is a `.exe` sitting highlighted in a
    /// folder window, which is precisely what somebody asking "where is this"
    /// wants to see and is not a way to start it. So the extension gate is
    /// absent on purpose, and the shape gate — absolute, nameable, no embedded
    /// NUL — is exactly the one its neighbour keeps.
    ///
    /// The one program this can ever launch is Explorer.
    pub fn reveal_in_explorer(hwnd: NonZeroIsize, path: &Path) -> Result<(), String> {
        validate_openable_path(path)?;
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut program = "explorer.exe".encode_utf16().collect::<Vec<_>>();
        program.push(0);
        let mut arguments = super::reveal_arguments(path, path.is_dir())
            .encode_wide()
            .collect::<Vec<_>>();
        arguments.push(0);
        // SAFETY: the target is a path this process enumerated or was given by
        // the user, `validate_openable_path` has refused any embedded NUL, all
        // three buffers stay live and NUL-terminated across this synchronous
        // call, and the only program named is Explorer.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(program.as_ptr()),
                PCWSTR(arguments.as_ptr()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// **Open Windows' own Fonts page** (user ruling 2026-08-19).
    ///
    /// **A fourth bridge, and the first one that hands `ShellExecuteW` a URI.**
    /// That is exactly the thing this codebase refuses everywhere else —
    /// `preview.rs` will open `http` and `https` and nothing else, because
    /// handing an arbitrary scheme to the shell is handing it whatever the
    /// machine has registered for that scheme. The difference here is that
    /// nothing arbitrary reaches this function: there is no parameter. The two
    /// strings it can pass are both constants of this build, and a reader who
    /// presses `Install fonts…` gets the one page or the other.
    ///
    /// `ms-settings:fonts` first, because it is where a font is installed by
    /// dropping a file on it and where the machine's own fonts already live. It
    /// is a Windows 10+ protocol handler and a machine can have it unregistered
    /// — policy-managed desktops do this — so the fall-back is the folder the
    /// page is a view of, opened through Explorer exactly as [`reveal_in_explorer`]
    /// opens any other folder. Two doors onto one place, and the reader is never
    /// told which one they came through: the answer to "where do fonts come
    /// from" is the same either way.
    ///
    /// **This product installs and deletes nothing.** A font is a machine-wide
    /// resource, installing one affects every program on the desk, and removing
    /// one a program is drawing with is a decision that was never a terminal's
    /// to take. So there is no in-app font management behind this door and there
    /// will not be — the door is the whole feature.
    pub fn open_system_fonts_page(hwnd: NonZeroIsize) -> Result<(), String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut operation = "open".encode_utf16().collect::<Vec<_>>();
        operation.push(0);
        let mut uri = super::FONT_SETTINGS_URI.encode_utf16().collect::<Vec<_>>();
        uri.push(0);
        // SAFETY: the target is a compile-time constant with no embedded NUL,
        // both buffers stay live and NUL-terminated across this synchronous
        // call, and parameters and working directory are null so Windows never
        // receives a command line to reparse.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(uri.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize > 32 {
            return Ok(());
        }
        // The URI was refused — no handler, or a policy that removed the page.
        // The folder it is a view of is still there, and Explorer is the one
        // program this fall-back can launch.
        let mut program = "explorer.exe".encode_utf16().collect::<Vec<_>>();
        program.push(0);
        let mut folder = super::fonts_folder()
            .into_os_string()
            .encode_wide()
            .collect::<Vec<_>>();
        folder.push(0);
        // SAFETY: as above, and the only program named is Explorer.
        let result = unsafe {
            ShellExecuteW(
                Some(hwnd),
                PCWSTR(operation.as_ptr()),
                PCWSTR(program.as_ptr()),
                PCWSTR(folder.as_ptr()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            Err(format!("ShellExecuteW failed with code {code}"))
        } else {
            Ok(())
        }
    }

    /// The refusal's own words, so the caller can tell "this window will not do
    /// that" apart from "Windows could not".
    pub const PROGRAM_REFUSED: &str = "the files tree does not run programs";

    /// The extensions that are a program whatever this machine's `PATHEXT` says.
    ///
    /// `PATHEXT` is the system's own list of what a *command line* will execute
    /// and it is read as well, but it is not the whole answer: a `.lnk` is not
    /// on it and points at anything at all, a `.scr` is an executable wearing a
    /// screensaver's name, and `.hta`, `.reg`, `.msi` and `.url` are each opened
    /// by a handler whose whole job is to act. Reading both means the list grows
    /// with a machine that has added to `PATHEXT` without shrinking on one that
    /// has emptied it.
    const ALWAYS_A_PROGRAM: &[&str] = &[
        "appref-ms",
        "bat",
        "cmd",
        "com",
        "cpl",
        "exe",
        "hta",
        "jar",
        "js",
        "jse",
        "lnk",
        "msc",
        "msi",
        "msp",
        "ps1",
        "pif",
        "reg",
        "scf",
        "scr",
        "url",
        "vb",
        "vbe",
        "vbs",
        "wsf",
        "wsh",
    ];

    /// Whether opening this name would start something rather than show it.
    ///
    /// Split out and given `pathext` as an argument rather than reading the
    /// environment itself, so the rule is answerable in a test on a machine
    /// whose own `PATHEXT` is whatever it is.
    fn names_a_program(path: &Path, pathext: &str) -> bool {
        let Some(extension) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
        else {
            // No extension means no registered handler to speak of, and
            // `ShellExecute` falls back to the "open with" chooser rather than
            // to running anything. That is a dialog, not an execution.
            return false;
        };
        ALWAYS_A_PROGRAM.contains(&extension.as_str())
            || pathext.split(';').any(|entry| {
                entry
                    .trim()
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(&extension)
            })
    }

    /// The shape gate the tree's own bridge keeps: absolute and nameable.
    ///
    /// Wider than [`validate_local_image_path`] in exactly one way — a UNC share
    /// is allowed — because a files column may legitimately be rooted at
    /// `\\server\share`, and a tree that can list a path it then refuses to open
    /// is a tree that lies about what its rows are.
    fn validate_openable_path(path: &Path) -> Result<(), String> {
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if units.contains(&0) {
            return Err("path contains an embedded NUL".to_owned());
        }
        let text = path.as_os_str().to_string_lossy();
        let bytes = text.as_bytes();
        let drive_rooted = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        let unc = text.starts_with(r"\\") && text.len() > 2;
        if !drive_rooted && !unc {
            return Err("path must be absolute".to_owned());
        }
        Ok(())
    }

    fn validate_local_image_path(path: &Path) -> Result<(), String> {
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if units.contains(&0) {
            return Err("local image path contains an embedded NUL".to_owned());
        }
        let text = path.as_os_str().to_string_lossy();
        let bytes = text.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_alphabetic()
            || bytes[1] != b':'
            || !matches!(bytes[2], b'\\' | b'/')
        {
            return Err("local image path must be drive-rooted and absolute".to_owned());
        }
        let allowed_extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                let extension = extension.to_ascii_lowercase();
                crate::IMAGE_FILE_EXTENSIONS.contains(&extension.as_str())
            });
        if !allowed_extension {
            return Err("local image path extension is not supported".to_owned());
        }
        Ok(())
    }

    /// Put the keyboard back on this window itself.
    ///
    /// The one thing a host can do when a hosted WebView2 says `Tab` walked off
    /// the end of its own controls (`MoveFocusRequested`): there is no call that
    /// takes focus *away* from a controller, only calls that give it to
    /// somewhere else, and a window with one `HWND` and no Win32 controls of its
    /// own has exactly one somewhere else. After this the next key arrives at
    /// winit, which is what "the keyboard came back to Folio" has to mean.
    ///
    /// Never called for anything but that: this window does not otherwise move
    /// its own focus, because there is nothing inside it that Win32 knows about.
    pub fn take_keyboard_focus(hwnd: NonZeroIsize) -> Result<(), String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
        // SAFETY: `SetFocus` takes a window handle by value. It answers the
        // previously focused window or null, and null with a last error of zero
        // is "there was no focus to take", which is not a failure.
        unsafe { SetLastError(WIN32_ERROR(0)) };
        match unsafe { SetFocus(Some(hwnd)) } {
            Ok(_) => Ok(()),
            Err(_) => {
                let error = unsafe { GetLastError().0 };
                if error == 0 {
                    Ok(())
                } else {
                    Err(format!("SetFocus failed: 0x{error:08X}"))
                }
            }
        }
    }

    /// The virtual key this **layout** produces a character on.
    ///
    /// The inverse of the mapping every keyboard event already carries, and the
    /// direction WebView2 forces: `AcceleratorKeyPressed` reports a Win32
    /// virtual key and nothing else, while this window's shortcut table is
    /// written in characters — `Ctrl+Shift+n`, `Alt+Shift+-` — because a
    /// character is what a person presses and a virtual key is what a US
    /// keyboard happens to reach it on. `VkKeyScanW` is the one call that
    /// answers for the layout actually installed; a table would answer for a
    /// keyboard the reader may not own.
    ///
    /// `None` when the active layout cannot produce the character at all, which
    /// is `-1` from Win32 and a shortcut nobody on this machine can press.
    #[must_use]
    pub fn virtual_key_for_character(character: char) -> Option<u16> {
        let mut units = [0u16; 2];
        let encoded = character.encode_utf16(&mut units);
        // Outside the basic plane there is no single UTF-16 unit to ask about,
        // and `VkKeyScanW` takes one.
        let [unit] = encoded else { return None };
        // SAFETY: `VkKeyScanW` reads one UTF-16 code unit by value.
        let answer = unsafe { VkKeyScanW(*unit) };
        if answer == -1 {
            return None;
        }
        // Low byte is the virtual key; the high byte is which modifiers reach
        // the character, and the caller's chord already says which it means.
        Some((answer as u16) & 0x00ff)
    }

    pub fn wheel_scroll_amount() -> Result<WheelScrollAmount, String> {
        let mut lines = 0u32;
        // SAFETY: SPI_GETWHEELSCROLLLINES writes one u32 to the provided live stack pointer.
        unsafe {
            SystemParametersInfoW(
                SPI_GETWHEELSCROLLLINES,
                0,
                Some((&mut lines as *mut u32).cast()),
                Default::default(),
            )
        }
        .map_err(|error| {
            format!("SystemParametersInfoW(SPI_GETWHEELSCROLLLINES) failed: {error}")
        })?;
        Ok(if lines == WHEEL_PAGESCROLL {
            WheelScrollAmount::Page
        } else {
            WheelScrollAmount::Lines(lines)
        })
    }

    /// Whether the system wants animation inside a window's client area.
    ///
    /// `SPI_GETCLIENTAREAANIMATION` is Windows' own name for the preference a
    /// browser reports as `prefers-reduced-motion` — Settings → Accessibility →
    /// Visual effects → Animation effects. It is the setting the mock-up's
    /// `@media (prefers-reduced-motion: reduce)` blocks answer to, so reading it
    /// here is what makes those rules real on this platform rather than a
    /// stylesheet branch nothing ever takes.
    ///
    /// `TRUE` means animation is *wanted*, so the polarity is the opposite of
    /// the CSS query's — the caller does that mapping, and pins it.
    pub fn client_area_animation_enabled() -> Result<bool, String> {
        // `BOOL` is a 32-bit int across the Win32 ABI, and this is the shape
        // `SystemParametersInfoW` writes through the void pointer it is given.
        let mut enabled = 0_i32;
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the provided
        // live stack pointer, exactly as SPI_GETWHEELSCROLLLINES writes one u32.
        unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&mut enabled as *mut i32).cast()),
                Default::default(),
            )
        }
        .map_err(|error| {
            format!("SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION) failed: {error}")
        })?;
        Ok(enabled != 0)
    }

    fn retry_open_clipboard(
        mut open: impl FnMut() -> Result<(), String>,
        mut wait: impl FnMut(std::time::Duration),
    ) -> Result<(), String> {
        for delay in CLIPBOARD_OPEN_RETRY_DELAYS {
            match open() {
                Ok(()) => return Ok(()),
                Err(_) => wait(delay),
            }
        }
        open().map_err(|error| {
            format!(
                "OpenClipboard failed after {} attempts (retry wait capped at {} ms): {error}",
                CLIPBOARD_OPEN_RETRY_DELAYS.len() + 1,
                CLIPBOARD_OPEN_RETRY_DELAYS
                    .iter()
                    .map(std::time::Duration::as_millis)
                    .sum::<u128>()
            )
        })
    }

    fn open_clipboard_with_retry(hwnd: HWND) -> Result<(), String> {
        retry_open_clipboard(
            || {
                // SAFETY: the caller supplies winit's live HWND and all clipboard transactions run
                // on its event-loop thread. A failed open acquires no resource that needs cleanup.
                unsafe { OpenClipboard(Some(hwnd)) }.map_err(|error| error.to_string())
            },
            std::thread::sleep,
        )
    }

    pub fn clipboard_text(hwnd: NonZeroIsize) -> Result<String, String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
        // SAFETY: all calls run on winit's event-loop thread. The clipboard remains open while the
        // borrowed global-memory handle is locked, its UTF-16 content is copied, and then both the
        // memory and clipboard are released before returning.
        open_clipboard_with_retry(hwnd)?;
        unsafe {
            let result = (|| {
                IsClipboardFormatAvailable(CF_UNICODETEXT)
                    .map_err(|_| "clipboard has no Unicode text".to_owned())?;
                let handle = GetClipboardData(CF_UNICODETEXT)
                    .map_err(|error| format!("GetClipboardData(CF_UNICODETEXT) failed: {error}"))?;
                let global = HGLOBAL(handle.0);
                let byte_len = GlobalSize(global);
                if byte_len < size_of::<u16>() {
                    return Ok(String::new());
                }
                let pointer = GlobalLock(global).cast::<u16>();
                if pointer.is_null() {
                    return Err(format!(
                        "GlobalLock(clipboard text) failed: {}",
                        GetLastError().0
                    ));
                }
                let units = std::slice::from_raw_parts(pointer, byte_len / size_of::<u16>());
                let nul = units
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(units.len());
                let copied = units[..nul].to_vec();
                // GlobalUnlock returns false after releasing the final lock; GetLastError
                // distinguishes that successful case, so no recovery is attached to its Result.
                let _ = GlobalUnlock(global);
                String::from_utf16(&copied)
                    .map_err(|error| format!("clipboard text is invalid UTF-16: {error}"))
            })();
            let close = CloseClipboard().map_err(|error| format!("CloseClipboard failed: {error}"));
            match (result, close) {
                (Ok(text), Ok(())) => Ok(text),
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            }
        }
    }

    pub fn set_clipboard_text(hwnd: NonZeroIsize, text: &str) -> Result<(), String> {
        let hwnd = HWND(hwnd.get() as *mut c_void);
        let mut units = text.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        // SAFETY: the event-loop thread owns the clipboard for this transaction. The movable
        // allocation is locked only while copying the NUL-terminated UTF-16 payload. After a
        // successful SetClipboardData Windows owns it; every earlier failure frees it locally.
        open_clipboard_with_retry(hwnd)?;
        unsafe {
            let result = (|| {
                EmptyClipboard().map_err(|error| format!("EmptyClipboard failed: {error}"))?;
                let byte_len = units.len() * size_of::<u16>();
                let global = GlobalAlloc(GMEM_MOVEABLE, byte_len)
                    .map_err(|error| format!("GlobalAlloc(clipboard text) failed: {error}"))?;
                let pointer = GlobalLock(global).cast::<u16>();
                if pointer.is_null() {
                    let error = GetLastError().0;
                    let _ = GlobalFree(Some(global));
                    return Err(format!("GlobalLock(clipboard text) failed: {error}"));
                }
                std::ptr::copy_nonoverlapping(units.as_ptr(), pointer, units.len());
                let _ = GlobalUnlock(global);
                if let Err(error) = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(global.0))) {
                    let _ = GlobalFree(Some(global));
                    return Err(format!("SetClipboardData(CF_UNICODETEXT) failed: {error}"));
                }
                Ok(())
            })();
            let close = CloseClipboard().map_err(|error| format!("CloseClipboard failed: {error}"));
            match (result, close) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(error), _) | (Ok(()), Err(error)) => Err(error),
            }
        }
    }

    /// One deferred native gesture, from asked-for to answered.
    ///
    /// `Ask` is what the request carried and `Answer` is what it came back with.
    /// The request's own arguments ride in `Posted` rather than in a second
    /// field beside the phase, because they are only meaningful while the phase
    /// is `Posted`: a start folder left lying around after the dialog closed is
    /// a value with no owner and no expiry.
    #[derive(Debug)]
    enum DeferredPhase<Ask, Answer> {
        Idle,
        Posted(Ask),
        Showing,
        Complete(Answer),
    }

    /// The gate that keeps a nested native message pump out of a winit callback.
    ///
    /// Generic over both ends so the one state machine serves every gesture that
    /// has to be deferred this way — the formula menu, whose nested pump is
    /// `TrackPopupMenu`'s, and the folder picker, whose nested pump is
    /// `IFileDialog::Show`'s. Both have exactly the same hazard and therefore
    /// exactly the same shape: post a private message, do the blocking thing
    /// after the initiating callback has returned, and leave the answer where
    /// the next turn of the loop will find it.
    #[derive(Debug)]
    struct DeferredState<Ask, Answer> {
        phase: Mutex<DeferredPhase<Ask, Answer>>,
    }

    impl<Ask, Answer> DeferredState<Ask, Answer> {
        fn new() -> Self {
            Self {
                phase: Mutex::new(DeferredPhase::Idle),
            }
        }

        fn begin_request(&self, ask: Ask) -> bool {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*phase, DeferredPhase::Idle) {
                return false;
            }
            *phase = DeferredPhase::Posted(ask);
            true
        }

        fn cancel_request(&self) {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if matches!(*phase, DeferredPhase::Posted(_)) {
                *phase = DeferredPhase::Idle;
            }
        }

        /// Take the gesture's arguments and move it to `Showing`, or `None` when
        /// this is not a posted request to begin.
        fn begin_showing(&self) -> Option<Ask> {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if !matches!(*phase, DeferredPhase::Posted(_)) {
                return None;
            }
            let DeferredPhase::Posted(ask) = std::mem::replace(&mut *phase, DeferredPhase::Showing)
            else {
                unreachable!("phase was matched as posted immediately before replacement")
            };
            Some(ask)
        }

        fn complete(&self, answer: Answer) {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            if matches!(*phase, DeferredPhase::Showing) {
                *phase = DeferredPhase::Complete(answer);
            }
        }

        fn take_result(&self) -> Option<Answer> {
            let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
            let DeferredPhase::Complete(_) = &*phase else {
                return None;
            };
            let DeferredPhase::Complete(answer) =
                std::mem::replace(&mut *phase, DeferredPhase::Idle)
            else {
                unreachable!("phase was matched as complete immediately before replacement")
            };
            Some(answer)
        }
    }

    type MathMenuState = DeferredState<(), Result<bool, String>>;

    /// Formula context-menu bridge whose nested native message pump never starts inside a winit
    /// application callback. `request` only posts a private window message. The subclass receives
    /// it after the current `DispatchMessageW`/winit callback has returned, so any RedrawRequested
    /// emitted by TrackPopupMenu's nested pump finds winit's event-handler slot restored.
    pub struct MathContextMenu {
        hwnd: HWND,
        state: Arc<MathMenuState>,
    }

    impl MathContextMenu {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Arc::new(MathMenuState::new());
            // SAFETY: installation and removal occur on the HWND's event-loop thread. The Arc
            // keeps dwRefData live for the full installed interval; the callback takes its own
            // temporary strong reference before entering the nested menu loop.
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(math_context_menu_subclass),
                    MATH_MENU_SUBCLASS_ID,
                    Arc::as_ptr(&state) as usize,
                )
            };
            if !installed.as_bool() {
                return Err(format!("SetWindowSubclass(math menu) failed: {}", unsafe {
                    GetLastError().0
                }));
            }
            Ok(Self { hwnd, state })
        }

        /// Queue the native menu once. A second request while the first is posted, showing, or
        /// waiting to be consumed is an ordinary coalesced UI race and returns `Ok(false)`.
        pub fn request(&self) -> Result<bool, String> {
            if !self.state.begin_request(()) {
                return Ok(false);
            }
            // SAFETY: PostMessageW copies these value parameters into the owning thread's queue
            // and never dispatches the subclass synchronously on this callback stack.
            if let Err(error) = unsafe {
                PostMessageW(
                    Some(self.hwnd),
                    DEFERRED_MATH_MENU_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                self.state.cancel_request();
                return Err(format!("PostMessageW(math menu) failed: {error}"));
            }
            Ok(true)
        }

        pub fn take_result(&self) -> Option<Result<bool, String>> {
            self.state.take_result()
        }
    }

    impl Drop for MathContextMenu {
        fn drop(&mut self) {
            // SAFETY: this object is dropped on the same event-loop thread that installed the
            // subclass. A callback already inside TrackPopupMenu owns a temporary Arc, so nested
            // CloseRequested teardown cannot invalidate its state.
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(math_context_menu_subclass),
                    MATH_MENU_SUBCLASS_ID,
                )
            };
        }
    }

    unsafe extern "system" fn math_context_menu_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        if message != DEFERRED_MATH_MENU_MESSAGE {
            // SAFETY: forwarding untouched messages is the required subclass contract.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        let state_pointer = reference_data as *const MathMenuState;
        if state_pointer.is_null() {
            return LRESULT(0);
        }
        // SAFETY: the installed MathContextMenu owns one Arc at callback entry. Incrementing before
        // constructing the temporary Arc keeps state alive even if a nested CloseRequested drops
        // the Runtime while TrackPopupMenu is open.
        unsafe { Arc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the strong reference consumed here.
        let state = unsafe { Arc::from_raw(state_pointer) };
        if state.begin_showing().is_some() {
            state.complete(track_math_context_menu(hwnd));
        }
        LRESULT(0)
    }

    fn track_math_context_menu(hwnd: HWND) -> Result<bool, String> {
        // SAFETY: the HWND and menu belong to the current GUI thread. This function is reached only
        // from the posted-message subclass, after winit's initiating callback has returned.
        unsafe {
            let menu =
                CreatePopupMenu().map_err(|error| format!("CreatePopupMenu failed: {error}"))?;
            let result = (|| {
                AppendMenuW(menu, MF_STRING, 1, windows::core::w!("Copy LaTeX"))
                    .map_err(|error| format!("AppendMenuW(Copy LaTeX) failed: {error}"))?;
                let mut cursor = POINT::default();
                GetCursorPos(&mut cursor)
                    .map_err(|error| format!("GetCursorPos failed: {error}"))?;
                let command = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    cursor.x,
                    cursor.y,
                    None,
                    hwnd,
                    None,
                );
                Ok(command.0 as usize == 1)
            })();
            let destroy = DestroyMenu(menu).map_err(|error| format!("DestroyMenu failed: {error}"));
            match (result, destroy) {
                (Ok(selected), Ok(())) => Ok(selected),
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            }
        }
    }

    type FolderPickerState = DeferredState<Vec<u16>, Result<Option<PathBuf>, String>>;

    /// The system's own folder chooser, on the same deferred footing as
    /// [`MathContextMenu`] and for a sharper version of the same reason (E55).
    ///
    /// `IFileDialog::Show` is **modal**: it runs a nested message loop that goes
    /// on dispatching to this window for as long as the dialog is open. Started
    /// from inside a winit callback that would re-enter the application's own
    /// event handling while the `&mut` borrow that started it is still live —
    /// the exact hazard the formula menu's comment records, except that a menu
    /// closes in a moment and a folder chooser can stand open for a minute while
    /// somebody goes looking. So the press only *posts*: the dialog opens from
    /// the subclass, after the callback has returned, and the answer waits in
    /// this state until the next turn of the loop collects it.
    pub struct FolderPicker {
        hwnd: HWND,
        state: Arc<FolderPickerState>,
    }

    impl FolderPicker {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Arc::new(FolderPickerState::new());
            // SAFETY: installation and removal occur on the HWND's event-loop thread. The Arc
            // keeps dwRefData live for the full installed interval; the callback takes its own
            // temporary strong reference before entering the nested dialog loop.
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(folder_picker_subclass),
                    FOLDER_PICKER_SUBCLASS_ID,
                    Arc::as_ptr(&state) as usize,
                )
            };
            if !installed.as_bool() {
                return Err(format!(
                    "SetWindowSubclass(folder picker) failed: {}",
                    unsafe { GetLastError().0 }
                ));
            }
            Ok(Self { hwnd, state })
        }

        /// Queue the chooser once, starting at `start` if that names a folder.
        ///
        /// A second request while one is posted, showing or waiting to be
        /// collected returns `Ok(false)` — the same coalescing the formula menu
        /// does, and here it is also what stops a second press on `Browse…`
        /// from stacking a second modal dialog behind the first.
        pub fn request(&self, start: Option<&Path>) -> Result<bool, String> {
            let start = start
                .map(|start| {
                    let mut units = start.as_os_str().encode_wide().collect::<Vec<_>>();
                    units.push(0);
                    units
                })
                .unwrap_or_default();
            if !self.state.begin_request(start) {
                return Ok(false);
            }
            // SAFETY: PostMessageW copies these value parameters into the owning thread's queue
            // and never dispatches the subclass synchronously on this callback stack.
            if let Err(error) = unsafe {
                PostMessageW(
                    Some(self.hwnd),
                    DEFERRED_FOLDER_PICKER_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                self.state.cancel_request();
                return Err(format!("PostMessageW(folder picker) failed: {error}"));
            }
            Ok(true)
        }

        /// The chosen folder, `None` for a cancelled dialog, or the reason the
        /// chooser could not be shown — once, and only once the dialog is shut.
        pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
            self.state.take_result()
        }
    }

    impl Drop for FolderPicker {
        fn drop(&mut self) {
            // SAFETY: this object is dropped on the same event-loop thread that installed the
            // subclass. A callback already inside the dialog owns a temporary Arc, so nested
            // CloseRequested teardown cannot invalidate its state.
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(folder_picker_subclass),
                    FOLDER_PICKER_SUBCLASS_ID,
                )
            };
        }
    }

    type ImagePickerState =
        DeferredState<(ShellPickKind, Vec<u16>), Result<Option<PathBuf>, String>>;

    /// The system's own **file** chooser — [`FolderPicker`]'s twin, and every
    /// word of its doc comment applies unchanged.
    ///
    /// It answers for two rows now (§7.1.6c-6b): the window's ground picture,
    /// filtered to the formats this product can decode, and a profile's program,
    /// which is filtered to nothing at all — a shell is whatever the reader says
    /// it is, and a chooser that only offered `*.exe` would refuse a `.bat`, a
    /// `.cmd` and every launcher shipped as one.
    ///
    /// **One bridge and not two**, unlike the folder chooser beside it, and the
    /// difference is the reason that one is separate: the deferral's contract is
    /// "one gesture in flight", and these two rows are in the same dialog and
    /// cannot both be pressed at once. The kind travels *with* the request, so
    /// the answer can never be handed to the row that did not ask.
    ///
    /// A second subclass on the same window rather than a mode on the first,
    /// because the deferral's whole contract is "one gesture in flight": a
    /// picker that coalesced a wallpaper request into a pending folder request
    /// would answer the wrong row. Two states, two private messages, two
    /// subclass ids — and one dialog function underneath
    /// ([`show_shell_picker`]), because the COM dance is the part that must not
    /// be written twice.
    pub struct ImagePicker {
        hwnd: HWND,
        state: Arc<ImagePickerState>,
    }

    /// Which of the two files a request is for.
    ///
    /// Public because the caller names it; [`ShellPickKind::Folder`] is not
    /// reachable through this bridge, which is [`FolderPicker`]'s.
    pub type FilePickKind = ShellPickKind;

    impl ImagePicker {
        pub fn new(hwnd: NonZeroIsize) -> Result<Self, String> {
            let hwnd = HWND(hwnd.get() as *mut c_void);
            let state = Arc::new(ImagePickerState::new());
            // SAFETY: installation and removal occur on the HWND's event-loop thread. The Arc
            // keeps dwRefData live for the full installed interval; the callback takes its own
            // temporary strong reference before entering the nested dialog loop.
            let installed = unsafe {
                SetWindowSubclass(
                    hwnd,
                    Some(image_picker_subclass),
                    IMAGE_PICKER_SUBCLASS_ID,
                    Arc::as_ptr(&state) as usize,
                )
            };
            if !installed.as_bool() {
                return Err(format!(
                    "SetWindowSubclass(image picker) failed: {}",
                    unsafe { GetLastError().0 }
                ));
            }
            Ok(Self { hwnd, state })
        }

        /// Queue the chooser once, starting in `start` if that names a folder.
        ///
        /// `start` is a **folder** and not the picture currently chosen: a shell
        /// item that is a file is not a place to open at, and the useful place
        /// to open at is the one the last picture came from.
        pub fn request(&self, kind: ShellPickKind, start: Option<&Path>) -> Result<bool, String> {
            let start = start
                .map(|start| {
                    let mut units = start.as_os_str().encode_wide().collect::<Vec<_>>();
                    units.push(0);
                    units
                })
                .unwrap_or_default();
            if !self.state.begin_request((kind, start)) {
                return Ok(false);
            }
            // SAFETY: PostMessageW copies these value parameters into the owning thread's queue
            // and never dispatches the subclass synchronously on this callback stack.
            if let Err(error) = unsafe {
                PostMessageW(
                    Some(self.hwnd),
                    DEFERRED_IMAGE_PICKER_MESSAGE,
                    WPARAM(0),
                    LPARAM(0),
                )
            } {
                self.state.cancel_request();
                return Err(format!("PostMessageW(image picker) failed: {error}"));
            }
            Ok(true)
        }

        /// The chosen picture, `None` for a cancelled dialog, or the reason the
        /// chooser could not be shown — once, and only once the dialog is shut.
        pub fn take_result(&self) -> Option<Result<Option<PathBuf>, String>> {
            self.state.take_result()
        }
    }

    impl Drop for ImagePicker {
        fn drop(&mut self) {
            // SAFETY: this object is dropped on the same event-loop thread that installed the
            // subclass. A callback already inside the dialog owns a temporary Arc, so nested
            // CloseRequested teardown cannot invalidate its state.
            let _ = unsafe {
                RemoveWindowSubclass(
                    self.hwnd,
                    Some(image_picker_subclass),
                    IMAGE_PICKER_SUBCLASS_ID,
                )
            };
        }
    }

    unsafe extern "system" fn image_picker_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        if message != DEFERRED_IMAGE_PICKER_MESSAGE {
            // SAFETY: forwarding untouched messages is the required subclass contract.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        let state_pointer = reference_data as *const ImagePickerState;
        if state_pointer.is_null() {
            return LRESULT(0);
        }
        // SAFETY: the installed ImagePicker owns one Arc at callback entry. Incrementing before
        // constructing the temporary Arc keeps state alive even if a nested CloseRequested drops
        // the Runtime while the dialog is open.
        unsafe { Arc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the strong reference consumed here.
        let state = unsafe { Arc::from_raw(state_pointer) };
        if let Some((kind, start)) = state.begin_showing() {
            state.complete(show_shell_picker(hwnd, &start, kind));
        }
        LRESULT(0)
    }

    /// Put this window above (or back among) the others — `HWND_TOPMOST` /
    /// `HWND_NOTOPMOST`.
    ///
    /// The first caller in this crate to touch z-order at all: every other
    /// `SetWindowPos` here passes `SWP_NOZORDER` because it is moving or
    /// resizing and has no business reordering anything. This one is *only*
    /// about order, so it passes neither a position nor a size.
    ///
    /// `SWP_NOACTIVATE`, because "stay in front" is not "come to the front now":
    /// switching the row on while another window has the keyboard must not steal
    /// it, and switching it off must not either.
    pub fn set_window_topmost(hwnd: NonZeroIsize, topmost: bool) -> Result<(), String> {
        let after = if topmost {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle, and the insert-after
        // handle is one of the two documented sentinels rather than a window we might outlive.
        unsafe {
            SetWindowPos(
                HWND(hwnd.get() as *mut c_void),
                Some(after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| format!("SetWindowPos(topmost={topmost}) failed: {error}"))
    }

    /// Tell DWM which of the two canvases this window is wearing —
    /// `DWMWA_USE_IMMERSIVE_DARK_MODE`.
    ///
    /// **This is the whole of "the acrylic plate follows the scheme"**
    /// (§7.1.6c-4f amendment, measured on this machine 2026-08-18). The plate
    /// `DWMSBT_TRANSIENTWINDOW` draws is DWM's, not ours, and the one thing DWM
    /// lets a window say about its colour is this flag — so the plate is dark
    /// exactly when the window has declared itself dark. Measured, light desktop
    /// `#F2F2F2` behind, Solarized Dark at 30 %: the pane body reads
    /// `(156,177,183)` without the flag and `(99,120,126)` with it.
    ///
    /// **The order does not matter and the plate is not latched.** 4f wrote down
    /// that setting this to 1 did not darken the material; it does. Setting it
    /// before the backdrop, after the backdrop, and either side of a
    /// `DWMSBT_NONE` round trip all measured the identical `(99,120,126)`, so a
    /// window may say this whenever its canvas moves and need not re-ask for the
    /// backdrop afterwards.
    ///
    /// **Not tied to the Acrylic row.** The flag is a statement about the
    /// window, not about one setting: it is also what colours the one-pixel DWM
    /// border (`(176,176,176)` -> `(153,153,153)` in the same measurement), and
    /// a dark window with the blur switched off still owes itself a dark border.
    ///
    /// Best-effort, like the corner preference above and unlike
    /// [`set_system_backdrop`]: there is no settings row whose position claims
    /// this happened, so a Windows too old to know the attribute (it is
    /// 20H1's; 1809 spelled it 19) simply keeps the border it already had.
    pub fn set_window_dark_mode(hwnd: NonZeroIsize, dark: bool) -> Result<(), String> {
        let value = i32::from(dark);
        // SAFETY: as `set_system_backdrop` — the pointer is to a live local of
        // exactly the size passed, and DWM copies it before returning.
        unsafe {
            DwmSetWindowAttribute(
                HWND(hwnd.get() as *mut c_void),
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                std::ptr::from_ref(&value).cast::<c_void>(),
                size_of::<i32>() as u32,
            )
        }
        .map_err(|error| format!("DwmSetWindowAttribute(immersive dark={dark}) failed: {error}"))
    }

    /// Ask DWM for (or withdraw) the acrylic system backdrop —
    /// `DWMWA_SYSTEMBACKDROP_TYPE` = `DWMSBT_TRANSIENTWINDOW` / `DWMSBT_NONE`.
    ///
    /// `DWMSBT_TRANSIENTWINDOW` and not `DWMSBT_MAINWINDOW`: the two differ in
    /// how much of what is behind survives the blur, and the transient one is
    /// the stronger, more saturated recipe Windows uses for flyouts and command
    /// palettes. A terminal with a picture behind it wants the one that still
    /// shows you what is back there.
    ///
    /// Withdrawing is `DWMSBT_NONE` and not `DWMSBT_AUTO`: `AUTO` hands the
    /// decision back to DWM, which is a different answer from "off" and would
    /// leave a Mica-eligible window quietly wearing Mica after the row said no.
    ///
    /// Failure is returned rather than swallowed, unlike the corner-preference
    /// call above it, because this one has a row on a settings page: a user who
    /// switches it on is owed the reason it did not take, and
    /// [`system_backdrop_available`] is that reason asked in advance.
    pub fn set_system_backdrop(hwnd: NonZeroIsize, acrylic: bool) -> Result<(), String> {
        let backdrop = if acrylic {
            DWMSBT_TRANSIENTWINDOW
        } else {
            DWMSBT_NONE
        };
        // SAFETY: the pointer is to a live local of exactly the size passed, and DWM copies it
        // before returning.
        unsafe {
            DwmSetWindowAttribute(
                HWND(hwnd.get() as *mut c_void),
                DWMWA_SYSTEMBACKDROP_TYPE,
                std::ptr::from_ref(&backdrop).cast::<c_void>(),
                size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
            )
        }
        .map_err(|error| format!("DwmSetWindowAttribute(system backdrop) failed: {error}"))
    }

    /// Whether this Windows knows what a system backdrop is.
    ///
    /// **Asked of DWM, not of a build number.** `DWMWA_SYSTEMBACKDROP_TYPE`
    /// arrived in Windows 11 22H2 and every older Windows answers `E_INVALIDARG`
    /// for an attribute it has never heard of — so setting the attribute to its
    /// own default (`DWMSBT_AUTO`, which is what an untouched window already
    /// has) both changes nothing and returns the exact fact the row needs. A
    /// build-number gate would be this crate maintaining a table of which
    /// Windows has which feature, which is a table that is wrong the moment
    /// anybody backports anything.
    #[must_use]
    pub fn system_backdrop_available(hwnd: NonZeroIsize) -> bool {
        let probe = DWMSBT_AUTO;
        // SAFETY: as `set_system_backdrop`; the value written is the attribute's own default.
        unsafe {
            DwmSetWindowAttribute(
                HWND(hwnd.get() as *mut c_void),
                DWMWA_SYSTEMBACKDROP_TYPE,
                std::ptr::from_ref(&probe).cast::<c_void>(),
                size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
            )
        }
        .is_ok()
    }

    unsafe extern "system" fn folder_picker_subclass(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        reference_data: usize,
    ) -> LRESULT {
        if message != DEFERRED_FOLDER_PICKER_MESSAGE {
            // SAFETY: forwarding untouched messages is the required subclass contract.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        let state_pointer = reference_data as *const FolderPickerState;
        if state_pointer.is_null() {
            return LRESULT(0);
        }
        // SAFETY: the installed FolderPicker owns one Arc at callback entry. Incrementing before
        // constructing the temporary Arc keeps state alive even if a nested CloseRequested drops
        // the Runtime while the dialog is open.
        unsafe { Arc::increment_strong_count(state_pointer) };
        // SAFETY: the increment immediately above created the strong reference consumed here.
        let state = unsafe { Arc::from_raw(state_pointer) };
        if let Some(start) = state.begin_showing() {
            state.complete(show_shell_picker(hwnd, &start, ShellPickKind::Folder));
        }
        LRESULT(0)
    }

    /// A NUL-terminated UTF-16 copy, for the Win32 calls that take a `PCWSTR`
    /// and read until the terminator.
    fn wide_null(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Which of the three things `IFileOpenDialog` is being asked for.
    ///
    /// One dialog function and three guises rather than three functions, because
    /// everything around the two lines that differ — the apartment, its balance,
    /// the cancel/error split, the shell allocation nobody else frees — is
    /// identical and is the part that is easy to get subtly wrong three times.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum ShellPickKind {
        Folder,
        Image,
        /// A profile's own shell (§7.1.6c-6b) — **unfiltered on purpose**: a
        /// chooser that offered only `*.exe` would refuse a `.bat`, a `.cmd` and
        /// every launcher shipped as one, and what may be started is the
        /// operating system's answer rather than this dialog's.
        Program,
    }

    /// Show `IFileOpenDialog` and report what came back.
    ///
    /// `IFileOpenDialog` rather than the older `SHBrowseForFolder`, because the
    /// latter draws a tree from 1995 with no address bar, no typing, no
    /// favourites and no resize — and the folder row exists precisely for the
    /// folders the quick list above it could not name, which are the ones you
    /// need to navigate to.
    ///
    /// `FOS_FORCEFILESYSTEM` is what keeps the answer a *path*: without it the
    /// dialog will happily return a shell namespace item — a library, a phone
    /// over MTP, a search results folder — that has no directory behind it for
    /// anything here to enumerate. For a picture it does a second job: an image
    /// on a phone over MTP has no path for the decoder to open.
    fn show_shell_picker(
        hwnd: HWND,
        start: &[u16],
        kind: ShellPickKind,
    ) -> Result<Option<PathBuf>, String> {
        // SAFETY: every call below runs on the window's own GUI thread, reached only from the
        // posted-message subclass after winit's initiating callback has returned. The apartment
        // is initialized and balanced here, and each COM object is dropped by the `windows`
        // crate's own `Release` at the end of its scope.
        unsafe {
            let apartment = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            if apartment == RPC_E_CHANGED_MODE {
                // The thread is already a multi-threaded apartment, which a shell
                // dialog may not be shown from. Reported rather than forced: the
                // caller's answer to a chooser it cannot show is to leave the
                // root where it was, which is the correct outcome and not one
                // worth breaking someone's apartment model to avoid.
                return Err("the event-loop thread is not a single-threaded apartment".to_owned());
            }
            // S_FALSE means "already initialized, and this call counted" — it is a
            // success that still owes a `CoUninitialize`, which is why the balance
            // is taken from `is_ok` rather than from `== S_OK`.
            let balance = apartment.is_ok();
            let result = (|| {
                let dialog: IFileOpenDialog =
                    CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                        .map_err(|error| format!("CoCreateInstance(FileOpenDialog): {error}"))?;
                let options = dialog
                    .GetOptions()
                    .map_err(|error| format!("IFileDialog::GetOptions: {error}"))?;
                let options = match kind {
                    ShellPickKind::Folder => {
                        options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST
                    }
                    // `FOS_FILEMUSTEXIST` and not `FOS_PATHMUSTEXIST` alone: the
                    // answer is going straight to a decoder, and a name typed
                    // into the box for a file that is not there would arrive as
                    // a decode failure a second later, with the dialog already
                    // gone and nothing on screen to connect the two.
                    // The same three for a program, and for the same reason one
                    // step further on: a name typed into the box for a file that
                    // is not there would be written into `profiles.json` as this
                    // profile's shell and would grey the row the next time the
                    // dialog was opened, with nothing on screen to connect the
                    // two.
                    ShellPickKind::Image | ShellPickKind::Program => {
                        options | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST
                    }
                };
                dialog
                    .SetOptions(options)
                    .map_err(|error| format!("IFileDialog::SetOptions: {error}"))?;
                // The filter is built from the one extension list the decoder
                // honours, so the chooser cannot offer a format that would be
                // refused a moment after it was picked. Its own failure is not
                // fatal — an unfiltered dialog still chooses a file, and the
                // decoder is still the thing that decides.
                let filter_name: Vec<u16>;
                let filter_spec: Vec<u16>;
                if kind == ShellPickKind::Image {
                    filter_name = wide_null("Images");
                    filter_spec = wide_null(&crate::image_file_filter_spec());
                    let filters = [COMDLG_FILTERSPEC {
                        pszName: PCWSTR(filter_name.as_ptr()),
                        pszSpec: PCWSTR(filter_spec.as_ptr()),
                    }];
                    let _ = dialog.SetFileTypes(&filters);
                }
                // Where the column is already rooted, so the chooser opens where
                // you are looking rather than wherever Windows last left it. Its
                // failure is not this function's failure: a root that has since
                // been deleted or unplugged is a perfectly good reason to open at
                // the system's default instead of refusing to open at all.
                if start.len() > 1
                    && let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                        PCWSTR(start.as_ptr()),
                        None,
                    )
                {
                    let _ = dialog.SetFolder(&folder);
                }
                if let Err(error) = dialog.Show(Some(hwnd)) {
                    return if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                        Ok(None)
                    } else {
                        Err(format!("IFileDialog::Show: {error}"))
                    };
                }
                let item = dialog
                    .GetResult()
                    .map_err(|error| format!("IFileOpenDialog::GetResult: {error}"))?;
                let name = item
                    .GetDisplayName(SIGDN_FILESYSPATH)
                    .map_err(|error| format!("IShellItem::GetDisplayName: {error}"))?;
                let path = name.to_string();
                // The shell allocated it; the shell's allocator frees it. This is
                // the one allocation in this function that Rust does not own.
                CoTaskMemFree(Some(name.0.cast()));
                let noun = match kind {
                    ShellPickKind::Folder => "folder",
                    ShellPickKind::Image => "picture",
                    ShellPickKind::Program => "program",
                };
                path.map(|path| Some(PathBuf::from(path)))
                    .map_err(|error| format!("the chosen {noun}'s name is not UTF-16: {error}"))
            })();
            if balance {
                CoUninitialize();
            }
            result
        }
    }

    /// Thread-affine native positioning bridge used by Microsoft Pinyin.
    ///
    /// Some Chinese IMEs follow the Win32 system caret instead of IMM32 candidate-form requests.
    /// winit remains the sole owner of the HIMC candidate/composition forms; this bridge only
    /// supplies the otherwise invisible 1x1 caret signal.
    pub struct ImeSystemCaret {
        hwnd: HWND,
        active: bool,
    }

    impl ImeSystemCaret {
        pub fn new(hwnd: NonZeroIsize) -> Self {
            Self {
                hwnd: HWND(hwnd.get() as *mut c_void),
                active: false,
            }
        }

        pub fn update(&mut self, x: i32, y: i32) -> Result<(), String> {
            if !active_layout_is_chinese() {
                self.destroy();
                return Ok(());
            }

            // SAFETY: the HWND originates from winit and calls occur on its event-loop thread.
            // A Win32 caret is owned by that GUI thread. The null bitmap plus 1x1 dimensions is
            // the non-painted compatibility caret used only as an IME positioning signal.
            unsafe {
                if !self.active {
                    CreateCaret(self.hwnd, None, 1, 1)
                        .map_err(|error| format!("CreateCaret(1x1) failed: {error}"))?;
                    self.active = true;
                }
                SetCaretPos(x, y).map_err(|error| format!("SetCaretPos failed: {error}"))?;
            }
            Ok(())
        }

        pub fn destroy(&mut self) {
            if !self.active {
                return;
            }
            // SAFETY: this object is dropped/disabled on the same event-loop thread that created
            // the thread-affine caret. There is no borrowed memory and failure needs no recovery.
            unsafe {
                let _ = DestroyCaret();
            }
            self.active = false;
        }
    }

    impl Drop for ImeSystemCaret {
        fn drop(&mut self) {
            self.destroy();
        }
    }

    fn active_layout_is_chinese() -> bool {
        // SAFETY: thread id zero asks user32 for the calling event-loop thread's active layout.
        let layout = unsafe { GetKeyboardLayout(0) };
        primary_language_id(layout.0 as usize as u16) == 0x04
    }

    fn primary_language_id(language_id: u16) -> u16 {
        language_id & 0x03ff
    }

    pub fn get_dpi_for_window(hwnd: NonZeroIsize) -> Result<u32, String> {
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle. GetDpiForWindow only
        // reads the DPI associated with that window and returns zero for an invalid handle.
        let dpi = unsafe { GetDpiForWindow(HWND(hwnd.get() as *mut c_void)) };
        if dpi == 0 {
            Err("GetDpiForWindow returned zero".to_owned())
        } else {
            Ok(dpi)
        }
    }

    /// Whether the window is minimized (iconic).
    ///
    /// The companion of the `IsZoomed` test the snapshot already makes, and it
    /// exists for the same reason: while a window is iconic, `GetWindowRect`
    /// stops describing the window and starts describing the icon — Windows
    /// parks the rectangle far off-screen at a token size (`-32000, -32000` in
    /// the classic shell; a 157x25 rect at `x = -16000` was what this app
    /// actually recorded). That rectangle is not a place the user ever put the
    /// window, so nothing may be derived from it.
    ///
    /// Deliberately *not* `GetWindowPlacement().rcNormalPosition`, which looks
    /// like the more direct answer and is a trap: `rcNormalPosition` is stated
    /// in workspace coordinates, so on any machine with a taskbar it differs
    /// from `GetWindowRect`'s screen coordinates by the work-area origin. Mixing
    /// the two would reintroduce exactly the per-restart drift that making this
    /// module speak one rectangle — the outer rect — was meant to end.
    pub fn is_window_minimized(hwnd: NonZeroIsize) -> bool {
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle.
        // IsIconic only reads window state and reports false for a bad handle.
        unsafe { IsIconic(HWND(hwnd.get() as *mut c_void)) }.as_bool()
    }

    pub fn get_window_rect(hwnd: NonZeroIsize) -> Result<WindowRect, String> {
        let mut rect = RECT::default();
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle and `rect` remains valid
        // and exclusively borrowed for the duration of this read-only query.
        unsafe { GetWindowRect(HWND(hwnd.get() as *mut c_void), &mut rect) }
            .map_err(|error| format!("GetWindowRect failed: {error}"))?;
        Ok(WindowRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
    }

    /// Place the window's *outer* rectangle exactly, in physical screen pixels.
    ///
    /// The counterpart of [`get_window_rect`], and the only sizing call that is
    /// meaningful once [`CustomWindowFrame`] is installed: winit sizes by client
    /// area and derives the outer rect with `AdjustWindowRectExForDpi`, but this
    /// window's `WM_NCCALCSIZE` has made the two the same rectangle, so that
    /// derivation adds a frame margin the window does not wear. Passing the outer
    /// rect straight through is what makes `GetWindowRect` -> save -> restore ->
    /// `GetWindowRect` an identity.
    pub fn set_window_outer_rect(hwnd: NonZeroIsize, rect: WindowRect) -> Result<(), String> {
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle. No
        // insert-after handle is passed, which `SWP_NOZORDER` makes inert.
        unsafe {
            SetWindowPos(
                HWND(hwnd.get() as *mut c_void),
                None,
                rect.left,
                rect.top,
                rect.right.saturating_sub(rect.left).max(1),
                rect.bottom.saturating_sub(rect.top).max(1),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| format!("SetWindowPos(restore window rect) failed: {error}"))
    }

    /// The work area — the monitor minus the taskbar and other appbars — of the
    /// display this window sits on, in physical pixels.
    ///
    /// `docs/M2-layout-solver-spec.md` §2.6.5 clamps the window's minimum inner
    /// size to 60% of *this* rectangle, not of the monitor: a minimum computed
    /// against the full monitor would quietly include the taskbar's strip and let
    /// the window refuse to shrink past something the user can never see all of.
    /// Failure is reported rather than guessed at, because tiny-window §4.4 rules
    /// that a never-observed work area means "set no minimum at all".
    pub fn get_work_area(hwnd: NonZeroIsize) -> Result<WindowRect, String> {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle.
        // MonitorFromWindow with MONITOR_DEFAULTTONEAREST always returns a valid
        // monitor handle, and `info` stays valid and exclusively borrowed across
        // this read-only query with its `cbSize` set as the API requires.
        let ok = unsafe {
            let monitor =
                MonitorFromWindow(HWND(hwnd.get() as *mut c_void), MONITOR_DEFAULTTONEAREST);
            GetMonitorInfoW(monitor, &mut info)
        };
        if !ok.as_bool() {
            return Err("GetMonitorInfoW failed".to_owned());
        }
        Ok(WindowRect {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom,
        })
    }

    /// **Send one file to the Recycle Bin** (§7.1.6c-4d).
    ///
    /// The whole of the difference between this and `std::fs::remove_file` is
    /// the promise the card makes: `Delete scheme` says the file was moved to
    /// the Recycle Bin, so it has to be findable there. A colour scheme is a
    /// file the user wrote by hand — often over an afternoon — and a delete
    /// button in a settings dialog that destroys one is a button nobody can
    /// afford to press by accident.
    ///
    /// `SHFileOperationW` rather than `IFileOperation`: the newer interface
    /// wants an apartment and an `IShellItem` per file, and what is being asked
    /// here is one path with one flag. The flags say exactly that and no more —
    /// no confirmation (the dialog already knows what it is deleting), no
    /// progress UI and no error UI for a single small file, and **`ALLOWUNDO`
    /// paired with `WANTNUKEWARNING`**: a file the bin cannot take must raise
    /// the shell's own "this will be deleted permanently" prompt rather than be
    /// destroyed silently, because "moved to the Recycle Bin" would then be a
    /// sentence this product had told and not kept.
    ///
    /// `Ok(false)` is the user answering that prompt with no: nothing happened,
    /// and it is not an error.
    ///
    /// The path is passed **double-NUL terminated**, which is this API's list
    /// convention: a single terminator would leave the shell reading whatever
    /// follows the buffer as the second file to delete.
    pub fn recycle(path: &std::path::Path) -> Result<bool, String> {
        let mut wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .chain(std::iter::once(0))
            .collect();
        if wide.iter().filter(|unit| **unit == 0).count() != 2 {
            return Err("a path with an interior NUL is not a file name".to_owned());
        }
        let mut operation = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(wide.as_mut_ptr()),
            fFlags: (FOF_ALLOWUNDO
                | FOF_NOCONFIRMATION
                | FOF_WANTNUKEWARNING
                | FOF_NOERRORUI
                | FOF_SILENT)
                .0 as u16,
            ..Default::default()
        };
        // SAFETY: `operation` is exclusively borrowed for the call and `wide`
        // outlives it, holding a double-NUL-terminated UTF-16 path as `pFrom`
        // requires. The call performs no callbacks and returns a status code.
        let status = unsafe { SHFileOperationW(&raw mut operation) };
        if status != 0 {
            return Err(format!("SHFileOperationW failed: {status}"));
        }
        Ok(!operation.fAnyOperationsAborted.as_bool())
    }

    /// Paint this window's own background in `rgb`, or in **nothing** at all.
    ///
    /// `None` installs the null brush, and that is what a translucent ground is
    /// made of (§7.1.6c-4b). The composition tree is created `topmost = true`,
    /// which composites it **above** the window's own painting (§2.3 A2) — so
    /// wherever the swapchain is translucent, what shows through is this brush
    /// and not the desktop. An alpha on the clear alone therefore produces a
    /// window that is see-through onto its own opaque background, which looks
    /// exactly like a window that is not see-through at all. Removing the brush
    /// is the other half of the feature.
    ///
    /// The cost is stated rather than hidden: with no brush, the band a resize
    /// opens up before the swapchain reaches it shows the desktop instead of the
    /// theme colour. That is the correct answer for a window the user has asked
    /// to be see-through, and it is why the opaque case keeps its brush.
    pub fn install_window_class_background(
        hwnd: NonZeroIsize,
        rgb: Option<[u8; 3]>,
    ) -> Result<(), String> {
        let mut installed = WINDOW_CLASS_BACKGROUND
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "window class background brush lock poisoned".to_owned())?;
        // SAFETY: `hwnd` originates from winit's live Win32WindowHandle on the event-loop thread.
        // CreateSolidBrush returns an independent GDI brush. SetClassLongPtrW atomically replaces
        // the class brush; only after that succeeds do we delete the previous brush that *we* own.
        // The original winit brush returned on the first call is not ours and is never deleted here.
        unsafe {
            let brush = match rgb {
                Some([r, g, b]) => {
                    let color = COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16));
                    let brush = CreateSolidBrush(color);
                    if brush.is_invalid() {
                        return Err(format!("CreateSolidBrush failed: {}", GetLastError().0));
                    }
                    Some(brush)
                }
                None => None,
            };

            SetLastError(WIN32_ERROR(0));
            let previous = SetClassLongPtrW(
                HWND(hwnd.get() as *mut c_void),
                GCLP_HBRBACKGROUND,
                brush.map_or(0, |brush| brush.0 as isize),
            );
            let error = GetLastError();
            if previous == 0 && error.0 != 0 {
                if let Some(brush) = brush {
                    let _ = DeleteObject(HGDIOBJ(brush.0));
                }
                return Err(format!(
                    "SetClassLongPtrW(GCLP_HBRBACKGROUND) failed: {}",
                    error.0
                ));
            }
            let old_owned = match brush {
                Some(brush) => installed.replace(brush.0 as isize),
                None => installed.take(),
            };
            if let Some(old_owned) = old_owned {
                let _ = DeleteObject(HGDIOBJ(old_owned as *mut c_void));
            }
        }
        Ok(())
    }

    /// The BCP 47 tag Windows would write this user's *interface* in, e.g.
    /// `"zh-Hans-CN"`, `"en-US"`, `"ja-JP"`.
    ///
    /// **The UI language and deliberately not the locale.** `GetUserDefaultLocaleName`
    /// answers a different question — which calendar, which decimal point, which
    /// sort order — and on a great many machines the two disagree: a Chinese
    /// developer running an English Windows has `zh-CN` formats and an `en-US`
    /// shell, and a product that read the locale would put its menus in a language
    /// that user deliberately did not install. What "System" means in the Language
    /// row is *the language the rest of your Windows is in*, and this is the call
    /// that answers it.
    ///
    /// Preferred-languages first because that is the ordered list the user
    /// actually edits in Settings, and it comes back as a name rather than as a
    /// LANGID — no table of 16-bit numbers to keep. [`GetUserDefaultUILanguage`]
    /// is the floor under it: it cannot fail, and the only two ids this product
    /// has to tell apart are the Chinese ones, whose primary language is
    /// `LANG_CHINESE = 0x04`. A machine that answered neither would be read as
    /// English, which is what the source language is.
    #[must_use]
    pub fn os_ui_language() -> String {
        let mut count = 0u32;
        let mut length = 0u32;
        if unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut length) }
            .is_ok()
            && length > 0
        {
            let mut buffer = vec![0u16; length as usize];
            if unsafe {
                GetUserPreferredUILanguages(
                    MUI_LANGUAGE_NAME,
                    &mut count,
                    Some(PWSTR(buffer.as_mut_ptr())),
                    &mut length,
                )
            }
            .is_ok()
                && count > 0
            {
                // A `MULTI_SZ`: the first NUL ends the first — the preferred — tag.
                let first: Vec<u16> = buffer.into_iter().take_while(|unit| *unit != 0).collect();
                if !first.is_empty() {
                    return String::from_utf16_lossy(&first);
                }
            }
        }
        // LANGID floor. `0x04` is `LANG_CHINESE`; the sub-language distinguishes
        // Simplified from Traditional, and neither this crate nor its caller has
        // to know which, because both are `zh`.
        let langid = unsafe { GetUserDefaultUILanguage() };
        if langid & 0x3ff == 0x04 {
            "zh".to_owned()
        } else {
            "en".to_owned()
        }
    }

    /// This user's `Documents` folder, as Windows itself resolves it.
    ///
    /// **Never `%USERPROFILE%\Documents`.** That string is wrong on a great many
    /// real machines and wrong in the way that matters here: a redirected
    /// Documents (OneDrive, a roaming profile, a second drive) is exactly where
    /// PowerShell's `$HOME\Documents\WindowsPowerShell\Modules` actually
    /// resolves to, because PowerShell asks the same known folder. Writing a
    /// module to the literal path on a redirected machine puts it somewhere
    /// PowerShell will never look, and the only symptom is a module that
    /// installs successfully and does nothing.
    ///
    /// `KF_FLAG_DEFAULT` and not `KF_FLAG_CREATE`: this answers where the folder
    /// *is*, and creating a user's Documents folder as a side effect of reading
    /// a path is not this function's business. The caller creates the
    /// directories it is about to write into, which it has to do anyway.
    #[must_use]
    pub fn documents_directory() -> Option<PathBuf> {
        let raw =
            unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None) }.ok()?;
        if raw.is_null() {
            return None;
        }
        let text = unsafe { raw.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(raw.0.cast())) };
        let text = text?;
        if text.is_empty() {
            None
        } else {
            Some(PathBuf::from(text))
        }
    }

    /// The `ProductVersion` string out of a file's own Win32 version resource.
    ///
    /// **The one field that can carry a pre-release tag.** `VS_FIXEDFILEINFO`
    /// holds four `u16`s and can only ever say `2.4.6.0`; the string table beside
    /// it is where a build stamps what it actually is, and PSReadLine's own
    /// packaging puts `2.4.6-bt.2` there. That is the whole reason this exists:
    /// `psreadline::installed_build` has to tell one Folio-patched build of the
    /// module from another, and the two agree in every number a fixed-info read
    /// would return.
    ///
    /// Asked of the *file* rather than of the module manifest beside it, because
    /// the manifest is where the number a build did **not** stamp lives: every
    /// `-bt` bundle this product has ever shipped carries the same
    /// `ModuleVersion = '2.4.6'` in its `.psd1`, so a manifest read cannot
    /// distinguish them, and a marker file written next to them would only ever
    /// answer for the copies written after the marker was invented.
    ///
    /// `\VarFileInfo\Translation` first and then the string table it names,
    /// which is the documented order: the block is keyed by language and code
    /// page, and hard-coding `040904B0` is the bug where a file built under any
    /// other locale reads as having no version at all.
    ///
    /// `None` on anything that is not a file, has no version resource, or has
    /// one this walk cannot read. A caller that cannot tell those apart is a
    /// caller that should not act, which is exactly how the PSReadLine row reads
    /// it: no answer means not Folio's.
    #[must_use]
    pub fn file_product_version(path: &Path) -> Option<String> {
        let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let file = PCWSTR(file.as_ptr());
        let mut ignored_handle = 0u32;
        let size = unsafe { GetFileVersionInfoSizeW(file, Some(&mut ignored_handle)) };
        if size == 0 {
            return None;
        }
        let mut block = vec![0u8; size as usize];
        unsafe { GetFileVersionInfoW(file, None, size, block.as_mut_ptr().cast()) }.ok()?;

        // The block owns every string this reads; `VerQueryValue` hands back
        // pointers *into* it and allocates nothing, so `block` has to outlive
        // both reads and neither pointer may leave this function.
        let query = |sub_block: &str| -> Option<(*mut core::ffi::c_void, u32)> {
            let wide: Vec<u16> = sub_block.encode_utf16().chain(Some(0)).collect();
            let mut value: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut units = 0u32;
            let found = unsafe {
                VerQueryValueW(
                    block.as_ptr().cast(),
                    PCWSTR(wide.as_ptr()),
                    &mut value,
                    &mut units,
                )
            };
            (found.as_bool() && !value.is_null() && units > 0).then_some((value, units))
        };

        let (translation, bytes) = query(r"\VarFileInfo\Translation")?;
        if bytes < 4 {
            return None;
        }
        let language = unsafe { translation.cast::<u16>().read_unaligned() };
        let code_page = unsafe { translation.cast::<u16>().add(1).read_unaligned() };
        let (text, units) = query(&format!(
            r"\StringFileInfo\{language:04x}{code_page:04x}\ProductVersion"
        ))?;
        // `units` counts UTF-16 units and includes the terminator on every
        // Windows this runs on; trimming NULs rather than subtracting one is
        // what makes that an observation instead of a dependency.
        let value =
            unsafe { std::slice::from_raw_parts(text.cast::<u16>(), units as usize) }.to_vec();
        let value = String::from_utf16_lossy(&value);
        let value = value.trim_end_matches(char::from(0)).trim().to_owned();
        (!value.is_empty()).then_some(value)
    }

    /// Every monospaced family installed on this machine, named and located,
    /// in the order a list draws them.
    ///
    /// # Why DirectWrite and not GDI
    ///
    /// GDI's `EnumFontFamiliesExW` is cheaper and answers half the question: it
    /// hands back family names and a `lfPitchAndFamily` whose low bits say
    /// `FIXED_PITCH`. What it never hands back is *where the family lives*, and
    /// without that the answer cannot be honoured — `bt-render` builds its font
    /// database from a fixed file list on purpose, so a family it has not loaded
    /// is a family its shaper silently falls back out of. DirectWrite answers
    /// name, monospace-ness and file paths in one walk, which is the only shape
    /// that can be fed to a `fontdb`.
    ///
    /// `IDWriteFont1::IsMonospacedFont` is the monospace criterion rather than a
    /// width comparison of two glyphs, because it is the answer the font itself
    /// gives: it reads the face's own `post` table `isFixedPitch` flag and its
    /// OS/2 panose, which is what "this is a programmer's font" means. Measuring
    /// `i` against `M` would additionally admit any proportional face whose two
    /// sampled glyphs happened to tie.
    ///
    /// # What a failure looks like
    ///
    /// A `Vec` and never a `Result`, because there is nothing a caller could do
    /// with the error that this function has not already done: a machine whose
    /// DirectWrite refuses still gets [`DEFAULT_MONOSPACE_FAMILY`] in the list,
    /// which is the face the renderer is already drawing. The picker degrades to
    /// one row rather than to an empty list or a dialog.
    ///
    /// A family whose faces are all in a *custom* loader — a font streamed by an
    /// application rather than installed as a file — is skipped, because
    /// `IDWriteLocalFontFileLoader` is the only loader that can name a path and
    /// a family with no path is a row that cannot be honoured if it is chosen.
    #[must_use]
    pub fn monospace_font_families() -> Vec<super::MonospaceFamily> {
        super::order_monospace_families(collect_monospace_families().unwrap_or_default())
    }

    fn collect_monospace_families() -> windows::core::Result<Vec<super::MonospaceFamily>> {
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }?;
        let mut collection: Option<IDWriteFontCollection> = None;
        // `false` — do not ask DirectWrite to re-scan the font directory. The
        // list is being drawn for a human who is about to pick from it, not
        // audited; a rescan is a disk walk this call has no reason to pay for.
        unsafe { factory.GetSystemFontCollection(&mut collection, false) }?;
        let Some(collection) = collection else {
            return Ok(Vec::new());
        };

        let locale = super::os_ui_language();
        let mut families = Vec::new();
        for index in 0..unsafe { collection.GetFontFamilyCount() } {
            let Ok(family) = (unsafe { collection.GetFontFamily(index) }) else {
                continue;
            };
            let mut files: Vec<std::path::PathBuf> = Vec::new();
            let mut monospaced = false;
            for face_index in 0..unsafe { family.GetFontCount() } {
                let Ok(font) = (unsafe { family.GetFont(face_index) }) else {
                    continue;
                };
                // `IDWriteFont1` is the Windows 8 interface. A machine that
                // cannot produce it cannot answer the question, and guessing
                // would put proportional faces in a monospace list.
                let Ok(font1) = font.cast::<IDWriteFont1>() else {
                    continue;
                };
                if !unsafe { font1.IsMonospacedFont() }.as_bool() {
                    continue;
                }
                monospaced = true;
                let Ok(face) = (unsafe { font.CreateFontFace() }) else {
                    continue;
                };
                for path in font_face_files(&face) {
                    if !files.contains(&path) {
                        files.push(path);
                    }
                }
            }
            if !monospaced || files.is_empty() {
                continue;
            }
            let Ok(names) = (unsafe { family.GetFamilyNames() }) else {
                continue;
            };
            if let Some(name) = localized_string(&names, &locale) {
                families.push(super::MonospaceFamily { name, files });
            }
        }
        Ok(families)
    }

    /// The files one face's outlines live in, skipping any the local loader
    /// cannot name.
    fn font_face_files(face: &IDWriteFontFace) -> Vec<std::path::PathBuf> {
        let mut count = 0u32;
        if unsafe { face.GetFiles(&mut count, None) }.is_err() || count == 0 {
            return Vec::new();
        }
        let mut slots: Vec<Option<IDWriteFontFile>> = vec![None; count as usize];
        if unsafe { face.GetFiles(&mut count, Some(slots.as_mut_ptr())) }.is_err() {
            return Vec::new();
        }
        slots
            .into_iter()
            .flatten()
            .filter_map(|file| font_file_path(&file))
            .collect()
    }

    fn font_file_path(file: &IDWriteFontFile) -> Option<std::path::PathBuf> {
        let mut key: *mut c_void = std::ptr::null_mut();
        let mut key_size = 0u32;
        unsafe { file.GetReferenceKey(&mut key, &mut key_size) }.ok()?;
        let loader = unsafe { file.GetLoader() }.ok()?;
        let local = loader.cast::<IDWriteLocalFontFileLoader>().ok()?;
        let length = unsafe { local.GetFilePathLengthFromKey(key, key_size) }.ok()?;
        // `GetFilePathFromKey` writes the terminating NUL, so the buffer is one
        // longer than the reported length and the NUL is trimmed back off.
        let mut buffer = vec![0u16; length as usize + 1];
        unsafe { local.GetFilePathFromKey(key, key_size, &mut buffer) }.ok()?;
        let text: Vec<u16> = buffer.into_iter().take_while(|unit| *unit != 0).collect();
        if text.is_empty() {
            return None;
        }
        Some(std::path::PathBuf::from(String::from_utf16_lossy(&text)))
    }

    /// The family's name in the machine's own UI language, falling back to the
    /// first name it has.
    ///
    /// The fallback is index 0 and not `"en-us"`, because a family may
    /// legitimately have exactly one localized name in a language neither this
    /// user nor English speaks — a Chinese-only face on a Chinese Windows — and
    /// dropping it would hide an installed font from its owner.
    fn localized_string(names: &IDWriteLocalizedStrings, locale: &str) -> Option<String> {
        let mut index = 0u32;
        let mut exists = windows::core::BOOL(0);
        let wide: Vec<u16> = locale.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = unsafe { names.FindLocaleName(PCWSTR(wide.as_ptr()), &mut index, &mut exists) };
        if !exists.as_bool() {
            index = 0;
        }
        let length = unsafe { names.GetStringLength(index) }.ok()?;
        let mut buffer = vec![0u16; length as usize + 1];
        unsafe { names.GetString(index, &mut buffer) }.ok()?;
        let text: Vec<u16> = buffer.into_iter().take_while(|unit| *unit != 0).collect();
        if text.is_empty() {
            None
        } else {
            Some(String::from_utf16_lossy(&text))
        }
    }

    // ── the registry side of Explorer's context menu ───────────────────────
    //
    // Three functions, and between them they open no key this product did not
    // name: every path below is `HKEY_CURRENT_USER\<classes>\<tree>\<verb>`,
    // built from the constants beside [`super::ContextMenuShape`] and from the
    // `classes` the caller hands in.
    //
    // **`classes` is a parameter and not the constant.** The product always
    // passes [`super::CONTEXT_MENU_CLASSES`]; the suite passes a subkey of its
    // own, because a test that wrote the real one would be a test that changes
    // the right-click menu of whoever is running it — and would then have to be
    // trusted to put it back. The parameter is the isolation.

    /// The verb's own key under one tree, as a path below `HKEY_CURRENT_USER`.
    fn context_menu_key(classes: &str, tree: &str) -> String {
        format!(
            "{classes}\\{tree}\\{verb}",
            verb = super::CONTEXT_MENU_VERB_KEY
        )
    }

    /// Write `value` into `key`'s named value, creating every key on the way.
    ///
    /// `name` empty is the key's **default** value, which is where the shell
    /// keeps a verb's label and a `command`'s command line.
    fn write_registry_string(key: &str, name: &str, value: &str) -> Result<(), String> {
        let key_units = wide_null(key);
        let mut opened = HKEY::default();
        // SAFETY: both buffers stay live and NUL-terminated across the call, and
        // the out-parameter is a handle this function closes on every path.
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_units.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut opened,
                None,
            )
        };
        if status.is_err() {
            return Err(format!(
                "RegCreateKeyExW({key}) failed with {code}",
                code = status.0
            ));
        }
        let name_units = wide_null(name);
        let value_units = wide_null(value);
        // A `REG_SZ` is measured in BYTES and the terminator is part of it: a
        // length that forgot the NUL leaves the shell reading a string whose end
        // is whatever follows it in the hive.
        let bytes = std::mem::size_of_val(value_units.as_slice());
        // SAFETY: `value_units` outlives the call and `bytes` is its own size in
        // bytes, so the slice handed over is exactly the buffer that exists.
        let status = unsafe {
            RegSetValueExW(
                opened,
                PCWSTR(name_units.as_ptr()),
                None,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    value_units.as_ptr().cast::<u8>(),
                    bytes,
                )),
            )
        };
        // SAFETY: `opened` came from the `RegCreateKeyExW` above and is not used
        // again after this.
        unsafe {
            let _ = RegCloseKey(opened);
        }
        if status.is_err() {
            return Err(format!(
                "RegSetValueExW({key}\\{name}) failed with {code}",
                code = status.0
            ));
        }
        Ok(())
    }

    /// One `REG_SZ` value, or `None` if the key or the value is not there.
    ///
    /// A value of any other type also reads as `None`: this function's question
    /// is "did this build write that", and a `REG_DWORD` under the name of a
    /// verb's label is not a label somebody else's Folio left behind.
    fn read_registry_string(key: &str, name: &str) -> Option<String> {
        let key_units = wide_null(key);
        let mut opened = HKEY::default();
        // SAFETY: the buffer is live and NUL-terminated across the call; the
        // handle is closed on every path below.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_units.as_ptr()),
                None,
                KEY_READ,
                &mut opened,
            )
        };
        if status.is_err() {
            return None;
        }
        let name_units = wide_null(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut bytes = 0u32;
        // First call sizes the value; `lpdata` null with a live `lpcbdata` is
        // the documented way to ask.
        // SAFETY: both out-parameters are owned locals and the name buffer is
        // live across the call.
        let status = unsafe {
            RegQueryValueExW(
                opened,
                PCWSTR(name_units.as_ptr()),
                None,
                Some(&mut kind),
                None,
                Some(&mut bytes),
            )
        };
        let text = if status.is_ok() && kind == REG_SZ && bytes > 0 {
            // Rounded up: a hand-written `REG_SZ` is allowed to hold an odd
            // number of bytes, and a `u16` buffer short by one would be a
            // truncating read.
            let mut buffer = vec![0u16; bytes.div_ceil(2) as usize];
            let mut size = std::mem::size_of_val(buffer.as_slice()) as u32;
            // SAFETY: `buffer` holds `size` bytes and stays live across the
            // call; `size` is updated to what was actually written.
            let status = unsafe {
                RegQueryValueExW(
                    opened,
                    PCWSTR(name_units.as_ptr()),
                    None,
                    Some(&mut kind),
                    Some(buffer.as_mut_ptr().cast::<u8>()),
                    Some(&mut size),
                )
            };
            if status.is_ok() && kind == REG_SZ {
                let units = (size as usize / 2).min(buffer.len());
                Some(String::from_utf16_lossy(
                    &buffer[..units]
                        .iter()
                        .copied()
                        .take_while(|unit| *unit != 0)
                        .collect::<Vec<u16>>(),
                ))
            } else {
                None
            }
        } else {
            None
        };
        // SAFETY: `opened` came from the `RegOpenKeyExW` above and is not used
        // again after this.
        unsafe {
            let _ = RegCloseKey(opened);
        }
        text
    }

    /// Whether a key exists at all, whatever it holds.
    ///
    /// The question the removal's own claim asks — "is it gone", and its harder
    /// twin "is *that* one still there" — and the one `read_registry_string`
    /// cannot answer, since a key that is there with no values and a key that is
    /// not there both read as `None`.
    ///
    /// Behind `cfg(test)` because nothing the product does needs it: the removal
    /// deletes unconditionally and the read-back reads values. `DirNews::is_armed`'s
    /// own ruling — a question only the suite asks is a question the shipped
    /// binary should not carry.
    #[cfg(test)]
    pub(super) fn registry_key_exists(key: &str) -> bool {
        let key_units = wide_null(key);
        let mut opened = HKEY::default();
        // SAFETY: the buffer is live and NUL-terminated across the call.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_units.as_ptr()),
                None,
                KEY_READ,
                &mut opened,
            )
        };
        if status.is_err() {
            return false;
        }
        // SAFETY: `opened` came from the `RegOpenKeyExW` immediately above.
        unsafe {
            let _ = RegCloseKey(opened);
        }
        true
    }

    /// `RegDeleteTreeW` on one key of `HKEY_CURRENT_USER`, with a key that is
    /// not there counted as success.
    ///
    /// Shared by [`remove_context_menu`] and by the suite's own teardown, so the
    /// isolated class store a test writes into is taken away by the same call
    /// that takes the verb away.
    pub(super) fn delete_registry_tree(key: &str) -> Result<(), String> {
        let key_units = wide_null(key);
        // SAFETY: the buffer is live and NUL-terminated across the call and names
        // a subkey of `HKEY_CURRENT_USER` built from this build's own constants.
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key_units.as_ptr())) };
        if status.is_err() && status != ERROR_FILE_NOT_FOUND {
            return Err(format!(
                "RegDeleteTreeW({key}) failed with {code}",
                code = status.0
            ));
        }
        Ok(())
    }

    /// Write the verb into every tree of [`super::CONTEXT_MENU_TREES`].
    ///
    /// **Idempotent, and that is what makes it the repair as well as the
    /// install.** Writing a value that is already there costs one call and
    /// changes nothing, so the launch-time re-assertion of a moved `folio.exe`
    /// is this same function with no special case in it.
    ///
    /// The first tree that refuses stops the run and carries the operating
    /// system's own code out. Half a menu is a state the caller has to be told
    /// about, and the verdict it reads afterwards says so on its own —
    /// [`super::context_menu_verdict`] calls a partial set `Stale`.
    pub fn install_context_menu(
        classes: &str,
        shape: &super::ContextMenuShape,
    ) -> Result<(), String> {
        for tree in super::CONTEXT_MENU_TREES {
            let key = context_menu_key(classes, tree);
            write_registry_string(&key, "", &shape.label)?;
            write_registry_string(&key, "Icon", &shape.icon)?;
            write_registry_string(&format!("{key}\\command"), "", &shape.command)?;
        }
        Ok(())
    }

    /// Take the verb back out of every tree.
    ///
    /// **Exactly the keys this product created and not one more.** The verb key
    /// and its `command` child go; `Directory\shell`, `Directory\Background\shell`
    /// and `Directory` itself are left standing whether or not they are now
    /// empty, and the reason is a measurement rather than a preference: on the
    /// machine this was written on, `HKCU\Software\Classes\Directory\Background\shell`
    /// and `HKCU\Software\Classes\Drive\shell` **already existed and were already
    /// empty** before Folio was ever run. A removal that swept up empty ancestors
    /// would have deleted two keys that were there first, which is a worse
    /// failure than the one it was written to avoid — deleting somebody else's
    /// key is not tidiness, and "was empty when I looked" is not evidence that I
    /// made it.
    ///
    /// What is left behind on a machine that genuinely had none of them is an
    /// empty container key with no values: the shape Windows itself leaves
    /// everywhere, invisible to Explorer, and the shape the next install reuses.
    ///
    /// Missing keys are not failures: this is the function that answers "make
    /// sure it is not there", and a machine where it never was is a machine
    /// where it is not there now.
    pub fn remove_context_menu(classes: &str) -> Result<(), String> {
        for tree in super::CONTEXT_MENU_TREES {
            delete_registry_tree(&context_menu_key(classes, tree))?;
        }
        Ok(())
    }

    /// What each tree actually holds, in [`super::CONTEXT_MENU_TREES`]'s order.
    ///
    /// A tree with no `command` reads as `None` even if a label is somehow
    /// sitting there: the command line is the whole of what the verb *does*, and
    /// a key without one is not a menu entry anybody can press.
    #[must_use]
    pub fn read_context_menu(classes: &str) -> Vec<Option<super::ContextMenuShape>> {
        super::CONTEXT_MENU_TREES
            .iter()
            .map(|tree| {
                let key = context_menu_key(classes, tree);
                let command = read_registry_string(&format!("{key}\\command"), "")?;
                Some(super::ContextMenuShape {
                    label: read_registry_string(&key, "").unwrap_or_default(),
                    icon: read_registry_string(&key, "Icon").unwrap_or_default(),
                    command,
                })
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::{
            CLIPBOARD_OPEN_RETRY_DELAYS, FolderPickerState, ImagePickerState, MathMenuState,
            ShellPickKind, compositor_failure, names_a_program, primary_language_id,
            retry_open_clipboard, validate_local_image_path, validate_openable_path, wide_null,
        };
        use std::path::{Path, PathBuf};

        /// A DirectComposition refusal has to be readable by the person holding
        /// the machine it refused on, and there is no fallback path to soften it
        /// — the window simply does not open. So the message must carry three
        /// things: which call refused, what Windows called it, and the hex code
        /// the documentation is indexed by.
        ///
        /// MUTATIONS:
        /// ① drop the `{step}` and the message no longer says which of the six
        ///    DComp calls failed — every one of them reads "failed: ...";
        /// ② format the code as `{:X}` of the `i32` and `E_OUTOFMEMORY` prints
        ///    as `-2147024882`'s hex, which is not a string anyone can search.
        #[test]
        fn a_composition_failure_names_the_call_and_carries_the_hresult_both_ways() {
            let error = windows::core::Error::from_hresult(super::HRESULT(0x8007_000E_u32 as i32));
            let message =
                compositor_failure("IDCompositionDesktopDevice::CreateTargetForHwnd", &error);
            assert!(
                message.starts_with("IDCompositionDesktopDevice::CreateTargetForHwnd failed: "),
                "the call that refused leads: {message}"
            );
            assert!(
                message.ends_with("(0x8007000E)"),
                "and the searchable code closes it: {message}"
            );
            assert!(
                message.len()
                    > "IDCompositionDesktopDevice::CreateTargetForHwnd failed:  (0x8007000E)".len(),
                "with Windows' own sentence in between: {message}"
            );
        }

        #[test]
        fn clipboard_open_retry_is_bounded_and_can_recover() {
            let mut attempts = 0;
            let mut waits = Vec::new();
            let result = retry_open_clipboard(
                || {
                    attempts += 1;
                    (attempts == 3)
                        .then_some(())
                        .ok_or_else(|| "clipboard busy".to_owned())
                },
                |delay| waits.push(delay),
            );

            assert_eq!(result, Ok(()));
            assert_eq!(attempts, 3);
            assert_eq!(waits, CLIPBOARD_OPEN_RETRY_DELAYS[..2]);
        }

        #[test]
        fn clipboard_open_retry_reports_the_last_failure_after_its_wait_budget() {
            let mut attempts = 0;
            let mut waits = Vec::new();
            let error = retry_open_clipboard(
                || {
                    attempts += 1;
                    Err(format!("busy-{attempts}"))
                },
                |delay| waits.push(delay),
            )
            .unwrap_err();

            assert_eq!(attempts, CLIPBOARD_OPEN_RETRY_DELAYS.len() + 1);
            assert_eq!(waits, CLIPBOARD_OPEN_RETRY_DELAYS);
            assert!(error.contains("busy-5"));
            assert!(error.contains("75 ms"));
        }

        #[test]
        fn chinese_system_caret_gate_uses_primary_language_bits() {
            assert_eq!(primary_language_id(0x0804), 0x0004);
            assert_eq!(primary_language_id(0x0404), 0x0004);
            assert_ne!(primary_language_id(0x0409), 0x0004);
        }

        #[test]
        fn deferred_menu_state_requires_posted_dispatch_before_showing() {
            let state = MathMenuState::new();
            assert_eq!(state.take_result(), None);
            assert!(state.begin_request(()));
            assert!(!state.begin_request(()));
            assert_eq!(state.take_result(), None);
            assert!(state.begin_showing().is_some());
            assert!(state.begin_showing().is_none());
            state.complete(Ok(true));
            assert_eq!(state.take_result(), Some(Ok(true)));
            assert!(state.begin_request(()));
        }

        /// PIN — the gesture's own arguments survive the deferral, and only the
        /// dispatch that actually shows the dialog receives them.
        ///
        /// The red gate this stands in front of: a folder chooser whose start
        /// folder was read at *show* time rather than at *request* time would
        /// open wherever the column happened to be pointing by then — which,
        /// because the whole point of the deferral is that time passes, is not
        /// necessarily where it was pointing when the row was pressed.
        #[test]
        fn a_deferred_request_carries_its_arguments_to_the_dispatch_that_shows_it() {
            let state = FolderPickerState::new();
            let start: Vec<u16> = "C:\\work\0".encode_utf16().collect();
            assert!(state.begin_request(start.clone()));
            assert!(
                !state.begin_request(Vec::new()),
                "a second ask while one is queued is coalesced, not stacked"
            );
            assert_eq!(state.begin_showing(), Some(start));
            assert_eq!(
                state.begin_showing(),
                None,
                "and the arguments are handed out exactly once"
            );
            state.complete(Ok(Some(PathBuf::from(r"C:\chosen"))));
            assert_eq!(
                state.take_result(),
                Some(Ok(Some(PathBuf::from(r"C:\chosen"))))
            );
            assert_eq!(state.take_result(), None);
        }

        /// PIN — a cancelled chooser and a chooser that could not be shown are
        /// two different answers, and neither is "a folder was chosen".
        #[test]
        fn a_cancelled_chooser_is_not_a_failure_and_not_a_choice() {
            let state = FolderPickerState::new();
            assert!(state.begin_request(Vec::new()));
            assert!(state.begin_showing().is_some());
            state.complete(Ok(None));
            assert_eq!(state.take_result(), Some(Ok(None)));
        }

        #[test]
        fn local_file_bridge_rejects_every_path_outside_the_image_slice() {
            assert!(validate_local_image_path(Path::new(r"C:\tmp\image.png")).is_ok());
            assert!(validate_local_image_path(Path::new("C:/tmp/IMAGE.JPEG")).is_ok());
            assert!(validate_local_image_path(Path::new(r"relative\image.png")).is_err());
            assert!(validate_local_image_path(Path::new(r"\\server\share\image.png")).is_err());
            assert!(validate_local_image_path(Path::new(r"C:\tmp\image.svg")).is_ok());
            assert!(validate_local_image_path(Path::new(r"C:\tmp\image.bmp")).is_err());
            assert!(validate_local_image_path(Path::new("C:\\tmp\\bad\0.png")).is_err());
        }

        /// PIN (§7.1.6c-4b) — the picture chooser's filter and the bridge's
        /// admission gate are one list read twice.
        ///
        /// The failure this forbids is a dialog that shows you a `.bmp`, lets
        /// you double-click it, and is then told by the decoder that it does not
        /// take those — with the dialog already closed and nothing left on
        /// screen connecting the refusal to the choice. Every spelling the
        /// filter offers must pass the gate, and the gate must admit nothing the
        /// filter hides.
        #[test]
        fn the_pictures_filter_offers_exactly_what_the_bridge_admits() {
            let spec = crate::image_file_filter_spec();
            for extension in crate::IMAGE_FILE_EXTENSIONS {
                assert!(
                    spec.contains(&format!("*.{extension}")),
                    "the filter must offer {extension}: {spec}"
                );
                assert!(
                    validate_local_image_path(Path::new(&format!(r"C:\tmp\picture.{extension}")))
                        .is_ok(),
                    "the gate must admit {extension}"
                );
                assert!(
                    validate_local_image_path(Path::new(&format!(
                        r"C:\tmp\picture.{}",
                        extension.to_ascii_uppercase()
                    )))
                    .is_ok(),
                    "a chooser hands back whatever case the file system holds"
                );
            }
            assert_eq!(
                spec.matches('*').count(),
                crate::IMAGE_FILE_EXTENSIONS.len(),
                "one entry per extension and no extras: {spec}"
            );
            assert!(
                !spec.contains("bmp") && validate_local_image_path(Path::new(r"C:\a.bmp")).is_err(),
                "a format the decoder is not built with must be absent from both"
            );
        }

        /// PIN — the picture chooser defers exactly the way the folder chooser
        /// does, and the two do not share a queue.
        ///
        /// The second half is the interesting one: one state per gesture is what
        /// stops a wallpaper request posted while a folder dialog is pending
        /// from being coalesced away and answering the wrong row.
        #[test]
        fn the_picture_chooser_defers_on_its_own_state_and_not_the_folders() {
            let folders = FolderPickerState::new();
            let pictures = ImagePickerState::new();
            let start: Vec<u16> = "C:\\Users\\me\\Pictures\0".encode_utf16().collect();

            assert!(pictures.begin_request((ShellPickKind::Image, start.clone())));
            assert!(
                folders.begin_request(Vec::new()),
                "a pending picture request must not swallow a folder request"
            );
            assert!(
                !pictures.begin_request((ShellPickKind::Program, Vec::new())),
                "a second ask for the same gesture is coalesced, not stacked —                  including one for the other row this bridge answers, which is                  what keeps a program from arriving on the wallpaper's row"
            );

            assert_eq!(
                pictures.begin_showing(),
                Some((ShellPickKind::Image, start)),
                "the kind travels with the request, so the dialog that opens is                  the one that was asked for"
            );
            assert_eq!(pictures.begin_showing(), None);
            pictures.complete(Ok(Some(PathBuf::from(r"C:\Users\me\Pictures\ridge.jpg"))));
            assert_eq!(
                pictures.take_result(),
                Some(Ok(Some(PathBuf::from(r"C:\Users\me\Pictures\ridge.jpg"))))
            );
            assert_eq!(pictures.take_result(), None);
            assert_eq!(
                folders.begin_showing(),
                Some(Vec::new()),
                "and the folder request is still sitting where it was left"
            );
        }

        /// PIN — a NUL-terminated wide copy is what the shell reads, and it is
        /// terminated exactly once.
        #[test]
        fn a_wide_string_for_win32_ends_at_its_terminator() {
            let units = wide_null("*.png;*.jpg");
            assert_eq!(units.last(), Some(&0));
            assert_eq!(
                units.iter().filter(|unit| **unit == 0).count(),
                1,
                "a second NUL would truncate the filter at the first one"
            );
            assert_eq!(
                String::from_utf16_lossy(&units[..units.len() - 1]),
                "*.png;*.jpg"
            );
            assert_eq!(wide_null(""), vec![0]);
        }

        /// PIN — the tree's bridge opens documents and refuses programs, and the
        /// refusal does not depend on this machine's `PATHEXT` being anything in
        /// particular.
        #[test]
        fn the_tree_bridge_opens_what_it_can_show_and_never_what_it_would_run() {
            let empty = "";
            for document in [
                r"C:\notes\readme.md",
                r"C:\notes\report.pdf",
                r"C:\notes\archive.zip",
                r"C:\notes\NOEXTENSION",
                r"C:\notes\photo.PNG",
            ] {
                assert!(
                    !names_a_program(Path::new(document), empty),
                    "{document} is something to look at"
                );
            }
            for program in [
                r"C:\bin\tool.exe",
                r"C:\bin\TOOL.EXE",
                r"C:\bin\run.bat",
                r"C:\bin\install.msi",
                r"C:\bin\shortcut.lnk",
                r"C:\bin\saver.scr",
                r"C:\bin\page.hta",
                r"C:\bin\keys.reg",
                r"C:\bin\script.ps1",
            ] {
                assert!(
                    names_a_program(Path::new(program), empty),
                    "{program} would run"
                );
            }
        }

        /// PIN — a machine that has taught its command line to execute a new
        /// extension has taught this bridge to refuse it.
        #[test]
        fn a_machine_that_makes_something_executable_makes_it_refused_here() {
            let path = Path::new(r"C:\bin\macro.xyz");
            assert!(!names_a_program(path, ".EXE;.BAT"));
            assert!(names_a_program(path, ".EXE;.XYZ"));
            assert!(names_a_program(path, ".exe;.xyz"));
            // An emptied `PATHEXT` cannot open the door the fixed list shuts.
            assert!(names_a_program(Path::new(r"C:\bin\tool.exe"), ""));
        }

        /// A column may be rooted on a share, so its rows have to be openable —
        /// which is the one way this gate is wider than the image bridge's.
        #[test]
        fn the_tree_bridge_takes_a_share_and_still_refuses_a_relative_name() {
            assert!(validate_openable_path(Path::new(r"C:\notes\a.txt")).is_ok());
            assert!(validate_openable_path(Path::new(r"\\server\share\a.txt")).is_ok());
            assert!(validate_openable_path(Path::new(r"notes\a.txt")).is_err());
            assert!(validate_openable_path(Path::new("C:\\notes\\bad\0.txt")).is_err());
        }
    }

    /// **A subscription to one directory tree's change notifications.**
    ///
    /// The kernel's own `ReadDirectoryChangesW`, held open by a thread of its
    /// own: the filesystem tells us when something under `path` moved, and
    /// nothing here ever asks. That distinction is the whole reason this type is
    /// allowed to exist under DESIGN §7.1.3g ② (R31) — a repository is not read
    /// because time passed, and a change notification is not time passing.
    ///
    /// **It says only that something changed.** The `FILE_NOTIFY_INFORMATION`
    /// records the kernel writes are read for nothing at all — not the names, not
    /// the actions — because the caller's next move is to ask `git status`, which
    /// is the one thing that can say what a change *means*. Parsing them here
    /// would be a second, worse answer to a question this crate cannot answer:
    /// whether a write to `target\debug\foo.pdb` matters is a question about a
    /// `.gitignore`, and a watcher that tried to decide it would be wrong on
    /// somebody's repository and silent about it. A zero-length completion —
    /// the kernel's way of saying the buffer overflowed and it has stopped
    /// keeping track — is therefore not a special case but the ordinary one:
    /// *something changed*, which is all any of them ever say.
    ///
    /// **Dropping it cancels.** The watcher thread waits on the directory's
    /// completion and on a stop event at once, so `drop` is a `SetEvent` and a
    /// join rather than a flag the thread notices on its next notification —
    /// which for a directory nothing is writing to would be never.
    pub struct DirWatch {
        dir: SendHandle,
        stop: SendHandle,
        change: SendHandle,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    /// A kernel handle on its way to the thread that will use it.
    ///
    /// A raw `HANDLE` is not `Send` because it is a pointer-shaped value and the
    /// compiler cannot know what it points at. These three are process-wide
    /// kernel objects with exactly one user apiece — the watcher thread — and
    /// [`DirWatch::drop`] joins that thread before closing any of them, so there
    /// is no moment at which two threads hold one of these and no moment at which
    /// a closed handle is still reachable.
    #[derive(Clone, Copy)]
    struct SendHandle(HANDLE);

    // SAFETY: see the type's own note. The handle is created on the calling
    // thread, used only by the watcher thread, and closed by the calling thread
    // after that thread has been joined.
    unsafe impl Send for SendHandle {}

    /// 64 KiB, the largest buffer the kernel will fill for a *network* directory
    /// and a comfortable one for a local tree.
    ///
    /// A bigger buffer buys fewer overflows and nothing else, and an overflow is
    /// not a failure here: it is the same word — *something changed* — arriving
    /// with less detail than usual, and this watcher reads no detail. So the size
    /// is chosen to be unremarkable rather than tuned.
    const DIR_WATCH_BUFFER_BYTES: usize = 64 * 1024;

    /// Everything that can move under a working tree and mean something to git:
    /// files and folders appearing, disappearing or being renamed, contents
    /// written, sizes changing, and attributes (which is how a read-only flag or
    /// a hidden bit arrives).
    ///
    /// Deliberately **not** `LAST_ACCESS`: reading a file changes nothing git can
    /// see, and a grep across the tree would otherwise be a storm of notifications
    /// about nothing.
    const DIR_WATCH_FILTER: FILE_NOTIFY_CHANGE = FILE_NOTIFY_CHANGE(
        FILE_NOTIFY_CHANGE_FILE_NAME.0
            | FILE_NOTIFY_CHANGE_DIR_NAME.0
            | FILE_NOTIFY_CHANGE_LAST_WRITE.0
            | FILE_NOTIFY_CHANGE_SIZE.0
            | FILE_NOTIFY_CHANGE_ATTRIBUTES.0,
    );

    /// **How far under a watched directory the kernel is asked to look.**
    ///
    /// Named rather than a bare `bool` at the call sites for
    /// [`VisualLayer`]'s reason one file over: `true` at a call site says
    /// nothing about what it is true *of*, and the two callers of this
    /// subscription want opposite answers for reasons that are worth reading.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum WatchDepth {
        /// The directory and everything under it - a working tree.
        Tree,
        /// The directory's own entries and nothing deeper - the folder one
        /// watched file happens to live in.
        HereOnly,
    }

    impl WatchDepth {
        /// The `bWatchSubtree` argument this depth is.
        #[must_use]
        pub fn recursive(self) -> bool {
            matches!(self, Self::Tree)
        }
    }

    /// **Test-only: a place to hold the watcher thread just short of a read.**
    ///
    /// The defect this exists to pin is a *window*, not a value. `DirWatch::start`
    /// used to return the moment the watcher thread had been **spawned**, and the
    /// first `ReadDirectoryChangesW` was issued by that thread whenever it got
    /// round to it. Anything that moved in the directory in between reached
    /// nobody: Windows reports what happens while a read is outstanding and does
    /// not keep a log for a subscriber who was not yet listening. The window is
    /// microseconds wide and a busy machine is what widens it, so a test that
    /// tried to open it by loading the box would be rolling dice — and the dice
    /// were rolled: five-second budgets went 1/3 green under load, and raising
    /// the budget six-fold produced 8/8 failures at the new ceiling, which is
    /// what "the notification is never coming" looks like and not what a starved
    /// machine looks like.
    ///
    /// So the watcher thread is given a place to be held instead, and the test
    /// holds it there while it makes its change. **The gate can only ever
    /// delay**: it is waited on before a read is issued, and `start` — once it
    /// keeps its promise — is on the far side of that read. That is the whole
    /// proof. Holding the gate holds `start` itself, so the change a test makes
    /// after `start` returns cannot land in a window that no longer exists,
    /// while the same test against the old code drops the change every time
    /// rather than one time in three.
    #[cfg(test)]
    pub(crate) mod first_read_gate {
        use std::{
            sync::{Condvar, Mutex, MutexGuard, PoisonError},
            time::Duration,
        };

        /// How long the watcher thread waits to be let through before going on
        /// regardless.
        ///
        /// **It breaks a deadlock and decides nothing.** `start` waits for the
        /// read this gate holds up, so a test that held the gate and then waited
        /// for `start` would be waiting for itself; the cap is what lets the
        /// green arm finish. It bounds how long that arm *takes* and has no say
        /// in what it *concludes* — the read is outstanding before `start`
        /// returns whether the gate opened by release or by cap.
        ///
        /// What it must not be is shorter than the single `fs::write` a test
        /// makes while the gate is held, or the window this exists to hold open
        /// would close early. That only ever costs the red arm its second piece
        /// of evidence: the first — that `start` returned with no read
        /// outstanding at all — is a flag, not a clock.
        const HELD_AT_MOST: Duration = Duration::from_millis(250);

        struct Gate {
            /// Set by a test that wants the next read held back.
            held: bool,
            /// Set by the watcher thread the moment a read is outstanding.
            read_issued: bool,
        }

        static GATE: Mutex<Gate> = Mutex::new(Gate {
            held: false,
            read_issued: false,
        });
        static RELEASED: Condvar = Condvar::new();

        /// The suite's own turn-taking.
        ///
        /// The gate is one flag for the whole process, so a second watcher
        /// thread running at the same time would trip it — and a `read_issued`
        /// set by somebody else's watcher is exactly the false green this test
        /// exists to make impossible. Every test in this crate that starts a
        /// watcher takes this turn first.
        static ONE_WATCHER_AT_A_TIME: Mutex<()> = Mutex::new(());

        /// The turn itself, for a test that only needs to not collide.
        pub(crate) fn watchers_take_turns() -> MutexGuard<'static, ()> {
            ONE_WATCHER_AT_A_TIME
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
        }

        /// A held gate, and the turn that comes with it.
        pub(crate) struct Held(#[allow(dead_code)] MutexGuard<'static, ()>);

        /// Hold the watcher thread's next read back until the returned guard is
        /// released or dropped.
        pub(crate) fn hold() -> Held {
            let turn = watchers_take_turns();
            *GATE.lock().unwrap_or_else(PoisonError::into_inner) = Gate {
                held: true,
                read_issued: false,
            };
            Held(turn)
        }

        impl Held {
            /// Whether a read is outstanding *right now* — which is the whole of
            /// the promise the word `start` makes.
            pub(crate) fn a_read_is_outstanding(&self) -> bool {
                GATE.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .read_issued
            }

            /// Let the watcher thread through.
            pub(crate) fn release(&self) {
                GATE.lock().unwrap_or_else(PoisonError::into_inner).held = false;
                RELEASED.notify_all();
            }
        }

        impl Drop for Held {
            fn drop(&mut self) {
                let mut gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
                *gate = Gate {
                    held: false,
                    read_issued: false,
                };
                RELEASED.notify_all();
            }
        }

        /// Called by the watcher thread immediately before it issues a read.
        pub(crate) fn wait_if_held() {
            let gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = RELEASED.wait_timeout_while(gate, HELD_AT_MOST, |gate| gate.held);
        }

        /// Called by the watcher thread the moment a read is outstanding.
        pub(crate) fn note_read_issued() {
            GATE.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .read_issued = true;
        }
    }

    impl DirWatch {
        /// Start watching `path` and everything under it.
        ///
        /// `wake` is called on the watcher thread, once per notification, and is
        /// expected to do nothing but record the news and nudge whatever loop is
        /// going to act on it. Anything slower belongs on the other side of that
        /// nudge: this thread is the only thing standing between the kernel's
        /// buffer and an overflow.
        ///
        /// **It returns already listening.** Not "a thread has been started that
        /// will begin listening": when this function hands back a `DirWatch`, a
        /// `ReadDirectoryChangesW` is outstanding on the directory, and every
        /// change from that instant onwards is news the caller will be told
        /// about. The contract has to be that strong because Windows keeps no
        /// log for a subscriber who was not yet listening — a change that
        /// happens while no read is outstanding is not late, it is gone — and
        /// because every caller's next move is to touch or to list the very
        /// directory it has just armed. The first read is therefore issued
        /// before this returns: the watcher thread issues it and says so, and
        /// this waits for that word.
        ///
        /// **Failure is quiet and final.** A path on a network share, a `\\wsl$`
        /// mount, a directory the process may not open — all of them come back as
        /// an error here and the caller's answer is to have no watcher for that
        /// repository, not to try again in a moment. Retrying is a timer, and a
        /// timer is the thing this whole mechanism exists to avoid. A first read
        /// that is refused is one of these: it comes back from here as an `Err`
        /// with a thread already joined behind it, not as a thread that quietly
        /// stops after the caller has been told it has a watcher.
        pub fn start(
            path: &Path,
            wake: impl Fn() + Send + 'static,
        ) -> Result<Self, std::io::Error> {
            Self::start_scoped(path, WatchDepth::Tree, wake)
        }

        /// The same subscription over **this directory and no deeper**.
        ///
        /// `ReadDirectoryChangesW` takes the depth as an argument, so this is
        /// one boolean and not a second mechanism - which is the whole reason it
        /// lives here rather than in a watcher of its own. The caller that wants
        /// it is `preview_watch`: a preview seat is looking at *one file*, and
        /// the directory holding it is only ever asked because Windows has no
        /// way to subscribe to a single file. Watching the tree instead would
        /// mean a `target\debug` under the folder somebody is previewing a
        /// README out of woke this thread for every object file a build wrote.
        pub fn start_shallow(
            path: &Path,
            wake: impl Fn() + Send + 'static,
        ) -> Result<Self, std::io::Error> {
            Self::start_scoped(path, WatchDepth::HereOnly, wake)
        }

        fn start_scoped(
            path: &Path,
            depth: WatchDepth,
            wake: impl Fn() + Send + 'static,
        ) -> Result<Self, std::io::Error> {
            let mut units = path.as_os_str().encode_wide().collect::<Vec<u16>>();
            if units.contains(&0) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "watched path contains an embedded NUL",
                ));
            }
            units.push(0);
            // `FILE_LIST_DIRECTORY` is the access right `ReadDirectoryChangesW`
            // needs, `BACKUP_SEMANTICS` is what lets `CreateFileW` open a
            // directory at all, and `OVERLAPPED` is what lets the read be
            // cancelled — without it the thread would block in the kernel with
            // no way out but a change that may never come.
            //
            // All three shares are granted because this handle must not be the
            // reason somebody else cannot rename or delete a file in their own
            // working tree.
            let dir = unsafe {
                CreateFileW(
                    PCWSTR(units.as_ptr()),
                    FILE_LIST_DIRECTORY.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                    None,
                )
            }
            .map_err(win32_io_error)?;
            // Manual-reset, both of them. The stop event has to stay signalled
            // once it is set — the thread may be anywhere between two waits when
            // `drop` fires — and the completion event is reset by hand before
            // each read so that a stale signal cannot be mistaken for an answer
            // to the read that has not been issued yet.
            let change = match unsafe { CreateEventW(None, true, false, PCWSTR::null()) } {
                Ok(handle) => handle,
                Err(error) => {
                    unsafe { close(dir) };
                    return Err(win32_io_error(error));
                }
            };
            let stop = match unsafe { CreateEventW(None, true, false, PCWSTR::null()) } {
                Ok(handle) => handle,
                Err(error) => {
                    unsafe { close(dir) };
                    unsafe { close(change) };
                    return Err(win32_io_error(error));
                }
            };
            let (dir, change, stop) = (SendHandle(dir), SendHandle(change), SendHandle(stop));
            // The word that makes `start` mean what it says. The thread sends it
            // once, the instant its first read is outstanding — or sends the
            // refusal instead, which is this function's error and not a thread
            // that dies in private after the caller has been told it has a
            // watcher.
            let (armed, listening) = std::sync::mpsc::channel::<Result<(), std::io::Error>>();
            let thread = std::thread::Builder::new()
                .name("bt-dir-watch".to_owned())
                .spawn(move || watch_loop(dir, change, stop, depth, armed, wake));
            let thread = match thread {
                Ok(thread) => thread,
                Err(error) => {
                    unsafe { close(dir.0) };
                    unsafe { close(change.0) };
                    unsafe { close(stop.0) };
                    return Err(error);
                }
            };
            // A `RecvError` is the thread ending without a word, which is a
            // panic between the spawn and the read — there is no return path
            // there that does not send. It is still an answer and not an
            // unreachable: this is a terminal, and a panic taken as a panic
            // takes somebody's scrollback with it.
            let refused = match listening.recv() {
                Ok(Ok(())) => {
                    return Ok(Self {
                        dir,
                        stop,
                        change,
                        thread: Some(thread),
                    });
                }
                Ok(Err(error)) => error,
                Err(_) => std::io::Error::other("the directory watcher ended before it listened"),
            };
            // Join first, close after — the same order `drop` keeps, and for the
            // same reason: a handle closed while that thread still held it is a
            // handle reused for something else by the time it gets to it.
            let _ = thread.join();
            unsafe {
                close(dir.0);
                close(change.0);
                close(stop.0);
            }
            Err(refused)
        }
    }

    impl Drop for DirWatch {
        fn drop(&mut self) {
            // The thread cancels its own read: the stop event is one of the two
            // things it is waiting on, so setting it is enough, and a
            // `CancelIoEx` from here would be a second thread reaching into an
            // operation whose `OVERLAPPED` lives on that thread's stack.
            unsafe {
                let _ = SetEvent(self.stop.0);
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            // Only now: a handle closed while the thread still held it would be a
            // handle reused for something else by the time it got round to using
            // it, which is the one Win32 bug that reads as a different subsystem
            // misbehaving.
            unsafe {
                close(self.dir.0);
                close(self.change.0);
                close(self.stop.0);
            }
        }
    }

    /// The watcher thread: issue a read, wait for it or for the stop, repeat.
    ///
    /// `armed` carries the outcome of the *first* read back to [`DirWatch::start`],
    /// which is blocked on it — see the promise that function makes. Every pass
    /// after that one is this thread's own business.
    fn watch_loop(
        dir: SendHandle,
        change: SendHandle,
        stop: SendHandle,
        depth: WatchDepth,
        armed: std::sync::mpsc::Sender<Result<(), std::io::Error>>,
        wake: impl Fn(),
    ) {
        // `u32` and not `u8`: the kernel writes `FILE_NOTIFY_INFORMATION` records
        // into this and requires DWORD alignment, which a `Vec<u8>` does not
        // promise. Nothing reads the records — see [`DirWatch`] — but the
        // alignment is a precondition of the call, not of the parsing.
        let mut buffer = vec![0u32; DIR_WATCH_BUFFER_BYTES / std::mem::size_of::<u32>()];
        // Declared once, outside the loop, so that its address is fixed for as
        // long as the kernel may be writing to it — and written afresh at the top
        // of every pass, because the kernel leaves its own status in the fields a
        // second read would otherwise inherit. The read is always awaited or
        // cancelled before the next iteration reuses it.
        let mut overlapped;
        // Taken by the first pass and never seen again: `start` is waiting for
        // exactly one word, and after it has had it this thread is on its own.
        let mut armed = Some(armed);
        loop {
            overlapped = OVERLAPPED {
                hEvent: change.0,
                ..OVERLAPPED::default()
            };
            let reset = unsafe { ResetEvent(change.0) };
            #[cfg(test)]
            first_read_gate::wait_if_held();
            let issued = match reset {
                Ok(()) => unsafe {
                    ReadDirectoryChangesW(
                        dir.0,
                        buffer.as_mut_ptr().cast::<std::ffi::c_void>(),
                        u32::try_from(DIR_WATCH_BUFFER_BYTES).unwrap_or(u32::MAX),
                        // A repository is a tree and a commit touches any
                        // depth of it; a preview seat is one file and its
                        // directory is only being asked because there is no way
                        // to subscribe to a file. The caller said which.
                        depth.recursive(),
                        DIR_WATCH_FILTER,
                        None,
                        Some(std::ptr::from_mut(&mut overlapped)),
                        None,
                    )
                },
                // The event could not be cleared, so a read issued now would be
                // answered by a stale signal. It is the same failure as a read
                // that was refused, and on the first pass it is `start`'s error.
                Err(error) => Err(error),
            };
            #[cfg(test)]
            if issued.is_ok() {
                first_read_gate::note_read_issued();
            }
            if let Some(armed) = armed.take() {
                let _ = armed.send(issued.clone().map_err(win32_io_error));
            }
            if issued.is_err() {
                // The directory went away, or the handle did. There is nothing
                // left to watch and nothing to report: the caller keeps whatever
                // it last knew, and the page's own refresh is still there.
                return;
            }
            let signalled = unsafe { WaitForMultipleObjects(&[stop.0, change.0], false, INFINITE) };
            if signalled != WAIT_EVENT(WAIT_OBJECT_0.0 + 1) {
                // Stopped, or the wait itself failed. Either way this thread is
                // finished — but the read it issued is still outstanding, and the
                // `OVERLAPPED` it is writing into is about to go out of scope, so
                // it is cancelled and *waited for* before that happens.
                unsafe {
                    let _ = CancelIoEx(dir.0, Some(std::ptr::from_ref(&overlapped)));
                    let mut ignored = 0u32;
                    let _ = GetOverlappedResult(dir.0, &overlapped, &mut ignored, true);
                }
                return;
            }
            let mut written = 0u32;
            let completed = unsafe { GetOverlappedResult(dir.0, &overlapped, &mut written, false) };
            if completed.is_err() {
                return;
            }
            // `written == 0` is the kernel saying the buffer overflowed and it
            // has stopped keeping track of what changed. It is reported exactly
            // like every other notification, because it carries exactly the same
            // information this watcher uses: something changed.
            wake();
        }
    }

    /// Close a handle, ignoring the failure that can only mean it was not one.
    unsafe fn close(handle: HANDLE) {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }

    /// A `windows::core::Error` as the `std::io::Error` the rest of this
    /// workspace matches on.
    ///
    /// **The unwrapping is the whole of it.** `windows::core::Error` carries an
    /// `HRESULT`, and a Win32 error that reaches it has been through
    /// `HRESULT_FROM_WIN32` on the way: `ERROR_FILE_NOT_FOUND` (2) arrives as
    /// `0x8007_0002`. `std::io::Error::from_raw_os_error` on Windows classifies
    /// by matching *Win32* codes — it knows `2` and knows nothing at all about
    /// `0x8007_0002` — so handing it the raw `HRESULT` produced an error that
    /// printed the right sentence and answered
    /// [`std::io::ErrorKind::Uncategorized`] to every question about it.
    ///
    /// The symptom that found it: `DirWatch::start` on a folder that is not
    /// there could never return `NotFound`, so `scheme_watch`'s quiet arm — the
    /// one written for exactly the fresh install where `%APPDATA%\Folio\schemes`
    /// has never existed — was unreachable, and every such launch printed a line
    /// about a folder whose absence is the ordinary case.
    ///
    /// Codes outside `FACILITY_WIN32` are passed through whole. They are not
    /// Win32 errors wearing an `HRESULT`; they are `HRESULT`s, and 16 bits of
    /// one of those is not an error code at all.
    fn win32_io_error(error: windows::core::Error) -> std::io::Error {
        std::io::Error::from_raw_os_error(win32_code(error.code()))
    }

    /// `HRESULT_FROM_WIN32`, undone — see [`win32_io_error`].
    ///
    /// Split out so the mapping can be claimed by a test that does not have to
    /// make the operating system fail in a particular way first.
    pub(super) fn win32_code(code: HRESULT) -> i32 {
        // `HRESULT_FROM_WIN32(x)` is `0x8007_0000 | (x & 0xFFFF)` for every
        // `x` that fits, which is every Win32 error code there is.
        const FACILITY_WIN32_FAILURE: u32 = 0x8007_0000;
        let raw = code.0 as u32;
        if raw & 0xFFFF_0000 == FACILITY_WIN32_FAILURE {
            (raw & 0xFFFF) as i32
        } else {
            code.0
        }
    }

    /// Put the calling thread in one of the three bands. See [`ThreadPriority`].
    ///
    /// The answer is whether the kernel took it. There is exactly one way this
    /// fails in practice — a job object or a policy that caps the process's
    /// priority — and the honest response to that is to go on running at
    /// whatever the machine allows, because a terminal that refused to start
    /// because it could not be one step more important than a background thread
    /// would be a worse program than a slightly slower one.
    pub fn set_current_thread_priority(priority: ThreadPriority) -> bool {
        // `GetCurrentThread` is a pseudo-handle — a constant meaning "me" — so
        // there is nothing to close and nothing that can outlive the call.
        unsafe { SetThreadPriority(GetCurrentThread(), win32_priority(priority)) }.is_ok()
    }

    /// Which band the calling thread is in, or `None` if it is in none of them.
    ///
    /// Exists so the spawn helper's contract can be *tested* rather than
    /// asserted in a comment: a worker is a thread that reports
    /// [`ThreadPriority::BelowNormal`] from inside itself.
    #[must_use]
    pub fn current_thread_priority() -> Option<ThreadPriority> {
        let raw = unsafe { GetThreadPriority(GetCurrentThread()) };
        match THREAD_PRIORITY(raw) {
            THREAD_PRIORITY_ABOVE_NORMAL => Some(ThreadPriority::AboveNormal),
            THREAD_PRIORITY_NORMAL => Some(ThreadPriority::Normal),
            THREAD_PRIORITY_BELOW_NORMAL => Some(ThreadPriority::BelowNormal),
            _ => None,
        }
    }

    fn win32_priority(priority: ThreadPriority) -> THREAD_PRIORITY {
        match priority {
            ThreadPriority::AboveNormal => THREAD_PRIORITY_ABOVE_NORMAL,
            ThreadPriority::Normal => THREAD_PRIORITY_NORMAL,
            ThreadPriority::BelowNormal => THREAD_PRIORITY_BELOW_NORMAL,
        }
    }

    /// Spawn a named thread that is already in its band before it does anything.
    ///
    /// **The band is set from inside the new thread, not from the spawner**, and
    /// that is the whole reason this helper exists rather than a
    /// `set_priority(&handle)` called after `spawn`: between a `spawn` and a
    /// call on its `JoinHandle` the new thread is already running, and under the
    /// exact saturation this is for, "already running" can mean "has already
    /// decoded the image" — a worker that spends its first and busiest
    /// milliseconds at the frame's priority. Here the first statement the thread
    /// executes is the one that gets out of the frame's way.
    pub fn spawn_at_priority<T: Send + 'static>(
        name: &str,
        priority: ThreadPriority,
        body: impl FnOnce() -> T + Send + 'static,
    ) -> std::io::Result<std::thread::JoinHandle<T>> {
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                set_current_thread_priority(priority);
                body()
            })
    }

    /// Put one block of text where the person who started this process will see
    /// it, and say whether there was anywhere to put it.
    ///
    /// **The one thing a Windows GUI program cannot do is print.** `folio.exe`
    /// answers `--help` and refuses a mistyped flag, and both answers are text —
    /// but a process launched from Explorer, from a shortcut or by a shell that
    /// does not wait for it has no console of its own to write to, and `println!`
    /// on such a process writes to a handle nobody is reading. `bt-app`'s front
    /// door therefore asks *this*, and falls back to [`message_box`] when the
    /// answer is `false`.
    ///
    /// Two steps, in this order:
    ///
    /// 1. **A console this process already has** — which today it always does,
    ///    because `folio.exe` is still a console-subsystem binary. `GetConsoleWindow`
    ///    is the test, and a non-null answer means the second step must be
    ///    skipped: `AttachConsole` fails with `ERROR_ACCESS_DENIED` on a process
    ///    that is already attached, and reading that as "no console" would send
    ///    a usage block to a message box on the one machine configuration where
    ///    the console was right there.
    /// 2. **The parent's console**, through `ATTACH_PARENT_PROCESS`. This is the
    ///    half written for the day the binary becomes a windows-subsystem one —
    ///    which the Explorer verb wants, since a right-click that flashes a
    ///    console window is a right-click that looks broken. It fails, correctly,
    ///    when the parent has no console either: a double-click from Explorer,
    ///    or a launch by the shell's COM activation.
    ///
    /// The text goes to `CONOUT$` rather than to `std::io::stdout`, and that is
    /// not belt-and-braces. `AttachConsole` gives the process a console; it does
    /// not reliably rewrite the three standard handles the C runtime and Rust's
    /// `Stdout` resolve through, and on a process that was started with its
    /// output redirected it must not — the redirection is the caller's. Opening
    /// the console's own pseudo-file names exactly one destination: the screen
    /// the console is drawn on.
    ///
    /// `WriteConsoleW` and not `WriteFile`, so that the UTF-16 goes to the
    /// console as characters. A console's code page is very often 936 or 437 on
    /// the machines this product runs on, and a byte-oriented write would put
    /// mojibake on the screen for the half of this text that is Chinese.
    /// Give a windows-subsystem process its parent's console for its standard
    /// handles, when it has none of its own.
    ///
    /// `#![windows_subsystem = "windows"]` (user report 2026-08-18: launching
    /// `folio.exe` raised a Windows Terminal window first) means the loader no
    /// longer conjures a console — which also means a developer running
    /// `folio.exe` from a shell with `BT_STARTUP_TRACE` set would watch nothing
    /// arrive: the process starts with null standard handles and Rust's `print`
    /// family quietly discards into them. This adopts the parent's console and
    /// points the null handles at its screen, so a trace asked for from a shell
    /// still lands in that shell.
    ///
    /// **A redirection is never touched.** Only a handle that is null is
    /// replaced; `folio.exe 2>trace.txt` keeps its file, because the caller who
    /// redirected owns where that stream goes. And a double-click stays silent:
    /// with no parent console `AttachConsole` fails and this is a no-op.
    pub fn adopt_parent_console() {
        use windows::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
        };
        let null = |slot| {
            unsafe { GetStdHandle(slot) }
                .map(|handle| handle.is_invalid())
                .unwrap_or(true)
        };
        if !null(STD_OUTPUT_HANDLE) && !null(STD_ERROR_HANDLE) {
            return;
        }
        if unsafe { GetConsoleWindow() }.is_invalid()
            && unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_err()
        {
            return;
        }
        let name: Vec<u16> = "CONOUT$ ".encode_utf16().collect();
        let Ok(conout) = (unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }) else {
            return;
        };
        for slot in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            if null(slot) {
                let _ = unsafe { SetStdHandle(slot, HANDLE(conout.0)) };
            }
        }
    }

    pub fn write_to_console(text: &str) -> bool {
        let attached = unsafe { !GetConsoleWindow().is_invalid() }
            || unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_ok();
        if !attached {
            return false;
        }
        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        let Ok(handle) = (unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }) else {
            return false;
        };
        // A console line ends in CRLF. `ENABLE_PROCESSED_OUTPUT` will usually
        // turn a bare LF into one, but "usually" is a mode the caller of this
        // process can have turned off, and the failure it produces is a usage
        // block drawn as a staircase down the right of the screen.
        let units: Vec<u16> = text
            .replace("\r\n", "\n")
            .replace('\n', "\r\n")
            .encode_utf16()
            .collect();
        let mut written = 0u32;
        let wrote = unsafe { WriteConsoleW(handle, &units, Some(&mut written), None) }.is_ok();
        let _ = unsafe { CloseHandle(handle) };
        wrote
    }

    /// Say one thing in a box, with no window behind it.
    ///
    /// **The single message box this product is allowed to raise**, and the
    /// reason it is allowed is the reason every other one is not: there is no
    /// window yet. Everything Folio says once it has a window it says on a card
    /// anchored at the surface the news is about (`Runtime::toast`); a modal is
    /// how a program interrupts you, and a program that has drawn nothing has
    /// nothing to interrupt.
    ///
    /// `HWND::default()` is the ownerless box: the process has no window to be
    /// modal to, which is the whole situation. `MB_SETFOREGROUND` because the
    /// caller is a shell or Explorer that has just taken the focus back — a box
    /// nobody sees is the same as no box, and this one carries the only
    /// explanation of a launch that is about to exit.
    pub fn message_box(title: &str, text: &str) {
        let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(
                Some(HWND::default()),
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND,
            );
        }
    }
}

/// The three scheduling bands this application's threads run in.
///
/// **A terminal under a saturated machine is a scheduling problem, not a
/// throughput problem.** Every core busy with `cargo` means the window's own
/// loop is one runnable thread among a hundred at the same priority, and the
/// scheduler's answer is a fair share — which for a thread that must answer a
/// wheel notch inside a frame is not the right answer at all. The window is not
/// asking for more of the machine; it is asking to be the first of this
/// process's threads to get whatever the machine hands it.
///
/// So the process is *ordered*, in three bands and no more:
///
/// * [`Self::AboveNormal`] — the one thread that owns the window: the event
///   loop, which is also the render thread. One step, not two: `+1` is enough to
///   put the loop ahead of every ordinary background thread on the machine, and
///   it leaves the priority classes above it (`HIGHEST`, `TIME_CRITICAL`) to the
///   things that genuinely cannot be late.
/// * [`Self::Normal`] — the PTY readers. A reader that falls behind is a child
///   process blocked on a full pipe, which is back-pressure reaching the wrong
///   place; it is not the frame's competitor either, because it does nothing but
///   move bytes into a ring.
/// * [`Self::BelowNormal`] — every worker. A `git status`, a directory read, a
///   PNG decode and a formula raster are all answers to questions nobody is
///   holding their breath for, and none of them may ever be the reason a frame
///   was late. This is the band that matters most under starvation: it is what
///   stops the process from competing with itself.
///
/// **Not MMCSS.** `AvSetMmThreadCharacteristicsW("Window Manager")` was
/// considered and declined. It pulls in `avrt.dll` and a revert that has to be
/// paired with it on every exit path; it lands the thread in the multimedia
/// scheduling class, whose boost is far larger than one step and would let the
/// loop starve *this process's own* PTY readers and workers under exactly the
/// saturation it is meant to survive; and MMCSS brings its own throttle — the
/// registry's `SystemResponsiveness` reserves a slice of every period for
/// non-multimedia work — so it can add latency as easily as remove it. One step
/// of ordinary thread priority is the smallest change that answers the actual
/// complaint, and it is a change this process can make about itself without
/// making a claim about the machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadPriority {
    AboveNormal,
    Normal,
    BelowNormal,
}

/// Which of the shell's five progress appearances a taskbar button is wearing.
///
/// One per `TBPF_*` flag and no more: this is a spelling of the Win32 enum for
/// callers that may not name Win32 types, not a vocabulary of its own. What maps
/// onto it — `bt_term::ProgressState`, and the rule by which a window full of
/// them becomes one reading — is the caller's business and is stated there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskbarProgressState {
    /// `TBPF_NOPROGRESS` — an ordinary button with no bar at all.
    None,
    /// `TBPF_NORMAL` — a green determinate bar.
    Normal,
    /// `TBPF_ERROR` — a red bar at whatever the value says.
    Error,
    /// `TBPF_PAUSED` — an amber bar at whatever the value says.
    Paused,
    /// `TBPF_INDETERMINATE` — a sweeping bar; the value is ignored by the shell.
    Indeterminate,
}

/// A whole reading for one taskbar button: an appearance, and how full it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskbarProgress {
    pub state: TaskbarProgressState,
    /// `(completed, total)`, or `None` for **say nothing about the value** —
    /// which is not the same as zero. `ITaskbarList3` remembers the last value
    /// it was given, so a state change that carries no value repaints the bar
    /// the button already had in the new colour: a run that fails at 30% goes
    /// red *at 30%*, which is the more informative picture and the one the
    /// shell gives for free. A button that has never been given a value shows
    /// the colour full, which is also the right answer for "it failed and
    /// nobody ever said how far it got".
    pub value: Option<(u64, u64)>,
}

impl TaskbarProgress {
    /// The reading of a window with nothing running: no bar, and nothing said
    /// about the value.
    pub const CLEARED: Self = Self {
        state: TaskbarProgressState::None,
        value: None,
    };
}

#[cfg(windows)]
pub use windows_impl::{
    Compositor, CustomWindowFrame, DirWatch, FilePickKind, FolderPicker, ImagePicker,
    ImeSystemCaret, MathContextMenu, Notifier, PROGRAM_REFUSED, Taskbar, adopt_parent_console,
    client_area_animation_enabled, clipboard_text, current_thread_priority, documents_directory,
    file_product_version, get_dpi_for_window, get_window_rect, get_work_area, install_context_menu,
    install_window_class_background, is_window_minimized, message_box, monospace_font_families,
    open_local_file, open_local_path, open_system_fonts_page, os_ui_language, read_context_menu,
    recycle, remove_context_menu, request_window_close, reveal_in_explorer, set_clipboard_text,
    set_current_thread_priority, set_system_backdrop, set_window_dark_mode, set_window_outer_rect,
    set_window_topmost, shell_execute, spawn_at_priority, system_backdrop_available,
    take_keyboard_focus, virtual_key_for_character, wheel_scroll_amount, write_to_console,
};

/// The bands, asked of the kernel rather than of the source.
///
/// These are here and not in `bt-app` because what is being pinned is the
/// *syscall's* effect: that a thread started through [`spawn_at_priority`] is
/// already out of the frame's way by the time its body runs, and that the band
/// a caller names is the band the thread is actually in. Which crate spawns
/// which worker is a separate question, tested where those spawns live.
#[cfg(all(test, windows))]
mod thread_priority_tests {
    use super::{ThreadPriority, current_thread_priority, set_current_thread_priority};

    /// PIN — a worker reports its band from *inside itself*.
    ///
    /// The bug this forecloses is the one the helper exists to prevent: setting
    /// the priority on the `JoinHandle` after `spawn`, which leaves the thread's
    /// first and busiest milliseconds — the decode, the `git status`, the
    /// directory walk — running at the frame's priority. Asking the thread
    /// itself, as its first act, is the only question whose answer distinguishes
    /// the two.
    #[test]
    fn a_worker_is_already_below_normal_when_its_body_starts() {
        let thread =
            super::spawn_at_priority("bt-test-worker", ThreadPriority::BelowNormal, || {
                current_thread_priority()
            })
            .expect("spawn a worker");
        assert_eq!(
            thread.join().expect("join the worker"),
            Some(ThreadPriority::BelowNormal),
            "a worker must be out of the frame's way before it does anything"
        );
    }

    /// PIN — the three bands are three different answers, and each round-trips.
    ///
    /// A mapping that collapsed two of them would be a process that believes it
    /// is ordered and is not, and nothing else in the tree would notice.
    #[test]
    fn every_band_round_trips_through_the_kernel() {
        for band in [
            ThreadPriority::AboveNormal,
            ThreadPriority::Normal,
            ThreadPriority::BelowNormal,
        ] {
            let thread = super::spawn_at_priority("bt-test-band", band, current_thread_priority)
                .expect("spawn a thread in a band");
            assert_eq!(
                thread.join().expect("join the thread"),
                Some(band),
                "{band:?} did not survive the round trip"
            );
        }
    }

    /// PIN — the loop's own thread can raise itself, which is the half of the
    /// design that has no spawn to hang off: `main` is handed its thread by the
    /// runtime and has to ask for the band in place.
    #[test]
    fn a_thread_can_raise_itself_in_place() {
        let raised = std::thread::spawn(|| {
            let taken = set_current_thread_priority(ThreadPriority::AboveNormal);
            (taken, current_thread_priority())
        })
        .join()
        .expect("join the raised thread");
        assert_eq!(raised, (true, Some(ThreadPriority::AboveNormal)));
    }
}

/// The one test in this crate that talks to the kernel about a real directory.
///
/// It is here rather than in `bt-app` because the thing under test is the
/// subscription itself: that `ReadDirectoryChangesW` reaches a callback at all,
/// and that dropping the handle stops it. Everything above this — when a change
/// becomes a re-read, and of what — is arithmetic and is tested where it lives
/// (`bt_app::git_watch`).
#[cfg(all(test, windows))]
mod dir_watch_tests {
    use super::{DirWatch, windows_impl::first_read_gate};
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    /// Generous, because this is a claim about *eventually* and the machine
    /// running it may be building something. A notification for a local
    /// directory arrives in single-digit milliseconds; five seconds is the
    /// difference between "slow" and "never", which is the only difference this
    /// test is about.
    const ARRIVES_WITHIN: Duration = Duration::from_secs(5);
    /// And the other way round, where the claim is "nothing at all": long enough
    /// that a notification which was going to come would have.
    const SILENCE_FOR: Duration = Duration::from_millis(600);

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "bt-dir-watch-{}-{name}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("make a scratch directory");
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// PIN — **a file appearing under a watched tree reaches the callback, and a
    /// dropped watch stops reaching it.**
    ///
    /// The two halves are one test because the second is only meaningful after
    /// the first: "no events arrived" is what a watcher that never worked also
    /// reports. Proving the wake-up first is what makes the silence afterwards
    /// evidence of a cancellation rather than of a mistake in the setup.
    #[test]
    fn a_watched_directory_reports_a_change_and_stops_when_dropped() {
        // One watcher at a time in this process — see `first_read_gate`.
        let _turn = first_read_gate::watchers_take_turns();
        let scratch = Scratch::new("basic");
        let (tx, rx) = mpsc::channel::<()>();
        let watch = DirWatch::start(&scratch.0, move || {
            let _ = tx.send(());
        })
        .expect("watch a directory this process just made");

        std::fs::write(scratch.0.join("appeared.txt"), b"hello").expect("write a file");
        rx.recv_timeout(ARRIVES_WITHIN)
            .expect("the kernel reports a file appearing under a watched tree");

        // Depth: a repository is a tree, and a commit touches any depth of it.
        while rx.try_recv().is_ok() {}
        std::fs::create_dir_all(scratch.0.join("a").join("b")).expect("make a subtree");
        std::fs::write(scratch.0.join("a").join("b").join("deep.txt"), b"hi").expect("write deep");
        rx.recv_timeout(ARRIVES_WITHIN)
            .expect("and reports one several directories down: the watch is recursive");

        drop(watch);
        // Drain whatever was already in flight when the watch was dropped — the
        // claim is about what happens *after* the cancellation, not about a
        // notification that had already been posted.
        while rx.try_recv().is_ok() {}
        std::fs::write(scratch.0.join("after.txt"), b"and this").expect("write after the drop");
        let deadline = Instant::now() + SILENCE_FOR;
        while Instant::now() < deadline {
            assert!(
                rx.try_recv().is_err(),
                "a dropped watch has stopped: nothing written afterwards reaches it"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// PIN (W2 slice 5) - **a shallow watch hears its own directory and
    /// nothing under it.**
    ///
    /// `preview_watch` subscribes to the folder a previewed file lives in,
    /// because Windows has no way to subscribe to a single file. Watching the
    /// tree instead would mean a README previewed out of a repository root woke
    /// this thread for every object file a build wrote into `target\debug` -
    /// a storm that the debounce would then have to absorb sixty times a second
    /// for news that was never about the watched file at all.
    ///
    /// Both halves in one test for the reason the recursive one above gives:
    /// "nothing arrived" is also what a subscription that never worked reports,
    /// so the shallow watch is proved to be *awake* on its own directory first
    /// and only then proved deaf to the subtree.
    ///
    /// MUTATION: make `start_shallow` pass `WatchDepth::Tree`, and the second
    /// half goes red.
    #[test]
    fn a_shallow_watch_hears_its_own_directory_and_not_the_tree_below_it() {
        let _turn = first_read_gate::watchers_take_turns();
        let scratch = Scratch::new("shallow");
        let subtree = scratch.0.join("nested");
        std::fs::create_dir_all(&subtree).expect("make the subtree before arming");
        let (tx, rx) = mpsc::channel::<()>();
        let watch = DirWatch::start_shallow(&scratch.0, move || {
            let _ = tx.send(());
        })
        .expect("watch a directory this process just made");

        std::fs::write(scratch.0.join("watched.txt"), b"hello").expect("write beside the file");
        rx.recv_timeout(ARRIVES_WITHIN)
            .expect("a shallow watch still hears its own directory");

        while rx.try_recv().is_ok() {}
        std::fs::write(subtree.join("deep.txt"), b"not ours").expect("write into the subtree");
        let deadline = Instant::now() + SILENCE_FOR;
        while Instant::now() < deadline {
            assert!(
                rx.try_recv().is_err(),
                "a shallow watch does not hear a directory below the one it was given"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(watch);
    }

    /// PIN — **`start` returns already listening.**
    ///
    /// The word is a promise about *when*: every caller in this workspace opens
    /// a folder and then goes on to do something to it or to show it, and the
    /// files column's own first move after arming is to list the directory the
    /// user has just opened — so anything that moves between the two has to be
    /// news, not a change that fell down the gap. Windows keeps no log for a
    /// subscriber who was not yet listening: a change made while no read is
    /// outstanding is not late, it is *gone*, and no budget however generous
    /// brings it back.
    ///
    /// The gap was real and it was a whole thread-spawn wide: `start` returned
    /// on `thread::spawn` and the first `ReadDirectoryChangesW` was issued by
    /// the new thread whenever it was scheduled.
    ///
    /// MUTATIONS: put the first read back on the far side of the spawn — hand
    /// the thread no way to report that it is listening and return as soon as it
    /// exists — and both halves of this go red: the flag says `start` returned
    /// with nothing outstanding, and the file written immediately afterwards
    /// reaches the callback never rather than in milliseconds.
    #[test]
    fn a_change_made_after_start_returns_is_never_missed() {
        let scratch = Scratch::new("armed");
        let (tx, rx) = mpsc::channel::<()>();
        // Held from before `start` until after the change: the watcher thread
        // cannot get a read out under its own steam, so "a read is outstanding"
        // can only mean `start` waited for one.
        let gate = first_read_gate::hold();
        let watch = DirWatch::start(&scratch.0, move || {
            let _ = tx.send(());
        })
        .expect("watch a directory this process just made");
        let outstanding_when_start_returned = gate.a_read_is_outstanding();

        // Strictly after `start` returned, and strictly before the gate opens:
        // the release below is what lets a read be issued, so this write
        // *happens before* any read the old code was going to get round to.
        std::fs::write(scratch.0.join("appeared.txt"), b"hello").expect("write a file");
        gate.release();

        rx.recv_timeout(ARRIVES_WITHIN).expect(
            "a change made after `start` returned reaches the callback: it returned listening",
        );
        assert!(
            outstanding_when_start_returned,
            "`start` returned with no read outstanding — everything that moved in that window \
             reached nobody, and Windows does not send it again"
        );
        drop(watch);
    }

    /// PIN — **a path that cannot be watched fails quietly and finally.**
    ///
    /// The caller's answer to this is to have no watcher for that repository,
    /// which is a state the rest of the machinery is built to live in: the
    /// window-focus trigger and the page's own refresh button are what cover a
    /// network share. What it must never be is an error a user is shown or a
    /// thing that is retried, because retrying on a schedule is the polling this
    /// whole mechanism exists to avoid.
    /// PIN — and **the refusal says which refusal it is**.
    ///
    /// `NotFound` and not merely "an error", because that is the distinction two
    /// callers act on: `scheme_watch` swallows a missing folder in silence —
    /// `%APPDATA%\Folio\schemes` does not exist on a fresh install and its
    /// absence is the ordinary case — and reports every other refusal on
    /// stderr. Until this was pinned, `win32_io_error` handed
    /// `std::io::Error::from_raw_os_error` the raw `HRESULT` `0x8007_0002`
    /// rather than the Win32 `2` it classifies by, so this kind was
    /// `Uncategorized`, the quiet arm was unreachable, and every fresh install
    /// printed a line about a folder that was never supposed to be there.
    ///
    /// Both codes, because Windows uses them for different shapes of absence: a
    /// leaf that is not there is `ERROR_FILE_NOT_FOUND`, a *parent* that is not
    /// there is `ERROR_PATH_NOT_FOUND`, and a folder under a folder that does
    /// not exist is the second.
    #[test]
    fn a_directory_that_is_not_there_declines_to_be_watched() {
        let missing = std::env::temp_dir().join("bt-dir-watch-no-such-directory-ever");
        let _ = std::fs::remove_dir_all(&missing);
        let error = DirWatch::start(&missing, || {})
            .err()
            .expect("nothing to subscribe to, and no thread left running to say so later");
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            "a missing folder is missing, not merely a failure: {error}"
        );

        let deeper = missing.join("nor").join("this");
        let error = DirWatch::start(&deeper, || {}).err().expect("nor this one");
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            "and a folder whose parent is missing reports the same thing: {error}"
        );
    }
}

/// The `HRESULT`-to-Win32 unwrapping, without having to make Windows fail.
#[cfg(all(test, windows))]
mod win32_error_tests {
    use super::windows_impl::win32_code;
    use windows::core::HRESULT;

    /// PIN — **`HRESULT_FROM_WIN32` is undone, and nothing else is touched.**
    ///
    /// MUTATIONS:
    /// ① return `code.0` unconditionally — what this file did until 2026-08-19
    ///    — and the first two go red, which is `DirWatch::start` never
    ///    answering `NotFound` again;
    /// ② mask every code regardless of facility and the last goes red:
    ///    `E_NOINTERFACE` would come out as Win32 error `0x4002`, a number that
    ///    means nothing at all, and `std` would describe a COM refusal as some
    ///    unrelated filesystem condition.
    #[test]
    fn a_facility_win32_hresult_comes_back_as_the_win32_code_it_wraps() {
        // ERROR_FILE_NOT_FOUND and ERROR_PATH_NOT_FOUND, wrapped. The second is
        // the shape a folder under a folder that does not exist arrives in.
        assert_eq!(win32_code(HRESULT(0x8007_0002_u32 as i32)), 2);
        assert_eq!(win32_code(HRESULT(0x8007_0003_u32 as i32)), 3);
        // ERROR_ACCESS_DENIED, which is the other refusal `DirWatch` can meet
        // and the one that must go on being reported.
        assert_eq!(win32_code(HRESULT(0x8007_0005_u32 as i32)), 5);
        // An `HRESULT` outside `FACILITY_WIN32` is not a Win32 code in a coat
        // and is carried through whole.
        let no_interface = 0x8000_4002_u32 as i32;
        assert_eq!(win32_code(HRESULT(no_interface)), no_interface);
    }
}

#[cfg(test)]
mod monospace_family_tests {
    use super::{DEFAULT_MONOSPACE_FAMILY, MonospaceFamily, order_monospace_families};

    fn named(name: &str) -> MonospaceFamily {
        MonospaceFamily {
            name: name.to_owned(),
            files: vec![std::path::PathBuf::from(format!(
                "C:\\Windows\\Fonts\\{name}.ttf"
            ))],
        }
    }

    fn names(families: &[MonospaceFamily]) -> Vec<&str> {
        families.iter().map(|f| f.name.as_str()).collect()
    }

    /// PIN — the list is sorted case-insensitively, so the row a user is looking
    /// for is where the alphabet says it is.
    ///
    /// DirectWrite hands families back in the font collection's internal order,
    /// which is neither alphabetical nor stable: it changes when a font is
    /// installed and differs between two machines with the same fonts. A picker
    /// whose rows move between launches is a picker nobody can use twice.
    ///
    /// MUTATIONS: ① sort by `name` directly and `MS Gothic` sorts before
    /// `Consolas`, because every uppercase letter sorts before every lowercase
    /// one; ② drop the sort and the order is whatever the machine said.
    #[test]
    fn the_family_list_is_alphabetical_without_regard_to_case() {
        let ordered = order_monospace_families(vec![
            named("MS Gothic"),
            named("consolas"),
            named("Cascadia Mono"),
            named("Lucida Console"),
        ]);
        assert_eq!(
            names(&ordered),
            vec!["Cascadia Mono", "consolas", "Lucida Console", "MS Gothic"]
        );
    }

    /// PIN — the default face is in the list even when the machine did not
    /// report it, and it lands in alphabetical position rather than on the end.
    ///
    /// The failure this guards is specific and silent: `settings.json` may hold
    /// no family at all, in which case the picker's selected row is the default
    /// one. If the enumeration came back without it — DirectWrite refused, or
    /// this Windows genuinely lacks Consolas — the combo would show a selected
    /// row that is not in its own list, which draws as a blank.
    #[test]
    fn the_default_face_is_always_a_row_and_sits_where_the_alphabet_puts_it() {
        let ordered = order_monospace_families(vec![named("Cascadia Mono"), named("MS Gothic")]);
        assert_eq!(
            names(&ordered),
            vec!["Cascadia Mono", DEFAULT_MONOSPACE_FAMILY, "MS Gothic"]
        );
        assert!(
            ordered[1].files.is_empty(),
            "a default the machine did not report needs no loading — the renderer \
             already has it from its fixed startup list, and an invented path \
             would be a file that does not exist"
        );
        assert_eq!(
            names(&order_monospace_families(Vec::new())),
            vec![DEFAULT_MONOSPACE_FAMILY],
            "a machine whose DirectWrite refused outright still gets one row"
        );
    }

    /// PIN — a machine that reports the default itself keeps its own files, and
    /// the inserted entry does not appear beside it.
    ///
    /// The bug this shape catches is inserting unconditionally: two `Consolas`
    /// rows, one of which cannot be loaded, is worse than either alternative.
    #[test]
    fn a_reported_default_keeps_its_files_and_is_not_doubled() {
        let ordered = order_monospace_families(vec![named("Consolas"), named("Cascadia Mono")]);
        assert_eq!(names(&ordered), vec!["Cascadia Mono", "Consolas"]);
        assert!(
            !ordered[1].files.is_empty(),
            "the machine's own files survive"
        );
    }

    /// PIN — a family reported twice becomes one row.
    ///
    /// DirectWrite will not normally repeat a family, but the localized-name
    /// walk can land two collection entries on one string — a family whose
    /// English and native names coincide — and a list with the same name twice
    /// gives a combo two rows that select differently while reading identically.
    #[test]
    fn a_family_reported_twice_is_one_row() {
        let ordered = order_monospace_families(vec![
            named("Consolas"),
            named("consolas"),
            named("Fira Code"),
        ]);
        assert_eq!(
            names(&ordered),
            vec!["Consolas", "Fira Code"],
            "which of the two spellings survives matters less than that the same              one survives every time — the tie is broken by exact bytes, so the              answer cannot depend on the order the machine reported them in"
        );
    }
}

/// The enumeration against the real font collection of the machine the tests run
/// on. Windows only, because there is nothing to enumerate elsewhere.
#[cfg(all(test, windows))]
mod monospace_enumeration_tests {
    use super::{DEFAULT_MONOSPACE_FAMILY, monospace_font_families};

    /// PIN — a real Windows answers with families that can actually be loaded.
    ///
    /// Deliberately not an assertion about *which* families: the machine running
    /// the test decides that, and pinning "Cascadia Mono is present" would fail
    /// on a Windows 10 without it. What is pinned is the contract every caller
    /// depends on — a name that is not empty, at least one file per row, files
    /// that exist on disk, and the default among them — because a row failing
    /// any of those is a picker row that silently does nothing when chosen.
    #[test]
    fn the_machines_own_monospaced_families_are_named_and_locatable() {
        let families = monospace_font_families();
        assert!(
            families
                .iter()
                .any(|family| family.name.eq_ignore_ascii_case(DEFAULT_MONOSPACE_FAMILY)),
            "the default face is promised to be a row on every machine"
        );
        for family in &families {
            assert!(!family.name.trim().is_empty(), "a row must have a name");
            if family.name.eq_ignore_ascii_case(DEFAULT_MONOSPACE_FAMILY) && family.files.is_empty()
            {
                // The inserted default — see `order_monospace_families`.
                continue;
            }
            assert!(
                !family.files.is_empty(),
                "{} came back with no file, so choosing it could not be honoured",
                family.name
            );
            for path in &family.files {
                assert!(
                    path.is_absolute(),
                    "{} names {path:?}, which is not a path a loader can open",
                    family.name
                );
            }
        }
    }
}

#[cfg(test)]
mod composition_offset_tests {
    use super::composition_visual_offset;

    /// PIN (WebView2 spike Q4) — **a visual's offset is in physical pixels and
    /// nothing multiplies it again.**
    ///
    /// The spike measured this on a 144-DPI monitor: the `DeviceRect` the layout
    /// produced, handed over unchanged, landed on the pixel. The failure this
    /// pins is the plausible one — someone reads "offset" and "DPI" in the same
    /// paragraph and scales by `scale_factor` on the way through, which is
    /// correct-looking at 96 DPI and puts the whole picture at double the offset
    /// on the machine this product is developed on.
    ///
    /// MUTATIONS:
    /// ① multiply by any constant other than one and the first two assertions
    ///    go red at once;
    /// ② round or clamp the input and the negative case goes red — a visual left
    ///    of its parent's origin is an ordinary thing to ask for.
    #[test]
    fn a_visual_offset_is_the_physical_pixel_it_was_handed() {
        assert_eq!(composition_visual_offset(0, 0), (0.0, 0.0));
        assert_eq!(composition_visual_offset(786, 710), (786.0, 710.0));
        assert_eq!(composition_visual_offset(-3780, -160), (-3780.0, -160.0));
    }

    /// And the cast is exact across every coordinate a desktop can name, which
    /// is what makes the `i32` -> `f32` narrowing a rename rather than a
    /// rounding. `f32` is exact to 2^24; the largest virtual desktop anyone can
    /// assemble is three orders of magnitude short of that.
    #[test]
    fn the_cast_to_the_compositors_float_loses_nothing_a_desktop_can_produce() {
        for pixel in [-131_072, -65_536, -1, 1, 65_536, 131_072] {
            let (x, y) = composition_visual_offset(pixel, pixel);
            assert_eq!(x as i32, pixel, "x survived the round trip");
            assert_eq!(y as i32, pixel, "y survived the round trip");
        }
    }
}

#[cfg(test)]
mod toast_tests {
    use super::{NOTIFICATION_AUMID, notification_aumid_key, toast_xml};

    /// PIN (Windows landing slice 3) — **a toast survives whatever a shell
    /// prints.**
    ///
    /// The body is arbitrary text from the far end of a pipe and a build log is
    /// full of `&`, `<` and `"`. The platform parses this string as XML, so one
    /// unescaped ampersand is not a mangled word — it is a document that fails to
    /// load, and a terminal that quietly stops notifying from then on.
    ///
    /// MUTATIONS: drop any one entity from `xml_escape` and the matching case
    /// below goes red; escape the body but not the `launch` attribute and the
    /// routing case goes red, which is the failure that would have shipped
    /// because a launch string is *ours* and looks safe until a tab id is put in
    /// a query string beside an `&`.
    #[test]
    fn every_field_of_a_toast_is_xml_escaped() {
        let xml = toast_xml(
            "make & co",
            r#"*** [all] Error 2 <see "log"> & stop"#,
            "n=1&t=2",
        );
        assert!(xml.contains("<text>make &amp; co</text>"), "{xml}");
        assert!(
            xml.contains("<text>*** [all] Error 2 &lt;see &quot;log&quot;&gt; &amp; stop</text>"),
            "{xml}"
        );
        assert!(xml.contains("launch=\"n=1&amp;t=2\""), "{xml}");
        assert!(
            xml.contains("activationType=\"foreground\""),
            "a click that is not an activation routes nothing: {xml}"
        );
    }

    /// PIN — **no body means no second line, not an empty one.**
    ///
    /// `OSC 777;notify;<title>` with nothing after it is a real shape, and an
    /// empty `<text/>` is a blank row the platform still lays out under the
    /// title.
    #[test]
    fn a_toast_with_no_body_carries_one_line() {
        let xml = toast_xml("built", "", "n=7");
        assert_eq!(xml.matches("<text>").count(), 1, "{xml}");
        assert!(xml.contains("<text>built</text>"), "{xml}");
    }

    /// PIN — **the identity is one string in one place.**
    ///
    /// The key path is derived from [`NOTIFICATION_AUMID`] rather than written
    /// out beside it, because the two disagreeing is the failure that registers
    /// one name and sends under another — and the symptom of that is `Show`
    /// succeeding while nothing appears, which is exactly what an unregistered
    /// AUMID does.
    #[test]
    fn the_registry_path_is_derived_from_the_aumid() {
        assert_eq!(
            notification_aumid_key(),
            r"Software\Classes\AppUserModelId\Folio.Terminal"
        );
        assert!(notification_aumid_key().ends_with(NOTIFICATION_AUMID));
    }
}

#[cfg(test)]
mod reveal_tests {
    use super::*;
    use std::path::Path;

    /// PIN (user ruling, 2026-08-13) — **a foot reveals the thing it names**,
    /// which for a file means Explorer opens with that file highlighted.
    ///
    /// The old answer was to open the containing folder, and against a folder
    /// of two hundred rows that is not an answer: the path the foot was
    /// printing arrived indistinguishable from everything beside it. `/select`
    /// is the verb that means what the foot says.
    ///
    /// The quoting is the whole of what can be wrong here. Explorer parses
    /// `/select,<path>` as one token and wants the path quoted; a command line
    /// one quote out opens `Documents` and reports success, which is the worst
    /// shape a failure can take.
    ///
    /// MUTATIONS:
    /// ① go back to a bare folder open — drop `/select,` — and the first
    ///    assertion goes red, which is the reported behaviour written down;
    /// ② drop the quotes and the third goes red: every path with a space in it
    ///    silently reveals the wrong thing.
    #[test]
    fn a_file_is_revealed_by_selecting_it_and_a_folder_by_opening_it() {
        let file = Path::new(r"C:\repo\test-assets\preview-samples\stress.md");
        let arguments = reveal_arguments(file, false);
        let text = arguments.to_string_lossy();
        assert!(
            text.starts_with("/select,"),
            "a file is highlighted where it lives, not merely surrounded: {text}"
        );
        assert!(
            text.contains(&*file.to_string_lossy()),
            "and it is that file that is named: {text}"
        );
        assert_eq!(
            text,
            format!("/select,\"{}\"", file.display()),
            "as one quoted token, which is the only form Explorer parses"
        );

        // A directory is opened rather than selected: `/select` on a folder
        // opens its *parent*, one level further out than a root is offering.
        let folder = Path::new(r"C:\repo\test-assets\preview-samples");
        let opened = reveal_arguments(folder, true);
        let text = opened.to_string_lossy();
        assert!(
            !text.contains("/select"),
            "a tree's own root is a place to look inside: {text}"
        );
        assert_eq!(text, format!("\"{}\"", folder.display()));

        // A path with a space survives, which is what the quotes are for.
        let spaced = Path::new(r"C:\My Documents\a file.md");
        assert_eq!(
            reveal_arguments(spaced, false).to_string_lossy(),
            "/select,\"C:\\My Documents\\a file.md\""
        );
    }
}

#[cfg(test)]
mod file_uri_tests {
    use super::*;
    use std::path::PathBuf;

    fn decoded(uri: &str) -> Option<String> {
        file_uri_to_path(uri).map(|path| path.to_string_lossy().into_owned())
    }

    /// PIN — **the URI a program printed becomes the path this machine names**,
    /// escape for escape.
    ///
    /// Every line here is a shape Claude Code or a shell actually emits. The
    /// space is the common one (`%20`, and every `[file]` line naming a document
    /// with a space in it goes through it); the Chinese name is the one that
    /// separates decoding *bytes* from decoding *characters*, because
    /// `%E4%B8%AD` is one character made of three escapes and a decoder that
    /// works escape-by-escape into a `String` produces three broken ones.
    ///
    /// MUTATIONS:
    /// ① decode without the UTF-8 join — push each byte as a `char` — and the
    ///    Chinese assertion goes red while every ASCII one stays green, which is
    ///    exactly the bug that would ship;
    /// ② drop the drive-letter slash strip and the first assertion becomes
    ///    `\C:\…`, a path with no root;
    /// ③ leave `/` alone instead of turning it into `\` and the last one goes
    ///    red — `C:/a/b` opens by luck through Win32 and by luck only, and it is
    ///    not what anything else in this crate hands the shell.
    #[test]
    fn a_file_uri_decodes_to_the_windows_path_it_names() {
        assert_eq!(
            decoded("file:///C:/Users/me/phd-application-timeline.html").as_deref(),
            Some(r"C:\Users\me\phd-application-timeline.html")
        );
        assert_eq!(
            decoded("file:///C:/My%20Documents/a%20file.md").as_deref(),
            Some(r"C:\My Documents\a file.md")
        );
        assert_eq!(
            decoded("file:///D:/%E4%B8%AD%E6%96%87/%E7%AC%94%E8%AE%B0.md").as_deref(),
            Some(r"D:\中文\笔记.md"),
            "three escapes are one character, not three"
        );
        // A drive letter is a drive letter in either case, and the case is kept
        // rather than normalised: Windows does not care and rewriting it would
        // make the path this window shows differ from the one that was printed.
        assert_eq!(
            decoded("file:///c:/tmp/x.md").as_deref(),
            Some(r"c:\tmp\x.md")
        );
        assert_eq!(
            decoded("FILE:///C:/tmp/x.md").as_deref(),
            Some(r"C:\tmp\x.md")
        );
        // Both separators arrive in the wild, including as an escape.
        assert_eq!(
            decoded(r"file:///C:\tmp\x.md").as_deref(),
            Some(r"C:\tmp\x.md")
        );
        assert_eq!(
            decoded("file:///C:%5Ctmp%5Cx.md").as_deref(),
            Some(r"C:\tmp\x.md")
        );
    }

    /// PIN — **the three authority readings**, which is the whole of what the
    /// `//` in `file://` is for.
    ///
    /// MUTATION: treat every authority as a host and `file:///C:/x` becomes
    /// `\\\C:\x`; treat every authority as empty and a real share silently
    /// becomes a local path, which is the more dangerous of the two.
    #[test]
    fn an_empty_host_and_localhost_are_this_machine_and_anything_else_is_a_share() {
        assert_eq!(
            decoded("file:///C:/tmp/x.md").as_deref(),
            Some(r"C:\tmp\x.md")
        );
        assert_eq!(
            decoded("file://localhost/C:/tmp/x.md").as_deref(),
            Some(r"C:\tmp\x.md")
        );
        assert_eq!(
            decoded("file://LocalHost/C:/tmp/x.md").as_deref(),
            Some(r"C:\tmp\x.md")
        );
        assert_eq!(
            decoded("file://server/share/notes.md").as_deref(),
            Some(r"\\server\share\notes.md")
        );
        // The authority-less spellings emitters produce, read as the same path.
        assert_eq!(
            decoded("file:/C:/tmp/x.md").as_deref(),
            Some(r"C:\tmp\x.md")
        );
        assert_eq!(decoded("file:C:/tmp/x.md").as_deref(), Some(r"C:\tmp\x.md"));
    }

    /// PIN — **what names nothing on this machine is refused**, rather than
    /// turned into something that would resolve against the process's current
    /// directory.
    ///
    /// MUTATION: drop the `drive_rooted || unc` gate and `file:///etc/passwd`
    /// becomes the relative path `etc\passwd`, which every door in this crate
    /// would then join to whatever directory the process is sitting in.
    #[test]
    fn a_uri_that_names_no_windows_path_is_refused_rather_than_guessed() {
        assert_eq!(
            decoded("file:///etc/passwd"),
            None,
            "a POSIX path is not one"
        );
        assert_eq!(decoded("file:///"), None, "a root with no name");
        assert_eq!(
            decoded("file://server"),
            None,
            "a machine, not a file on it"
        );
        assert_eq!(decoded("https://example.test/x"), None, "not our scheme");
        assert_eq!(decoded("C:/tmp/x.md"), None, "not a URI at all");
        // A stray `%` is a malformed URI, not a literal percent: a filename with
        // a real one in it is `%25`, and reading the broken case as text is how
        // a decoder starts naming files nobody wrote down.
        assert_eq!(decoded("file:///C:/100%/x.md"), None);
        assert_eq!(decoded("file:///C:/%zz/x.md"), None);
        assert_eq!(decoded("file:///C:/%E4%B8/x.md"), None, "not UTF-8");
        // Control characters and NUL, before decoding and after.
        assert_eq!(decoded("file:///C:/a\nb.md"), None);
        assert_eq!(decoded("file:///C:/a%00b.md"), None);
        assert_eq!(decoded("file:///C:/a%0Ab.md"), None);
    }

    /// PIN — a fragment and a query are URI syntax and never part of a filename,
    /// and a trailing separator is kept because it is the emitter saying
    /// "directory".
    #[test]
    fn a_fragment_and_a_query_are_cut_and_a_trailing_separator_is_kept() {
        assert_eq!(
            decoded("file:///C:/docs/notes.md#heading").as_deref(),
            Some(r"C:\docs\notes.md")
        );
        assert_eq!(
            decoded("file:///C:/docs/notes.md?v=2").as_deref(),
            Some(r"C:\docs\notes.md")
        );
        assert_eq!(
            file_uri_to_path("file:///C:/docs/"),
            Some(PathBuf::from(r"C:\docs\"))
        );
    }
}

#[cfg(test)]
mod context_menu_tests {
    use super::*;
    use std::path::Path;

    fn desired() -> ContextMenuShape {
        context_menu_shape(
            Path::new(r"C:\Program Files\Folio\folio.exe"),
            "Open Folio here",
        )
    }

    /// PIN — **the command line the shell will run, quoted the way the shell
    /// parses it.**
    ///
    /// Two quotes and one flag, and every one of them is load-bearing. The
    /// spike's own probe log (`spike-win-landing.md` §4) is why `--cwd` is
    /// there rather than nothing: `%V` arrives as an *argument* and the launched
    /// process's working directory is `folio.exe`'s own folder, so a build that
    /// read `current_dir()` would open every right-click in wherever the binary
    /// lives.
    ///
    /// MUTATIONS:
    /// ① drop the quotes round the path and `C:\Program Files\…` becomes two
    ///    arguments — the second assertion goes red, and on a real machine the
    ///    verb launches nothing;
    /// ② drop the quotes round `%V` and every folder with a space in its name
    ///    opens the wrong place, which is the failure that reports success;
    /// ③ write `%1` instead of `%V` and the folder tree still works while the
    ///    background tree silently stops substituting anything at all.
    #[test]
    fn the_verb_runs_the_exe_with_the_clicked_folder_quoted() {
        let shape = desired();
        assert_eq!(shape.label, "Open Folio here");
        assert_eq!(
            shape.command,
            r#""C:\Program Files\Folio\folio.exe" --cwd "%V""#
        );
        assert_eq!(shape.icon, r"C:\Program Files\Folio\folio.exe,0");
    }

    /// PIN — **the three answers the launch and the switch are both reading.**
    ///
    /// `Absent` is the fresh machine, `Current` is the machine that needs
    /// nothing done to it, and `Stale` is every way the registry can hold this
    /// verb and be wrong: a `folio.exe` that has since been moved, a label in a
    /// language the reader has left, and a set of trees somebody deleted half
    /// of.
    ///
    /// MUTATIONS:
    /// ① make a partial set `Absent` and the fourth assertion goes red — a
    ///    machine with a live menu entry would show a switch reading `Off`;
    /// ② compare only the command and the second-to-last goes red, so a
    ///    language change would never be carried into the menu;
    /// ③ ignore the tree count and a build that grew a third tree would call
    ///    a two-tree machine `Current` and never write the third.
    #[test]
    fn a_verdict_tells_a_fresh_machine_from_one_that_needs_rewriting() {
        let desired = desired();
        let absent = vec![None, None];
        assert_eq!(
            context_menu_verdict(&absent, &desired),
            ContextMenuState::Absent
        );

        let current = vec![Some(desired.clone()), Some(desired.clone())];
        assert_eq!(
            context_menu_verdict(&current, &desired),
            ContextMenuState::Current
        );

        let moved = context_menu_shape(Path::new(r"D:\tools\folio.exe"), "Open Folio here");
        assert_eq!(
            context_menu_verdict(&[Some(moved.clone()), Some(moved)], &desired),
            ContextMenuState::Stale,
            "an exe that has been moved leaves a menu entry that opens nothing"
        );

        assert_eq!(
            context_menu_verdict(&[Some(desired.clone()), None], &desired),
            ContextMenuState::Stale,
            "half a menu is not no menu"
        );

        let renamed = context_menu_shape(
            Path::new(r"C:\Program Files\Folio\folio.exe"),
            "\u{5728} Folio \u{4e2d}\u{6253}\u{5f00}",
        );
        assert_eq!(
            context_menu_verdict(&[Some(renamed.clone()), Some(renamed)], &desired),
            ContextMenuState::Stale,
            "the label is part of the shape, so a language change is a rewrite"
        );

        let short = vec![Some(desired.clone())];
        assert_eq!(
            context_menu_verdict(&short, &desired),
            ContextMenuState::Stale,
            "fewer trees than this build writes is not a machine that is current"
        );
    }
}

/// The registry half, against a class store of the suite's own.
///
/// **Nothing here touches `Software\Classes`.** Every call is given an isolated
/// prefix of its own under `HKCU\Software`, so a run of the suite cannot change
/// the right-click menu of whoever is running it — and the teardown that follows
/// is about the suite's own litter rather than about somebody's Explorer.
///
/// **The prefix is one key deep on purpose.** The first draft nested it —
/// `Software\Folio\test-context-menu\<pid>-<thread>-<name>` — and the teardown
/// removed the leaf, which left `Software\Folio\test-context-menu` standing with
/// one more empty child after every single run: ten runs of the suite on the
/// machine this was written on, ten keys, growing without bound in a developer's
/// own registry. One level means the only ancestor is `Software`, which was
/// there first and is nobody's to delete.
#[cfg(all(test, windows))]
mod context_menu_registry_tests {
    use super::windows_impl::{delete_registry_tree, registry_key_exists};
    use super::{
        CONTEXT_MENU_TREES, CONTEXT_MENU_VERB_KEY, ContextMenuState, context_menu_shape,
        context_menu_verdict, install_context_menu, read_context_menu, remove_context_menu,
    };
    use std::path::Path;

    /// A class store nothing but this test will ever look in, removed **whole**
    /// on the way out however the test ends.
    ///
    /// `delete_registry_tree` on the store's own root and not
    /// `remove_context_menu`: the product's removal deliberately leaves the
    /// container keys standing (see its note), so a teardown built on it would
    /// leave a growing pile of empty `test-context-menu\<pid>-…` keys in the
    /// registry of whoever runs the suite.
    struct Isolated(String);

    impl Isolated {
        fn new(name: &str) -> Self {
            let root = format!(
                "Software\\folio-context-menu-test-{}-{:?}-{name}",
                std::process::id(),
                std::thread::current().id()
            );
            let _ = delete_registry_tree(&root);
            Self(root)
        }

        /// The verb's own key under one tree of this store.
        fn verb(&self, tree: &str) -> String {
            format!("{}\\{tree}\\{CONTEXT_MENU_VERB_KEY}", self.0)
        }
    }

    impl Drop for Isolated {
        fn drop(&mut self) {
            let _ = delete_registry_tree(&self.0);
        }
    }

    /// PIN — **install, read back, repair, and remove exactly what was made.**
    ///
    /// The four claims the product rests on: what was written is what
    /// [`context_menu_verdict`] calls `Current`; a second install over the first
    /// is not an error, which is what makes the launch-time repair the same code
    /// as the install; a moved exe is `Stale` and one more write makes it
    /// `Current` again; and the removal takes the verb key away **and leaves the
    /// container keys where it found them**, which is the half that keeps this
    /// from deleting keys that were somebody else's.
    ///
    /// MUTATIONS:
    /// ① write the label without its NUL in the byte count and the read-back
    ///    comes off the end of the string — the first `Current` goes red;
    /// ② give the removal an ancestor prune and the last assertion goes red,
    ///    which on a real machine is `HKCU\Software\Classes\Directory\Background\shell`
    ///    — a key that was there, and empty, before Folio ever ran — being
    ///    deleted by a program that did not create it.
    #[test]
    fn a_verb_installs_reads_back_and_removes_exactly_what_it_made() {
        let store = Isolated::new("roundtrip");
        let shape = context_menu_shape(Path::new(r"C:\tools\folio.exe"), "Open Folio here");

        assert_eq!(
            context_menu_verdict(&read_context_menu(&store.0), &shape),
            ContextMenuState::Absent,
            "nothing has been written yet"
        );

        install_context_menu(&store.0, &shape).expect("write the verb into an isolated store");
        assert_eq!(
            context_menu_verdict(&read_context_menu(&store.0), &shape),
            ContextMenuState::Current
        );

        // Idempotent: this is the repair path, run on a machine that needs no
        // repair.
        install_context_menu(&store.0, &shape).expect("write it a second time");
        assert_eq!(
            context_menu_verdict(&read_context_menu(&store.0), &shape),
            ContextMenuState::Current
        );

        // And the repair proper: the exe has moved.
        let moved = context_menu_shape(Path::new(r"D:\elsewhere\folio.exe"), "Open Folio here");
        assert_eq!(
            context_menu_verdict(&read_context_menu(&store.0), &moved),
            ContextMenuState::Stale
        );
        install_context_menu(&store.0, &moved).expect("write the new path over the old");
        assert_eq!(
            context_menu_verdict(&read_context_menu(&store.0), &moved),
            ContextMenuState::Current,
            "one idempotent write is the whole of the self-repair"
        );

        remove_context_menu(&store.0).expect("take it back out");
        assert!(
            read_context_menu(&store.0).iter().all(Option::is_none),
            "the verb is gone from every tree"
        );
        for tree in CONTEXT_MENU_TREES {
            assert!(
                !registry_key_exists(&store.verb(tree)),
                "the verb's own key went with its values: {tree}"
            );
            assert!(
                registry_key_exists(&format!("{}\\{tree}", store.0)),
                "and the container it sat in is left where it was found — a key \
                 that is empty now is not a key this product created: {tree}"
            );
        }

        // Removing what is not there is what "make sure it is gone" means.
        remove_context_menu(&store.0).expect("a second removal is not a failure");
    }
}

#[cfg(test)]
mod custom_frame_tests {
    use super::*;

    fn metrics(dpi: u32) -> CustomFrameMetrics {
        CustomFrameMetrics {
            width: logical_px_for_dpi(960, dpi),
            height: logical_px_for_dpi(600, dpi),
            title_bar_height: logical_px_for_dpi(40, dpi),
            tab_strip_right_px: logical_px_for_dpi(240, dpi),
            caption_button_width: logical_px_for_dpi(46, dpi),
            caption_button_count: 4,
            resize_border: logical_px_for_dpi(8, dpi),
            resizable: true,
        }
    }

    /// Red gate: every region returned to Windows is pinned, including corners
    /// (which must win over caption), the drag band and the app-owned buttons.
    #[test]
    fn custom_frame_hit_test_maps_drag_resize_edges_and_caption_buttons() {
        let m = metrics(96);
        assert_eq!(custom_frame_hit_test(m, 0, 0), CustomFrameHit::TopLeft);
        assert_eq!(custom_frame_hit_test(m, 959, 0), CustomFrameHit::TopRight);
        assert_eq!(custom_frame_hit_test(m, 0, 599), CustomFrameHit::BottomLeft);
        assert_eq!(
            custom_frame_hit_test(m, 959, 599),
            CustomFrameHit::BottomRight
        );
        assert_eq!(custom_frame_hit_test(m, 0, 300), CustomFrameHit::Left);
        assert_eq!(custom_frame_hit_test(m, 959, 300), CustomFrameHit::Right);
        assert_eq!(custom_frame_hit_test(m, 400, 0), CustomFrameHit::Top);
        assert_eq!(custom_frame_hit_test(m, 400, 599), CustomFrameHit::Bottom);
        assert_eq!(custom_frame_hit_test(m, 100, 20), CustomFrameHit::Client);
        assert_eq!(custom_frame_hit_test(m, 300, 20), CustomFrameHit::Caption);
        assert_eq!(custom_frame_hit_test(m, 800, 20), CustomFrameHit::Client);
        assert_eq!(custom_frame_hit_test(m, 300, 60), CustomFrameHit::Client);
    }

    /// DPI changes scale both the logical title/button geometry and the resize
    /// band; testing equivalent logical points catches a hard-coded 96-DPI map.
    #[test]
    fn custom_frame_hit_test_is_dpi_scaled() {
        for dpi in [96, 120, 144, 168, 192, 240] {
            let m = metrics(dpi);
            assert_eq!(
                custom_frame_hit_test(m, logical_px_for_dpi(120, dpi), logical_px_for_dpi(20, dpi),),
                CustomFrameHit::Client,
                "tab strip at {dpi} DPI"
            );
            assert_eq!(
                custom_frame_hit_test(m, logical_px_for_dpi(300, dpi), logical_px_for_dpi(20, dpi),),
                CustomFrameHit::Caption,
                "drag region beside tab strip at {dpi} DPI"
            );
            assert_eq!(
                custom_frame_hit_test(
                    m,
                    m.width - logical_px_for_dpi(23, dpi),
                    logical_px_for_dpi(20, dpi),
                ),
                CustomFrameHit::Client,
                "caption button at {dpi} DPI"
            );
            assert_eq!(
                custom_frame_hit_test(m, 1, m.height / 2),
                CustomFrameHit::Left,
                "resize border at {dpi} DPI"
            );
        }
    }

    #[test]
    fn maximized_frame_has_no_resize_hits_but_keeps_caption_dragging() {
        let mut m = metrics(144);
        m.resizable = false;
        m.resize_border = 0;
        assert_eq!(custom_frame_hit_test(m, 0, 0), CustomFrameHit::Client);
        assert_eq!(
            custom_frame_hit_test(m, m.tab_strip_right_px + 1, 1),
            CustomFrameHit::Caption
        );
        assert_eq!(
            custom_frame_hit_test(m, m.width - 1, 1),
            CustomFrameHit::Client
        );
    }
}
