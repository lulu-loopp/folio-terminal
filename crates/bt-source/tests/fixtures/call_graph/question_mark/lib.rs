//! `?` values and variant patterns (plan row 101): a foreign `Result` alias on the fixed table,
//! a one-argument standard alias's `Err`, the adapters that keep a value's type, nested and rest
//! patterns — and what the grammar refuses, which stays an untyped receiver's call.

use std::io;

use anyhow::{Context, Result};

pub struct Pane;
pub struct Other;

impl Pane {
    pub fn shared(&self) {}
    pub fn only_pane(&self) {}
}

impl Other {
    pub fn shared(&self) {}
}

/// Two first-party `kind`s, so an untyped `e.kind()` has two candidates.
pub struct KindA;
pub struct KindB;

impl KindA {
    pub fn kind(&self) {}
}

impl KindB {
    pub fn kind(&self) {}
}

pub enum Landing {
    Waiting,
    Landed(Box<Pane>),
    Pair(u8, u8, Pane),
    Both(Pane, u8, u8),
}

pub fn pane() -> Result<Pane> {
    Ok(Pane)
}

pub fn land() -> Result<Landing> {
    Ok(Landing::Waiting)
}

pub fn maybe() -> Option<Pane> {
    None
}

pub fn read() -> io::Result<Pane> {
    Ok(Pane)
}

pub fn formatted() -> std::fmt::Result {
    Ok(())
}

pub fn joined() -> std::thread::Result<Pane> {
    Ok(Pane)
}

pub fn plain() -> std::result::Result<Pane, Other> {
    Ok(Pane)
}

pub fn win() -> windows::core::Result<Pane> {
    Ok(Pane)
}

pub fn off_table() -> syn::Result<Pane> {
    Ok(Pane)
}

// ── typed through the alias table ────────────────────────────────────────

pub fn bound() -> Result<()> {
    let p = pane()?;
    p.shared();
    Ok(())
}

pub fn chained() -> Result<()> {
    pane()?.shared();
    Ok(())
}

pub fn matched() -> Result<()> {
    match land()? {
        Landing::Landed(p) => p.shared(),
        _ => {}
    }
    Ok(())
}

pub fn windowed() -> windows::core::Result<()> {
    let p = win()?;
    p.shared();
    Ok(())
}

pub fn thread_joined() {
    if let Ok(p) = joined() {
        p.shared();
    }
}

pub fn io_error() {
    if let Err(e) = read() {
        e.kind();
    }
}

pub fn fmt_error() {
    if let Err(e) = formatted() {
        e.kind();
    }
}

// ── nested and rest patterns ─────────────────────────────────────────────

pub fn nested() {
    if let Ok(Landing::Landed(p)) = land() {
        p.shared();
    }
}

pub fn looped() {
    while let Some(p) = maybe() {
        p.shared();
    }
}

pub fn rest_variant(landing: Landing) {
    match landing {
        Landing::Pair(.., last) => last.shared(),
        Landing::Both(first, ..) => first.shared(),
        _ => {}
    }
}

pub fn rest_slice(panes: &[Pane]) {
    if let [rest @ .., last] = panes {
        rest.len();
        last.shared();
    }
}

pub fn rest_tuple(pair: (Pane, u8, Pane)) {
    let (a, ..) = pair;
    a.shared();
    let (.., c) = pair;
    c.shared();
}

// ── the adapters that keep the value's type ──────────────────────────────

pub fn adapted() -> Result<()> {
    let p = maybe().ok_or_else(|| Other)?;
    p.shared();
    let q = plain().map_err(|_| Other)?;
    q.shared();
    let r = read().context("read")?;
    r.shared();
    let s = maybe().context("none")?;
    s.shared();
    let t = plain().or_else(|_| plain())?;
    t.shared();
    Ok(())
}

// ── refused ──────────────────────────────────────────────────────────────

pub fn refused() -> Result<()> {
    let p = off_table()?;
    p.shared();
    p.only_pane();
    Ok(())
}

pub fn untyped_operand() -> Result<()> {
    match undeclared()? {
        Some(p) => p.shared(),
        _ => {}
    }
    Ok(())
}
