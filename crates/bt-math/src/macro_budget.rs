//! A static bound for the pinned MiTeX macro engine, before it can expand anything.
//!
//! Use its *plain* lexer so comments, escaped commands and starred names have
//! exactly the converter's token boundaries. Accept only complete, literal,
//! zero-argument command definitions. Parameter substitution, custom environments
//! and generated declarations need a different proof and are refused. In
//! particular a parameter can introduce recursion absent from a name graph.
//! Duplicate definitions contribute the union of their edges and the sum of
//! their costs, covering every scope/redefinition order conservatively.
//!
//! Each DAG node costs its body bytes plus every child's cost, once per reference
//! (not once per distinct child). Summing those costs at *every* original command
//! token overcounts even unused definitions. This bounds copied bytes and token
//! visits by 32 KiB, including intermediate expansion, not just final output.
//! At most 128 definitions and 8 KiB source bound the iterative proof itself.
//! There is no timeout thread to leak and no unbounded conversion to abandon.

use std::collections::BTreeMap;

use mitex_lexer::{BraceKind, CommandName, Lexer, Token};
use mitex_spec_gen::DEFAULT_SPEC;

use crate::MathRenderError;

const MAX_WORK: usize = 32 * 1024;
const MAX_DEFINITIONS: usize = 128;
type Tok<'a> = (Token, &'a str);

#[derive(Default)]
struct Definition<'a> {
    bytes: usize,
    references: Vec<&'a str>,
}

fn command(token: Tok<'_>) -> Option<&str> {
    match token {
        (Token::CommandName(CommandName::Generic), text) => text.strip_prefix('\\'),
        _ => None,
    }
}

fn declaration(name: &str) -> bool {
    matches!(
        name.trim_end_matches('*'),
        "newcommand" | "renewcommand" | "providecommand" | "DeclareRobustCommand" | "def"
    )
}

fn unsupported(name: &str) -> bool {
    matches!(
        name.trim_end_matches('*'),
        "newenvironment"
            | "renewenvironment"
            | "gdef"
            | "edef"
            | "xdef"
            | "let"
            | "futurelet"
            | "csname"
            | "endcsname"
            | "expandafter"
            | "catcode"
            | "mitexrecurse"
    )
}

fn group<'a>(tokens: &[Tok<'a>], at: &mut usize) -> Result<Vec<Tok<'a>>, MathRenderError> {
    if tokens.get(*at).map(|t| t.0) != Some(Token::Left(BraceKind::Curly)) {
        return Err(MathRenderError::UnboundedMacro);
    }
    *at += 1;
    let start = *at;
    let mut depth = 1;
    while let Some(token) = tokens.get(*at) {
        *at += 1;
        match token.0 {
            Token::Left(BraceKind::Curly) => depth += 1,
            Token::Right(BraceKind::Curly) => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return Ok(tokens[start..*at - 1].to_vec());
        }
    }
    Err(MathRenderError::UnboundedMacro)
}

pub(super) fn validate(source: &str) -> Result<(), MathRenderError> {
    let mut lexer = Lexer::<()>::new(source, DEFAULT_SPEC.clone());
    let tokens: Vec<_> = std::iter::from_fn(|| lexer.eat())
        .filter(|token| !token.0.is_trivia())
        .collect();
    let mut definitions = BTreeMap::<&str, Definition<'_>>::new();
    let mut at = 0;
    let mut count = 0;
    while at < tokens.len() {
        let token = tokens[at];
        at += 1;
        let Some(kind) = command(token) else { continue };
        if unsupported(kind) {
            return Err(MathRenderError::UnboundedMacro);
        }
        if !declaration(kind) {
            continue;
        }
        count += 1;
        if count > MAX_DEFINITIONS {
            return Err(MathRenderError::MacroExpansionLimit);
        }
        let name_tokens = if kind == "def" {
            let name = *tokens.get(at).ok_or(MathRenderError::UnboundedMacro)?;
            at += 1;
            vec![name]
        } else {
            group(&tokens, &mut at)?
        };
        let [name] = name_tokens.as_slice() else {
            return Err(MathRenderError::UnboundedMacro);
        };
        let name = command(*name).ok_or(MathRenderError::UnboundedMacro)?;
        if name.is_empty() || declaration(name) || unsupported(name) {
            return Err(MathRenderError::UnboundedMacro);
        }
        // An explicit [0] is the same literal definition. All other parameter
        // lists are refused: an acyclic name graph alone cannot bound them.
        if tokens.get(at).map(|t| t.0) == Some(Token::Left(BraceKind::Bracket)) {
            if tokens.get(at + 1).map(|t| t.1) != Some("0")
                || tokens.get(at + 2).map(|t| t.0) != Some(Token::Right(BraceKind::Bracket))
            {
                return Err(MathRenderError::UnboundedMacro);
            }
            at += 3;
        }
        let body_start = tokens
            .get(at)
            .ok_or(MathRenderError::UnboundedMacro)?
            .1
            .as_ptr() as usize;
        let body = group(&tokens, &mut at)?;
        let definition = definitions.entry(name).or_default();
        // Include trivia omitted by the plain-token walk in the byte charge.
        // Charging the whole source per definition is a safe upper bound, but
        // the actual braced source range is tighter and preserves useful macros.
        let body_bytes = tokens[at - 1].1.as_ptr() as usize - body_start;
        definition.bytes = definition.bytes.saturating_add(body_bytes + 1);
        for token in body {
            if let Some(reference) = command(token) {
                if declaration(reference) || unsupported(reference) {
                    return Err(MathRenderError::UnboundedMacro);
                }
                definition.references.push(reference);
            }
        }
    }

    // Iterative topological evaluation avoids a recursive validator stack.
    let mut costs = BTreeMap::<&str, usize>::new();
    while costs.len() < definitions.len() {
        let before = costs.len();
        for (&name, definition) in &definitions {
            if costs.contains_key(name) {
                continue;
            }
            let children: Vec<_> = definition
                .references
                .iter()
                .filter(|name| definitions.contains_key(**name))
                .collect();
            if children.iter().all(|name| costs.contains_key(**name)) {
                let cost = children.iter().fold(definition.bytes, |total, name| {
                    total.saturating_add(costs[**name]).min(MAX_WORK + 1)
                });
                costs.insert(name, cost);
            }
        }
        if costs.len() == before {
            return Err(MathRenderError::MacroCycle);
        }
    }
    let work = tokens
        .iter()
        .filter_map(|token| command(*token))
        .filter_map(|name| costs.get(name))
        .fold(source.len(), |total, cost| total.saturating_add(*cost));
    if work > MAX_WORK {
        return Err(MathRenderError::MacroExpansionLimit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_cycles_include_redefinitions_and_def() {
        for source in [
            r"\newcommand{\a}{\a}\a",
            r"\def\a{\b}\def\b{\a}\a",
            r"\newcommand{\a}{x}\renewcommand{\a}{\b}\newcommand{\b}{\a}\a",
            r"\newcommand{\a}{\b}\newcommand{\b}{\a}",
        ] {
            assert_eq!(
                validate(source),
                Err(MathRenderError::MacroCycle),
                "{source}"
            );
        }
    }

    #[test]
    fn exponential_expansion_counts_repeated_edges_and_intermediate_work() {
        let mut source = String::new();
        for (name, next) in ('a'..='y').zip('b'..='z') {
            source.push_str(&format!(r"\newcommand{{\{name}}}{{\{next}\{next}}}"));
        }
        source.push_str(r"\newcommand{\z}{}\a");
        assert_eq!(validate(&source), Err(MathRenderError::MacroExpansionLimit));
    }

    #[test]
    fn total_invocations_and_definition_count_are_bounded() {
        let definition = format!("\\newcommand{{\\a}}{{{}}}", "x".repeat(1000));
        assert!(validate(&format!("{definition}{}", r"\a".repeat(20))).is_ok());
        assert_eq!(
            validate(&format!("{definition}{}", r"\a".repeat(40))),
            Err(MathRenderError::MacroExpansionLimit)
        );
        assert_eq!(
            validate(&r"\renewcommand{\a}{}".repeat(MAX_DEFINITIONS + 1)),
            Err(MathRenderError::MacroExpansionLimit)
        );
    }

    #[test]
    fn unproven_dynamic_macro_shapes_are_refused() {
        for source in [
            r"\newcommand{\a}[1]{#1{#1}}\a{\a}",
            r"\newenvironment{a}{\begin{a}}{}\begin{a}\end{a}",
            r"\newcommand{\a}{\newcommand{\b}{\b}}\a\b",
            r"\newcommand{\newcommand}{x}",
            r"\def\a#1{#1}",
        ] {
            assert_eq!(validate(source), Err(MathRenderError::UnboundedMacro));
        }
    }

    #[test]
    fn plain_lexer_keeps_comments_and_escaped_commands_out_of_the_graph() {
        assert!(validate("% \\newcommand{\\a}{\\a}\n x+1").is_ok());
        assert!(validate(r"\\newcommand{a}{a}").is_ok());
        assert!(validate(r"\newcommand{\a}{\b}\newcommand{\b}{\c}\newcommand{\c}{x}\a").is_ok());
        assert!(validate(r"\newcommand{\a}{#}").is_ok());
    }
}
