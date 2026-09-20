//! **Which addresses a web seat may go to** — plan §3 written as a rule, and
//! nothing else (`docs/plans/web-preview/plan.md` §3; W2 片②, 2026-08-22).
//!
//! # Two doors, one rule
//!
//! The address bar decides what may be *asked for*. `NavigationStarting`
//! decides what may actually *load*, and it asks the same question a second
//! time because a redirect, a link in the page or a page script can start a
//! navigation the address bar never saw. Both doors are [`check`]; the two
//! spellings [`address_bar`] and [`navigation_starting`] exist so that the call
//! site reads like the event it is answering.
//!
//! A pin is not an authorisation. A string out of `pins.json` comes back
//! through [`address_bar`] like a string somebody typed, because the pin was
//! written by an earlier build, or edited by hand, or made under a policy that
//! has since tightened — and a store that could hand out permissions would be a
//! store whose file is the permission.
//!
//! # Why the mint exists (W0′ gate 9)
//!
//! §3 refuses `about:` unconditionally. §4 navigates a fresh seat to
//! `about:blank` and expects it to load. W0′ measured the engine and found that
//! `NavigationStarting` **does** fire for `about:blank`
//! (`w0p-evidence/evidence.md` §4.1), so an enforcer written from §3's letter
//! cancels the product's own navigation and leaves the seat on the page before
//! it. The hole is not a security hole — it is a *liveness* hole — and the fix
//! is not to soften the door but to say who is knocking: [`Mint`] is what the
//! host itself put in front of this seat, and it is the only thing that can make
//! `about:blank` or a `file:` URL pass. The engine cannot tell the host which
//! navigation the host asked for, so the mint — a record the host keeps, not an
//! event flag — is what separates the seat's own blank page from a page that
//! asked for one. **The address bar's door does not move**: nobody types
//! `about:` in.
//!
//! The `file:` arm of the mint is the shape this is copied from. The controlled
//! file entry (files column, `.html` / `.htm` / `.pdf`) hands over a `PathBuf`
//! the column already resolved, [`Mint::file`] turns it into the one `file:` URL
//! this seat may load, and every other `file:` URL — typed, redirected to, or
//! walked to with `..` from inside the sanctioned page — is refused.
//!
//! # What this module is not
//!
//! No I/O, no COM, no host. It does not resolve DNS, read `hosts`, ask the disk
//! whether a path exists, or know which search engine the user picked. Loopback
//! is decided **by syntax alone**, because a name that resolves to 127.0.0.1
//! today and to a LAN box tomorrow must not silently change what the preview
//! opens by default; and a non-URL leaves here as [`Decision::Search`] carrying
//! the text, because *which* engine receives it is 片④'s question.
//!
//! **Nothing in this file is called from the window yet, and that is the slice
//! boundary.** 片② delivers the rule and its contract tests; 片① calls
//! [`navigation_starting`] from the `NavigationStarting` handler, 片③ takes
//! [`switcher_key`] as the preview pool's de-duplication key, 片④ owns the
//! address bar's UI and the search engine, 片⑤ owns the files-column entry that
//! calls [`Mint::file`].
#![cfg_attr(not(test), allow(dead_code))]

use std::path::{Path, PathBuf};

/// The one blank page the host mints for itself, spelled once.
pub const BLANK_PAGE: &str = "about:blank";

/// Why an address was refused. Each variant is one row of the red matrix, and
/// one thing the blocked card (§7.7 ④) could be asked to say out loud.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// `javascript:`, `data:`, `blob:`, `vbscript:` — script and inline content
    /// smuggled in as a location.
    ScriptOrInlineScheme,
    /// A `file:` URL that is not the one this seat's mint holds. The only file a
    /// preview opens is the one the files column handed it as a canonicalised
    /// path.
    FileScheme,
    /// `view-source:`, `devtools:`, `edge:`, `chrome:`, `about:` — browser
    /// internals, including the blank page when nobody minted it.
    BrowserInternalScheme,
    /// `mailto:`, `ftp:`, `ws:`, `tel:` — anything the shell would launch.
    /// External protocols are refused outright and never confirmed
    /// (DESIGN §7.1.1, §7.1.5g ⑤).
    ExternalScheme,
    /// `https://user:pass@host` — the phishing shape.
    UserInfo,
    /// A UNC or network path offered to the mint. The product's existing refusal
    /// (DESIGN §7.1.3, §7.1.5g ④) reaches the web seat unchanged.
    NetworkPath,
    /// A control character anywhere, or whitespace inside something that has
    /// already named a scheme. §7.1.5g's `http(s)` arm reads the same way.
    ControlOrWhitespace,
    /// A scheme with no authority after it — `http://` and nothing else.
    NoHost,
    /// The host asked to navigate to a target it had not minted.
    NotMinted,
    /// Empty, or nothing but whitespace.
    Empty,
}

/// What a door decided.
///
/// [`Decision::Navigate`] carries the URL **after** normalisation and
/// rewriting, and the host's side of the contract is to navigate to *that*
/// rather than to what it asked about — at `NavigationStarting` a verdict whose
/// URL differs from the candidate means cancel this navigation and start the
/// returned one, which terminates because normalisation is idempotent.
///
/// [`Decision::Search`] is reachable from [`Origin::AddressBar`] alone: a
/// redirect target that is not a URL is not a search, it is nonsense, and the
/// engine never offers one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    /// Go here. The string is the URL to navigate to.
    Navigate(String),
    /// Not an address — hand this text to the default search engine. Which
    /// engine that is belongs to 片④.
    Search(String),
    /// Do not navigate, and this is why.
    Refuse(Refusal),
}

/// **A `file:` URL parsed into the one thing it names: a path on this disk**
/// (issue #7, 2026-09-20).
///
/// # Why a type and not three comparisons
///
/// A URL is a *spelling* of a path, and a path has many spellings. Three times
/// now this product has compared two spellings where it meant to compare the
/// thing:
///
/// 1. a Mac minted `file:////Users/…` — four slashes — the engine normalised it
///    to three, and a local seat refused its own document (M4-2);
/// 2. the drive letter's case, which `eq_ignore_ascii_case` papered over rather
///    than answered;
/// 3. percent-encoding. Measured on 2026-09-20 against WebView2 153.0.4234.48
///    (`scratchpad/pdf-vhost-spike/REPORT.md`, 17 recorded pairs): the engine
///    **canonicalises** the URI it is navigated to — every byte outside its
///    `file:`-path safe set comes back percent-encoded as upper-case-hex UTF-8,
///    escapes that are already there are left verbatim, and the drive letter is
///    upper-cased. Observed outside that safe set: all non-ASCII, `{`, `}`, `^`
///    and `` ` ``. [`Mint::file`] escapes five characters and leaves the rest
///    raw, so `报告.pdf` went out raw and came back as `%E6%8A%A5…`; the mint
///    compared the two strings, they differed, and Folio refused the document it
///    had just opened — issue #7.
///
/// Minting in the engine's spelling would work today and is an allowlist: it
/// requires this product to carry the engine's safe set, which is a list of the
/// characters somebody thought of, on an engine that may lengthen it, on two
/// engines that need not agree. Parsing both sides to the path they name is
/// total. So: **a URL that names a local file is parsed to a path at the door,
/// once; past the door nobody compares or slices URL strings.**
///
/// # The grammar
///
/// ```text
/// local-file-url := "file:///" body [ tail ]      (the scheme ASCII-case-blind)
/// tail           := ( "?" | "#" ) *ANY            (the page's, never the path's)
/// body           := *( "%" HEXDIG HEXDIG | ANY )
/// ```
///
/// The body is percent-decoded and the bytes read as UTF-8, and then the path
/// has to be **one absolute path with a name in it**, which is the whole of what
/// [`Mint::file`] can ever have written:
///
/// * invalid UTF-8 is refused, never replaced — a candidate that matches the
///   mint only after a lossy decode is a different file wearing its name;
/// * a control character, NUL included, is refused;
/// * a decoded `\` is refused rather than guessed at. It is the separator on one
///   of this product's two machines and a legal name character on the other, and
///   a rule that read it as a name would hand Windows a traversal
///   (`…/page/%5C../secret`) while a rule that read it as a separator would name
///   a file on macOS that nobody asked for;
/// * an empty, `.` or `..` segment is refused **after** decoding, so `%2e%2e`
///   and `%2E%2E` are the same refusal `..` is. Double encoding is not: `%252e`
///   decodes once, to the two characters `%2e`, which is a *name* — the disk
///   resolves no traversal out of it and neither does this;
/// * the root is read off the string and not off the machine (M4-3), because a
///   `session.json` travels: a first segment of `X:` is the drive-rooted form —
///   its separator is `\` and its drive letter is upper-cased, which is what the
///   engine and the OS both call it — and anything else is the slash-rooted form
///   whose one leading separator the `file:///` prefix already ate.
///
/// Everything else the body carries is a name character, escaped or raw, which
/// is what lets one of these read both spellings of a stored row: the raw
/// non-ASCII [`Mint::file`] wrote into `session.json` before this change, and
/// the percent-encoded form the engine commits.
///
/// # What it does *not* normalise
///
/// **The Windows trailing-dot and trailing-space aliases** (`report.pdf.`,
/// `report.pdf `) are left alone, so they do not compare equal to `report.pdf`.
/// Windows opens all three as one file, which is an argument for folding them
/// and a better argument against: on the other machine they are three different
/// files, folding them would make this gate admit a name the engine never
/// produced, and refusing them is what it already did. A gate may be wrong by
/// refusing a page; it may not be wrong by opening one.
#[derive(Clone, Debug)]
pub struct LocalFileUrl {
    /// The text this was parsed from — the spelling the engine is navigated to,
    /// kept because that is what a navigation needs and never because anything
    /// compares it.
    url: String,
    /// **The fact.** Absolute, decoded, separators and drive-letter case as the
    /// machine the path is *for* spells them.
    path: PathBuf,
    /// The `?query` and `#fragment` the *page* is answerable for: a local
    /// report's table of contents is `report.html#ch3`, and a row that dropped
    /// the fragment would reopen the report at the top.
    tail: String,
}

/// **Two of these are equal when they name the same file and the same place in
/// it** — never when they are spelled the same way.
///
/// Case-insensitively, and by ASCII case alone: that is what
/// `eq_ignore_ascii_case` has always given this comparison, both of this
/// product's machines default to a case-insensitive filesystem, and full Unicode
/// case folding would answer *yes* for pairs of names that are two files
/// everywhere (`K` and the Kelvin sign, `i` and `İ`).
impl PartialEq for LocalFileUrl {
    fn eq(&self, other: &Self) -> bool {
        self.names_the_same_file_as(other) && self.tail == other.tail
    }
}

impl Eq for LocalFileUrl {}

impl LocalFileUrl {
    /// The one parser. `None` is "this string does not name one local file",
    /// and the caller's answer to that is always to refuse rather than to guess.
    #[must_use]
    pub fn parse(url: &str) -> Option<Self> {
        let rest = url
            .get(..8)
            .filter(|head| head.eq_ignore_ascii_case("file:///"))
            .map(|_| &url[8..])?;
        let cut = rest.find(['?', '#']).unwrap_or(rest.len());
        let (body, tail) = rest.split_at(cut);

        let mut bytes = Vec::with_capacity(body.len());
        let mut characters = body.chars();
        while let Some(character) = characters.next() {
            if character == '%' {
                let high = characters.next()?.to_digit(16)?;
                let low = characters.next()?.to_digit(16)?;
                bytes.push(u8::try_from(high * 16 + low).ok()?);
            } else {
                let mut buffer = [0u8; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            }
        }
        // `from_utf8` and never `from_utf8_lossy`: a replacement character is
        // this door inventing a name.
        let mut decoded = String::from_utf8(bytes).ok()?;
        if decoded
            .chars()
            .any(|character| character.is_control() || character == '\\')
        {
            return None;
        }

        // **The root, read off the string**, and every segment read after the
        // decoding rather than before it.
        let mut segments = decoded.split('/');
        let first = segments.next()?;
        let mut letters = first.chars();
        let drive_rooted = letters
            .next()
            .is_some_and(|letter| letter.is_ascii_alphabetic())
            && letters.next() == Some(':')
            && letters.next().is_none();
        // A drive is a root and not a name, so `file:///D:` names no file.
        let mut names = usize::from(!drive_rooted);
        if !drive_rooted && matches!(first, "" | "." | "..") {
            return None;
        }
        for segment in segments {
            if matches!(segment, "" | "." | "..") {
                return None;
            }
            names += 1;
        }
        if names == 0 {
            return None;
        }

        let path = if drive_rooted {
            decoded[..1].make_ascii_uppercase();
            decoded.replace('/', "\\")
        } else {
            let mut rooted = String::with_capacity(decoded.len() + 1);
            rooted.push('/');
            rooted.push_str(&decoded);
            rooted
        };
        Some(Self {
            url: url.to_owned(),
            path: PathBuf::from(path),
            tail: tail.to_owned(),
        })
    }

    /// The spelling to navigate to. The engine is the only reader.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.url
    }

    /// The file this URL names.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `?query` and `#fragment`, empty when there is none.
    #[must_use]
    pub fn tail(&self) -> &str {
        &self.tail
    }

    /// The pair, for a caller that wants both and keeps neither.
    #[must_use]
    pub fn into_path_and_tail(self) -> (PathBuf, String) {
        (self.path, self.tail)
    }

    /// **Whether these two URLs name one file**, whatever either of them did to
    /// spell it. The tail does not participate: a jump to `#section` inside the
    /// page that was minted is the same page.
    #[must_use]
    pub fn names_the_same_file_as(&self, other: &Self) -> bool {
        self.path
            .as_os_str()
            .as_encoded_bytes()
            .eq_ignore_ascii_case(other.path.as_os_str().as_encoded_bytes())
    }

    /// **Whether this URL names something inside the folder `minted` sits in** —
    /// the whole of a local seat's reach.
    ///
    /// The folder and the folders under it, because that is what a document is:
    /// a report and its `images/` directory are one thing a person opened, and a
    /// rule that admitted only the exact file would show them a report with no
    /// pictures in it. The comparison is against the folder **plus its
    /// separator**, so `D:\tmp\page` does not admit `D:\tmp\pageant\x.png`.
    #[must_use]
    pub fn is_inside_the_folder_of(&self, minted: &Self) -> bool {
        let minted = minted.path.as_os_str().as_encoded_bytes();
        let Some(last) = minted
            .iter()
            .rposition(|byte| *byte == b'/' || *byte == b'\\')
        else {
            return false;
        };
        let folder = &minted[..=last];
        let candidate = self.path.as_os_str().as_encoded_bytes();
        candidate.len() >= folder.len() && candidate[..folder.len()].eq_ignore_ascii_case(folder)
    }
}

/// What the host itself put in front of one web seat.
///
/// Last write wins, mirroring §4's `desired_url`: a seat has at most one minted
/// target at a time, and the moment the host navigates somewhere of its own
/// choosing the previous mint stops being an answer. [`Mint::Nothing`] is the
/// resting state and admits nothing at all, which is why an address bar that
/// never consults a mint and a seat whose mint is empty give the same answers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Mint {
    /// Nothing minted. Every candidate is judged on the allowlist alone.
    #[default]
    Nothing,
    /// The host's own empty page — a fresh seat, a seat between two documents,
    /// the target of a popup the host is about to redirect into this pane.
    Blank,
    /// **The one local file the controlled file entry minted**, as the path it
    /// names rather than as one spelling of it ([`LocalFileUrl`]). A mint that
    /// could not be parsed cannot be built, so no seat ever carries a mint that
    /// nothing can compare.
    File(LocalFileUrl),
}

impl Mint {
    /// Mint the one `file:` URL a controlled file entry may load.
    ///
    /// The path is expected to have been canonicalised already — that is the
    /// files column's job and it has the disk to do it with. What is done here
    /// is the part that is a *string* question: network paths keep the
    /// product's existing refusal, and the four characters that would re-open
    /// the parse (`%`, `#`, `?`, space) are percent-encoded, because a file
    /// named `notes#1.html` is a file and not a fragment.
    pub fn file(path: &Path) -> Result<Self, Refusal> {
        let text = path.to_string_lossy();
        let stripped = text.strip_prefix(r"\\?\").unwrap_or(&text);
        if stripped.starts_with(r"\\") || stripped.starts_with("UNC\\") {
            return Err(Refusal::NetworkPath);
        }
        // **Three slashes, counting the one the path may already carry**
        // (M4-2). `file:` URLs are `file://` plus an empty authority plus an
        // absolute path, and the two kinds of absolute path this product sees
        // spell their root differently: `D:\report.html` has no leading
        // separator and `/Users/somebody/report.html` is nothing but one. The
        // old spelling assumed the first, so a Mac minted
        // `file:////Users/…` — four slashes — which is a URL the engine
        // normalises back to three and which therefore matched neither
        // [`Self::admits`] nor [`LocalFileUrl::is_inside_the_folder_of`]: a
        // local seat refused its own document.
        //
        // A question about the string and not about the machine, so this file
        // still names no platform.
        let mut url = String::from("file://");
        if !stripped.starts_with('/') {
            url.push('/');
        }
        for character in stripped.chars() {
            match character {
                '\\' => url.push('/'),
                '%' => url.push_str("%25"),
                '#' => url.push_str("%23"),
                '?' => url.push_str("%3F"),
                ' ' => url.push_str("%20"),
                other => url.push(other),
            }
        }
        // **Minted through the same door everything else is read through**
        // (issue #7). The string just written is handed to [`LocalFileUrl`] and
        // the mint carries what comes back, so there is one grammar rather than
        // an encoder and a reader that can disagree, and a mint that exists is
        // a mint every comparison can answer about. What this can now refuse is
        // a path that is not one absolute path with a name in it — a `.` or
        // `..` segment, a control character, a bare drive. Those are not
        // addresses a reader asked for: the files column canonicalises before
        // it mints, and `canonicalize` returns none of them.
        LocalFileUrl::parse(&url)
            .map(Self::File)
            .ok_or(Refusal::FileScheme)
    }

    /// The URL this mint stands for, which is what the host navigates to.
    pub fn target(&self) -> Option<&str> {
        match self {
            Self::Nothing => None,
            Self::Blank => Some(BLANK_PAGE),
            Self::File(url) => Some(url.as_str()),
        }
    }

    /// Whether `candidate` is this mint, and if so the URL to allow.
    ///
    /// **Both sides are read as the path they name** ([`LocalFileUrl`], issue
    /// #7) and never as text. The sanctioned file answers for its own fragments
    /// and queries — a jump to `#section` inside the page that was minted is
    /// the same page — and a candidate that names anything else, or that names
    /// nothing this door can read, is not this mint. There is no normalisation
    /// of `..`: the mint was made from a canonicalised path, so a candidate
    /// that would need normalising to match is a candidate that did not come
    /// from the mint, and the parser refuses one outright.
    ///
    /// The URL handed back is the **candidate's own** spelling, because that is
    /// the string the engine is already loading; answering with the mint's
    /// would be [`check`]'s contract asking for a cancel-and-restart on every
    /// local page.
    pub(crate) fn admits(&self, candidate: &str) -> Option<String> {
        match self {
            Self::Nothing => None,
            Self::Blank => candidate
                .eq_ignore_ascii_case(BLANK_PAGE)
                .then(|| BLANK_PAGE.to_owned()),
            Self::File(minted) => LocalFileUrl::parse(candidate)?
                .names_the_same_file_as(minted)
                .then(|| candidate.to_owned()),
        }
    }
}

/// **A local file, as this product spells a local file** (user ruling
/// 2026-08-25) — `D:\Developer\notes.html`, not `file:///D:/Developer/notes.html`.
///
/// The ruling came from two screenshots side by side: a page's band said
/// `file:///D:/…` under a globe, and the preview beside it said `D:\…` under a
/// folder, for two files on the same disk. **One machine, one spelling of a
/// path** — so every surface that *shows* a local file shows the OS form, and
/// the `file:` URI stays what it always was underneath: what the engine is
/// navigated to, what a mint compares, what `session.json` keeps.
///
/// [`LocalFileUrl`] read for display, which is why it is here and not spelled
/// again at each surface. A string that does not name one local file answers
/// `None` and is shown exactly as it arrived — a `file:` URL with a host on it
/// is a share and not a path, and guessing at one is how a path comes out of
/// something that never named one.
///
/// **A page whose path is in Chinese is shown as a path too** (issue #7,
/// 2026-09-20). The engine commits the URL in its own spelling — `报告.pdf`
/// comes back as `%E6%8A%A5%E5%91%8A.pdf` — and the reader this used to call
/// knew four escapes, so every such page showed the reader a percent-encoded
/// URI where the ruling says a path goes. The parser is total now and the row
/// says `D:\文档\报告.pdf`.
///
/// **The root is the machine's, and so is the spelling** (M4-3): `D:\Developer\notes.html`
/// where a drive rooted the path and `/Users/somebody/notes.html` where a slash
/// did. It is the *absolute* path in both cases and not the `~`-relative crumb
/// form §13.32 ③ gives the files column's breadcrumb row, because this string is
/// not only shown: it seeds the address field, and what that field hands back
/// goes through [`file_url_of_local_path`], which takes an absolute path and
/// nothing else. A row that displayed `~/notes.html` would be a field whose own
/// content it refuses.
///
/// The tail rides along: `report.html#ch3` is a place in a report, and a
/// displayed path that dropped the fragment would name the top of it.
#[must_use]
pub fn local_path_form(url: &str) -> Option<String> {
    let parsed = LocalFileUrl::parse(url)?;
    Some(format!("{}{}", parsed.path().display(), parsed.tail()))
}

/// The same sentence read the other way: **a drive-absolute path typed into an
/// address field is that file** (user ruling 2026-08-25).
///
/// The ruling asked this door to be checked and it does *not* take a bare path:
/// `D:\x` splits as the scheme `d`, which `classify_scheme` refuses like any
/// other unknown scheme. So the field shows the OS form, and what it hands the
/// door is minted back into a `file:` URL first — one conversion, at the door,
/// so that a path typed by a person and a path arriving from the files column
/// are the same string by the time anything decides whether to load it.
///
/// `None` for everything that is not a local path, which is every real address:
/// `http://…` carries a scheme this refuses to touch, and a bare `example.com`
/// is not absolute.
#[must_use]
pub fn file_url_of_local_path(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let path = std::path::Path::new(trimmed);
    // A scheme is never a path. `C:\x` and `c:/x` are absolute *and* split as a
    // scheme, which is exactly the collision this function exists to resolve —
    // so the test is the shape of the path rather than the absence of a colon:
    // a drive letter, a separator, and no `..` to re-open.
    if !path.is_absolute()
        || trimmed.starts_with("//")
        || trimmed.starts_with(r"\\")
        || path.components().any(|part| part.as_os_str() == "..")
    {
        return None;
    }
    match Mint::file(path) {
        Ok(mint) => mint.target().map(ToOwned::to_owned),
        Err(_) => None,
    }
}

/// Which door a candidate arrived at — the only thing that makes two identical
/// strings get two different answers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin<'a> {
    /// A string from outside the host: typed into the address bar, restored
    /// from a session, loaded out of `pins.json`, chosen from the preview
    /// switcher, or handed over by the command palette. None of them is an
    /// authorisation and all of them read the same.
    AddressBar,
    /// `NavigationStarting`: a redirect, a link in the page, a script, or the
    /// host's own request coming back around. The mint is what tells those
    /// apart.
    NavigationStarting(&'a Mint),
    /// The host's own minted target, on the way *out* to the engine. Only what
    /// the mint holds passes, so that a stale or mismatched mint is caught by
    /// this module at the point of issue rather than by the engine — and so
    /// that every navigation the product starts has been through a door.
    HostMinted(&'a Mint),
}

/// The rule. Everything else in this file is a spelling of it or a piece of it.
pub fn check(candidate: &str, origin: Origin<'_>) -> Decision {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return Decision::Refuse(Refusal::Empty);
    }
    if trimmed.chars().any(char::is_control) {
        return Decision::Refuse(Refusal::ControlOrWhitespace);
    }
    match origin {
        Origin::HostMinted(mint) => {
            return match mint.admits(trimmed) {
                Some(url) => Decision::Navigate(url),
                None => Decision::Refuse(Refusal::NotMinted),
            };
        }
        // **A mint that refuses is a refusal, not a fall-through** (R1-16).
        //
        // A seat carrying a mint is a seat the host put a specific document in
        // front of: the one local file the files column handed over, or the
        // blank page the host makes for itself. Everything else that seat could
        // be asked to load — a link in that document, a redirect, a script —
        // is a navigation *away* from the thing that was minted, and the
        // allow-list is not the right question about it: the allow-list says
        // which addresses this product will browse to, and browsing is what a
        // seat with no mint does. Letting a refused mint fall through to it
        // meant a local page could walk the seat onto the network, carrying
        // whatever the local document's origin had reached.
        //
        // So the mint is the whole answer while there is one. `Mint::Nothing`
        // is the resting state of an ordinary browsing seat and falls through
        // exactly as it always did — which is why the host installs
        // `Mint::Nothing` before it navigates to an ordinary address.
        Origin::NavigationStarting(mint) => {
            if let Some(url) = mint.admits(trimmed) {
                return Decision::Navigate(url);
            }
            if *mint != Mint::Nothing {
                // The allow-list is still asked, but only so that the card can
                // name the reason it already had words for: a `file:` URL is
                // refused as `FileScheme`, `about:` as `BrowserInternalScheme`,
                // and what the list would have *admitted* is refused as
                // `NotMinted` rather than navigated to. One sentence changes —
                // the one that used to be `Navigate`.
                return match check_by_scheme(trimmed, origin) {
                    Decision::Refuse(refusal) => Decision::Refuse(refusal),
                    Decision::Navigate(_) | Decision::Search(_) => {
                        Decision::Refuse(Refusal::NotMinted)
                    }
                };
            }
        }
        Origin::AddressBar => {}
    }
    check_by_scheme(trimmed, origin)
}

/// The allow-list half of [`check`] — what an address's own text says, for the
/// doors that have nothing else to go on.
///
/// Split out so that [`check`]'s mint arm can name it rather than fall into it:
/// a fall-through is a control-flow accident waiting to be read as a decision,
/// and R1-16 is what that reading cost. Here the two callers are visible.
fn check_by_scheme(trimmed: &str, origin: Origin<'_>) -> Decision {
    match split_scheme(trimmed) {
        Some((scheme, rest)) => {
            if let Err(refusal) = classify_scheme(&scheme) {
                return Decision::Refuse(refusal);
            }
            if trimmed.contains(char::is_whitespace) {
                return Decision::Refuse(Refusal::ControlOrWhitespace);
            }
            if !rest.starts_with("//") {
                // §7.1.5g's `http(s)` arm reads "must start with `//`", and it
                // is the same sentence here: `http:8080` names a scheme and
                // then no host at all.
                return Decision::Refuse(Refusal::NoHost);
            }
            let authority = authority(rest);
            if authority.contains('@') {
                // `@` anywhere in the authority is userinfo. A bare `@` in a
                // path or a query is ordinary and never reaches here.
                return Decision::Refuse(Refusal::UserInfo);
            }
            if authority.is_empty() {
                return Decision::Refuse(Refusal::NoHost);
            }
            Decision::Navigate(rewrite_unspecified_host(trimmed))
        }
        None => match origin {
            // A navigation target without a scheme never comes out of the
            // engine, and a search is not something a redirect can ask for.
            Origin::NavigationStarting(_) | Origin::HostMinted(_) => {
                Decision::Refuse(Refusal::ExternalScheme)
            }
            Origin::AddressBar => {
                if trimmed.contains(char::is_whitespace) {
                    return Decision::Search(trimmed.to_owned());
                }
                let (host, _) = split_host_port(authority(trimmed));
                if is_loopback_host(&host) || (host.contains('.') && !host.ends_with('.')) {
                    Decision::Navigate(rewrite_unspecified_host(&format!("http://{trimmed}")))
                } else {
                    Decision::Search(trimmed.to_owned())
                }
            }
        },
    }
}

/// The address bar's door, which is also every pin's, every switcher row's and
/// every restored session's.
pub fn address_bar(input: &str) -> Decision {
    check(input, Origin::AddressBar)
}

/// The `NavigationStarting` door — **the same rule, asked again** — with the
/// seat's mint as the one thing that can widen it.
///
/// This is the seam 片① calls: `candidate` is `ICoreWebView2NavigationStartingEventArgs::get_Uri`,
/// `mint` is what this seat last minted for itself, and the verdict says either
/// what to navigate to (cancel and restart if it differs from `candidate`) or
/// which refusal to draw.
pub fn navigation_starting(candidate: &str, mint: &Mint) -> Decision {
    check(candidate, Origin::NavigationStarting(mint))
}

/// **The third door: everything a page asks for that is not a navigation**
/// (R1-10).
///
/// [`navigation_starting`] answers for the address the seat goes to. It is
/// asked once per top-level navigation, and a document is not one request — it
/// is a request and then every picture, stylesheet, script, font and frame it
/// names. Those never reached a door at all, so a previewed local report could
/// name `<img src="…/another/folder/secret.png">` or
/// `<iframe src="file:///C:/Users/…">` and the engine would fetch it, because
/// the only rule in this file ran on the address bar's question.
///
/// # A seat is opened on one thing, and that is what it may read
///
/// The mint says which. A seat opened on a local file may read **that file's
/// own folder and the folders under it**, and nothing else on the disk: not a
/// sibling folder, not another drive, and not a share — the product's UNC
/// refusal (DESIGN §7.1.3) reaches this door in the same words it reaches the
/// others. It may not reach the network either: a local page is a document
/// somebody opened out of the files column, and a stylesheet it pulls from a
/// server is that document telling somebody it was opened. A browsing seat is
/// the mirror image — the network is what it is for, and the disk is what it
/// may not touch.
///
/// # The three schemes that are neither
///
/// `data:` and `blob:` name bytes the page already holds, and `about:blank`
/// and `about:srcdoc` are the two empty documents a frame is made of. None of
/// them is a fetch of anything outside the document, so all four pass on every
/// seat. The navigation door goes on refusing `data:` and `about:` as
/// *locations*, which is a different question and stays answered the way it
/// was: what may be *loaded into* a document is not what a seat may be
/// *pointed at*.
///
/// # The engine's own furniture
///
/// A `.pdf` opened out of the files column is drawn by the engine's built-in
/// viewer, and that viewer is a page of the browser's own served over
/// `chrome-extension:`. So the engine's internal schemes pass here. They are
/// still refused at the navigation door, which is the door that decides where a
/// reader can be taken; this one decides what a document already on the glass
/// may be built out of, and the browser building its own viewer out of its own
/// parts is not a document reaching anywhere.
#[must_use]
pub fn resource_request(candidate: &str, mint: &Mint) -> Decision {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return Decision::Refuse(Refusal::Empty);
    }
    if trimmed.chars().any(char::is_control) {
        return Decision::Refuse(Refusal::ControlOrWhitespace);
    }
    let Some((scheme, _)) = split_scheme(trimmed) else {
        // Everything the engine hands this door carries a scheme; it resolved
        // the document's relative references before it asked anybody. A
        // candidate with none is not a request this window can reason about.
        return Decision::Refuse(Refusal::ExternalScheme);
    };
    // **Read off the four tables above and not out of a `match`'s patterns**
    // (M4-2). The order is the order the arms stood in and the answers are the
    // answers they gave; what changed is that the scheme sets are now named
    // values, because [`content_rules`] has to be built out of the same ones or
    // the two spellings of this rule drift apart.
    let scheme = scheme.as_str();
    if DOCUMENTS_OWN_BYTES.contains(&scheme) {
        return Decision::Navigate(trimmed.to_owned());
    }
    if scheme == ABOUT {
        return if is_an_empty_document(trimmed) {
            Decision::Navigate(trimmed.to_owned())
        } else {
            Decision::Refuse(Refusal::BrowserInternalScheme)
        };
    }
    // The engine's own parts, listed rather than pattern-matched: a scheme this
    // door has not been told about is refused, and adding one is a line
    // somebody types on purpose.
    if ENGINE_INTERNAL.contains(&scheme) {
        return Decision::Navigate(trimmed.to_owned());
    }
    if scheme == DISK {
        return match mint {
            Mint::File(minted)
                if LocalFileUrl::parse(trimmed)
                    .is_some_and(|candidate| candidate.is_inside_the_folder_of(minted)) =>
            {
                Decision::Navigate(trimmed.to_owned())
            }
            // A `file:` URL that names a host is a share, and it is refused
            // under the name the rest of the product refuses shares by — even
            // on a seat that has no local file at all, where the answer would
            // otherwise be the blander `FileScheme`.
            _ if names_a_file_host(trimmed) => Decision::Refuse(Refusal::NetworkPath),
            _ => Decision::Refuse(Refusal::FileScheme),
        };
    }
    if NETWORK.contains(&scheme) {
        return match mint {
            // Nothing minted is an ordinary browsing seat, and its page's own
            // subresources are the whole of what it is for.
            Mint::Nothing => Decision::Navigate(trimmed.to_owned()),
            Mint::Blank | Mint::File(_) => Decision::Refuse(Refusal::NotMinted),
        };
    }
    Decision::Refuse(match classify_scheme(scheme) {
        Err(refusal) => refusal,
        // A scheme the allow-list would have taken is still not a thing a
        // document may be built out of unless one of the arms above named it.
        // Nothing reaches here today; the arm exists so that a scheme added to
        // `classify_scheme` cannot quietly become a subresource.
        Ok(()) => Refusal::ExternalScheme,
    })
}

// ── The same sentence, compiled in advance (M4-2) ──────────────────────────

/// **The schemes that name bytes the document already holds.**
///
/// Neither is a fetch of anything outside the document, so both pass on every
/// seat and no compiled rule ever names them.
pub const DOCUMENTS_OWN_BYTES: [&str; 2] = ["data", "blob"];

/// **The two empty documents a frame is made of**, under the one scheme that
/// carries them. Which two is [`is_an_empty_document`]'s.
pub const ABOUT: &str = "about";

/// **The engine's own furniture** — the pages a browser draws its own viewers
/// with. Refused as a *location* by [`check`] and allowed as a document's
/// contents here, which is the difference between where a reader can be taken
/// and what is already on the glass.
pub const ENGINE_INTERNAL: [&str; 3] = ["chrome-extension", "chrome-untrusted", "devtools"];

/// **The disk.**
pub const DISK: &str = "file";

/// **The network.**
pub const NETWORK: [&str; 2] = ["http", "https"];

/// **One compiled rule: an address pattern, and the refusal that is the only
/// action this product ever emits.**
///
/// A type rather than a string so that [`content_rules`]'s JSON has exactly one
/// author, and so that the test which holds the JSON to
/// [`resource_request`]'s own answers can read what was emitted rather than
/// re-derive it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentRule {
    /// The pattern, in the content-blocker dialect: anchored at `^`, an
    /// optional alternation of scheme names, then the separator.
    pub url_filter: String,
}

/// **Every scheme `resource_request` refuses outright for this mint, as
/// patterns** — the list [`content_rules`] serialises.
///
/// # Why a seat needs this at all
///
/// The Windows engine asks the host about **every** request a document makes
/// (`WebResourceRequested` with a filter over every context), so
/// [`resource_request`] is called per request and this list has no reader
/// there. WKWebView has no such callback — X-2 measured a picture, a
/// stylesheet, a script and a `fetch` all reaching the far socket with no
/// delegate ever naming them — and the only door left is a list of URL patterns
/// compiled before the document loads. So the same sentence is written twice,
/// in two languages, and `the_two_spellings_of_the_resource_rule_agree` is what
/// keeps them one sentence.
///
/// # What the patterns can and cannot say, and who holds the rest
///
/// A pattern speaks about an **address**, so it can say *this seat reaches no
/// server* and *this seat reaches no disk* — the two sentences the mint turns
/// on. It cannot say *this file is inside that folder*, which is the local
/// seat's other half, and nothing here tries: that half is carried by
/// `-[WKWebView loadFileURL:allowingReadAccessToURL:]` with the minted file's
/// own folder, which X-2 measured enforcing it with no rule list at all. The
/// pair is the enforcement, and the test asserts the **pair** against
/// [`resource_request`] rather than this list alone.
#[must_use]
pub fn content_rule_list(mint: &Mint) -> Vec<ContentRule> {
    // **One rule per scheme, and no alternation in any of them.**
    //
    // `^(http|https)://` is the same sentence in one rule and is what this
    // emitted first; `WKContentRuleListStore` refused to compile it — measured
    // on the machine, M4-2's `.app` proof — because a content blocker's
    // `url-filter` is a *subset* of regular expressions and a group is not in
    // it. One rule per entry of the table is the spelling that compiles, and it
    // is the one that is generated rather than written: a scheme added to
    // [`NETWORK`] becomes another rule rather than another branch somebody has
    // to remember to add.
    let network = NETWORK.iter().map(|scheme| ContentRule {
        url_filter: format!("^{scheme}://"),
    });
    let disk = ContentRule {
        url_filter: format!("^{DISK}:"),
    };
    match mint {
        // A browsing seat: the network is what it is for, and the disk is what
        // it may not touch. WebKit refuses a `file:` subresource of an `http:`
        // document before any callback of ours could — measured — so this rule
        // is the belt beside that brace, and it is here because the pattern
        // language can say what `resource_request` says.
        Mint::Nothing => vec![disk],
        // The host's own empty page fetches nothing at all.
        Mint::Blank => network.chain(std::iter::once(disk)).collect(),
        // A local document reads its own folder — which the load's read access
        // is what grants — and reaches no server.
        Mint::File(_) => network.collect(),
    }
}

/// **The same list as Safari content-blocker JSON**, which is what
/// `WKContentRuleListStore` compiles.
///
/// One rule per line of the list above, `block` for every one of them, and
/// nothing else in it: no `resource-type`, because the sentence is about the
/// address and not about what the document wanted the bytes for, and no
/// `if-domain`, because a mint is not a domain.
///
/// Never empty. A mint whose list were empty would be a JSON document
/// `WKContentRuleListStore` refuses to compile, and a seat whose compilation
/// failed is a seat with no third door at all — so the browsing seat carries
/// its `file:` refusal rather than an empty list.
#[must_use]
pub fn content_rules(mint: &Mint) -> String {
    let mut json = String::from("[");
    for (index, rule) in content_rule_list(mint).into_iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str(r#"{"trigger":{"url-filter":""#);
        // The patterns this module writes carry neither, and an escape written
        // for a string that never needs one is the line that keeps it true.
        for character in rule.url_filter.chars() {
            match character {
                '"' => json.push_str("\\\""),
                '\\' => json.push_str("\\\\"),
                other => json.push(other),
            }
        }
        json.push_str(r#""},"action":{"type":"block"}}"#);
    }
    json.push(']');
    json
}

/// The two `about:` documents that are documents rather than destinations.
fn is_an_empty_document(candidate: &str) -> bool {
    candidate.eq_ignore_ascii_case(BLANK_PAGE) || candidate.eq_ignore_ascii_case("about:srcdoc")
}

/// Whether a `file:` URL carries an authority — `file://server/share/x`.
///
/// The one spelling this product's `file:` URLs have is `file:///` with the
/// drive letter straight after it ([`Mint::file`]), so anything between the two
/// slashes and the path is a host, and a host on a `file:` URL is a share.
fn names_a_file_host(url: &str) -> bool {
    let Some(rest) = url.get(5..) else {
        return false;
    };
    if !url[..5].eq_ignore_ascii_case("file:") {
        return false;
    }
    // `file://` then anything that is not immediately the third slash.
    rest.starts_with("//") && !rest.starts_with("///")
}

// **Retired on 2026-09-20 (issue #7), and its absence is the repair.**
//
// There were two readers of a `file:` URL in this file and they disagreed:
// `Mint::path_and_tail_of_file_url` knew four escapes because its job was to
// prove a stored string came out of this product's own door, and
// `decoded_file_path` knew every escape because its job was to read what the
// *engine* wrote. Two readers of one grammar is two grammars, and the third
// failure of the obligation ("a local seat refused its own document") was the
// seam between them: `Mint::admits` compared spellings, so a document whose
// name is in Chinese — which this door minted raw and the engine committed
// percent-encoded — was refused by the window that had just opened it.
//
// One parser now, [`LocalFileUrl`], and everything that used to compare,
// slice or decode a `file:` URL asks it instead:
// `file_url_is_inside_the_folder_of` is
// [`LocalFileUrl::is_inside_the_folder_of`], `strip_the_tail` is the tail the
// parse already split off, and `decoded_file_path` is the parse.

/// Split `input` into `(scheme, rest)` when it carries an explicit scheme.
///
/// Deliberately stricter than "find a colon": `localhost:3000` and
/// `localhost:5173/app?x=1` have a colon and are a host and a port, and a rule
/// that read them as schemes would send every dev-server address to the search
/// engine. What tells the two apart is **not punctuation** — the probe's
/// version drew the line at "everything after the colon is digits" and so read
/// `localhost:5173/app` as a scheme called `localhost` — it is that nobody has
/// ever registered `localhost:`. So a name this door recognises is a scheme,
/// and a name it does not recognise followed by a port is a host and a port.
fn split_scheme(input: &str) -> Option<(String, &str)> {
    let colon = input.find(':')?;
    let scheme = &input[..colon];
    let mut characters = scheme.chars();
    let first = characters.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    let rest = &input[colon + 1..];
    let lowered = scheme.to_ascii_lowercase();
    if known_scheme(&lowered).is_none() && looks_like_a_port(rest) {
        return None;
    }
    Some((lowered, rest))
}

/// Whether what follows a colon is a port: digits, and then the end of the
/// authority — a `/`, a `?`, a `#`, or nothing at all.
fn looks_like_a_port(rest: &str) -> bool {
    let after_digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    after_digits.len() < rest.len()
        && matches!(after_digits.chars().next(), None | Some('/' | '?' | '#'))
}

/// The scheme of an address, for a card that has to name it. `None` when the
/// text does not carry one.
pub fn scheme_of(input: &str) -> Option<String> {
    split_scheme(input.trim()).map(|(scheme, _)| scheme)
}

/// The host of an address, for the card that has to say **which name did not
/// answer** (§7.7 ④'s 「加载失败」 row). `None` when the text carries no
/// authority at all.
///
/// Here and not beside the card, for the reason `scheme_of` is here: this file
/// already splits an address four ways and a second splitter would be a second
/// answer about the same string. The port is dropped — a connection failure is
/// about the name, and the port is on the head in full.
pub fn host_of(input: &str) -> Option<String> {
    let (_, rest) = split_scheme(input.trim())?;
    let (host, _) = split_host_port(authority(rest));
    (!host.is_empty()).then_some(host)
}

/// The schemes this door has an opinion about **by name**, and what that
/// opinion is. `None` means "never heard of it", which is two things at once:
/// the catch-all refusal below, and the reason [`split_scheme`] is allowed to
/// read `name:8080` as a host and a port. One list, two readers — a second copy
/// would be a build in which `localhost` is a scheme in one function and a host
/// in the other.
fn known_scheme(scheme: &str) -> Option<Result<(), Refusal>> {
    Some(match scheme {
        "http" | "https" => Ok(()),
        "javascript" | "data" | "blob" | "vbscript" => Err(Refusal::ScriptOrInlineScheme),
        "file" => Err(Refusal::FileScheme),
        "view-source"
        | "devtools"
        | "edge"
        | "chrome"
        | "chrome-error"
        | "chrome-extension"
        | "about"
        | "ms-browser-extension" => Err(Refusal::BrowserInternalScheme),
        _ => return None,
    })
}

fn classify_scheme(scheme: &str) -> Result<(), Refusal> {
    // Everything else is a protocol the shell would launch, and those are
    // refused outright rather than confirmed (DESIGN §7.1.1, §7.1.5g ⑤).
    known_scheme(scheme).unwrap_or(Err(Refusal::ExternalScheme))
}

/// The authority of an `http(s)` URL: everything between `//` and the first
/// `/`, `?` or `#`.
fn authority(rest_after_scheme: &str) -> &str {
    let rest = rest_after_scheme
        .strip_prefix("//")
        .unwrap_or(rest_after_scheme);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    &rest[..end]
}

/// Host and port from an authority, with IPv6 brackets stripped.
fn split_host_port(authority: &str) -> (String, Option<String>) {
    if let Some(rest) = authority.strip_prefix('[')
        && let Some(close) = rest.find(']')
    {
        let host = rest[..close].to_ascii_lowercase();
        let port = rest[close + 1..].strip_prefix(':').map(str::to_owned);
        return (host, port);
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host.to_ascii_lowercase(), Some(port.to_owned()))
        }
        _ => (authority.to_ascii_lowercase(), None),
    }
}

/// Whether a host is loopback **by syntax alone**.
///
/// No DNS, no `hosts` file, no connect. §3's first sentence, and the reason for
/// it: the preview's default destination is a promise about a string somebody
/// read, and resolving it would make the same printed address open in two
/// different places on two different days.
pub fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if host == "::1" || host == "0:0:0:0:0:0:0:1" {
        return true;
    }
    if host == "0.0.0.0" {
        return true;
    }
    let octets: Vec<&str> = host.split('.').collect();
    octets.len() == 4
        && octets
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        && octets[0].parse::<u32>().is_ok_and(|first| first == 127)
        && octets[1..]
            .iter()
            .all(|part| part.parse::<u32>().is_ok_and(|value| value <= 255))
}

// **`opens_in_preview_by_default` was retired on 2026-08-29, and its absence is
// the ruling.** It answered "which address does a link clicked in the terminal
// open in the preview without being asked twice" with "the loopback ones", from
// the plan's §3 — and it was never wired to anything: the arm it was written for
// (§7.1.5g's `http(s)` plain half) still returned `None` for every address alike,
// so a dev server's own URL did not open on a click either, and the predicate sat
// here stating a default nothing consulted.
//
// The user's report of 2026-08-29 settled the row the other way, and it settled
// it for every host: a plain click opens the address on this tab's seat, `Ctrl`
// hands it to the machine. Keeping a public predicate that says loopback is
// special would leave two written rules about one gesture, which is the thing
// this module exists to prevent — and the loopback/elsewhere distinction it drew
// still lives where it is load-bearing, in [`is_loopback_host`], which decides
// what a *page* does with its own links.
//
// What was never this predicate's question, and still is not: **whether an
// address may load**. That is [`check`]'s, it is asked of every address at every
// door, and the test below is the half of the old one that outlived it.

/// `0.0.0.0` is a bind address, not a destination — what a dev server prints
/// about itself, not somewhere to go. Rewrite it, and keep the port, path,
/// query and fragment exactly as they were.
pub fn rewrite_unspecified_host(url: &str) -> String {
    let Some((scheme, rest)) = split_scheme(url) else {
        return url.to_owned();
    };
    let body = rest.strip_prefix("//").unwrap_or(rest);
    let end = body.find(['/', '?', '#']).unwrap_or(body.len());
    let (host, port) = split_host_port(&body[..end]);
    if host != "0.0.0.0" {
        return url.to_owned();
    }
    let tail = &body[end..];
    match port {
        Some(port) => format!("{scheme}://127.0.0.1:{port}{tail}"),
        None => format!("{scheme}://127.0.0.1{tail}"),
    }
}

/// The preview switcher's identity for a URL: normalised, with query and
/// fragment **participating** (§3 「去重键 = 规范化后的完整 URL」).
///
/// Query and fragment are part of identity because they are part of what was
/// asked for, and §3 decided the matching privacy clause in the same breath:
/// they are persisted verbatim into `session.json` and `pins.json`, in the
/// clear, tokens and all. Only a default port is dropped, because `:443` on an
/// `https` URL is the same row wearing a hat.
pub fn switcher_key(url: &str) -> String {
    let Some((scheme, rest)) = split_scheme(url) else {
        return url.to_owned();
    };
    let body = rest.strip_prefix("//").unwrap_or(rest);
    let end = body.find(['/', '?', '#']).unwrap_or(body.len());
    let (host, port) = split_host_port(&body[..end]);
    let tail = &body[end..];
    let kept_port = match (scheme.as_str(), port.as_deref()) {
        ("http", Some("80")) | ("https", Some("443")) => None,
        (_, port) => port,
    };
    match kept_port {
        Some(port) => format!("{scheme}://{host}:{port}{tail}"),
        None => format!("{scheme}://{host}{tail}"),
    }
}

/// What a seat is remembered as, after redirects.
///
/// §3: 「重定向后以**最后一次成功提交的 URL** 为身份」. A seat that has never
/// committed anything has no identity — remembering the URL that was *asked*
/// for would put a page that never existed into the switcher, and §4 already
/// says a failed navigation does not overwrite the recoverable URL.
pub fn switcher_identity(last_committed: Option<&str>) -> Option<String> {
    last_committed.map(switcher_key)
}

/// **What a page is called where nobody has its title** — its site, `host[:port]`.
///
/// A page's name is the page's title (`docs/DESIGN.md` §7.7 ②), and the two
/// surfaces that have no title to read are exactly the two that stand for a page
/// nothing has opened: a Recent row (the vault stores places, never names) and a
/// pinned URL with no buffer behind it. §7.7 ③ already names the half of a URL
/// that is its identity — "scheme 与 host 是身份" — and this is that half,
/// through the *same* splitters `switcher_key` normalises with, so a row cannot
/// name a site the key does not agree it is.
///
/// A string this module cannot read as a URL answers with itself. That is not a
/// fallback but the boundary: `pins.json` is a file a person may edit, and a row
/// whose target is not a URL is drawn as what it says — it is refused at the
/// navigation gate, not silently renamed here.
pub fn site_label(url: &str) -> String {
    let Some((_, rest)) = split_scheme(url) else {
        return url.to_owned();
    };
    let (host, port) = split_host_port(authority(rest));
    if host.is_empty() {
        return url.to_owned();
    }
    match port {
        Some(port) => format!("{host}:{port}"),
        None => host,
    }
}

/// **Whose icon this is** — `scheme://host[:port]`, the key a favicon is filed
/// under (§7.7 ②, the favicon slice, `docs/DESIGN.md` §7.13).
///
/// A favicon belongs to a *site* and not to a page: `/`, `/docs` and
/// `/docs?q=1` on one server wear one icon, and the engine only re-announces it
/// when it actually changes. So the store this feeds is keyed by the part of an
/// address that names the server, which is the same half [`site_label`] shows a
/// reader — with the **scheme kept**, because `http://localhost:8642` and
/// `https://localhost:8642` are two servers and the store must not hand one of
/// them the other's icon.
///
/// The default port is dropped exactly where [`switcher_key`] drops it, and for
/// its reason: `:443` on an `https` URL is the same site wearing a hat, and a
/// build in which the switcher row and the icon disagree about that is a build
/// where one row draws a globe beside another row's favicon for one server.
///
/// `None` — rather than the string itself, which is what `site_label` answers —
/// for anything this module cannot read as a URL. `site_label` has a row to
/// fill and must print *something*; this has a cache to key, and a key made out
/// of a string nobody could parse would file an icon under a name no lookup
/// will ever be able to spell again.
pub fn site_key(url: &str) -> Option<String> {
    let (scheme, rest) = split_scheme(url.trim())?;
    let (host, port) = split_host_port(authority(rest));
    if host.is_empty() {
        return None;
    }
    let kept_port = match (scheme.as_str(), port.as_deref()) {
        ("http", Some("80")) | ("https", Some("443")) => None,
        (_, port) => port,
    };
    Some(match kept_port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    })
}

/// **The third door, held without a browser** (R1-10).
///
/// The rule is a function of two strings, so the whole of it can be shot at
/// here and the engine-side registration has nothing left to decide.
#[cfg(test)]
mod resource_gate_tests {
    use super::*;

    /// The seat a reader opens out of the files column: one report, in one
    /// folder, with a second folder beside it that has nothing to do with it.
    fn a_report() -> Mint {
        Mint::file(Path::new(r"D:\tmp\page\report.html")).expect("a local path mints")
    }

    /// RED — **a picture in a previewed page cannot come from outside the page's
    /// own folder** (R1-10, and the whole of why this door exists).
    ///
    /// The review's reproduction, as strings: a local `.html` under one
    /// temporary folder naming an image under a second one. Before this door
    /// existed the engine fetched it, because `NavigationStarting` is asked
    /// about the document and about nothing the document contains.
    ///
    /// The share is spelled here and is **not** the stand-in the runtime probe
    /// uses: a test that actually ran would be a machine reaching for somebody
    /// else's server, so the reachable case a probe can run is the second
    /// folder, and the share is held here where it is only ever text.
    ///
    /// RED GATE: answer `Decision::Navigate` for every `file:` candidate —
    /// which is what a seat with no resource door does — and every assertion
    /// below fails.
    #[test]
    fn a_previewed_page_may_not_pull_a_file_from_outside_its_own_folder() {
        for outside in [
            // The stand-in the runtime probe uses: a real path this machine has,
            // in a folder the page was not opened in.
            "file:///D:/tmp/other/secret.png",
            // One directory up, which is the same sentence said with a shorter
            // path.
            "file:///D:/tmp/secret.png",
            // Another drive.
            "file:///C:/Windows/win.ini",
            // The share, refused by the name every other door in this product
            // refuses shares by.
            "file://attacker/share/x.png",
            // The same walk written with escapes, so that the rule is about
            // paths and not about the text of a prefix.
            "file:///D:/tmp/page/%2E%2E/other/secret.png",
            // And the folder next door whose name begins with this one's.
            "file:///D:/tmp/pageant/secret.png",
        ] {
            assert!(
                matches!(
                    resource_request(outside, &a_report()),
                    Decision::Refuse(Refusal::FileScheme | Refusal::NetworkPath)
                ),
                "a page in D:\\tmp\\page reached {outside}"
            );
        }
    }

    /// RED — **and it may still read the folder it was opened in.**
    ///
    /// The other half of the same rule, because a door that refused everything
    /// would pass the test above and show a report with no pictures in it.
    #[test]
    fn a_previewed_page_reads_its_own_folder_and_the_folders_under_it() {
        for inside in [
            "file:///D:/tmp/page/report.html",
            "file:///D:/tmp/page/images/figure-1.png",
            "file:///D:/tmp/page/style.css",
            // A name with a space and a name with characters outside ASCII, both
            // as the engine percent-encodes them.
            "file:///D:/tmp/page/my%20notes.css",
            "file:///D:/tmp/page/%E5%9B%BE.png",
            // The document's own fragment and query ride along.
            "file:///D:/tmp/page/report.html#ch3",
        ] {
            assert!(
                matches!(resource_request(inside, &a_report()), Decision::Navigate(_)),
                "the page could not read {inside}, which is beside it"
            );
        }
    }

    /// RED — **a local page reaches no server** (R1-10's second half).
    ///
    /// A stylesheet or a script pulled from a host is the previewed document
    /// telling somebody it was opened, which is the one thing a file a person
    /// chose out of their own disk must not be able to do.
    #[test]
    fn a_previewed_local_page_reaches_no_server() {
        for outward in [
            "https://cdn.example.com/style.css",
            "http://127.0.0.1:9/beacon.gif",
        ] {
            assert_eq!(
                resource_request(outward, &a_report()),
                Decision::Refuse(Refusal::NotMinted),
                "{outward}"
            );
        }
    }

    /// RED — **and a browsing seat reaches no disk.**
    ///
    /// The mirror image, and the reason the rule is one function: a page from a
    /// server naming `file:///C:/Users/…` is the same defect read the other way
    /// round.
    #[test]
    fn a_browsing_seat_reaches_no_file_at_all() {
        assert_eq!(
            resource_request("file:///C:/Windows/win.ini", &Mint::Nothing),
            Decision::Refuse(Refusal::FileScheme),
        );
        assert_eq!(
            resource_request("file://attacker/share/x.png", &Mint::Nothing),
            Decision::Refuse(Refusal::NetworkPath),
        );
        assert!(matches!(
            resource_request("https://example.com/app.js", &Mint::Nothing),
            Decision::Navigate(_)
        ));
    }

    /// The bytes a document is built out of rather than fetched from: inline
    /// content, memory inside the page, the two empty documents a frame is made
    /// of, and the parts the engine builds its own PDF viewer out of. All four
    /// pass on a local seat, which is what keeps a previewed `.pdf` drawn and an
    /// `<iframe srcdoc>` filled.
    #[test]
    fn what_a_document_already_holds_is_not_a_fetch() {
        for held in [
            "data:image/png;base64,iVBORw0KGgo=",
            "blob:https://example.com/1234",
            "about:blank",
            "about:srcdoc",
            "chrome-extension://mhjfbmdgcfjbbpaeojofohoefgiehjai/index.html",
        ] {
            assert!(
                matches!(resource_request(held, &a_report()), Decision::Navigate(_)),
                "{held}"
            );
        }
    }

    /// Every other scheme keeps the refusal the address bar already gives it, so
    /// this door adds no vocabulary and invents no verdict.
    #[test]
    fn every_other_scheme_keeps_the_refusal_it_already_had() {
        for (candidate, refusal) in [
            ("javascript:fetch('/x')", Refusal::ScriptOrInlineScheme),
            ("mailto:someone@example.com", Refusal::ExternalScheme),
            (
                "view-source:https://example.com",
                Refusal::BrowserInternalScheme,
            ),
            ("about:history", Refusal::BrowserInternalScheme),
            ("", Refusal::Empty),
        ] {
            assert_eq!(
                resource_request(candidate, &Mint::Nothing),
                Decision::Refuse(refusal),
                "{candidate}"
            );
        }
    }
}

/// **The compiled spelling of the third door, held to the spoken one** (M4-2,
/// `docs/DESIGN.md` §13.29).
///
/// The engine on one platform asks [`resource_request`] about every request a
/// document makes; the engine on the other never asks at all and takes a list
/// of patterns compiled in advance ([`content_rules`]). Two spellings of one
/// rule is two rules the day somebody edits one of them, so every case X-2
/// measured on the machine is asked of **both** here, on every platform, with
/// no WebKit in the room.
#[cfg(test)]
mod content_rule_tests {
    use super::*;

    /// The local seat, as a Windows path — the shape [`Mint::file`] writes.
    fn a_report() -> Mint {
        Mint::file(Path::new(r"D:\seat\open\report.html")).expect("a local path mints")
    }

    /// **The emitted pattern, read rather than re-derived.**
    ///
    /// The whole dialect this module writes: `^`, an optional parenthesised
    /// alternation of scheme names, and then the separator the scheme is
    /// followed by. Anything else is a pattern somebody added without teaching
    /// this reader about it, and the panic says so rather than answering
    /// `false` — a filter nobody can evaluate must not read as a filter that
    /// blocks nothing.
    fn blocks(filter: &str, candidate: &str) -> bool {
        let body = filter
            .strip_prefix('^')
            .unwrap_or_else(|| panic!("`{filter}` is not anchored"));
        let cut = body
            .find(':')
            .unwrap_or_else(|| panic!("`{filter}` names no scheme"));
        let (scheme, separator) = body.split_at(cut);
        assert!(
            scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "`{filter}` carries a pattern this reader does not understand"
        );
        // A content blocker's `url-filter` is case-insensitive unless the rule
        // says otherwise, and none of these does.
        let head = format!("{scheme}{separator}");
        candidate.len() >= head.len() && candidate[..head.len()].eq_ignore_ascii_case(&head)
    }

    /// The other half of the local seat's enforcement:
    /// `-[WKWebView loadFileURL:allowingReadAccessToURL:]`, which admits the
    /// minted file's own folder and refuses every other path on the disk. X-2
    /// measured it doing exactly this with no rule list in the room.
    fn outside_the_read_access(mint: &Mint, candidate: &str) -> bool {
        match mint {
            // The scheme is asked of the splitter rather than sliced off the
            // front, and the reach is asked of the parsed URL rather than of
            // its text — a candidate this door cannot read as one local path is
            // outside every read access there is.
            Mint::File(minted) => {
                split_scheme(candidate).is_some_and(|(scheme, _)| scheme == DISK)
                    && !LocalFileUrl::parse(candidate)
                        .is_some_and(|candidate| candidate.is_inside_the_folder_of(minted))
            }
            // A seat that opened no file was granted no read access, so this
            // half refuses nothing and the patterns carry the whole sentence.
            Mint::Nothing | Mint::Blank => false,
        }
    }

    /// RED — **the compiled rules and the spoken rule refuse the same things**,
    /// case by case, over X-2's own fixture set.
    ///
    /// Each row is a line of the probe report's table
    /// (`docs/plans/port/probe-x2-wkwebview-policy-2026-09-12.md`): the
    /// cross-origin picture, stylesheet, script and `fetch`; the frame; the
    /// `file:` subresource of a browsing page; the local seat's own folder, the
    /// sibling folder it may not read, and the server it may not reach; the
    /// share; and the three schemes that are a document's own bytes rather than
    /// a fetch.
    ///
    /// What is compared is the **pair** — the patterns plus the load's read
    /// access — because that pair is what stands on the machine. Comparing the
    /// patterns alone would demand that a rule list say a thing the pattern
    /// language cannot say.
    ///
    /// RED GATE: drop the `file:` rule from [`Mint::Nothing`]'s list, or the
    /// network rule from a mint that has one, and the row that rule was written
    /// for names itself.
    #[test]
    fn the_two_spellings_of_the_resource_rule_agree() {
        let fixtures: [(&str, Mint); 21] = [
            // A browsing seat: its own origins are its business.
            ("http://127.0.0.1:9002/img.png", Mint::Nothing),
            ("http://127.0.0.1:9002/style.css", Mint::Nothing),
            ("http://127.0.0.1:9002/third.js", Mint::Nothing),
            ("http://127.0.0.1:9002/fetched.txt", Mint::Nothing),
            ("http://127.0.0.1:9002/frame.html", Mint::Nothing),
            ("https://example.com/a.png", Mint::Nothing),
            // …and the disk is what it may not touch.
            ("file:///etc/hosts", Mint::Nothing),
            ("file:///D:/seat/open/inside.png", Mint::Nothing),
            ("file://server/share/x.png", Mint::Nothing),
            // The document's own bytes, on every seat.
            ("data:text/html,%3Cb%3Ehi%3C/b%3E", Mint::Nothing),
            ("blob:http://127.0.0.1:9002/9d1", Mint::Nothing),
            ("about:blank", Mint::Nothing),
            ("data:text/html,%3Cb%3Ehi%3C/b%3E", a_report()),
            // The local seat: its own folder, and the folders under it.
            ("file:///D:/seat/open/inside.png", a_report()),
            ("file:///D:/seat/open/images/plate.png", a_report()),
            ("file:///D:/seat/open/report.html#ch3", a_report()),
            // …and nothing else on the disk, and no server at all.
            ("file:///D:/seat/outside/secret.png", a_report()),
            ("file:///D:/seat/outside/secret.html", a_report()),
            ("http://127.0.0.1:9002/img.png", a_report()),
            ("https://example.com/a.png", a_report()),
            // The host's own empty page fetches nothing.
            ("http://127.0.0.1:9002/img.png", Mint::Blank),
        ];
        for (candidate, mint) in fixtures {
            let rules = content_rule_list(&mint);
            let by_pattern = rules.iter().any(|rule| blocks(&rule.url_filter, candidate));
            let by_read_access = outside_the_read_access(&mint, candidate);
            let spoken = matches!(resource_request(candidate, &mint), Decision::Refuse(_));
            assert_eq!(
                by_pattern || by_read_access,
                spoken,
                "{candidate} on {mint:?}: the compiled rules say {}, the rule says {}",
                if by_pattern || by_read_access {
                    "refuse"
                } else {
                    "allow"
                },
                if spoken { "refuse" } else { "allow" },
            );
        }
    }

    /// RED — **the patterns are made out of the tables the rule reads**, so a
    /// scheme added to one arrives in both spellings or in neither.
    ///
    /// RED GATE: write `^https?://` out by hand and add a third scheme to
    /// [`NETWORK`]; this fails while the test above still passes, because no
    /// fixture names the third one yet.
    #[test]
    fn the_patterns_name_the_schemes_the_tables_do() {
        let out_of_a_table: Vec<String> = NETWORK
            .iter()
            .map(|scheme| format!("^{scheme}://"))
            .chain(std::iter::once(format!("^{DISK}:")))
            .collect();
        assert_eq!(out_of_a_table, ["^http://", "^https://", "^file:"]);
        for mint in [Mint::Nothing, Mint::Blank, a_report()] {
            for rule in content_rule_list(&mint) {
                assert!(
                    out_of_a_table.contains(&rule.url_filter),
                    "{:?} carries a pattern out of no table: {}",
                    mint,
                    rule.url_filter
                );
            }
        }
        // **And no rule carries a group.** `WKContentRuleListStore` refuses one
        // — measured on the machine, not reasoned about — so the one-rule-per-
        // scheme shape above is a compile requirement rather than a style.
        for mint in [Mint::Nothing, Mint::Blank, a_report()] {
            for rule in content_rule_list(&mint) {
                assert!(
                    !rule.url_filter.contains('(') && !rule.url_filter.contains('|'),
                    "{} is a pattern a content blocker will not compile",
                    rule.url_filter
                );
            }
        }
    }

    /// RED — **the JSON is what a content blocker reads, and no mint's is
    /// empty.**
    ///
    /// `WKContentRuleListStore` refuses an empty list, and a seat whose
    /// compilation failed has no third door at all — so "the browsing seat has
    /// nothing to block" would be a browsing seat with no gate on its
    /// subresources whatsoever.
    ///
    /// RED GATE: answer `"[]"` for [`Mint::Nothing`].
    #[test]
    fn every_mint_compiles_to_a_rule_list_with_something_in_it() {
        assert_eq!(
            content_rules(&Mint::Nothing),
            r#"[{"trigger":{"url-filter":"^file:"},"action":{"type":"block"}}]"#
        );
        assert_eq!(
            content_rules(&a_report()),
            concat!(
                r#"[{"trigger":{"url-filter":"^http://"},"action":{"type":"block"}},"#,
                r#"{"trigger":{"url-filter":"^https://"},"action":{"type":"block"}}]"#
            )
        );
        assert_eq!(
            content_rules(&Mint::Blank),
            concat!(
                r#"[{"trigger":{"url-filter":"^http://"},"action":{"type":"block"}},"#,
                r#"{"trigger":{"url-filter":"^https://"},"action":{"type":"block"}},"#,
                r#"{"trigger":{"url-filter":"^file:"},"action":{"type":"block"}}]"#
            )
        );
        for mint in [Mint::Nothing, Mint::Blank, a_report()] {
            let json = content_rules(&mint);
            assert!(json.starts_with('[') && json.ends_with(']'), "{json}");
            assert!(json.len() > 2, "{mint:?} compiles to an empty list");
            assert_eq!(
                json.matches(r#""type":"block""#).count(),
                content_rule_list(&mint).len(),
                "{json}"
            );
        }
    }

    /// RED — **the reader this file measures with refuses a pattern it does not
    /// understand**, rather than reading it as a filter that blocks nothing.
    #[test]
    #[should_panic(expected = "is not anchored")]
    fn an_unanchored_pattern_is_not_silently_evaluated() {
        blocks("http://", "http://example.com/");
    }

    /// RED — **a minted `file:` URL carries three slashes, on a machine whose
    /// absolute paths begin with one and on a machine whose do not** (M4-2).
    ///
    /// The defect this pins is not hypothetical: `file:///` plus
    /// `/Users/somebody/report.html` is `file:////Users/…`, which every engine
    /// normalises back to three slashes — so the string the seat minted matched
    /// neither the address the engine committed nor any candidate the folder
    /// rule was asked about, and a local page on a Mac refused itself.
    ///
    /// RED GATE: put the third slash back into the literal and the second row
    /// fails; take it out of the `push` and the first does.
    #[test]
    fn a_minted_file_url_has_one_root_however_the_path_spelled_it() {
        let windows = Mint::file(Path::new(r"D:\seat\open\report.html")).expect("a drive path");
        assert_eq!(
            windows.target(),
            Some("file:///D:/seat/open/report.html"),
            "the spelling this product has always minted on Windows"
        );
        let posix =
            Mint::file(Path::new("/Users/somebody/seat/open/report.html")).expect("a rooted path");
        assert_eq!(
            posix.target(),
            Some("file:///Users/somebody/seat/open/report.html")
        );
        // And the two doors that compare one against the other agree with it,
        // which is the whole of what the slash count costs.
        assert_eq!(
            navigation_starting("file:///Users/somebody/seat/open/report.html", &posix),
            Decision::Navigate(String::from("file:///Users/somebody/seat/open/report.html"))
        );
        assert!(matches!(
            resource_request("file:///Users/somebody/seat/open/inside.png", &posix),
            Decision::Navigate(_)
        ));
        assert!(matches!(
            resource_request("file:///Users/somebody/seat/outside/secret.png", &posix),
            Decision::Refuse(_)
        ));
    }
}

/// **A `file:` URL is the path it names** (issue #7, 2026-09-20).
///
/// The evidence these tables are built out of is a measurement, not a guess:
/// `scratchpad/pdf-vhost-spike/REPORT.md` part 2 recorded 17 (passed, reported)
/// pairs from a standalone WebView2 host on runtime 153.0.4234.48, with the
/// URLs minted by a byte-for-byte copy of [`Mint::file`] and handed to
/// `Navigate` raw. The fixture table below is those pairs with the machine's own
/// directory replaced by a synthetic one; the file names are the spike's and
/// carry nobody's data.
#[cfg(test)]
mod file_url_tests {
    use super::*;

    /// The folder the spike's files sat in, shortened.
    const ROOT: &str = "file:///D:/spike/docroot/";

    /// One recorded pair: what was passed to `Navigate`, and what
    /// `NavigationStarting` reported back.
    fn pair(passed: &str, reported: &str) -> (String, String) {
        (format!("{ROOT}{passed}"), format!("{ROOT}{reported}"))
    }

    /// The seven `file:` rows of the spike's table.
    fn recorded() -> [(String, String); 7] {
        [
            // F-a — issue #7's own shape: Chinese, spaces, parentheses.
            pair(
                "文档%20测试%20V1.1%20(1).pdf",
                "%E6%96%87%E6%A1%A3%20%E6%B5%8B%E8%AF%95%20V1.1%20(1).pdf",
            ),
            // F-b — ASCII only; the engine changed nothing.
            pair("plain%20(1).pdf", "plain%20(1).pdf"),
            // F-c1 — `{`, `}` and `^` are outside the engine's safe set; `[`,
            // `]` and `~` are inside it.
            pair(
                "sym-a%20[b]%20{c}%20^d%20~e.pdf",
                "sym-a%20[b]%20%7Bc%7D%20%5Ed%20~e.pdf",
            ),
            // F-c2 — nine symbols the engine leaves raw.
            pair(
                "sym-b%20'q'%20&a%20+p%20,c%20;s%20=e%20@a%20!b%20$d.pdf",
                "sym-b%20'q'%20&a%20+p%20,c%20;s%20=e%20@a%20!b%20$d.pdf",
            ),
            // F-c3 — escapes already there are kept verbatim, not re-encoded.
            pair("sym-c%20%23h%20%25p.pdf", "sym-c%20%23h%20%25p.pdf"),
            // F-c4 — the backtick.
            pair("sym-d%20backtick`bt.pdf", "sym-d%20backtick%60bt.pdf"),
            // F-d — the same four escapes on an `.html`: not PDF-specific.
            pair(
                "文档%20测试%20page.html",
                "%E6%96%87%E6%A1%A3%20%E6%B5%8B%E8%AF%95%20page.html",
            ),
        ]
    }

    /// RED — **the seat admits its own document, in the spelling the engine
    /// gives it back** (issue #7; A1).
    ///
    /// On `main` this fails on F-a, F-c1, F-c4 and F-d — the four rows where the
    /// engine re-spelled something — with `admitted its own document` naming the
    /// row. Those four are exactly the reported defect: a user whose report
    /// lives under `D:\文档\` clicked it and Folio drew its own refusal card.
    ///
    /// RED GATE: compare the two strings with `eq_ignore_ascii_case` again, as
    /// `Mint::admits` did until this change, and the same four rows fail.
    #[test]
    fn the_engines_own_spelling_of_a_minted_file_is_the_minted_file() {
        for (passed, reported) in recorded() {
            let minted =
                Mint::File(LocalFileUrl::parse(&passed).expect("the spike passed a local path"));
            assert_eq!(
                navigation_starting(&reported, &minted),
                Decision::Navigate(reported.clone()),
                "the seat refused its own document: passed {passed}, reported {reported}"
            );
            // And so does the door the document's pictures come through.
            assert!(
                matches!(resource_request(&reported, &minted), Decision::Navigate(_)),
                "the document could not read itself: {reported}"
            );
        }
        // F-e — the drive letter, which the engine upper-cases and the old
        // comparison hid behind `eq_ignore_ascii_case`. The parser answers it
        // rather than hiding it: the path it reads back is upper-cased.
        let lower = Mint::file(Path::new(r"c:\spike\docroot\plain (1).pdf")).expect("a local path");
        assert_eq!(
            navigation_starting("file:///C:/spike/docroot/plain%20(1).pdf", &lower),
            Decision::Navigate("file:///C:/spike/docroot/plain%20(1).pdf".to_owned())
        );
        assert_eq!(
            local_path_form(lower.target().expect("a minted URL")).as_deref(),
            Some(r"C:\spike\docroot\plain (1).pdf")
        );
    }

    /// RED — **and nothing else becomes admitted** (A2, the security table).
    ///
    /// Every row of it is a thing that is refused on `main` and must stay
    /// refused: a traversal in any spelling, a sibling outside the folder, a
    /// share, a candidate that matches only after a lossy decode, a NUL, a
    /// separator smuggled in as an escape, and the two Windows aliases.
    ///
    /// RED GATE ①: decode with `String::from_utf8_lossy` and the `%FF` rows go
    /// green — a different file admitted under the minted file's name. RED GATE
    /// ②: fold the `.` and `..` check to before the decoding and every `%2e`
    /// row goes green. RED GATE ③: strip trailing dots and spaces to make the
    /// Windows aliases compare equal and the last block goes green on a machine
    /// where those are three different files.
    #[test]
    fn the_gate_admits_nothing_it_did_not_admit_before() {
        let minted = Mint::file(Path::new(r"D:\tmp\page\report.html")).expect("a local page");
        for refused in [
            // ① traversal, in every spelling
            "file:///D:/tmp/page/../other/secret.png",
            "file:///D:/tmp/page/%2e%2e/other/secret.png",
            "file:///D:/tmp/page/%2E%2E/other/secret.png",
            "file:///D:/tmp/page/.%2e/other/secret.png",
            "file:///D:/tmp/page/./report.html",
            // ② a sibling outside the minted folder
            "file:///D:/tmp/other/secret.png",
            "file:///D:/tmp/secret.png",
            "file:///C:/Windows/win.ini",
            "file:///D:/tmp/pageant/secret.png",
            // ③ a share, spelled as a host on a `file:` URL
            "file://server/share/x.png",
            "file://localhost/D:/tmp/page/report.html",
            // ④ equal to the mint only after a lossy decode
            "file:///D:/tmp/page/report%FF.html",
            "file:///D:/tmp/page/%FFreport.html",
            // ⑤ NUL, and the rest of the control characters with it
            "file:///D:/tmp/page/report%00.html",
            "file:///D:/tmp/page/report%0A.html",
            // ⑥ a separator smuggled in as an escape
            "file:///D:/tmp/page/%5C../secret.png",
            "file:///D:%5Ctmp%5Cpage%5Creport.html",
            // ⑦ a half-written escape
            "file:///D:/tmp/page/rep%rt.html",
            "file:///D:/tmp/page/report.html%2",
            // ⑧ nothing at all
            "file:///",
            "file:///D:",
            "",
        ] {
            // Every one of these is refused as `FileScheme` — the allow-list's
            // own word for the disk — because the mint did not admit it and the
            // list refuses `file:` from every door. The empty string never
            // reaches that far.
            let expected = if refused.is_empty() {
                Refusal::Empty
            } else {
                Refusal::FileScheme
            };
            assert_eq!(
                navigation_starting(refused, &minted),
                Decision::Refuse(expected),
                "the gate admitted {refused}"
            );
            assert!(
                matches!(
                    resource_request(refused, &minted),
                    Decision::Refuse(Refusal::FileScheme | Refusal::NetworkPath | Refusal::Empty)
                ),
                "the document read {refused}"
            );
        }

        // ⑨ **Double encoding is one decode, and one decode is what the disk
        // sees.** `%252e` is the two-character *name* `%2e` — the disk resolves
        // no traversal out of it and neither does this — so it names a child of
        // the folder it was written in and can never leave it.
        assert!(matches!(
            resource_request("file:///D:/tmp/%252e%252e/other/secret.png", &minted),
            Decision::Refuse(_)
        ));
        assert_eq!(
            LocalFileUrl::parse("file:///D:/tmp/page/%252e%252e/x.png")
                .expect("a name, not a traversal")
                .path(),
            Path::new(r"D:\tmp\page\%2e%2e\x.png")
        );

        // ⑩ **A `file:` URL typed into the address field is still refused**, by
        // the ruling that has always said so and not by anything here.
        for typed in [
            "file:///D:/tmp/page/report.html",
            "FILE:///D:/tmp/page/report.html",
            "file://server/share/x.png",
        ] {
            assert_eq!(
                address_bar(typed),
                Decision::Refuse(Refusal::FileScheme),
                "{typed}"
            );
        }

        // ⑪ **The Windows trailing-dot and trailing-space aliases.** Windows
        // opens all three of these as `report.html`; the other machine opens
        // three different files. They are refused — which is what `main` did,
        // and the direction a gate is allowed to be wrong in.
        for alias in [
            "file:///D:/tmp/page/report.html.",
            "file:///D:/tmp/page/report.html%20",
            "file:///D:/tmp/page/report.html%2E",
        ] {
            assert_eq!(
                navigation_starting(alias, &minted),
                Decision::Refuse(Refusal::FileScheme),
                "the gate took the Windows alias {alias} for the file it minted"
            );
        }

        // ⑫ …and a mixed-case scheme is the same scheme it always was, on the
        // door that has a mint to compare against.
        assert_eq!(
            navigation_starting("FILE:///D:/tmp/page/report.html", &minted),
            Decision::Navigate("FILE:///D:/tmp/page/report.html".to_owned())
        );

        // ⑬ The other half of the rule, so that a door which refused
        // everything could not pass this test: the file, its folder and the
        // folders under it.
        for inside in [
            "file:///D:/tmp/page/report.html",
            "file:///D:/tmp/page/report.html#ch3",
            "file:///D:/tmp/page/images/figure-1.png",
            "file:///D:/tmp/page/%E5%9B%BE.png",
            "file:///D:/tmp/page/图.png",
        ] {
            assert!(
                matches!(resource_request(inside, &minted), Decision::Navigate(_)),
                "the document could not read {inside}, which is beside it"
            );
        }
    }

    /// RED — **path → URL → path is the identity**, on both roots (A5).
    ///
    /// RED GATE: drop a character from [`Mint::file`]'s escape table, or teach
    /// the parser one escape fewer, and the name that needs it comes back
    /// different.
    #[test]
    fn a_path_survives_the_round_trip_whatever_is_in_its_name() {
        let long = "n".repeat(250);
        let names = [
            "报告.pdf",
            "文档 测试 V1.1 (1).pdf",
            "a b.html",
            "hash#1.html",
            "100% done?.htm",
            "brace{a}^b`c.html",
            "sq[a](b).html",
            "🙂-page.html",
            // A combining sequence: one grapheme, two scalars, six bytes.
            "e\u{301}clair.html",
            "ünïcödé.html",
            long.as_str(),
        ];
        for name in names {
            for original in [
                format!(r"D:\文档\项目 (1)\{name}"),
                format!("/Users/somebody/项目 (1)/{name}"),
            ] {
                let Mint::File(minted) =
                    Mint::file(Path::new(&original)).expect("a local path mints")
                else {
                    panic!("`Mint::file` makes a file mint");
                };
                assert_eq!(
                    minted.path(),
                    Path::new(&original),
                    "{original} went out as {}",
                    minted.as_str()
                );
                assert_eq!(minted.tail(), "");
                // …and what is written next time is what was written this time,
                // which is what makes a `session.json` stable across a save.
                let again = Mint::file(minted.path()).expect("mints again");
                assert_eq!(again.target(), Some(minted.as_str()), "{original}");
            }
        }
    }

    /// RED — **a `session.json` written by 0.4.2 still opens its page** (A6).
    ///
    /// Two spellings of one file were persisted by that build: the raw one
    /// [`Mint::file`] writes, which is what a mint carried, and the
    /// percent-encoded one the engine committed, which is what a switcher row
    /// and a restore were keyed by. Both are read back to the same path, and
    /// what is written now is the first of them.
    #[test]
    fn a_session_written_before_this_change_still_names_its_page() {
        let committed = "file:///D:/%E6%96%87%E6%A1%A3/report.html";
        let minted = "file:///D:/文档/report.html";
        let path = Path::new(r"D:\文档\report.html");
        for stored in [committed, minted] {
            let read = LocalFileUrl::parse(stored).expect("a stored row names a path");
            assert_eq!(read.path(), path, "{stored}");
        }
        assert_eq!(
            LocalFileUrl::parse(committed).expect("reads"),
            LocalFileUrl::parse(minted).expect("reads"),
            "one file, two rows"
        );
        assert_eq!(
            Mint::file(path).expect("mints").target(),
            Some(minted),
            "and what is written now is the spelling this door has always written"
        );
    }

    /// RED — **a page opened from a Chinese path is shown as that path** (A4,
    /// the ruling of 2026-08-25 applied to the string the engine commits).
    ///
    /// RED GATE: teach the parser only the four escapes [`Mint::file`] writes,
    /// as the reader this replaced knew, and the first row comes back `None` —
    /// which on screen is the percent-encoded URI in the address row, under a
    /// globe, for a file on this disk.
    #[test]
    fn a_page_under_a_chinese_path_is_shown_as_a_path() {
        assert_eq!(
            local_path_form("file:///D:/%E7%BB%8F%E6%B5%8E/%E6%8A%A5%E5%91%8A%20V1.1%20(1).pdf")
                .as_deref(),
            Some(r"D:\经济\报告 V1.1 (1).pdf")
        );
        assert_eq!(
            local_path_form("file:///Users/somebody/%E6%96%87%E6%A1%A3/a.html#ch3").as_deref(),
            Some("/Users/somebody/文档/a.html#ch3")
        );
        // What is not one local path is still shown exactly as it arrived.
        for foreign in [
            "file://server/share/x.html",
            "http://example.com/a",
            "file:///D:/tmp/../secret.html",
        ] {
            assert_eq!(local_path_form(foreign), None, "{foreign}");
        }
    }

    /// PIN — **nothing in this crate compares the *text* of a `file:` URL**
    /// (A3; the class this change closes).
    ///
    /// Read off the crate's own sources rather than off any one file's name, so
    /// that a comparison added to a file that does not exist yet is caught too.
    /// The one line that may hold both a `file://` literal and a comparison is
    /// the parser reading its own scheme; every other question about a `file:`
    /// URL is a question about the path it names, and [`LocalFileUrl`] is where
    /// it is asked.
    ///
    /// RED GATE: put `minted.eq_ignore_ascii_case(candidate)` back into
    /// `Mint::admits` and this names the line.
    #[test]
    fn no_file_url_is_compared_as_text() {
        // Spelled in halves so that this test is not its own first offender:
        // every line below would otherwise carry the needle it is looking for.
        let a_file_url = concat!("file:", "//");
        let the_one_line = concat!(
            r#".filter(|head| head.eq_ignore_ascii_case("file:"#,
            r#"///"))"#
        );
        let mut stack = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        let mut found: Vec<String> = Vec::new();
        while let Some(directory) = stack.pop() {
            for entry in std::fs::read_dir(&directory).expect("a directory of this crate") {
                let path = entry.expect("a directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("a source file");
                for line in text.lines() {
                    let code = line.trim_start();
                    // A comment is prose about a rule and not a use of it. The
                    // test is the *start* of the line rather than the first
                    // `//` in it, because `file://` carries two slashes of its
                    // own and a cleverer reader would cut every needle in half.
                    if code.starts_with("//") || !code.contains(a_file_url) {
                        continue;
                    }
                    if [
                        "eq_ignore_ascii_case",
                        "starts_with",
                        "ends_with",
                        "strip_prefix",
                        "to_lowercase(",
                        "to_ascii_lowercase(",
                        ".contains(",
                    ]
                    .iter()
                    .any(|needle| code.contains(needle))
                    {
                        found.push(code.to_owned());
                    }
                }
            }
        }
        found.sort();
        found.dedup();
        assert_eq!(
            found,
            [the_one_line],
            "a `file:` URL is compared as text somewhere in this crate"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN (user ruling 2026-08-25) — **a local file is shown as a path and
    /// typed as a path, and the URI never stops being what is loaded.**
    ///
    /// The ruling came from two screenshots of one machine that spelled one disk
    /// two ways, so what is pinned is the round trip a reader actually makes:
    /// the row shows `D:\…`, the field is seeded with it, the door takes it
    /// back, and the string the engine is handed is the `file:` URL it always
    /// was. A real address is untouched at every step of that.
    ///
    /// MUTATIONS:
    /// ① show the URL instead of the path — the first assertion goes red, which
    ///    is the screenshot the ruling was filed with;
    /// ② let `file_url_of_local_path` answer for `http://example.com` — a real
    ///    address is minted into a `file:` URL and the page goes nowhere;
    /// ③ take a bare path at `address_bar` instead of minting at the door — the
    ///    refusal assertion goes green for the wrong reason and `D:` becomes a
    ///    scheme this product recognises.
    #[test]
    fn a_local_file_is_shown_and_typed_as_a_path_and_loaded_as_a_uri() {
        let uri = "file:///D:/Developer/notes%20and%20more.html#ch3";
        assert_eq!(
            local_path_form(uri).as_deref(),
            Some(r"D:\Developer\notes and more.html#ch3"),
            "① the row shows what this machine calls the file, fragment and all"
        );
        // ③ The door does not take that path on its own — checked rather than
        // assumed, which is what the ruling asked for.
        assert!(
            matches!(address_bar(r"D:\Developer\notes.html"), Decision::Refuse(_)),
            "a drive letter splits as an unknown scheme, so the conversion has \
             to happen before the door"
        );
        // …and the conversion puts it back where it came from.
        assert_eq!(
            file_url_of_local_path(r"D:\Developer\notes and more.html").as_deref(),
            Some("file:///D:/Developer/notes%20and%20more.html"),
            "what the field hands over is the URL the files column would mint"
        );
        assert!(matches!(
            address_bar(&file_url_of_local_path(r"D:\Developer\notes.html").expect("a mint")),
            Decision::Refuse(_) | Decision::Navigate(_)
        ));
        // ② A real address is not a path and is left alone in both directions.
        for real in [
            "http://example.com/a",
            "https://localhost:5173/app",
            "example.com",
        ] {
            assert_eq!(file_url_of_local_path(real), None, "{real} is not a path");
            assert_eq!(local_path_form(real), None, "{real} is shown as it is");
        }
        // A share is refused as a path exactly as `Mint::file` refuses it, and a
        // `file:` URL this product did not write is shown as it arrived.
        assert_eq!(file_url_of_local_path(r"\\server\share\x.html"), None);
        assert_eq!(local_path_form("file://server/share/x.html"), None);
        assert_eq!(file_url_of_local_path(r"..\x.html"), None);
    }

    /// PIN (W2 slice 5) - **the file door read backwards is the file door read
    /// forwards.**
    ///
    /// [`LocalFileUrl`] exists so that a URL this window wrote into a switcher
    /// row, a session file or a pin can be taken back to a *path* and minted
    /// again from the disk - never so that the string itself can authorise
    /// anything. Being the exact inverse of the encoder is one half of its
    /// contract; the other half, since issue #7, is that it reads **any**
    /// spelling of a local path, because the engine commits its own.
    #[test]
    fn a_minted_file_url_reads_back_as_the_path_it_was_minted_from() {
        for original in [
            r"C:\Users\x\report.html",
            r"C:\a b\p#1.html",
            r"D:\notes\100% done?.htm",
            r"C:\reports\Q3.pdf",
        ] {
            let path = Path::new(original);
            let Mint::File(url) = Mint::file(path).expect("a local path mints") else {
                panic!("`Mint::file` makes a file mint");
            };
            assert_eq!(
                LocalFileUrl::parse(url.as_str()).map(LocalFileUrl::into_path_and_tail),
                Some((PathBuf::from(original), String::new())),
                "{original}"
            );
        }

        // The tail the *page* is answerable for comes back separately, because
        // it is not part of the path and the disk must never be asked about it.
        let minted = Mint::file(Path::new(r"C:\site\report.html")).expect("mints");
        let url = minted.target().expect("a file mint names its URL");
        for (tail, spelled) in [("#chapter-3", "#chapter-3"), ("?page=2#top", "?page=2#top")] {
            assert_eq!(
                LocalFileUrl::parse(&format!("{url}{tail}")).map(LocalFileUrl::into_path_and_tail),
                Some((PathBuf::from(r"C:\site\report.html"), spelled.to_owned()))
            );
        }

        // What the *engine* writes reads back as the same path, which is the
        // whole of issue #7: an escape this door never writes is still an
        // escape, and the bytes under it are UTF-8.
        assert_eq!(
            LocalFileUrl::parse("file:///C:/site/%C3%A9.html")
                .expect("the engine's spelling is a spelling")
                .path(),
            Path::new(r"C:\site\é.html")
        );

        // And everything that is not one local path.
        for foreign in [
            "http://localhost:5173/app",
            "file://server/share/page.html",
            "file:///C:/site/../secret.html",
            "file:///Users/somebody/../secret.html",
            "file:///C:/site/%2",
            // A drive is a root and not a name. Before issue #7 this read as a
            // slash-rooted path to a file *called* `C:` — legal on the machine
            // that spells paths that way — and answered with it; one parser
            // cannot hold both readings, and the one it holds is the one that
            // refuses. A file named `C:` at the root of a Mac is not a page
            // anybody opened, and a bare drive handed to Windows is the one
            // absolute-looking string that is not absolute.
            "file:///C:",
            "file:///",
            "",
        ] {
            assert_eq!(
                LocalFileUrl::parse(foreign).map(LocalFileUrl::into_path_and_tail),
                None,
                "not one local path: {foreign}"
            );
        }
    }

    /// RED (M4-3) — **the reader roots a path the way the minter rooted it**,
    /// and neither of them asks which machine is doing the reading.
    ///
    /// M4-2 fixed the encoder for a path that is nothing but a root
    /// (`a_minted_file_url_has_one_root_however_the_path_spelled_it`) and left
    /// the decoder assuming a drive letter, so the reader answered `None` for
    /// every URL a Mac ever minted and [`local_path_form`] therefore showed the
    /// URI where the ruling of 2026-08-25 says a path goes — the carry-forward
    /// §13.29 ⑬ wrote down.
    ///
    /// **Both spellings are asked of both, on one machine**, which is the whole
    /// point: these are string questions, a session file travels, and a test that
    /// only asked about the host it happens to be running on would go green on
    /// Windows for a function that was still broken on a Mac.
    ///
    /// MUTATION: ① root the slash-rooted arm with `\` and the second assertion
    /// reads `\Users\…`; ② read `Path::is_absolute` again and every slash-rooted
    /// row answers `None` on Windows; ③ drop the `..` split and the two
    /// traversals come back as paths.
    #[test]
    fn a_file_url_reads_back_rooted_the_way_it_was_minted() {
        // Drive-rooted: unchanged, to the character.
        assert_eq!(
            local_path_form("file:///D:/Developer/notes%20and%20more.html#ch3").as_deref(),
            Some(r"D:\Developer\notes and more.html#ch3")
        );
        // Slash-rooted: the same sentence about the other kind of machine.
        assert_eq!(
            local_path_form("file:///Users/somebody/notes%20and%20more.html#ch3").as_deref(),
            Some("/Users/somebody/notes and more.html#ch3")
        );
        assert_eq!(
            LocalFileUrl::parse("file:///Users/somebody/report.html?page=2#top")
                .map(LocalFileUrl::into_path_and_tail),
            Some((
                PathBuf::from("/Users/somebody/report.html"),
                "?page=2#top".to_owned()
            ))
        );
        // A slash-rooted path at the top of its tree still has a name in it.
        assert_eq!(
            local_path_form("file:///Applications").as_deref(),
            Some("/Applications")
        );
        // And the mint of a slash-rooted path reads back as the path it was
        // made from — the round trip the drive-rooted arm has always had.
        let original = "/Users/somebody/a b/p#1.html";
        let Mint::File(url) = Mint::file(Path::new(original)).expect("a local path mints") else {
            panic!("`Mint::file` makes a file mint");
        };
        assert_eq!(url.as_str(), "file:///Users/somebody/a%20b/p%231.html");
        assert_eq!(url.path(), Path::new(original));
        assert_eq!(url.tail(), "");
    }

    // ---- the twelve carried over from the W0′ probe (w0-evidence.md) ----

    #[test]
    fn loopback_is_syntax_only() {
        for host in [
            "localhost",
            "app.localhost",
            "127.0.0.1",
            "127.1.2.3",
            "0.0.0.0",
            "::1",
        ] {
            assert!(is_loopback_host(host), "{host}");
        }
        for host in [
            "192.168.1.5",
            "10.0.0.1",
            "172.16.0.1",
            "my-desktop",
            "localhost.evil.com",
            "128.0.0.1",
        ] {
            assert!(!is_loopback_host(host), "{host}");
        }
    }

    #[test]
    fn unspecified_host_is_rewritten_keeping_everything_else() {
        assert_eq!(
            rewrite_unspecified_host("http://0.0.0.0:5173/app?x=1#top"),
            "http://127.0.0.1:5173/app?x=1#top"
        );
        assert_eq!(
            rewrite_unspecified_host("http://0.0.0.0/"),
            "http://127.0.0.1/"
        );
        assert_eq!(
            rewrite_unspecified_host("http://localhost:5173/"),
            "http://localhost:5173/"
        );
    }

    #[test]
    fn address_bar_red_matrix() {
        let rows: &[(&str, Refusal)] = &[
            ("javascript:alert(1)", Refusal::ScriptOrInlineScheme),
            ("JavaScript:alert(1)", Refusal::ScriptOrInlineScheme),
            ("data:text/html,<h1>x", Refusal::ScriptOrInlineScheme),
            (
                "blob:https://example.com/abc",
                Refusal::ScriptOrInlineScheme,
            ),
            ("vbscript:msgbox", Refusal::ScriptOrInlineScheme),
            ("file:///C:/Windows/win.ini", Refusal::FileScheme),
            (
                "view-source:https://example.com",
                Refusal::BrowserInternalScheme,
            ),
            (
                "devtools://devtools/bundled/inspector.html",
                Refusal::BrowserInternalScheme,
            ),
            ("edge://settings", Refusal::BrowserInternalScheme),
            ("chrome://version", Refusal::BrowserInternalScheme),
            ("about:blank", Refusal::BrowserInternalScheme),
            ("ftp://example.com/x", Refusal::ExternalScheme),
            ("ws://example.com/socket", Refusal::ExternalScheme),
            ("wss://example.com/socket", Refusal::ExternalScheme),
            ("mailto:someone@example.com", Refusal::ExternalScheme),
            ("tel:+1234", Refusal::ExternalScheme),
            ("https://user:pass@example.com/", Refusal::UserInfo),
            ("   ", Refusal::Empty),
        ];
        for (input, expected) in rows {
            assert_eq!(address_bar(input), Decision::Refuse(*expected), "{input}");
        }
    }

    #[test]
    fn address_bar_green_rows() {
        assert_eq!(
            address_bar("http://localhost:5173/"),
            Decision::Navigate("http://localhost:5173/".into())
        );
        assert_eq!(
            address_bar("example.com"),
            Decision::Navigate("http://example.com".into())
        );
        assert_eq!(
            address_bar("localhost:3000"),
            Decision::Navigate("http://localhost:3000".into())
        );
        assert_eq!(
            address_bar("how do i exit vim"),
            Decision::Search("how do i exit vim".into())
        );
        assert_eq!(address_bar("rustdoc"), Decision::Search("rustdoc".into()));
    }

    /// **The defect the migration found.** `localhost:5173/app` — what a dev
    /// server prints on every start, and the single most likely thing to be
    /// typed into this address bar — has a colon in it. The probe's rule
    /// ("a scheme is whatever precedes a colon, unless everything after the
    /// colon is digits") read it as a scheme named `localhost` and refused it
    /// as an external protocol; the probe never noticed because nothing ever
    /// handed it a port with a path behind it. The rule here asks a different
    /// question — is this a name this door knows — and the row above with the
    /// bare port keeps passing.
    #[test]
    fn a_host_and_port_is_not_a_scheme() {
        for (input, expected) in [
            ("localhost:3000", "http://localhost:3000"),
            (
                "localhost:5173/app?x=1#top",
                "http://localhost:5173/app?x=1#top",
            ),
            ("127.0.0.1:8080/x", "http://127.0.0.1:8080/x"),
            ("0.0.0.0:5173/app", "http://127.0.0.1:5173/app"),
            ("example.com:8443/a?b=c", "http://example.com:8443/a?b=c"),
        ] {
            assert_eq!(
                address_bar(input),
                Decision::Navigate(expected.to_owned()),
                "{input}"
            );
            assert_eq!(scheme_of(input), None, "{input}");
        }
        // A scheme this door *does* know stays a scheme even when a port is
        // the only thing after it — it simply has no host, which §7.1.5g's
        // `//` clause already said.
        assert_eq!(address_bar("http:8080"), Decision::Refuse(Refusal::NoHost));
        assert_eq!(scheme_of("http:8080").as_deref(), Some("http"));
        assert_eq!(
            address_bar("https:example.com"),
            Decision::Refuse(Refusal::NoHost)
        );
        // A name it does not know, followed by a port, is read as a host — and
        // a host nobody can reach is a search, which is not a navigation.
        assert!(matches!(address_bar("ftp:1234/x"), Decision::Search(_)));
    }

    #[test]
    fn navigation_starting_is_the_same_rule_again() {
        assert_eq!(
            navigation_starting("javascript:void(0)", &Mint::Nothing),
            Decision::Refuse(Refusal::ScriptOrInlineScheme)
        );
        assert_eq!(
            navigation_starting("file:///C:/secret.txt", &Mint::Nothing),
            Decision::Refuse(Refusal::FileScheme)
        );
        // …including for a redirect that lands somewhere the address bar never
        // saw.
        assert_eq!(
            navigation_starting("edge://settings", &Mint::Nothing),
            Decision::Refuse(Refusal::BrowserInternalScheme)
        );
        assert_eq!(
            navigation_starting("https://example.com/", &Mint::Nothing),
            Decision::Navigate("https://example.com/".into())
        );
    }

    #[test]
    fn only_the_minted_file_url_passes() {
        let minted = Mint::file(Path::new(r"C:\Users\x\report.html")).expect("a local path mints");
        assert_eq!(minted.target(), Some("file:///C:/Users/x/report.html"));
        let url = minted.target().expect("minted").to_owned();
        assert_eq!(
            navigation_starting(&url, &minted),
            Decision::Navigate(url.clone())
        );
        assert_eq!(
            navigation_starting(&url.to_uppercase(), &minted),
            Decision::Navigate(url.to_uppercase())
        );
        assert_eq!(
            navigation_starting("file:///C:/Windows/win.ini", &minted),
            Decision::Refuse(Refusal::FileScheme)
        );
        // A page inside the sanctioned file cannot walk out of it.
        assert_eq!(
            navigation_starting("file:///C:/Users/x/../../Windows/win.ini", &minted),
            Decision::Refuse(Refusal::FileScheme)
        );
        // And the mint of one seat is no help to another.
        assert_eq!(
            navigation_starting(&url, &Mint::Nothing),
            Decision::Refuse(Refusal::FileScheme)
        );
    }

    #[test]
    fn unc_paths_are_refused_at_the_mint() {
        assert_eq!(
            Mint::file(Path::new(r"\\server\share\page.html")),
            Err(Refusal::NetworkPath)
        );
        assert_eq!(
            Mint::file(Path::new(r"\\?\UNC\server\share\page.html")),
            Err(Refusal::NetworkPath)
        );
        let local = Mint::file(Path::new(r"\\?\C:\a b\p#1.html")).expect("a local path mints");
        assert_eq!(local.target(), Some("file:///C:/a%20b/p%231.html"));
    }

    #[test]
    fn a_pin_gets_no_special_pass() {
        // The pin store is just strings, and they come back through the same
        // door as anything a person typed — including the blank page, which no
        // amount of pinning turns into an address.
        for pinned in [
            "javascript:alert(1)",
            "file:///C:/Windows/win.ini",
            "about:blank",
        ] {
            assert!(
                matches!(address_bar(pinned), Decision::Refuse(_)),
                "{pinned}"
            );
        }
        // A pin that is fine is fine, and gets nothing extra for being pinned.
        assert_eq!(
            address_bar("https://example.com/docs?p=2#intro"),
            Decision::Navigate("https://example.com/docs?p=2#intro".into())
        );
    }

    /// **The red matrix has to be able to fail.**
    ///
    /// A refusal table that a naive check would also pass proves nothing about
    /// the rule it is testing. This row fires the same attacker strings at the
    /// check somebody writes when they are in a hurry — "does it start with
    /// `javascript:`" — and requires that it let most of them through while
    /// this module lets none.
    #[test]
    fn the_red_matrix_is_not_vacuous() {
        let attackers = [
            "javascript:alert(1)",
            "data:text/html,<h1>x",
            "file:///C:/Windows/win.ini",
            "edge://settings",
            "view-source:https://example.com",
            "https://user:pass@example.com/",
            "blob:https://example.com/abc",
            "vbscript:msgbox",
        ];
        let naive = |url: &str| !url.starts_with("javascript:");
        let slipped_past_the_naive_check = attackers.iter().filter(|url| naive(url)).count();
        assert!(
            slipped_past_the_naive_check >= 6,
            "the matrix would pass a check that only looks for javascript:, so it tests nothing"
        );
        for url in attackers {
            assert!(
                matches!(address_bar(url), Decision::Refuse(_)),
                "address_bar admitted {url}"
            );
            assert!(
                matches!(
                    navigation_starting(url, &Mint::Nothing),
                    Decision::Refuse(_)
                ),
                "navigation_starting admitted {url}"
            );
        }
    }

    /// The spellings a scheme can wear when somebody is trying to get it past a
    /// string comparison. None of these may become a [`Decision::Navigate`];
    /// becoming a [`Decision::Search`] is fine, because a search box is not a
    /// navigation.
    ///
    /// Widened here beyond the probe's nine rows with the full-width forms —
    /// `ｊａｖａｓｃｒｉｐｔ:` reads as the word to a person and is nine
    /// entirely different code points to a parser.
    #[test]
    fn obfuscated_scheme_spellings_never_become_a_navigation() {
        let rows = [
            "JaVaScRiPt:alert(1)",
            "\tjavascript:alert(1)",
            "java\tscript:alert(1)",
            "java\nscript:alert(1)",
            "\u{0}javascript:alert(1)",
            "%6aavascript:alert(1)",
            " \r\n file:///C:/Windows/win.ini",
            "FILE:///C:/Windows/win.ini",
            "EDGE://settings",
            "ｊａｖａｓｃｒｉｐｔ:alert(1)",
            "ｆｉｌｅ:///C:/Windows/win.ini",
            "ｈｔｔｐ://example.com",
            "java\u{200b}script:alert(1)",
            "javascript\u{a0}:alert(1)",
        ];
        for input in rows {
            if let Decision::Navigate(url) = address_bar(input) {
                panic!("{input:?} became a navigation to {url}");
            }
            if let Decision::Navigate(url) = navigation_starting(input, &Mint::Nothing) {
                panic!("{input:?} became a navigation to {url}");
            }
        }
    }

    /// **The gate-9 row, and the day its answer changed.**
    ///
    /// The probe's version of this test pinned the plan's letter: `about:blank`
    /// refused at both doors, with a note that the day somebody changed it had
    /// to be a day somebody *chose* to. W0′ made that the day
    /// (`w0p-evidence/evidence.md` §4.1) — the engine fires
    /// `NavigationStarting` for the host's own blank page, so the letter
    /// cancels the product's own navigation. The choice made here is the
    /// narrowest one that exists: the blank page passes when, and only when,
    /// this seat's mint is the one that made it. **The address bar's half of
    /// the test is unchanged.**
    #[test]
    fn about_blank_passes_only_through_the_mint() {
        assert_eq!(
            address_bar(BLANK_PAGE),
            Decision::Refuse(Refusal::BrowserInternalScheme)
        );
        assert_eq!(
            navigation_starting(BLANK_PAGE, &Mint::Nothing),
            Decision::Refuse(Refusal::BrowserInternalScheme)
        );
        assert_eq!(
            navigation_starting(BLANK_PAGE, &Mint::Blank),
            Decision::Navigate(BLANK_PAGE.into())
        );
        // A seat holding a file mint has not minted a blank page.
        let file = Mint::file(Path::new(r"C:\Users\x\report.html")).expect("a local path mints");
        assert_eq!(
            navigation_starting(BLANK_PAGE, &file),
            Decision::Refuse(Refusal::BrowserInternalScheme)
        );
    }

    #[test]
    fn switcher_identity_keeps_query_and_fragment() {
        assert_eq!(
            switcher_key("https://example.com:443/a?b=1#c"),
            "https://example.com/a?b=1#c"
        );
        assert_ne!(
            switcher_key("https://example.com/a?b=1"),
            switcher_key("https://example.com/a?b=2")
        );
        assert_ne!(
            switcher_key("https://example.com/a#x"),
            switcher_key("https://example.com/a#y")
        );
    }

    // ---- added here, one per §3 sentence the probe had not written down ----

    /// §3 pairs "query and fragment participate in identity" with "they are
    /// persisted verbatim". Both halves are the same promise — that what comes
    /// back is what was asked for — so nothing along the way may quietly drop
    /// them.
    #[test]
    fn query_and_fragment_survive_every_door() {
        let full = "https://example.com/search?q=a%20b&page=2#results";
        assert_eq!(address_bar(full), Decision::Navigate(full.into()));
        assert_eq!(
            navigation_starting(full, &Mint::Nothing),
            Decision::Navigate(full.into())
        );
        assert_eq!(switcher_key(full), full);
        assert_eq!(rewrite_unspecified_host(full), full);
        // …and through the rewrite, which is the one place that rebuilds the
        // string instead of passing it along.
        assert_eq!(
            address_bar("http://0.0.0.0:8080/a/b?q=1&r=2#frag"),
            Decision::Navigate("http://127.0.0.1:8080/a/b?q=1&r=2#frag".into())
        );
        // A scheme-less address keeps them too, on its way to getting `http://`.
        assert_eq!(
            address_bar("localhost:5173/app?x=1#top"),
            Decision::Navigate("http://localhost:5173/app?x=1#top".into())
        );
    }

    /// §3's rewrite clause, field by field, because "keeps the rest" is exactly
    /// the kind of sentence a rebuild-the-string implementation passes for five
    /// of the six fields.
    #[test]
    fn the_rewrite_touches_the_host_and_nothing_else() {
        struct Row {
            input: &'static str,
            expected: &'static str,
        }
        let rows = [
            // scheme survives
            Row {
                input: "https://0.0.0.0/",
                expected: "https://127.0.0.1/",
            },
            // no port stays no port
            Row {
                input: "http://0.0.0.0",
                expected: "http://127.0.0.1",
            },
            // port survives
            Row {
                input: "http://0.0.0.0:5173",
                expected: "http://127.0.0.1:5173",
            },
            // path survives, empty query and fragment stay empty
            Row {
                input: "http://0.0.0.0:5173/a/b/c",
                expected: "http://127.0.0.1:5173/a/b/c",
            },
            // query survives with its own `?` and `&`
            Row {
                input: "http://0.0.0.0:5173/a?x=1&y=2",
                expected: "http://127.0.0.1:5173/a?x=1&y=2",
            },
            // fragment survives, including one that contains a `?`
            Row {
                input: "http://0.0.0.0:5173/a#b?c",
                expected: "http://127.0.0.1:5173/a#b?c",
            },
            // a query with no path keeps having no path
            Row {
                input: "http://0.0.0.0:5173?x=1",
                expected: "http://127.0.0.1:5173?x=1",
            },
        ];
        for Row { input, expected } in rows {
            assert_eq!(rewrite_unspecified_host(input), expected, "{input}");
            assert_eq!(
                address_bar(input),
                Decision::Navigate(expected.to_owned()),
                "{input}"
            );
        }
        // Hosts that merely look like it are not it.
        for untouched in [
            "http://0.0.0.1:5173/",
            "http://10.0.0.0/",
            "http://0.0.0.0.example.com/",
            "http://127.0.0.1:5173/",
        ] {
            assert_eq!(
                rewrite_unspecified_host(untouched),
                untouched,
                "{untouched}"
            );
        }
    }

    /// **Where a host stands on the network decides nothing about whether it may
    /// load** (§3; the half of `only_loopback_opens_in_the_preview_without_being_asked`
    /// that outlived the predicate it was written for, 2026-08-29).
    ///
    /// The retired predicate sorted these same addresses into "opens by itself"
    /// and "needs a verb". That sorting is gone — a plain click on any of them
    /// opens the seat now — and what is left is the sentence it was always
    /// careful to say it was *not*: this door reads the address's own text, so a
    /// dev server on the far side of the house, a private range and a public
    /// host are one answer, and the whole of the difference between them is
    /// where the reader chose to point.
    ///
    /// MUTATION: give [`check`] a host rule of any kind and the row for that
    /// shape goes red while every other row still passes — which is how a
    /// network-shaped exception would have got in unnoticed.
    #[test]
    fn no_address_is_admitted_or_refused_for_where_its_host_lives() {
        for address in [
            "http://localhost:5173/",
            "https://localhost/",
            "http://app.localhost:3000/x",
            "http://127.0.0.1:8080/",
            "http://127.9.9.9/",
            "http://[::1]:5173/",
            "http://192.168.1.5:5173/",
            "http://10.0.0.1/",
            "http://172.16.0.1/",
            "http://my-desktop:5173/",
            "http://localhost.evil.com/",
            "https://example.com/",
            "https://claude.ai/code/artifact/04c0a133-319b-4c8e-b988-7965fe063626",
        ] {
            assert_eq!(
                address_bar(address),
                Decision::Navigate(address.to_owned()),
                "{address}"
            );
        }
        // The one host this door does rewrite, and it is not a judgement about
        // the network either — `0.0.0.0` is a bind address and not a place.
        assert_eq!(
            address_bar("http://0.0.0.0:5173/"),
            Decision::Navigate("http://127.0.0.1:5173/".into())
        );
    }

    /// §7.1.5g's `http(s)` arm reads "must start with `//`, no control
    /// characters and no whitespace"; this door says the same thing so that a
    /// link in the terminal and an address in the preview head cannot disagree
    /// about one string.
    #[test]
    fn a_navigation_target_carries_no_control_characters() {
        for input in [
            "http://example.com/\u{0}evil",
            "https://exa\u{7}mple.com/",
            "http://example.com/a\u{1b}[31m",
            "http://exa mple.com/",
            "https://example.com/a b",
        ] {
            assert_eq!(
                address_bar(input),
                Decision::Refuse(Refusal::ControlOrWhitespace),
                "{input:?}"
            );
            assert_eq!(
                navigation_starting(input, &Mint::Nothing),
                Decision::Refuse(Refusal::ControlOrWhitespace),
                "{input:?}"
            );
        }
        // Whitespace with no scheme in front of it is a search, not a refusal —
        // that is the whole of the address bar's second job.
        assert_eq!(
            address_bar("rust trait objects"),
            Decision::Search("rust trait objects".into())
        );
        // A scheme and no host is its own answer.
        assert_eq!(address_bar("http://"), Decision::Refuse(Refusal::NoHost));
    }

    /// The mint admits exactly what it holds and not its neighbours.
    #[test]
    fn a_mint_admits_one_target_and_no_relatives() {
        assert_eq!(
            navigation_starting("about:config", &Mint::Blank),
            Decision::Refuse(Refusal::BrowserInternalScheme)
        );
        assert_eq!(
            navigation_starting("about:srcdoc", &Mint::Blank),
            Decision::Refuse(Refusal::BrowserInternalScheme)
        );
        assert_eq!(
            navigation_starting("about:blank#evil", &Mint::Blank),
            Decision::Refuse(Refusal::BrowserInternalScheme)
        );
        let minted = Mint::file(Path::new(r"C:\Users\x\report.html")).expect("a local path mints");
        // A sibling in the same folder is a different file.
        assert_eq!(
            navigation_starting("file:///C:/Users/x/report.html.txt", &minted),
            Decision::Refuse(Refusal::FileScheme)
        );
        assert_eq!(
            navigation_starting("file:///C:/Users/x/", &minted),
            Decision::Refuse(Refusal::FileScheme)
        );
    }

    /// A fragment inside the sanctioned page is the sanctioned page. A table of
    /// contents in a local `.html` report is the ordinary reason this matters,
    /// and refusing it would make the mint narrower than the file it minted.
    #[test]
    fn a_fragment_inside_the_sanctioned_page_still_loads() {
        let minted = Mint::file(Path::new(r"C:\Users\x\report.html")).expect("a local path mints");
        assert_eq!(
            navigation_starting("file:///C:/Users/x/report.html#chapter-3", &minted),
            Decision::Navigate("file:///C:/Users/x/report.html#chapter-3".into())
        );
        assert_eq!(
            navigation_starting("file:///C:/Users/x/report.html?v=2", &minted),
            Decision::Navigate("file:///C:/Users/x/report.html?v=2".into())
        );
    }

    /// The host's own target goes through a door too, so that "every navigation
    /// this product starts was checked" is a statement with no exceptions in
    /// it.
    #[test]
    fn the_host_checks_its_own_minted_target() {
        assert_eq!(
            check(BLANK_PAGE, Origin::HostMinted(&Mint::Blank)),
            Decision::Navigate(BLANK_PAGE.into())
        );
        // Nothing else passes this arm — not even an address the address bar
        // would have allowed, because this arm is only ever asked about a mint.
        assert_eq!(
            check("https://example.com/", Origin::HostMinted(&Mint::Blank)),
            Decision::Refuse(Refusal::NotMinted)
        );
        assert_eq!(
            check(BLANK_PAGE, Origin::HostMinted(&Mint::Nothing)),
            Decision::Refuse(Refusal::NotMinted)
        );
        let minted = Mint::file(Path::new(r"C:\Users\x\report.html")).expect("a local path mints");
        let url = minted.target().expect("minted").to_owned();
        assert_eq!(
            check(&url, Origin::HostMinted(&minted)),
            Decision::Navigate(url)
        );
    }

    /// [`Decision::Search`] belongs to the address bar. A redirect that lands
    /// on something which is not an address is not a search anybody asked for.
    #[test]
    fn only_the_address_bar_can_answer_with_a_search() {
        for not_a_url in ["how do i exit vim", "rustdoc", "not a url at all"] {
            assert!(matches!(address_bar(not_a_url), Decision::Search(_)));
            assert_eq!(
                navigation_starting(not_a_url, &Mint::Nothing),
                Decision::Refuse(Refusal::ExternalScheme),
                "{not_a_url}"
            );
            assert_eq!(
                check(not_a_url, Origin::HostMinted(&Mint::Blank)),
                Decision::Refuse(Refusal::NotMinted),
                "{not_a_url}"
            );
        }
    }

    /// The verdict is the URL to go to, not a yes about the URL that was
    /// offered — which is the whole reason `Navigate` carries a string.
    #[test]
    fn the_verdict_hands_back_the_url_to_use() {
        assert_eq!(
            navigation_starting("http://0.0.0.0:5173/app#x", &Mint::Nothing),
            Decision::Navigate("http://127.0.0.1:5173/app#x".into())
        );
        // Idempotent, so a host that cancels and restarts on a rewrite stops
        // after one round.
        let once = "http://127.0.0.1:5173/app#x";
        assert_eq!(
            navigation_starting(once, &Mint::Nothing),
            Decision::Navigate(once.into())
        );
    }

    /// §3: 「重定向后以最后一次成功提交的 URL 为身份」, and §4: a navigation
    /// that never committed does not become the thing the seat is remembered
    /// as.
    #[test]
    fn identity_is_the_last_url_that_committed() {
        assert_eq!(switcher_identity(None), None);
        assert_eq!(
            switcher_identity(Some("https://example.com:443/docs?p=2#a")),
            Some("https://example.com/docs?p=2#a".to_owned())
        );
        // The address that was asked for and the address that committed are two
        // strings, and only the second one is an identity.
        let asked = "http://example.com/old";
        let committed = "https://example.com/new";
        assert_ne!(
            switcher_identity(Some(committed)),
            Some(switcher_key(asked))
        );
    }

    /// A card that has to say `{scheme}: addresses do not open in a preview.`
    /// (§7.7 ④) reads the scheme from here rather than re-parsing the string a
    /// second way.
    #[test]
    fn the_blocked_card_can_name_the_scheme() {
        assert_eq!(
            scheme_of("javascript:alert(1)").as_deref(),
            Some("javascript")
        );
        assert_eq!(scheme_of("  MAILTO:a@b  ").as_deref(), Some("mailto"));
        assert_eq!(
            scheme_of("view-source:https://x/").as_deref(),
            Some("view-source")
        );
        assert_eq!(scheme_of("localhost:3000"), None);
        assert_eq!(scheme_of("how do i exit vim"), None);
    }

    /// Every refusal in the enum is reachable, so that none of them is a row
    /// somebody added to a card and no rule ever produces.
    #[test]
    fn every_refusal_has_a_witness() {
        let witnesses: &[(Refusal, Decision)] = &[
            (
                Refusal::ScriptOrInlineScheme,
                address_bar("javascript:alert(1)"),
            ),
            (Refusal::FileScheme, address_bar("file:///C:/x")),
            (
                Refusal::BrowserInternalScheme,
                address_bar("edge://settings"),
            ),
            (Refusal::ExternalScheme, address_bar("mailto:a@b")),
            (Refusal::UserInfo, address_bar("https://u:p@example.com/")),
            (
                Refusal::ControlOrWhitespace,
                address_bar("http://example.com/\u{0}"),
            ),
            (Refusal::NoHost, address_bar("https://")),
            (
                Refusal::NotMinted,
                check("https://example.com/", Origin::HostMinted(&Mint::Nothing)),
            ),
            (Refusal::Empty, address_bar("   ")),
        ];
        for (expected, decision) in witnesses {
            assert_eq!(decision, &Decision::Refuse(*expected));
        }
        assert_eq!(
            Mint::file(Path::new(r"\\server\share\x.html")),
            Err(Refusal::NetworkPath)
        );
    }

    /// **A page with no title is called by its site** (W2 slice ③) — and by the
    /// *same* splitters `switcher_key` normalises with.
    ///
    /// Two surfaces have no title to read, because neither has a buffer: a Recent
    /// row and a pinned URL nobody has opened. §7.7 ③ already names the half of a
    /// URL that is its identity — "scheme 与 host 是身份" — and this is that half,
    /// port included, because `localhost:5173` and `localhost:8080` are two dev
    /// servers and not one.
    ///
    /// A string this module cannot read as a URL answers with itself, which is
    /// the boundary rather than a fallback: `pins.json` is hand-editable, and a
    /// row whose target is not a URL is drawn as what it says and refused at the
    /// gate.
    #[test]
    fn a_page_with_no_title_is_called_by_its_site() {
        for (url, expected) in [
            ("http://localhost:5173/app?tab=logs#top", "localhost:5173"),
            ("https://example.com/a/b", "example.com"),
            ("https://EXAMPLE.com", "example.com"),
            ("http://127.0.0.1:8080/report?run=7", "127.0.0.1:8080"),
            ("http://[::1]:3000/", "::1:3000"),
            ("not a url at all", "not a url at all"),
            ("", ""),
        ] {
            assert_eq!(site_label(url), expected, "for {url}");
        }
    }

    /// **Red gate (the favicon slice, `docs/DESIGN.md` §7.13): the key an icon is filed under is the
    /// server and the whole server.**
    ///
    /// Four claims. The path, query and fragment are not part of it — one server
    /// wears one icon and re-asking per page would be a fetch per click. The
    /// scheme *is* part of it: plain text and TLS on one host are two servers.
    /// The default port is dropped where `switcher_key` drops it, so the row and
    /// the icon cannot disagree about which site a page is on. And a string this
    /// module cannot read is refused outright rather than becoming a key nothing
    /// will ever spell the same way twice.
    ///
    /// MUTATION: build the key out of `site_label` and the fourth row equals the
    /// third — the plain-text server is handed the secure one's icon.
    #[test]
    fn a_favicon_is_filed_under_scheme_host_and_port() {
        for (url, expected) in [
            (
                "http://localhost:5173/app?tab=logs#top",
                Some("http://localhost:5173"),
            ),
            ("http://localhost:5173/", Some("http://localhost:5173")),
            ("https://example.com/a/b", Some("https://example.com")),
            ("http://example.com/a/b", Some("http://example.com")),
            ("https://example.com:443/", Some("https://example.com")),
            ("http://example.com:80/", Some("http://example.com")),
            (
                "https://example.com:8443/",
                Some("https://example.com:8443"),
            ),
            ("https://EXAMPLE.com/", Some("https://example.com")),
            ("  https://example.com/  ", Some("https://example.com")),
            ("http://[::1]:3000/x", Some("http://::1:3000")),
            ("not a url at all", None),
            ("https://", None),
            ("", None),
        ] {
            assert_eq!(site_key(url).as_deref(), expected, "for {url}");
        }
        assert_eq!(
            site_key("https://example.com:443/one"),
            site_key("https://example.com/two"),
            "one server however its default port is spelled"
        );
        assert_ne!(
            site_key("https://example.com/"),
            site_key("http://example.com/"),
            "and two servers where the scheme differs"
        );
    }

    /// PIN (R1-16) — **a mint that refuses is a refusal, not a fall-through.**
    ///
    /// A seat holding [`Mint::File`] is showing the one local document the
    /// files column handed it. When the mint said no, the check went on to the
    /// generic scheme test, which admits every `http(s)` address there is — so
    /// a link, a redirect or a script inside that document could walk the seat
    /// off the disk and onto the network, and the document's own origin went
    /// with it.
    ///
    /// The blank page reads the same way: the host mints it for itself, so a
    /// navigation away from it is a navigation the host did not ask for.
    ///
    /// A seat with no mint at all is the ordinary browsing seat and its rule
    /// does not move — that is the last two assertions, and they are what makes
    /// this a rule about *minted* seats rather than a rule that stops browsing.
    ///
    /// MUTATION: let the `NavigationStarting` arm fall through when `admits`
    /// answers `None` and the first two assertions navigate.
    #[test]
    fn a_seat_the_host_minted_goes_where_the_mint_says_and_nowhere_else() {
        let file = Mint::file(Path::new(r"D:\notes\report.html")).expect("a local page");
        assert_eq!(
            check("https://evil.test/steal", Origin::NavigationStarting(&file)),
            Decision::Refuse(Refusal::NotMinted),
            "a local document does not walk onto the network"
        );
        assert_eq!(
            check(
                "https://evil.test/steal",
                Origin::NavigationStarting(&Mint::Blank)
            ),
            Decision::Refuse(Refusal::NotMinted),
            "and neither does the page the host minted for itself"
        );
        // What the mint does hold still loads, fragment and all.
        assert_eq!(
            check(
                "file:///D:/notes/report.html#ch3",
                Origin::NavigationStarting(&file)
            ),
            Decision::Navigate("file:///D:/notes/report.html#ch3".to_owned())
        );
        assert_eq!(
            check(BLANK_PAGE, Origin::NavigationStarting(&Mint::Blank)),
            Decision::Navigate(BLANK_PAGE.to_owned())
        );
        // An ordinary browsing seat carries no mint, and its rule is the
        // allow-list exactly as it always was.
        assert_eq!(
            check(
                "https://example.test/next",
                Origin::NavigationStarting(&Mint::Nothing)
            ),
            Decision::Navigate("https://example.test/next".to_owned())
        );
        assert_eq!(
            check(
                "javascript:alert(1)",
                Origin::NavigationStarting(&Mint::Nothing)
            ),
            Decision::Refuse(Refusal::ScriptOrInlineScheme)
        );
    }
}
