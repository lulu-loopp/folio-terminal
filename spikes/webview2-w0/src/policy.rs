//! §3 of the plan, written as code so gate 9 can shoot at it.
//!
//! Two doors, one rule. The address bar decides what may be *asked for*;
//! `NavigationStarting` decides what may actually *load*, and it re-asks the
//! same question because a redirect or a page script can start a navigation the
//! address bar never saw. A pin is not an authorisation: it comes back through
//! `address_bar` like anything a person typed.

/// Why a URL was refused. Each variant is one row of the red matrix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// `javascript:`, `data:`, `blob:`, `vbscript:` — script and inline content
    /// smuggled in as a location.
    ScriptOrInlineScheme,
    /// `file:` typed or navigated to. The only file the preview opens is one the
    /// files column handed it as a canonicalized path.
    FileScheme,
    /// `view-source:`, `devtools:`, `edge:`, `chrome:` — browser internals.
    BrowserInternalScheme,
    /// `mailto:`, `ftp:`, `ws:`, `tel:`, anything the shell would launch.
    ExternalScheme,
    /// `https://user:pass@host` — the phishing shape.
    UserInfo,
    /// A UNC or network path offered as a controlled file entry.
    NetworkPath,
    /// Empty, or all whitespace.
    Empty,
}

/// What the address bar decided.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    /// Navigate here. The string is the URL after normalisation.
    Navigate(String),
    /// Not a URL — hand the text to the default search engine.
    Search(String),
    Refuse(Refusal),
}

/// Split `input` into `(scheme, rest)` if it carries an explicit scheme.
///
/// Deliberately stricter than "find a colon": `localhost:3000` has a colon and
/// is a host and port, and a rule that read it as a scheme would send every
/// dev-server address to the search engine.
fn split_scheme(input: &str) -> Option<(String, &str)> {
    let colon = input.find(':')?;
    let scheme = &input[..colon];
    if scheme.is_empty() {
        return None;
    }
    let mut characters = scheme.chars();
    let first = characters.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    // A scheme is followed by `//` or by opaque data; a host:port is followed by
    // digits and then a `/` or nothing. `http:3000` is not a thing anyone means.
    let rest = &input[colon + 1..];
    if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
        return None;
    }
    Some((scheme.to_ascii_lowercase(), rest))
}

fn classify_scheme(scheme: &str) -> Result<(), Refusal> {
    match scheme {
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
        _ => Err(Refusal::ExternalScheme),
    }
}

/// The authority component of an `http(s)` URL: everything between `//` and the
/// first `/`, `?` or `#`.
fn authority(rest_after_scheme: &str) -> &str {
    let rest = rest_after_scheme
        .strip_prefix("//")
        .unwrap_or(rest_after_scheme);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    &rest[..end]
}

fn has_userinfo(authority: &str) -> bool {
    // `@` anywhere in the authority means userinfo; a bare `@` in a path or
    // query is fine and never reaches here.
    authority.contains('@')
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
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => {
            (host.to_ascii_lowercase(), Some(port.to_owned()))
        }
        _ => (authority.to_ascii_lowercase(), None),
    }
}

/// Whether a host is loopback **by syntax alone**. No DNS, no `hosts` file: a
/// name that resolves to 127.0.0.1 today and to a LAN box tomorrow must not
/// change what the preview does by default.
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
    if octets.len() == 4
        && octets
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        && let Ok(first) = octets[0].parse::<u32>()
        && first == 127
        && octets[1..]
            .iter()
            .all(|part| part.parse::<u32>().is_ok_and(|value| value <= 255))
    {
        return true;
    }
    false
}

/// Whether a fully-formed URL is one the *localhost entry* opens in the preview
/// without being asked twice. RFC1918 and the machine's own name are excluded on
/// purpose.
pub fn opens_in_preview_by_default(url: &str) -> bool {
    let Some((scheme, rest)) = split_scheme(url) else {
        return false;
    };
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let (host, _) = split_host_port(authority(rest));
    is_loopback_host(&host)
}

/// `0.0.0.0` is a bind address, not a destination: rewrite it before navigating
/// and keep port, path, query and fragment exactly as they were.
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

/// The address bar's allowlist. `search` is what a non-URL becomes.
pub fn address_bar(input: &str) -> Decision {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Decision::Refuse(Refusal::Empty);
    }
    match split_scheme(trimmed) {
        Some((scheme, rest)) => {
            if let Err(refusal) = classify_scheme(&scheme) {
                return Decision::Refuse(refusal);
            }
            let authority = authority(rest);
            if has_userinfo(authority) {
                return Decision::Refuse(Refusal::UserInfo);
            }
            if authority.is_empty() {
                return Decision::Refuse(Refusal::ExternalScheme);
            }
            Decision::Navigate(rewrite_unspecified_host(trimmed))
        }
        None => {
            // No scheme. It is a URL if it looks like a host — a dot or a
            // loopback name, and no whitespace — otherwise it is a search.
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
    }
}

/// The second door. Every top-level navigation — typed, clicked, redirected to,
/// or started by a page script — comes through here, and `file:` is allowed only
/// when it is the exact URL the controlled file entry minted.
pub fn navigation_starting(url: &str, sanctioned_file: Option<&str>) -> Result<(), Refusal> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(Refusal::Empty);
    }
    let Some((scheme, rest)) = split_scheme(trimmed) else {
        // A navigation target with no scheme never comes out of the engine.
        return Err(Refusal::ExternalScheme);
    };
    if scheme == "file" {
        return match sanctioned_file {
            Some(sanctioned) if same_file_url(sanctioned, trimmed) => Ok(()),
            _ => Err(Refusal::FileScheme),
        };
    }
    classify_scheme(&scheme)?;
    if has_userinfo(authority(rest)) {
        return Err(Refusal::UserInfo);
    }
    Ok(())
}

/// Two `file:` URLs naming the same file. Compared case-insensitively because
/// Windows paths are, and without any normalisation of `..` — the sanctioned URL
/// was minted from a canonicalized path, so a target that needs normalising to
/// match is a target that did not come from that mint.
fn same_file_url(sanctioned: &str, candidate: &str) -> bool {
    sanctioned.eq_ignore_ascii_case(candidate.split(['?', '#']).next().unwrap_or(candidate))
}

/// The controlled file entry: a path the files column already resolved becomes a
/// `file:` URL. Network paths keep the product's existing refusal.
pub fn file_url_from_canonical_path(path: &std::path::Path) -> Result<String, Refusal> {
    let text = path.to_string_lossy();
    let stripped = text.strip_prefix(r"\\?\").unwrap_or(&text);
    if stripped.starts_with(r"\\") || stripped.starts_with("UNC\\") {
        return Err(Refusal::NetworkPath);
    }
    let mut url = String::from("file:///");
    for character in stripped.chars() {
        match character {
            '\\' => url.push('/'),
            // Percent-encode what would otherwise re-open the parse: a literal
            // `#` in a file name is a fragment to any URL parser.
            '%' => url.push_str("%25"),
            '#' => url.push_str("%23"),
            '?' => url.push_str("%3F"),
            ' ' => url.push_str("%20"),
            other => url.push(other),
        }
    }
    Ok(url)
}

/// The switcher's identity for a URL: normalised, with query and fragment
/// participating (§3, "去重键 = 规范化后的完整 URL").
///
/// Plan §3's "切换器确定性三则" clause; proven by its own test below and
/// otherwise never called outside it.
#[cfg_attr(not(test), allow(dead_code))]
pub fn switcher_key(url: &str) -> String {
    let Some((scheme, rest)) = split_scheme(url) else {
        return url.to_owned();
    };
    let body = rest.strip_prefix("//").unwrap_or(rest);
    let end = body.find(['/', '?', '#']).unwrap_or(body.len());
    let (host, port) = split_host_port(&body[..end]);
    let tail = &body[end..];
    let default_port = match (scheme.as_str(), port.as_deref()) {
        ("http", Some("80")) | ("https", Some("443")) => None,
        (_, port) => port,
    };
    match default_port {
        Some(port) => format!("{scheme}://{host}:{port}{tail}"),
        None => format!("{scheme}://{host}{tail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn navigation_starting_is_the_same_rule_again() {
        assert_eq!(
            navigation_starting("javascript:void(0)", None),
            Err(Refusal::ScriptOrInlineScheme)
        );
        assert_eq!(
            navigation_starting("file:///C:/secret.txt", None),
            Err(Refusal::FileScheme)
        );
        // …including for a redirect that lands somewhere the address bar never
        // saw.
        assert_eq!(
            navigation_starting("edge://settings", None),
            Err(Refusal::BrowserInternalScheme)
        );
        assert!(navigation_starting("https://example.com/", None).is_ok());
    }

    #[test]
    fn only_the_minted_file_url_passes() {
        let minted = "file:///C:/Users/x/report.html";
        assert!(navigation_starting(minted, Some(minted)).is_ok());
        assert!(navigation_starting(&minted.to_uppercase(), Some(minted)).is_ok());
        assert_eq!(
            navigation_starting("file:///C:/Windows/win.ini", Some(minted)),
            Err(Refusal::FileScheme)
        );
        // A page inside the sanctioned file cannot walk out of it.
        assert_eq!(
            navigation_starting("file:///C:/Users/x/../../Windows/win.ini", Some(minted)),
            Err(Refusal::FileScheme)
        );
    }

    #[test]
    fn unc_paths_are_refused_at_the_mint() {
        assert_eq!(
            file_url_from_canonical_path(std::path::Path::new(r"\\server\share\page.html")),
            Err(Refusal::NetworkPath)
        );
        assert_eq!(
            file_url_from_canonical_path(std::path::Path::new(r"\\?\UNC\server\share\page.html")),
            Err(Refusal::NetworkPath)
        );
        assert_eq!(
            file_url_from_canonical_path(std::path::Path::new(r"\\?\C:\a b\p#1.html")),
            Ok("file:///C:/a%20b/p%231.html".to_owned())
        );
    }

    #[test]
    fn a_pin_gets_no_special_pass() {
        // The pin store is just strings; they come back through the same door.
        for pinned in ["javascript:alert(1)", "file:///C:/Windows/win.ini"] {
            assert!(
                matches!(address_bar(pinned), Decision::Refuse(_)),
                "{pinned}"
            );
        }
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
}
