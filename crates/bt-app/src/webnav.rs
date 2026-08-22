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

use std::path::Path;

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
    /// The one `file:` URL the controlled file entry minted from a
    /// canonicalised path.
    File(String),
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
        let mut url = String::from("file:///");
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
        Ok(Self::File(url))
    }

    /// The URL this mint stands for, which is what the host navigates to.
    pub fn target(&self) -> Option<&str> {
        match self {
            Self::Nothing => None,
            Self::Blank => Some(BLANK_PAGE),
            Self::File(url) => Some(url),
        }
    }

    /// Whether `candidate` is this mint, and if so the URL to allow.
    ///
    /// The sanctioned file answers for its own fragments and queries — a jump
    /// to `#section` inside the page that was minted is the same page — and the
    /// comparison is case-insensitive because Windows paths are. There is no
    /// normalisation of `..`: the mint was made from a canonicalised path, so a
    /// candidate that would need normalising to match is a candidate that did
    /// not come from the mint.
    fn admits(&self, candidate: &str) -> Option<String> {
        match self {
            Self::Nothing => None,
            Self::Blank => candidate
                .eq_ignore_ascii_case(BLANK_PAGE)
                .then(|| BLANK_PAGE.to_owned()),
            Self::File(minted) => {
                let without_tail = candidate.split(['?', '#']).next().unwrap_or(candidate);
                minted
                    .eq_ignore_ascii_case(without_tail)
                    .then(|| candidate.to_owned())
            }
        }
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
        Origin::NavigationStarting(mint) => {
            if let Some(url) = mint.admits(trimmed) {
                return Decision::Navigate(url);
            }
        }
        Origin::AddressBar => {}
    }
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

/// Whether a URL is one the **localhost entry** opens in the preview without
/// being asked twice (a link clicked in the terminal, §7.1.5g ①).
///
/// RFC1918, the machine's own name and `hosts` aliases are excluded on purpose;
/// they still open through an explicit verb. This answers "where does a plain
/// click go by default", never "may this load" — that is [`check`]'s question
/// and it has a different answer.
pub fn opens_in_preview_by_default(url: &str) -> bool {
    let Some((scheme, rest)) = split_scheme(url.trim()) else {
        return false;
    };
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let (host, _) = split_host_port(authority(rest));
    is_loopback_host(&host)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    // ---- the twelve carried over from the W0′ probe (spikes/webview2-w0) ----

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
        assert_eq!(
            Mint::file(Path::new(r"\\?\C:\a b\p#1.html")),
            Ok(Mint::File("file:///C:/a%20b/p%231.html".to_owned()))
        );
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

    /// §3's other half of the loopback sentence: what does **not** open in the
    /// preview by itself. This is the entry default, not a permission — every
    /// one of these still loads when a verb says so, which is why the second
    /// half of the test checks that [`check`] admits them.
    #[test]
    fn only_loopback_opens_in_the_preview_without_being_asked() {
        for opens in [
            "http://localhost:5173/",
            "https://localhost/",
            "http://app.localhost:3000/x",
            "http://127.0.0.1:8080/",
            "http://127.9.9.9/",
            "http://0.0.0.0:5173/",
            "http://[::1]:5173/",
        ] {
            assert!(opens_in_preview_by_default(opens), "{opens}");
        }
        for elsewhere in [
            "http://192.168.1.5:5173/",
            "http://10.0.0.1/",
            "http://172.16.0.1/",
            "http://my-desktop:5173/",
            "http://localhost.evil.com/",
            "https://example.com/",
            "file:///C:/x.html",
            "about:blank",
        ] {
            assert!(!opens_in_preview_by_default(elsewhere), "{elsewhere}");
        }
        // …and "does not open by default" is not "may not load".
        assert_eq!(
            address_bar("http://192.168.1.5:5173/"),
            Decision::Navigate("http://192.168.1.5:5173/".into())
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
}
