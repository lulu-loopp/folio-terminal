//! **What this program answers for `bt-term`, said once, before the first session**
//! (`docs/ARCHITECTURE.md` §3.2).
//!
//! `bt-term` builds without the platform layer and without a rasterizer, so the two facts about
//! the machine a terminal session needs, and the codec its inline pictures need, are installed
//! into it by the host: the names this machine answers to, which a `file://<host>/`
//! working-directory report is compared against; what every thread of its image resample pool
//! runs first; and the SVG codec ([`SVG_CODEC`]). [`install`] gives all three, and `main` calls it
//! before the event loop exists — every session this process makes is made by that loop. The
//! fourth answer, the name a hand-off door would open, is handed to `bt_term::verify_path` by the
//! path-verification lane, its one product caller.

/// Install this machine's answers into `bt-term`. Called once, by `main`, before the event loop.
pub(crate) fn install() {
    install_the_repeatable_answers();
    bt_term::install_pool_thread_start(enter_the_band_below_normal);
}

/// The two answers a second installation of which is nothing: this machine's names and the SVG
/// codec. [`install`] gives them, and so does every test of this program that makes a session or
/// decodes a picture (`test_support::install_host_answers`) — the pool's thread-start hook
/// installs once, so a test process does not.
pub(crate) fn install_the_repeatable_answers() {
    bt_term::install_host_names(this_machines_names());
    bt_term::install_svg_rasterizer(SVG_CODEC);
}

/// The codec an inline picture's SVG bytes are rasterized by: the math crate's resvg rasterizer,
/// at the document's intrinsic size, with the machine's fonts and no `<image href>` door.
pub(crate) const SVG_CODEC: bt_term::SvgRasterizer = bt_math::rasterize_svg_document;

/// Every spelling of this machine's name a shell on it may put in a `file://<host>/` report —
/// the operating system's answer (`bt_platform::host_names`), never an environment variable.
pub(crate) fn this_machines_names() -> Vec<String> {
    bt_platform::host_names()
}

/// What a resample-pool thread runs first: the band below normal, the band every worker of this
/// process runs in, so a wallpaper-sized pass cannot take the window thread's cores.
fn enter_the_band_below_normal() {
    bt_platform::set_current_thread_priority(bt_platform::ThreadPriority::BelowNormal);
}

#[cfg(test)]
mod tests {
    use crate::test_support::{free_fn_body, squeezed};

    /// RED (CC-7) — **the SVG codec this program installs rasterizes an inline SVG through
    /// `bt-term` at the document's intrinsic size, in straight alpha** (design T-COMPOSE-CRATE
    /// §6.1 CC-7: `svg_document_rasterizes_at_intrinsic_size_with_straight_alpha`, through the
    /// installed codec). The fill is half transparent, so a premultiplied answer has half the red.
    ///
    /// MUTATION ①: make `SVG_CODEC` `bt_term::test_svg_rasterizer` — red: the document is not
    /// that codec's one document, and the decode answers `UnsupportedFormat`. MUTATION ②: drop
    /// `unpremultiply_srgb_rgba` from `bt_math::rasterize_svg_document` — the pixel assertion goes
    /// red. MUTATION ③: drop the codec's installation from `install_the_repeatable_answers` — red:
    /// nothing in this process installs a codec, and the decode panics with "the SVG rasterizer
    /// read before the host installed it".
    #[test]
    fn an_svg_document_decodes_through_the_installed_codec_at_intrinsic_size_with_straight_alpha() {
        use base64::Engine as _;
        super::install_the_repeatable_answers();
        let document = br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="6">
            <rect x="0" y="0" width="8" height="6" fill="#ff0000" fill-opacity="0.5"/>
        </svg>"##;
        let decoded = bt_term::decode_inline_image(bt_term::InlineImageTask {
            occurrence_id: 1,
            source: bt_term::InlineImageSource::Osc1337(
                base64::engine::general_purpose::STANDARD
                    .encode(document)
                    .into_bytes(),
            ),
        })
        .expect("an SVG document decodes through the installed codec");
        assert_eq!((decoded.width_px, decoded.height_px), (8, 6));
        assert_eq!(decoded.rgba.len(), 8 * 6 * 4);
        assert!(!decoded.animated);
        assert_eq!(
            &decoded.rgba[..4],
            &[255, 0, 0, 128],
            "full red at half coverage: straight alpha"
        );
    }

    /// RED (B-AUDIT-046 TRM-3, moved here by CC-4) — **the names this program installs are the
    /// machine's own, and a working-directory report naming the machine is this machine's
    /// directory.**
    ///
    /// `bt-term`'s own pin
    /// (`session::tests::a_cwd_message_naming_this_host_is_accepted_and_a_foreign_one_ignored`)
    /// runs on the names its tests install; this is the half that is the host's: the operating
    /// system's name — the real producer, not a name handed in by the test — installed and then
    /// accepted from fish's own report.
    ///
    /// MUTATION: make `this_machines_names` answer the `COMPUTERNAME` variable — red on macOS and
    /// Linux, where it is unset; make it answer nothing — red everywhere.
    #[test]
    fn the_names_installed_are_the_machines_and_a_report_naming_it_is_accepted() {
        let names = super::this_machines_names();
        let host = bt_platform::host_names()
            .into_iter()
            .next()
            .expect("this machine has a name to ask for");
        assert!(
            names.iter().any(|name| name.eq_ignore_ascii_case(&host)),
            "the installed names hold the operating system's {host:?}: {names:?}"
        );
        bt_term::install_host_names(names);
        let directory = std::env::temp_dir();
        let bare = bt_transcript::paths::local_path_to_file_uri(&directory);
        let (_, path) = bare
            .split_once("://")
            .expect("a local file URI opens with the scheme and an empty authority");
        let mut session = bt_term::DualPlaneSession::new(
            std::num::NonZeroU32::new(80).unwrap(),
            std::num::NonZeroU32::new(24).unwrap(),
        );
        session
            .feed(format!("\x1b]7;file://{host}{path}\x07").as_bytes())
            .unwrap();
        assert_eq!(
            session.working_directory(),
            bt_term::file_uri_to_local_path(&bare, &[]).as_deref(),
            "fish's report naming this host as {host:?} is this machine's directory"
        );
    }

    /// Whether `main`'s body installs the host answers before anything in it could make a session:
    /// before the event loop is built (every session is made by that loop's turns) and before any
    /// session is named. `Err` names the violation.
    ///
    /// GUARD: its subject is how `main` is written (CONVENTIONS, "A test pins behaviour by running
    /// it"); `main` cannot run in a test, and the order of its statements is the fact.
    fn installs_before_the_first_session(main_body: &str) -> Result<(), String> {
        let body = squeezed(main_body);
        let install = body
            .find("host_answers::install();")
            .ok_or("`main` never installs the host answers")?;
        let first_session = ["EventLoop::", "DualPlaneSession", "create_leaf_session("]
            .iter()
            .filter_map(|spelling| body.find(spelling).map(|at| (at, *spelling)))
            .min()
            .ok_or("`main` builds no event loop, so this guard reads the wrong function")?;
        if first_session.0 < install {
            return Err(format!(
                "`main` reaches `{}` before it installs the host answers",
                first_session.1
            ));
        }
        Ok(())
    }

    /// GUARD (CC-4) — **`main` installs the host answers before the first session can exist.**
    ///
    /// A session constructed before the install reads no host names and panics on the first
    /// working-directory report it is sent.
    ///
    /// MUTATION: move `host_answers::install();` below `EventLoop::<AppEvent>::with_user_event()`
    /// in `main`, and this goes red.
    #[test]
    fn the_host_answers_are_installed_before_the_first_session_can_exist() {
        installs_before_the_first_session(free_fn_body("main")).unwrap();
    }

    /// GUARD (CC-4) — **the guard above reads each violation it exists for**: an install after the
    /// loop, an install after a session is named, and no install at all; and it passes the order
    /// `main` has.
    ///
    /// MUTATION: drop `DualPlaneSession` from the guard's list and the second planted body passes.
    #[test]
    fn the_install_guard_names_an_install_after_the_first_session() {
        let ordered = "{ enter(); host_answers::install(); let b = EventLoop::<E>::new(); }";
        assert_eq!(installs_before_the_first_session(ordered), Ok(()));
        for (planted, reached) in [
            (
                "{ let b = EventLoop::<E>::new(); host_answers::install(); }",
                "EventLoop::",
            ),
            (
                "{ let s = DualPlaneSession::new(c, r); host_answers::install(); \
                 let b = EventLoop::<E>::new(); }",
                "DualPlaneSession",
            ),
        ] {
            assert_eq!(
                installs_before_the_first_session(planted),
                Err(format!(
                    "`main` reaches `{reached}` before it installs the host answers"
                )),
                "{planted}"
            );
        }
        assert_eq!(
            installs_before_the_first_session("{ let b = EventLoop::<E>::new(); }"),
            Err("`main` never installs the host answers".to_owned())
        );
    }
}
