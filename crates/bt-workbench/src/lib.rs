//! **The attention domain: the ledger that decides whether a pane is asking, and nothing that
//! draws, carries or delivers that answer** (`docs/ARCHITECTURE.md` §12.1, D-57).
//!
//! This is where a caller stops (`docs/ARCHITECTURE.md` §1). Everything here is a pure function of
//! what is handed in; nothing reads a window, a clock, a file or the environment.
//!
//! | into the domain | out of the domain |
//! |---|---|
//! | a normalized producer event ([`attention::Event`]) | trace lines, one per decision ([`attention::Outcome::lines`]) |
//! | the pane it is about ([`attention::Site`]) | the one interruption an episode may spend ([`attention::Raised`]) |
//! | how far a notification can reach ([`attention::Reach`]) | the state and grounds of a pane's account |
//! | the supplied time (an [`std::time::Instant`], never read here) | the next deadline a pane owes ([`attention::expiry::WaitClock`]) |
//! | the window's place allocator ([`attention::Places`]) | a place in the window's queue |
//! | the notification switches ([`attention::NotificationSwitches`]) | |
//!
//! **The ledger's invariants** (`docs/RULES.md` row 29): an episode is one unanswered request from
//! one pane, and only [`attention::AttentionLedger`] may mint one; each credential carries a
//! strictly increasing generation and each answer a watermark, so a generation above the watermark
//! is unanswered and one at or below it has been dealt with; seeing is not answering; announced
//! signals mint nothing and take no place; a desktop interruption is spent at most once per episode.
//! Places in the window's queue are issued only by the ledger ([`attention::Places`]).
//!
//! **What stays out.** Ingress (the endpoint, the verb, capability minting), the per-agent mapping
//! tables, the hook installers, the reach rule, and the routing of an arrival to a pane by walking
//! the window's tabs all live in `bt-app` (`docs/plans/design/ownership-census-2026-09-25.md` §5.2
//! and revision (b) §R6). `Instant` is an in-process input here and is never serialized; an outward
//! protocol (`docs/ARCHITECTURE.md` §12.3) is not offered yet.

pub mod attention;
