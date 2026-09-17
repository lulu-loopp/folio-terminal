//! How deep the recursion under one formula will go, counted without recursing.
//!
//! **A stack overflow is not a panic.** `catch_unwind` cannot contain one, the process dies, and
//! the input is text a program merely printed into a terminal — so the depth has to be refused
//! before anything walks the source, not measured afterwards. Both of the walks that matter recurse
//! on the same shape: `mitex-parser` descends through groups, `\left…\right`, environments, the
//! arguments a command takes and the scripts `^`/`_` chain right-associatively, and Typst's parser
//! and layout then descend through what that produced.
//!
//! So this counts exactly those, in one linear pass over the lexer's own tokens, with an explicit
//! stack of frames and no recursion of its own. The arities come from the spec the converter itself
//! is given (`DEFAULT_SPEC`), so a command that takes no argument costs nothing — `\alpha\beta…`
//! repeated a thousand times is flat and stays accepted — and `\sqrt\sqrt\sqrt x` is three deep
//! because each `\sqrt` is still waiting for its one term when the next one starts.
//!
//! **Macros are counted where they are called, from what they expand to**, because expansion is
//! what the parser will see: each definition's own depth is folded once over the definition graph
//! `macro_budget` has already proved acyclic, and a call adds that depth to the depth it stands at.
//! Otherwise a hundred `\sqrt` in a body invoked forty-eight times would pass a check written
//! against the source's bytes while presenting four thousand eight hundred levels to the parser.

use std::collections::BTreeMap;

use mitex_lexer::{BraceKind, CommandName, Token};
use mitex_spec::{ArgPattern, ArgShape, CommandSpecItem};
use mitex_spec_gen::DEFAULT_SPEC;

use crate::MathRenderError;

/// One token as [`macro_budget`](crate::macro_budget) reads them.
pub(super) type Tok<'a> = (Token, &'a str);

/// The deepest nesting a formula may present to the converter.
///
/// See [`crate::MATH_WORKER_STACK_BYTES`] for where the number comes from: it is the measured
/// per-level cost of the whole chain against the stack the math worker is given, with the safety
/// factor stated there. It is not a taste about formulas — 64 levels of nesting is already far past
/// anything a reader writes, and the limit exists to make the *unreadable* cases refusable rather
/// than fatal.
pub(super) const MAX_NESTING_DEPTH: u32 = 256;

/// What a command is still owed after it is read.
enum Frame {
    /// A `{…}`, a `\left…\right`, or a `\begin…\end`: closed by its own closer.
    Closed(Closer),
    /// A command waiting for this many more complete terms.
    Terms(u32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Closer {
    Brace,
    Bracket,
    Paren,
    Left,
    Environment,
}

/// How many terms a command takes, read off the spec the converter is given.
///
/// `Left1` and `InfixGreedy` take what is to their *left*, which has already been counted where it
/// stood, so they open nothing. A greedy or glob pattern is counted as one term: it consumes the
/// rest of its group, which the group's own frame already accounts for.
fn argument_terms(name: &str) -> u32 {
    match DEFAULT_SPEC.get(name) {
        Some(CommandSpecItem::Cmd(shape)) => match &shape.args {
            ArgShape::Right { pattern } => match pattern {
                ArgPattern::None => 0,
                ArgPattern::FixedLenTerm { len } => u32::from(*len),
                ArgPattern::RangeLenTerm { max, .. } => u32::from(*max),
                ArgPattern::Greedy | ArgPattern::Glob { .. } => 1,
            },
            ArgShape::Left1 | ArgShape::InfixGreedy => 0,
        },
        _ => 0,
    }
}

fn closer_for(brace: BraceKind) -> Closer {
    match brace {
        BraceKind::Curly => Closer::Brace,
        BraceKind::Bracket => Closer::Bracket,
        BraceKind::Paren => Closer::Paren,
    }
}

/// What a run of tokens does to the depth: the deepest it reaches, and how much of that it leaves
/// standing when it ends.
///
/// **The second number is what makes a macro's calls compose.** A body of a hundred `\sqrt` ends
/// with a hundred commands each still owed an argument, so the next call's expansion lands *inside*
/// them: forty-eight such calls are four thousand eight hundred levels and not a hundred. Counting
/// only the deepest point a body reaches would read that as a hundred and let it through.
#[derive(Clone, Copy, Default)]
pub(super) struct Depth {
    pub(super) deepest: u32,
    pub(super) open: u32,
}

/// The nesting these tokens present, with `macros` giving what each defined name expands to.
///
/// Linear, and iterative by construction: the only stack here is the `Vec` of frames.
pub(super) fn scan(tokens: &[Tok<'_>], macros: &BTreeMap<&str, Depth>) -> Depth {
    let mut frames = Vec::<Frame>::new();
    let mut deepest = 0_u32;
    let mut depth = 0_u32;

    // A term has just been completed: every command waiting on one is one term nearer to done, and
    // a command that finishes is itself the term its own caller was waiting for.
    fn settle(frames: &mut Vec<Frame>, depth: &mut u32) {
        while let Some(Frame::Terms(remaining)) = frames.last_mut() {
            *remaining -= 1;
            if *remaining != 0 {
                break;
            }
            frames.pop();
            *depth = depth.saturating_sub(1);
        }
    }

    fn open(frames: &mut Vec<Frame>, depth: &mut u32, deepest: &mut u32, frame: Frame) {
        frames.push(frame);
        *depth = depth.saturating_add(1);
        *deepest = (*deepest).max(*depth);
    }

    // A closer pops back to its own opener, so an unbalanced source cannot leave a frame standing
    // for the rest of the scan — which would read as depth this formula does not have.
    fn close(frames: &mut Vec<Frame>, depth: &mut u32, closer: Closer) {
        let Some(at) = frames
            .iter()
            .rposition(|frame| matches!(frame, Frame::Closed(open) if *open == closer))
        else {
            return;
        };
        *depth = depth.saturating_sub((frames.len() - at) as u32);
        frames.truncate(at);
    }

    let mut index = 0;
    while index < tokens.len() {
        let (token, text) = tokens[index];
        index += 1;
        match token {
            Token::Left(brace) => {
                open(
                    &mut frames,
                    &mut depth,
                    &mut deepest,
                    Frame::Closed(closer_for(brace)),
                );
            }
            Token::Right(brace) => {
                close(&mut frames, &mut depth, closer_for(brace));
                settle(&mut frames, &mut depth);
            }
            Token::Caret | Token::Underscore => {
                // Right-associative: `x^y^z` is `x^(y^(z))`, so each one is still owed its term
                // when the next arrives. Eight thousand of them is eight thousand levels.
                open(&mut frames, &mut depth, &mut deepest, Frame::Terms(1));
            }
            Token::CommandName(name) => match name {
                CommandName::BeginEnvironment | CommandName::ErrorBeginEnvironment => {
                    open(
                        &mut frames,
                        &mut depth,
                        &mut deepest,
                        Frame::Closed(Closer::Environment),
                    );
                }
                CommandName::EndEnvironment | CommandName::ErrorEndEnvironment => {
                    close(&mut frames, &mut depth, Closer::Environment);
                    settle(&mut frames, &mut depth);
                }
                CommandName::Left => {
                    open(
                        &mut frames,
                        &mut depth,
                        &mut deepest,
                        Frame::Closed(Closer::Left),
                    );
                }
                CommandName::Right => {
                    close(&mut frames, &mut depth, Closer::Left);
                    settle(&mut frames, &mut depth);
                }
                _ => {
                    let name = text.strip_prefix('\\').unwrap_or(text);
                    if let Some(expanded) = macros.get(name) {
                        // The body nests on top of the depth the call stands at, and whatever it
                        // leaves open stays open for what follows it.
                        deepest = deepest.max(depth.saturating_add(expanded.deepest));
                        if expanded.open == 0 {
                            settle(&mut frames, &mut depth);
                        } else {
                            for _ in 0..expanded.open {
                                open(&mut frames, &mut depth, &mut deepest, Frame::Terms(1));
                            }
                        }
                    } else {
                        let terms = argument_terms(name);
                        if terms == 0 {
                            settle(&mut frames, &mut depth);
                        } else {
                            open(&mut frames, &mut depth, &mut deepest, Frame::Terms(terms));
                        }
                    }
                }
            },
            _ => settle(&mut frames, &mut depth),
        }
    }
    Depth {
        deepest,
        open: depth,
    }
}

/// Refuse a formula whose nesting would take the converter deeper than the worker's stack allows.
pub(super) fn bound(depth: Depth) -> Result<(), MathRenderError> {
    if depth.deepest > MAX_NESTING_DEPTH {
        return Err(MathRenderError::NestingTooDeep);
    }
    Ok(())
}
