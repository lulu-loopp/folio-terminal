// MODIFIED BY THE FOLIO CONTRIBUTORS — not the upstream
// mitex 0.2.4 converter.
// Change: `Converter::convert` counts its own recursion and refuses past
// `mitex_parser::MAX_TREE_DEPTH`; a math-content-only mode refuses every site
// that would copy source text into the Typst output as code; and the bounded
// conversion entry point carries both refusals out under names.
// Index: vendor/mitex/CHANGES-FOLIO.md
// Notice given under section 4(b) of the Apache License, Version 2.0.

use core::fmt;
use std::cell::Cell;
use std::cell::RefCell;
use std::fmt::Write;
use std::rc::Rc;

pub use mitex_parser::spec::*;

use mitex_parser::NestingTooDeep;
use mitex_parser::syntax::CmdItem;
use mitex_parser::syntax::EnvItem;
use mitex_parser::syntax::FormulaItem;
use mitex_parser::syntax::SyntaxNode;
use mitex_spec_gen::DEFAULT_SPEC;
use rowan::ast::AstNode;

#[derive(Debug, Clone, Copy, Default)]
pub enum LaTeXMode {
    #[default]
    Text,
    Math,
}

#[derive(Debug, Clone, Copy, Default)]
enum LaTeXEnv {
    #[default]
    // Text mode
    None,
    Figure,
    Table,
    Itemize,
    Enumerate,
    // Math mode
    Math,
    Matrix,
    Cases,
    SubStack,
    MathCurlyGroup,
}

pub struct Converter {
    mode: LaTeXMode,
    env: LaTeXEnv,
    // indent for itemize and enumerate
    indent: usize,
    // label for block equation
    label: Option<String>,
    // skip the space at the beginning of the line
    skip_next_space: bool,
    /// Folio: how many levels of the syntax tree this conversion is standing inside.
    ///
    /// **Belt to `mitex-parser`'s braces.** `convert` walks the tree by recursing on children, so
    /// its stack depth is the tree's depth, and a stack overflow is not something a caller can
    /// catch. The parser already refuses to build a tree deeper than `MAX_TREE_DEPTH`, so this
    /// counter cannot fire on a tree that came from it — which is the point of keeping it: it
    /// answers for a tree that arrived some other way, and it costs one add per node.
    depth: usize,
    /// Folio: refuse anything that would put source text into the output as Typst *code*.
    ///
    /// **A formula is mathematics, not a program.** Three sites in this converter copy source
    /// text through unchanged instead of mapping it token by token — `\iftypst`'s body,
    /// `\includegraphics`'s path and lengths, and `\label`'s name — and each of them lands in a
    /// place where Typst reads code: respectively the output itself, the inside of a string
    /// literal, and markup after a `<`. Everywhere else, a source character reaches the output
    /// only through the token map below, which escapes `#`, `"`, `^`, `_`, `*`, `@`, `;`, `,`,
    /// `/`, `(`, `)`, `[` and `]`, drops `$`, `{`, `}` and comments, and otherwise emits a
    /// `Word`, whose own lexer class excludes every one of those characters. So refusing these
    /// three is what makes "the converted source is math content" true rather than hopeful.
    math_content_only: bool,
}

/// Folio: how many grid cells the emitted Typst asks a layout engine to build.
///
/// **Counted over what is emitted, not over the source constructs that were thought to create it.**
/// The first version of this counted an environment's own `&` and `\\`, and that was wrong twice:
/// `&` and a line break are emitted outside environments too — a bare `x` followed by N `&` and N
/// `\\x` is a rectangle at the root of the formula, with no environment anywhere — and a `{…}`
/// group is *flattened* into the text around it, so its ampersands belong to the enclosing scope
/// while the source counter attributed them to the group. Both were measured on 2026-09-17: an
/// eight-hundred-wide one of either shape passed the environment counter untouched. Reading the
/// converted text instead cannot be fooled by LaTeX grouping or by a macro, because the converted
/// text is exactly what Typst is going to parse.
///
/// A *scope* is a run of Typst math that is laid out as one sequence: the whole formula, and
/// whatever a `(`, `[` or `{` this converter wrote opens — a function's arguments, a `lr(…)`, an
/// attachment, a content block. Inside one, `&` and `,` each open a column and a Typst line break
/// (`\ `) and `;` each open a row, which covers alignment points and grid separators under one
/// rule; a scope costs its rows times its widest row, and a formula costs the sum. Counting an
/// argument comma as a column over-counts a little — `frac(a ,b )` is charged two — and over-
/// counting is the safe direction.
///
/// Escapes are honoured, which is the whole reason this reads characters rather than greps: the
/// converter writes a source comma as `\,` and a source hash as `\#`, and neither is a separator.
/// Iterative, with one `Vec` of open scopes, over a string the source and macro budgets have
/// already bounded.
fn layout_cells(converted: &str) -> usize {
    struct Scope {
        rows: usize,
        columns: usize,
        widest: usize,
    }

    impl Scope {
        fn new() -> Self {
            Self {
                rows: 1,
                columns: 1,
                widest: 1,
            }
        }

        fn column(&mut self) {
            self.columns += 1;
            self.widest = self.widest.max(self.columns);
        }

        fn row(&mut self) {
            self.rows += 1;
            self.columns = 1;
        }

        fn cells(&self) -> usize {
            self.rows.saturating_mul(self.widest)
        }
    }

    let mut total = 0usize;
    let mut open = vec![Scope::new()];
    let mut characters = converted.chars().peekable();
    while let Some(character) = characters.next() {
        // `open` is never emptied below, so the innermost scope is always there.
        let scope = open.last_mut().expect("a formula is a scope");
        match character {
            // `\ ` is Typst's line break; `\x` draws an `x` and is not a separator.
            '\\' => {
                if characters.next() == Some(' ') {
                    scope.row();
                }
            }
            // A string literal carries no separators of its own.
            '"' => {
                while let Some(inside) = characters.next() {
                    match inside {
                        '\\' => {
                            characters.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '(' | '[' | '{' => open.push(Scope::new()),
            ')' | ']' | '}' => {
                if open.len() > 1 {
                    let closed = open.pop().expect("more than one scope is open");
                    total = total.saturating_add(closed.cells());
                }
            }
            '&' | ',' => scope.column(),
            ';' => scope.row(),
            _ => {}
        }
    }
    for scope in &open {
        total = total.saturating_add(scope.cells());
    }
    total
}

/// Folio: the grid cells one formula may ask a layout engine to build.
///
/// Measured 2026-09-17 on the math worker's own thread, rendering a sparse `array` and a dense
/// `pmatrix` of the same cell count: the cost is linear in cells, about 60µs each under
/// `cargo test`'s `opt-level = 1` — 81 cells in 13ms, 1089 in 55ms, 2401 in 121ms, 4225 in 254ms.
/// So this bounds the time as well as the memory, and it bounds it for the whole formula rather
/// than for one scope of it.
///
/// The two shapes that need no environment — a bare row of alignment points with a column of line
/// breaks, and the same with the alignment points hidden inside a group — turn out to be far
/// cheaper in the pinned Typst, which pads them virtually: four million cells came back in 547ms,
/// refused for raster size rather than for work. They are charged at the same rate anyway. What
/// makes them cheap is an implementation detail of somebody else's layout engine, and one rule
/// that covers every rectangle is worth more than three that cover the ones already known.
///
/// Four thousand and ninety-six is a 64x64 matrix, which is far past anything written to be read —
/// a twelve-by-twelve matrix is 144, a forty-row `aligned` with three alignment points is 160, a
/// thirty-case `cases` is 60, and a three-line derivation is 6.
pub const MAX_LAYOUT_CELLS: usize = 4096;

impl Converter {
    fn new(mode: LaTeXMode, math_content_only: bool) -> Self {
        Self {
            mode,
            env: LaTeXEnv::default(),
            indent: 0,
            label: None,
            skip_next_space: true,
            depth: 0,
            math_content_only,
        }
    }

    /// Folio: refuse a site that would copy source text into the output as code.
    fn refuse_raw_typst(&self) -> Result<(), ConvertError> {
        if self.math_content_only {
            return Err(ConvertError::RawTypstCode);
        }
        Ok(())
    }

    #[must_use]
    fn enter_mode(&mut self, context: LaTeXMode) -> LaTeXMode {
        let prev = self.mode;
        self.mode = context;
        prev
    }

    fn exit_mode(&mut self, prev: LaTeXMode) {
        self.mode = prev;
    }

    #[must_use]
    fn enter_env(&mut self, context: LaTeXEnv) -> LaTeXEnv {
        let prev = self.env;
        self.env = context;
        if matches!(self.env, LaTeXEnv::Itemize | LaTeXEnv::Enumerate) {
            self.indent += 2;
        }
        prev
    }

    fn exit_env(&mut self, prev: LaTeXEnv) {
        if matches!(self.env, LaTeXEnv::Itemize | LaTeXEnv::Enumerate) {
            self.indent -= 2;
        }
        self.env = prev;
    }
}

// fn empty_node() -> GreenNode {
//     rowan::GreenNode::new(LatexSyntaxKind::TEXT.into(), [])
// }

use mitex_parser::syntax::SyntaxElement as LatexSyntaxElem;
use mitex_parser::syntax::SyntaxKind as LatexSyntaxKind;
// use mitex_parser::syntax::SyntaxNode as LatexSyntaxNode;

#[derive(Debug)]
enum ConvertError {
    Fmt(fmt::Error),
    Str(String),
    /// Folio: the tree handed to the converter is deeper than it will descend.
    NestingTooDeep,
    /// Folio: the source asked for Typst code rather than mathematics.
    RawTypstCode,
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fmt(e) => write!(f, "fmt: {}", e),
            Self::Str(e) => write!(f, "error: {}", e),
            Self::NestingTooDeep => write!(f, "error: nesting too deep"),
            Self::RawTypstCode => write!(f, "error: raw Typst code in a formula"),
        }
    }
}

impl From<fmt::Error> for ConvertError {
    fn from(e: fmt::Error) -> Self {
        Self::Fmt(e)
    }
}

impl From<String> for ConvertError {
    fn from(e: String) -> Self {
        Self::Str(e)
    }
}

/// Folio: why a bounded conversion could not answer.
///
/// The depth refusal is a variant of its own rather than a message, because it is the one failure a
/// caller has to be able to act on: it is what stands between a printed line of mathematics and a
/// recursion nobody can catch, and a reader is owed the source text rather than a diagnostic about
/// their own formula.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundedConvertError {
    /// The formula nests deeper than `mitex_parser::MAX_TREE_DEPTH`.
    NestingTooDeep,
    /// The formula asked for Typst code rather than mathematics.
    RawTypstCode,
    /// The formula asked for more than `MAX_LAYOUT_CELLS` grid cells.
    TooManyCells,
    /// Everything else MiTeX says about a formula it cannot convert.
    Convert(String),
}

impl fmt::Display for BoundedConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NestingTooDeep => NestingTooDeep.fmt(f),
            Self::RawTypstCode => f.write_str("raw Typst code in a formula"),
            Self::TooManyCells => f.write_str("too many cells in a formula"),
            Self::Convert(message) => f.write_str(message),
        }
    }
}

impl Converter {
    /// Folio: the one door into the recursive walk, so the depth is counted once and before the
    /// descent rather than after it.
    fn convert(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
    ) -> Result<(), ConvertError> {
        if self.depth >= mitex_parser::MAX_TREE_DEPTH {
            return Err(ConvertError::NestingTooDeep);
        }
        self.depth += 1;
        let converted = self.convert_element(f, elem, spec);
        self.depth -= 1;
        converted
    }

    fn convert_element(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
    ) -> Result<(), ConvertError> {
        use LatexSyntaxKind::*;

        match elem.kind() {
            TokenWhiteSpace => {}
            _ => {
                self.skip_next_space = false;
            }
        }
        match elem.kind() {
            TokenError => Err(match elem {
                LatexSyntaxElem::Node(node) => format!("error unexpected: {:?}", node.text()),
                LatexSyntaxElem::Token(token) => format!("error unexpected: {:?}", token.text()),
            })?,
            ItemLR | ClauseArgument | ScopeRoot | ItemText | ItemBracket | ItemParen => {
                for child in elem.as_node().unwrap().children_with_tokens() {
                    self.convert(f, child, spec)?;
                }
            }
            ItemFormula => {
                self.convert_formula(f, elem, spec)?;
            }
            ItemCurly => {
                self.convert_curly_group(f, elem, spec)?;
            }
            ClauseLR => {
                self.convert_clause_lr(f, elem, spec)?;
            }
            ItemAttachComponent => {
                self.convert_attach_component(f, elem, spec)?;
            }
            ClauseCommandName => Err("command name outside of command".to_owned())?,
            ItemBegin | ItemEnd => Err("clauses outside of environment".to_owned())?,
            TokenWord => {
                if matches!(self.mode, LaTeXMode::Math) {
                    // break up words into individual characters and add a space
                    let text = elem.as_token().unwrap().text().to_string();
                    for prev in text.chars() {
                        f.write_char(prev)?;
                        f.write_char(' ')?;
                    }
                } else {
                    // write the word directly in text mode
                    f.write_str(elem.as_token().unwrap().text())?;
                }
            }
            // do nothing
            TokenLBrace | TokenRBrace | TokenDollar | TokenBeginMath | TokenEndMath
            | TokenComment | ItemBlockComment => {}
            // space identical
            TokenWhiteSpace => {
                // indent for itemize and enumerate
                if self.skip_next_space {
                    self.skip_next_space = false;
                    return Ok(());
                }
                write!(f, "{}", elem.as_token().unwrap().text())?;
            }
            TokenLineBreak => {
                write!(f, "{}", elem.as_token().unwrap().text())?;
                // indent for itemize and enumerate
                for _ in 0..self.indent {
                    f.write_char(' ')?;
                }
                self.skip_next_space = true;
            }
            // escapes
            TokenApostrophe => {
                f.write_char('\'')?;
            }
            TokenComma => {
                f.write_str("\\,")?;
            }
            TokenSlash => {
                f.write_str("\\/")?;
            }
            TokenCaret => {
                f.write_str("\\^")?;
            }
            TokenUnderscore => {
                f.write_str("\\_")?;
            }
            TokenHash => {
                f.write_str("\\#")?;
            }
            TokenAsterisk => {
                f.write_str("\\*")?;
            }
            TokenAtSign => {
                f.write_str("\\@")?;
            }
            TokenDitto => {
                f.write_str("\\\"")?;
            }
            TokenSemicolon => {
                f.write_str("\\;")?;
            }
            TokenLParen => {
                f.write_str("\\(")?;
            }
            TokenRParen => {
                f.write_str("\\)")?;
            }
            TokenLBracket => {
                f.write_str("\\[")?;
            }
            TokenRBracket => {
                f.write_str("\\]")?;
            }
            TokenTilde => {
                if matches!(self.mode, LaTeXMode::Math) {
                    f.write_str("space.nobreak ")?;
                } else {
                    f.write_str("\\~")?;
                }
            }
            TokenAmpersand => match self.env {
                LaTeXEnv::Matrix => f.write_str("zws ,")?,
                _ => f.write_str("&")?,
            },
            ItemNewLine => match self.env {
                LaTeXEnv::Matrix => f.write_str("zws ;")?,
                LaTeXEnv::Cases => f.write_str(",")?,
                LaTeXEnv::MathCurlyGroup => {}
                _ => f.write_str("\\ ")?,
            },
            TokenCommandSym => {
                self.convert_command_sym(f, elem, spec)?;
            }
            ItemCmd => {
                let cmd = CmdItem::cast(elem.as_node().unwrap().clone()).unwrap();
                let name = cmd.name_tok().unwrap();
                let name = name.text();
                // remove prefix \
                let name = &name[1..];

                match name {
                    "item" => {
                        self.convert_command_item(f)?;
                    }
                    "label" => {
                        self.convert_command_label(f, &cmd)?;
                    }
                    "includegraphics" => {
                        self.convert_command_includegraphics(f, &cmd)?;
                    }
                    _ => {
                        self.convert_normal_command(f, elem, spec)?;
                    }
                }
            }
            ItemEnv => {
                self.convert_env(f, elem, spec)?;
            }
            ItemTypstCode => {
                // Folio: `\iftypst … \fi` is collected by the parser without its structure being
                // read, and emitted here verbatim — a formula that a program merely printed can
                // therefore carry a Typst program. Refused where it would be copied.
                self.refuse_raw_typst()?;
                write!(f, "{}", elem.as_node().unwrap().text())?;
            }
        };

        Ok(())
    }

    /// Convert formula like `$x$` or `$$x$$`
    fn convert_formula(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
    ) -> Result<(), ConvertError> {
        let formula = FormulaItem::cast(elem.as_node().unwrap().clone()).unwrap();
        if !formula.is_valid() {
            Err("formula is not valid".to_owned())?
        }
        if matches!(self.mode, LaTeXMode::Text) {
            if formula.is_inline() {
                f.write_str("#math.equation(block: false, $")?;
            } else {
                f.write_str("$ ")?;
            }
        }
        let prev = self.enter_mode(LaTeXMode::Math);
        for child in elem.as_node().unwrap().children_with_tokens() {
            self.convert(f, child, spec)?;
        }
        self.exit_mode(prev);
        if matches!(self.mode, LaTeXMode::Text) {
            if formula.is_inline() {
                f.write_str("$);")?;
            } else {
                f.write_str(" $")?;
            }
        }
        Ok(())
    }

    /// Convert curly group like `{abc}`
    fn convert_curly_group(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
    ) -> Result<(), ConvertError> {
        use LatexSyntaxKind::*;
        // deal with case like `\begin{pmatrix}x{\\}x\end{pmatrix}`
        let mut prev = LaTeXEnv::None;
        let mut enter_new_env = false;
        // hack for \substack{abc \\ bcd}
        if matches!(self.mode, LaTeXMode::Math) && !matches!(self.env, LaTeXEnv::SubStack) {
            prev = self.enter_env(LaTeXEnv::MathCurlyGroup);
            enter_new_env = true;
        }
        // whether to add zws for empty curly group
        let mut zws = true;
        for child in elem.as_node().unwrap().children_with_tokens() {
            match &child.kind() {
                TokenWhiteSpace | TokenLineBreak | TokenLBrace | TokenRBrace => {}
                _ => {
                    zws = false;
                }
            }
            self.convert(f, child, spec)?;
        }
        if matches!(self.mode, LaTeXMode::Math) {
            if zws {
                // deal with case like `{}_1^2x_3^4`
                f.write_str("zws ")?;
            }
            if enter_new_env {
                self.exit_env(prev);
            }
        }
        Ok(())
    }

    /// Convert \left and \right
    fn convert_clause_lr(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
    ) -> Result<(), ConvertError> {
        let name_and_args = elem
            .as_node()
            .unwrap()
            .children_with_tokens()
            .collect::<Vec<_>>();
        let name = name_and_args[0].as_token().unwrap().text();
        // remove prefix \
        let name = &name[1..];
        let args = name_and_args[1..].to_owned();
        if name == "left" {
            f.write_str("lr(")?;
        }
        for arg in args {
            match arg {
                LatexSyntaxElem::Node(node) => {
                    self.convert(f, LatexSyntaxElem::Node(node), spec)?;
                }
                LatexSyntaxElem::Token(token) => match token.kind() {
                    LatexSyntaxKind::TokenWord if token.text() == "." => {}
                    _ => self.convert(f, rowan::NodeOrToken::Token(token), spec)?,
                },
            }
            // add space
            f.write_char(' ')?;
        }
        if name == "right" {
            f.write_char(')')?;
        }
        Ok(())
    }

    /// Convert attach component like `x_1^2`
    fn convert_attach_component(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
    ) -> Result<(), ConvertError> {
        if matches!(self.mode, LaTeXMode::Math) {
            // if there is already a base, if not, we need to add zws, like `_1^2`
            let mut based = false;
            // is the first non-empty element
            let mut first = true;
            for child in elem.as_node().unwrap().children_with_tokens() {
                if first {
                    let kind = child.as_token().map(|n| n.kind());
                    // the underscore _ or caret ^ is the split point
                    if matches!(
                        kind,
                        Some(LatexSyntaxKind::TokenUnderscore | LatexSyntaxKind::TokenCaret)
                    ) {
                        if !based {
                            f.write_str("zws")?;
                        }
                        write!(f, "{}(", child.as_token().unwrap().text())?;
                        first = false;
                        continue;
                    } else if !matches!(kind, Some(LatexSyntaxKind::TokenWhiteSpace)) {
                        based = true;
                    }
                }
                self.convert(f, child, spec)?;
            }
            if !first {
                f.write_char(')')?;
            }
        } else {
            // convert directly in text mode
            for child in elem.as_node().unwrap().children_with_tokens() {
                self.convert(f, child, spec)?;
            }
        }
        Ok(())
    }

    /// Convert command symbol like `\alpha`
    fn convert_command_sym(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
    ) -> Result<(), ConvertError> {
        let name = elem.as_token().unwrap().text();
        // remove prefix \
        let name = &name[1..];
        // get cmd_shape and arg_shape from spec
        let cmd_shape = spec
            .get_cmd(name)
            .ok_or_else(|| format!("unknown command: \\{}", name))?;
        // typst alias name
        let typst_name = cmd_shape.alias.as_deref().unwrap_or(name);
        // write to output
        write!(f, "{}", typst_name)?;
        Ok(())
    }

    /// Convert command `\item` for itemize and enumerate
    fn convert_command_item(&mut self, f: &mut fmt::Formatter<'_>) -> Result<(), ConvertError> {
        if matches!(self.env, LaTeXEnv::Itemize | LaTeXEnv::Enumerate) {
            f.write_char('\n')?;
            for _ in 0..(self.indent - 2) {
                f.write_char(' ')?;
            }
            if matches!(self.env, LaTeXEnv::Itemize) {
                f.write_str("- ")?;
            } else {
                f.write_str("+ ")?;
            }
        } else {
            Err("item command outside of itemize or enumerate".to_owned())?;
        }
        Ok(())
    }

    /// Convert command `\label`
    fn convert_command_label(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        cmd: &CmdItem,
    ) -> Result<(), ConvertError> {
        let arg = cmd
            .arguments()
            .next()
            .expect("\\label command must have one argument");
        // remove { and } then trim
        let label = arg.text().to_string();
        let label = &label[1..(label.len() - 1)];
        let label = label.trim();
        match self.env {
            LaTeXEnv::None | LaTeXEnv::Itemize | LaTeXEnv::Enumerate => {
                if matches!(self.mode, LaTeXMode::Text) {
                    // Folio: `label` is the source text between the braces, unmapped, and `<…>` in
                    // Typst markup ends at the first `>` — so the rest of it is markup, where `#`
                    // is code. In math mode nothing is written here and nothing changes.
                    self.refuse_raw_typst()?;
                    f.write_char('<')?;
                    f.write_str(label)?;
                    f.write_char('>')?;
                }
            }
            _ => {
                self.label = Some(label.to_string());
            }
        }
        Ok(())
    }

    /// Convert command `\includegraphics[width=0.5\textwidth]{example-image}`
    fn convert_command_includegraphics(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        cmd: &CmdItem,
    ) -> Result<(), ConvertError> {
        // Folio: this writes `#image("<source text>")` — a Typst call whose string argument is
        // copied from the formula, quote marks and all, so a `"` in the path closes the string and
        // what follows is code. It is also a file read, which nothing in a printed line of
        // mathematics has any business asking for.
        self.refuse_raw_typst()?;
        let opt_arg = cmd.arguments().find(|arg| {
            matches!(
                arg.first_child().unwrap().kind(),
                LatexSyntaxKind::ItemBracket
            )
        });
        let arg = cmd
            .arguments()
            .find(|arg| {
                matches!(
                    arg.first_child().unwrap().kind(),
                    LatexSyntaxKind::ItemCurly
                )
            })
            .expect("\\includegraphics command must have one argument");
        // remove { and } then trim
        let body = arg.text().to_string();
        let body = &body[1..(body.len() - 1)];
        let body = body.trim();
        f.write_str("#image(")?;
        // optional arguments
        if let Some(opt_arg) = opt_arg {
            let arg_text = opt_arg.text().to_string();
            let arg_text = &arg_text[1..(arg_text.len() - 1)];
            let arg_text = arg_text.trim();
            // example: \includegraphics[width=0.5\textwidth, height=3cm,
            // angle=45]{example-image} split by comma and convert
            // to key-value pairs
            let args = arg_text.split(',').collect::<Vec<_>>();
            let args = args
                .iter()
                .map(|arg| {
                    let arg = arg.trim();
                    let arg = arg.split('=').collect::<Vec<_>>();
                    let key = arg[0].trim();
                    let value = if arg.len() == 2 { arg[1].trim() } else { "" };
                    (key, value)
                })
                .collect::<Vec<_>>();
            for (key, value) in args.iter() {
                if matches!(key, &"width" | &"height") {
                    f.write_str(key)?;
                    f.write_char(':')?;
                    f.write_char(' ')?;
                    if value.ends_with("\\textwidth") {
                        let value = value.trim_end_matches("\\textwidth");
                        f.write_str(value)?;
                        f.write_str(" * 100%")?;
                    } else if value.ends_with("\\textheight") {
                        let value = value.trim_end_matches("\\textheight");
                        f.write_str(value)?;
                        f.write_str(" * 100%")?;
                    } else {
                        f.write_str(value)?;
                    }
                    f.write_char(',')?;
                    f.write_char(' ')?;
                }
            }
        }
        // image path
        f.write_char('"')?;
        f.write_str(body)?;
        f.write_char('"')?;
        f.write_char(')')?;
        Ok(())
    }

    /// Convert normal command
    fn convert_normal_command(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
    ) -> Result<(), ConvertError> {
        let cmd = CmdItem::cast(elem.as_node().unwrap().clone()).unwrap();
        let name = cmd.name_tok().unwrap();
        let name = name.text();
        // remove prefix \
        let name = &name[1..];
        let args = elem
            .as_node()
            .unwrap()
            .children_with_tokens()
            .filter(|node| node.kind() != LatexSyntaxKind::ClauseCommandName)
            .collect::<Vec<_>>();

        // get cmd_shape and arg_shape from spec
        let cmd_shape = spec
            .get_cmd(name)
            .ok_or_else(|| format!("unknown command: \\{}", name))?;
        let arg_shape = &cmd_shape.args;

        // typst alias name
        let mut typst_name = cmd_shape.alias.as_deref().unwrap_or(name);

        // hack for textbf and textit
        if matches!(self.mode, LaTeXMode::Text) {
            if name == "textbf" {
                typst_name = "#strong";
            } else if name == "textit" {
                typst_name = "#emph";
            }
        }

        // normal command
        write!(f, "{}", typst_name)?;

        // hack for \substack{abc \\ bcd}
        let mut prev = LaTeXEnv::None;
        if typst_name == "substack" {
            prev = self.enter_env(LaTeXEnv::SubStack);
        }

        if let ArgShape::Right {
            pattern: ArgPattern::None,
        } = arg_shape
        {
            f.write_char(' ')?
        } else if let ArgShape::Right {
            pattern: ArgPattern::Greedy,
        } = arg_shape
        {
            f.write_char('(')?;
            // there is only one arg in greedy
            let args = args
                .first()
                .unwrap()
                .as_node()
                .unwrap()
                .children_with_tokens()
                .collect::<Vec<_>>();
            let mut cnt = 0;
            let args_len = args.len();
            for arg in args {
                cnt += 1;
                let kind = arg.kind();
                self.convert(f, arg, spec)?;
                if matches!(kind, LatexSyntaxKind::ItemCurly) && cnt != args_len {
                    f.write_char(',')?;
                }
            }

            f.write_char(')')?;
        } else if matches!(self.mode, LaTeXMode::Math) && !typst_name.starts_with('#') {
            f.write_char('(')?;

            let mut cnt = 0;
            let args_len = args.len();
            for arg in args {
                cnt += 1;
                let kind = arg.kind();
                self.convert(f, arg, spec)?;
                if matches!(kind, LatexSyntaxKind::ClauseArgument) && cnt != args_len {
                    f.write_char(',')?;
                }
            }

            f.write_char(')')?;
        } else {
            // Text mode
            for arg in args {
                let kind = arg.kind();
                if matches!(kind, LatexSyntaxKind::ClauseArgument) {
                    f.write_char('[')?;
                    let prev_mode = self.enter_mode(LaTeXMode::Text);
                    self.convert(f, arg, spec)?;
                    self.exit_mode(prev_mode);
                    f.write_char(']')?;
                }
            }
            f.write_char(';')?;
        }

        // hack for \substack{abc \\ bcd}
        if typst_name == "substack" {
            self.exit_env(prev);
        }

        Ok(())
    }

    /// Convert environments
    fn convert_env(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
    ) -> Result<(), ConvertError> {
        let env = EnvItem::cast(elem.as_node().unwrap().clone()).unwrap();
        let name = env
            .name_tok()
            .expect("environment name must be non-empty")
            .text()
            .to_string();
        let name = name.trim();
        let args = env.arguments();

        let env_shape = spec
            .get_env(name)
            .ok_or_else(|| format!("unknown environment: \\{}", name))?;
        let typst_name = env_shape.alias.as_deref().unwrap_or(name);

        let env_kind = match env_shape.ctx_feature {
            ContextFeature::None => LaTeXEnv::None,
            ContextFeature::IsMath => LaTeXEnv::Math,
            ContextFeature::IsMatrix => LaTeXEnv::Matrix,
            ContextFeature::IsCases => LaTeXEnv::Cases,
            ContextFeature::IsFigure => LaTeXEnv::Figure,
            ContextFeature::IsTable => LaTeXEnv::Table,
            ContextFeature::IsItemize => LaTeXEnv::Itemize,
            ContextFeature::IsEnumerate => LaTeXEnv::Enumerate,
        };

        // hack for itemize and enumerate
        if matches!(env_kind, LaTeXEnv::Itemize | LaTeXEnv::Enumerate) {
            let prev = self.enter_env(env_kind);

            for child in elem.as_node().unwrap().children_with_tokens() {
                if matches!(
                    child.kind(),
                    LatexSyntaxKind::ItemBegin | LatexSyntaxKind::ItemEnd
                ) {
                    continue;
                }

                self.convert(f, child, spec)?;
            }

            self.exit_env(prev);

            return Ok(());
        }

        // is environment for math
        let is_math_env = matches!(
            env_kind,
            // math environments
            LaTeXEnv::Math
                | LaTeXEnv::Matrix
                | LaTeXEnv::Cases
                | LaTeXEnv::SubStack
                | LaTeXEnv::MathCurlyGroup
        );

        // convert math environments into functions like `mat(a, b; c, d)`
        if is_math_env {
            // text mode to math mode with $ ... $
            let with_dollar = matches!(self.mode, LaTeXMode::Text) && is_math_env;
            let prev = self.enter_env(env_kind);
            let prev_mode = self.enter_mode(LaTeXMode::Math);
            if with_dollar {
                f.write_str("$ ")?;
            }

            // environment name
            f.write_str(typst_name)?;

            f.write_char('(')?;
            // named args
            for (index, arg) in args.enumerate() {
                f.write_str(format!("arg{}: ", index).as_str())?;
                self.convert(f, rowan::NodeOrToken::Node(arg), spec)?;
                f.write_char(',')?;
            }

            for child in elem.as_node().unwrap().children_with_tokens() {
                // skip \begin and \end commands
                if matches!(
                    child.kind(),
                    LatexSyntaxKind::ItemBegin | LatexSyntaxKind::ItemEnd
                ) {
                    continue;
                }
                self.convert(f, child, spec)?;
            }

            f.write_char(')')?;

            self.exit_env(prev);

            if with_dollar {
                f.write_str(" $")?;
                self.exit_mode(prev_mode);
            }
        } else {
            // convert text environments
            // 1. \begin{quote}xxx\end{quote} -> #quote(block: true)[xxx]
            //    \begin{abstract}xxx\end{abstract} -> #quote(block: true)[xxx]
            // 2. \begin{figure}xxx\end{figure} -> #figure(image(), caption: [])
            //    \begin{table}xxx\end{table} -> #figure(table(), caption: [])
            // 3. \begin{tabular}xxx\end{tabular}
            // environment name
            match env_kind {
                LaTeXEnv::Figure => {
                    self.convert_env_figure(f, elem, spec, env_kind, typst_name)?;
                }
                LaTeXEnv::Table => {
                    self.convert_env_table(f, elem, spec, env_kind, typst_name)?;
                }
                _ => {
                    // normal environment
                    let prev = self.enter_env(env_kind);
                    f.write_char('#')?;
                    f.write_str(typst_name)?;
                    f.write_char('[')?;
                    for child in elem.as_node().unwrap().children_with_tokens() {
                        // skip \begin and \end commands
                        if matches!(
                            child.kind(),
                            LatexSyntaxKind::ItemBegin | LatexSyntaxKind::ItemEnd
                        ) {
                            continue;
                        }
                        self.convert(f, child, spec)?;
                    }
                    f.write_str("];")?;
                    self.exit_env(prev);
                }
            }
        }

        // handle label, only add <label> for text mode
        if matches!(self.mode, LaTeXMode::Text) {
            if let Some(label) = self.label.take() {
                // Folio: the same unmapped source text as `convert_command_label`'s.
                self.refuse_raw_typst()?;
                f.write_char('<')?;
                f.write_str(label.as_str())?;
                f.write_char('>')?;
                self.label = None;
            }
        }

        Ok(())
    }

    /// Convert figure environment
    fn convert_env_figure(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
        env_kind: LaTeXEnv,
        typst_name: &str,
    ) -> Result<(), ConvertError> {
        fn is_named_arg(child: &LatexSyntaxElem) -> bool {
            matches!(
                child.kind(),
                LatexSyntaxKind::ItemCmd
                    if matches!(
                        CmdItem::cast(child.as_node().unwrap().clone())
                            .unwrap()
                            .name_tok()
                            .unwrap()
                            .text(),
                        "\\caption" | "\\centering"
                    )
            )
        }
        // collect named args
        let mut caption = None;
        for child in elem.as_node().unwrap().children_with_tokens() {
            if is_named_arg(&child) {
                let cmd = CmdItem::cast(child.as_node().unwrap().clone()).unwrap();
                let name = cmd.name_tok().unwrap();
                let name = name.text();
                // remove prefix \
                let name = &name[1..];
                if name == "caption" {
                    let arg = cmd
                        .arguments()
                        .next()
                        .expect("\\caption command must have one argument");
                    caption = Some(arg);
                }
            }
        }
        // convert to #figure
        let prev = self.enter_env(env_kind);
        f.write_char('#')?;
        f.write_str(typst_name)?;
        f.write_char('(')?;
        if let Some(caption) = caption {
            f.write_str("caption: [")?;
            self.convert(f, caption.into(), spec)?;
            f.write_str("],")?;
        }
        f.write_str(")[")?;
        for child in elem.as_node().unwrap().children_with_tokens() {
            // skip \begin and \end commands
            if matches!(
                child.kind(),
                LatexSyntaxKind::ItemBegin | LatexSyntaxKind::ItemEnd
            ) || matches!(child.kind(), LatexSyntaxKind::ItemCmd) && is_named_arg(&child)
            {
                continue;
            }
            self.convert(f, child, spec)?;
        }
        f.write_str("];")?;
        self.exit_env(prev);

        Ok(())
    }

    /// Convert tabular environment
    fn convert_env_table(
        &mut self,
        f: &mut fmt::Formatter<'_>,
        elem: LatexSyntaxElem,
        spec: &CommandSpec,
        env_kind: LaTeXEnv,
        typst_name: &str,
    ) -> Result<(), ConvertError> {
        let env = EnvItem::cast(elem.as_node().unwrap().clone()).unwrap();
        let arg = env
            .arguments()
            .next()
            .expect("tabular environment must have one argument");
        let arg = arg.text().to_string();
        // remove { and } then trim
        let arg = &arg[1..(arg.len() - 1)];
        let arg = arg.trim();
        // split arg to get alignments and vertical lines
        let mut alignments: Vec<char> = vec![];
        let mut vlines: Vec<usize> = vec![];
        let mut index = 0;
        for ch in arg.chars() {
            match ch {
                'l' | 'c' | 'r' => {
                    alignments.push(ch);
                    index += 1;
                }
                '|' => {
                    vlines.push(index);
                }
                _ => {
                    // unknown character
                    Err(format!("unknown alignment: {}", ch))?;
                }
            }
        }
        // convert to #figure
        let prev = self.enter_env(env_kind);
        f.write_char('#')?;
        f.write_str(typst_name)?;
        f.write_char('(')?;
        // stroke: none,
        f.write_str("stroke: none,\n")?;
        // columns: 2,
        f.write_str(format!("columns: {},\n", alignments.len()).as_str())?;
        // align: (left, center, right, ),
        f.write_str("align: (")?;
        for align in alignments {
            match align {
                'l' => f.write_str("left, ")?,
                'c' => f.write_str("center, ")?,
                'r' => f.write_str("right, ")?,
                _ => {}
            }
        }
        f.write_str("),\n")?;
        // table.vline(x: 1),
        for vline in vlines {
            f.write_str(format!("table.vline(stroke: .5pt, x: {}), ", vline).as_str())?;
        }
        f.write_str("\n")?;
        let mut exterior = true;
        for child in elem.as_node().unwrap().children_with_tokens() {
            // skip \begin and \end commands
            if matches!(
                child.kind(),
                LatexSyntaxKind::ItemBegin | LatexSyntaxKind::ItemEnd
            ) {
                continue;
            }
            // preprocess if exterior
            if exterior {
                match child.kind() {
                    LatexSyntaxKind::TokenWhiteSpace | LatexSyntaxKind::TokenLineBreak => {}
                    LatexSyntaxKind::TokenAmpersand => {
                        // write `[],` for empty exterior when encountering &
                        f.write_str("[], ")?;
                    }
                    LatexSyntaxKind::ItemNewLine => {
                        // write `[],\n` for empty exterior when encountering //
                        f.write_str("[],\n")?;
                    }
                    LatexSyntaxKind::ItemCmd => {
                        let cmd_name = CmdItem::cast(child.as_node().unwrap().clone())
                            .unwrap()
                            .name_tok()
                            .unwrap();
                        let cmd_name = &cmd_name.text()[1..];
                        match cmd_name {
                            "hline" => {
                                f.write_str("table.hline(stroke: .5pt),\n")?;
                            }
                            "toprule" => {
                                f.write_str("table.hline(stroke: 1pt),\n")?;
                            }
                            "midrule" => {
                                f.write_str("table.hline(stroke: .5pt),\n")?;
                            }
                            "bottomrule" => {
                                f.write_str("table.hline(stroke: 1pt),\n")?;
                            }
                            _ => {
                                // enter interior for normal commands
                                exterior = false;
                                f.write_char('[')?;
                            }
                        }
                    }
                    _ => {
                        // enter interior for normal text
                        exterior = false;
                        f.write_char('[')?;
                    }
                }
            }
            // process if interior
            if !exterior {
                match child.kind() {
                    LatexSyntaxKind::TokenAmpersand => {
                        // write `[],` when encountering &
                        f.write_str("], ")?;
                        exterior = true;
                    }
                    LatexSyntaxKind::ItemNewLine => {
                        // write `[],\n` when encountering //
                        f.write_str("],\n")?;
                        exterior = true;
                    }
                    _ => {
                        self.convert(f, child, spec)?;
                    }
                }
            }
        }
        f.write_str(");")?;
        self.exit_env(prev);
        Ok(())
    }
}

struct TypstRepr {
    elem: LatexSyntaxElem,
    mode: LaTeXMode,
    spec: CommandSpec,
    error: Rc<RefCell<String>>,
    /// Folio: the conversion is held to math content only.
    math_content_only: bool,
    /// Folio: a refusal for depth, kept apart from the message so a caller can act on it.
    nesting_too_deep: Rc<Cell<bool>>,
    /// Folio: a refusal for raw Typst code, likewise.
    raw_typst_code: Rc<Cell<bool>>,
}

impl fmt::Display for TypstRepr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut ctx = Converter::new(self.mode, self.math_content_only);
        if let Err(e) = ctx.convert(f, self.elem.clone(), &self.spec) {
            match e {
                ConvertError::NestingTooDeep => self.nesting_too_deep.set(true),
                ConvertError::RawTypstCode => self.raw_typst_code.set(true),
                _ => {}
            }
            self.error.borrow_mut().push_str(&e.to_string());
            return Err(fmt::Error);
        }
        Ok(())
    }
}

#[inline(always)]
pub fn convert_inner(
    input: &str,
    mode: LaTeXMode,
    spec: Option<CommandSpec>,
    do_parse: fn(input: &str, spec: CommandSpec) -> SyntaxNode,
) -> Result<String, String> {
    let node = do_parse(input, spec.unwrap_or_else(|| DEFAULT_SPEC.clone()));
    convert_node(node, mode, false).map_err(|error| error.to_string())
}

/// Folio: [`convert_inner`] over a parse that may itself have been refused for depth, and with the
/// conversion held to math content only.
#[inline(always)]
pub fn convert_inner_bounded(
    input: &str,
    mode: LaTeXMode,
    spec: Option<CommandSpec>,
    do_parse: fn(input: &str, spec: CommandSpec) -> Result<SyntaxNode, NestingTooDeep>,
) -> Result<String, BoundedConvertError> {
    let node = do_parse(input, spec.unwrap_or_else(|| DEFAULT_SPEC.clone()))
        .map_err(|NestingTooDeep| BoundedConvertError::NestingTooDeep)?;
    convert_node(node, mode, true)
}

fn convert_node(
    node: SyntaxNode,
    mode: LaTeXMode,
    math_content_only: bool,
) -> Result<String, BoundedConvertError> {
    // println!("{:#?}", node);
    // println!("{:#?}", node.text());
    let mut output = String::new();
    let err = String::new();
    let err = Rc::new(RefCell::new(err));
    let too_deep = Rc::new(Cell::new(false));
    let raw_code = Rc::new(Cell::new(false));
    let repr = TypstRepr {
        elem: LatexSyntaxElem::Node(node),
        mode,
        spec: DEFAULT_SPEC.clone(),
        error: err.clone(),
        math_content_only,
        nesting_too_deep: too_deep.clone(),
        raw_typst_code: raw_code.clone(),
    };
    core::fmt::write(&mut output, format_args!("{}", repr)).map_err(|_| {
        if too_deep.get() {
            BoundedConvertError::NestingTooDeep
        } else if raw_code.get() {
            BoundedConvertError::RawTypstCode
        } else {
            BoundedConvertError::Convert(err.borrow().to_owned())
        }
    })?;
    // Folio: the last thing asked of a formula, and the only one that can be asked of *what it
    // became*. The text below is precisely what Typst will parse, so a rectangle in it cannot have
    // been hidden by a brace, by an environment name or by a macro.
    if math_content_only && layout_cells(&output) > MAX_LAYOUT_CELLS {
        return Err(BoundedConvertError::TooManyCells);
    }
    Ok(output)
}
