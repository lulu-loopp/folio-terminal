//! **The gated regions: every stretch of a file a `#[cfg]` keeps out of every product build**
//! (T-CFG-TEST-USE-IS-NOT-PRODUCT, plan row 77).
//!
//! [`crate::Occurrence::in_the_product`] reads three grains — the file, the innermost item and
//! the innermost inline module — and none of them sees a gate written on anything else: a
//! module-level `#[cfg(test)] use`, `const`, `static` or `type`, an `impl` or a `trait` block's
//! own bytes, a `macro_rules!` or an item macro, an `extern` block, or a `let`, an expression, a
//! match arm or a field initialiser inside a product function. Each of those was counted as
//! product whatever its `cfg` said (the root's `#[cfg(test)] use winit::event::{ElementState,
//! MouseButton};` kept two pointer-debt rows K2 batch 23 had emptied). This is the fourth grain:
//! one [`Span`] per node whose own `cfg`s say [`Compilation::NeverInProduct`], read at every
//! position the language lets a `cfg` stand on —
//!
//! * the file's inner attributes; every item, `impl` item, `trait` item and foreign item;
//! * every `let`, statement macro, expression, match arm and field initialiser;
//! * every field, variant, function parameter (typed or `self`) and field pattern;
//! * every generic parameter;
//! * inside a macro's tokens, the one body this walk reads (below).
//!
//! A gated node's bytes are the gate, attributes included, and nothing inside it is read again
//! (no product build compiles any of it). A `cfg` on any other node `syn` parses is refused
//! ([`Rejection::UnplacedCfg`]), so a parsed position this walk does not know is a refusal and
//! never a silent product count; so is a `cfg` whose predicate the three-valued reading cannot
//! read ([`Rejection::UnreadableCfg`]: a malformed predicate, or the literal `true` or `false`)
//! and a `cfg_attr` that writes a `cfg` ([`Rejection::ConditionalCfg`]).
//!
//! **Inside a macro's tokens (T-CFG-INSIDE-MACRO-TOKENS, plan row 109).** `syn` does not parse a
//! macro's tokens — an invocation's (`foo! { #[cfg(test)] X, }`, `vec![#[cfg(x)] a]`) or a
//! `macro_rules!` definition's — so a `cfg` there is read only where this walk knows what the
//! macro makes of it. The grammar:
//!
//! * **Read: a list of attribute-carrying names.** A `macro_rules!` carrying no `cfg` and no
//!   attribute that may run a macro (below), with one arm whose matcher is
//!   `$( $(#[$a:meta])* $v:ident, )*`, and whose transcriber is top-level items (attributes, an
//!   optional `pub`, then `impl`, `enum`, `struct`, `union`, `trait` or `fn` up to its brace body,
//!   or `const`, `static`, `type` or `use` up to its `;`) carrying no attribute that may run a
//!   macro, each of which either a `cfg` keeps out of the product, or is `[pub] enum Name { $(
//!   $(#[$a])* $v, )* }` with exactly that body, or names neither `$a` nor `$v`. So every name
//!   lands as a variant behind its own attributes, where the compiler applies them, and nowhere
//!   else a product build compiles — not inside another macro's tokens (`stringify!`, `concat!`,
//!   a local macro), where the attributes would gate nothing. The definition's top-level items
//!   are read like items (a gated one is a gate). An invocation by a one-segment path that
//!   resolves to such a definition is read as `$( $(#[…])* name, )*`: each entry's attributes
//!   are read as a variant's are, and a gated entry's bytes, from its first attribute to its
//!   name, are a gate. A body that is not that list is refused
//!   ([`Rejection::UnreadableMacroBody`]). The tree's one such macro is
//!   `crates/bt-app/src/i18n.rs`' `text_entries!`.
//! * **Resolution.** The nearest definition of the name written earlier in the same file, in a
//!   scope (the file, an inline module, a block) that holds the invocation, with no hazard written
//!   between them in a scope that holds it. A hazard is anything that may define another macro of
//!   that name where this walk cannot see it: a macro invocation in item or statement position
//!   that is not itself read, and an item carrying an attribute that may run a macro — an
//!   attribute outside the language's own (an attribute macro), a `derive` of anything but the
//!   language's derives, `macro_use`, or a `cfg_attr` writing any of those. A definition that a
//!   `cfg` keeps out of some build is never read, so an invocation of it is refused.
//! * **Refused: every other `cfg` in a macro's tokens** ([`Rejection::CfgInMacroTokens`]): a
//!   `#[cfg]`, a `#![cfg]` or a `cfg_attr` that writes one, at any depth of the tokens of any
//!   other invocation (`thread_local!`, `vec!`, a path-qualified or unresolved one) or definition,
//!   or of an ungated item of a read definition. The refusal names the innermost macro written
//!   around the attribute — the one whose grammar would have to be read — so a new macro carrying
//!   a `cfg` is a refusal naming its file, line and path, never a silent product count; the read
//!   shape grows only by a row that brings its grammar, a fixture and a pin.
//! * Not refused: any other attribute in a macro's tokens (a doc comment, `derive`, a `cfg_attr`
//!   that writes no `cfg`), `cfg!(…)` (an expression, not a gate), and anything inside a gated
//!   node, which is not read again.
//! * **Under an attribute macro (T-GATE-WALK-UNDER-ATTRIBUTE-MACROS, plan row 122).** An item,
//!   `impl` item, `trait` item or foreign item that carries an attribute that may rewrite it
//!   ([`may_rewrite`]) hands its tokens to that macro, which may emit anything; so, once the
//!   item's own `cfg`s are read (the compiler applies them before any attribute macro), every
//!   `cfg` and `cfg`-writing `cfg_attr` inside it, at any depth and inside macro tokens too, is
//!   refused ([`Rejection::CfgUnderAttributeMacro`], naming the outermost such attribute), an
//!   entry of a read list included. The item itself is not refused. Inert, and read through: the
//!   language's own attributes, the tool namespaces (not a `::`-led path, which names a crate),
//!   a `derive` of anything, a derive helper beside a derive that declares it, spelt as the table
//!   holds it ([`DERIVE_HELPERS`], a closed table of bare and declaring-crate spellings), and a
//!   `cfg_attr` writing only those; anything else may rewrite, a `cfg_attr` writing it whatever
//!   its predicate. Not reached: a `use` that replaces a built-in attribute macro (`use
//!   test_log::test;`), since the allow-list is keyed by name (the review's N2; none in the
//!   tree). The item is also a hazard when an attribute may run a macro, as above.
//! * **Not reached (a limit older than this row):** a macro written in an attribute's value
//!   (`#[doc = m!(…)]`) and the tokens `syn` keeps unparsed in a `Verbatim` node are visited by
//!   neither reading, so a `cfg` there is neither read nor refused (no attribute-value macro in
//!   the tree carries one: the lexical scan of 2026-10-30, plan row 109's report §1).

use std::collections::BTreeSet;
use std::ops::Range;
use std::path::Path;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::ext::IdentExt as _;
use syn::parse::{ParseStream, Parser as _};
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::declarations::{
    Compilation, cfg_attr_writes_a_cfg, cfg_is_readable, cfg_predicates, expr_attrs, names_cfg,
};
use crate::index::Span;
use crate::lower::{BUILT_IN_ATTRIBUTES, BUILT_IN_DERIVES, at_union};
use crate::reject::{Position, Rejection};

/// Walk `file` (parsed from `text`, which starts at union offset `base`) and push the span of
/// every node its own `cfg`s keep out of the product onto `gates`, and every `cfg` this reading
/// cannot place or read onto `rejections`.
pub(crate) fn read(
    file: &syn::File,
    text: &str,
    base: usize,
    path: &Path,
    gates: &mut Vec<Span>,
    rejections: &mut Vec<Rejection>,
) {
    let mut walk = Gates {
        text,
        base,
        path,
        gates,
        rejections,
        read: BTreeSet::new(),
        scopes: Vec::new(),
        definitions: Vec::new(),
        hazards: Vec::new(),
        rewritten_by: None,
    };
    walk.visit_file(file);
}

struct Gates<'a> {
    text: &'a str,
    base: usize,
    path: &'a Path,
    gates: &'a mut Vec<Span>,
    rejections: &'a mut Vec<Rejection>,
    /// The start of every attribute a placed node's reading has seen, so that the visit of the
    /// attribute itself — which `syn` makes for every attribute of every node — can tell a `cfg`
    /// read at its node from one written where no gate is read.
    read: BTreeSet<usize>,
    /// The scopes open at the walk's position — the file, every inline module, every block — in
    /// file offsets: the stretch a `macro_rules!` written directly in one is visible to by name.
    scopes: Vec<Range<usize>>,
    /// Every `macro_rules!` the walk has passed, in the order written.
    definitions: Vec<Definition>,
    /// Every place the walk has passed that could give a name a definition it does not see.
    hazards: Vec<Hazard>,
    /// While the walk is inside an item that carries an attribute macro ([`may_rewrite`]): that
    /// attribute, as written between its brackets. The macro receives the item's tokens and may
    /// emit anything, so no `cfg` in there is read as a gate; each is refused.
    rewritten_by: Option<String>,
}

/// One `macro_rules!`, and whether its body is the one this walk reads.
struct Definition {
    name: String,
    /// Where it ends: an invocation at or after this offset may be one of it.
    end: usize,
    /// The scope it is written in, which is where it is visible.
    scope: Range<usize>,
    /// Whether it is a list of attribute-carrying names (the module's grammar).
    reads_entries: bool,
}

/// A macro invocation in item or statement position this walk does not read, or an item carrying
/// an attribute that may run a macro ([`may_run_a_macro`]): either may define a `macro_rules!` of
/// any name, visible from `at` on in `scope`.
struct Hazard {
    at: usize,
    scope: Range<usize>,
}

impl Gates<'_> {
    /// Read the gate `attrs` write on `node`: refuse what does not read, and when the node is
    /// kept out of every product build, record its bytes and say so — the caller then does not
    /// descend into it.
    fn gated<T: Spanned>(&mut self, attrs: &[syn::Attribute], node: &T) -> bool {
        self.gated_range(attrs, node.span().byte_range())
    }

    /// [`Self::gated`] for a node `syn` does not hold, given by its bytes.
    fn gated_range(&mut self, attrs: &[syn::Attribute], range: Range<usize>) -> bool {
        let mut cfg = false;
        for attribute in attrs {
            let at = attribute.pound_token.span.byte_range().start;
            self.read.insert(at);
            if self.rewritten_by.is_some() {
                self.refuse_under_the_rewrite(attribute);
                continue;
            }
            self.judge(attribute);
            cfg |= attribute.path().is_ident("cfg");
        }
        if !cfg || cfg_predicates(attrs, self.text).1 != Compilation::NeverInProduct {
            return false;
        }
        let start = attrs
            .iter()
            .map(|attribute| attribute.pound_token.span.byte_range().start)
            .chain([range.start])
            .min()
            .unwrap_or(range.start);
        let end = range.end.max(start);
        self.gates.push(Span::new(
            at_union(self.base, start),
            at_union(self.base, end),
        ));
        true
    }

    /// The refusals one attribute can earn wherever it stands.
    fn judge(&mut self, attribute: &syn::Attribute) {
        if attribute.path().is_ident("cfg") && !cfg_is_readable(attribute) {
            self.rejections.push(Rejection::UnreadableCfg {
                file: self.path.to_path_buf(),
                at: position(attribute.pound_token.span),
                spelling: spelling(attribute, self.text),
            });
        }
        if cfg_attr_writes_a_cfg(attribute) {
            self.rejections.push(Rejection::ConditionalCfg {
                file: self.path.to_path_buf(),
                at: position(attribute.pound_token.span),
                spelling: spelling(attribute, self.text),
            });
        }
    }

    /// Inside an item an attribute macro may rewrite: refuse `attribute` when it is a gate — a
    /// `cfg`, or a `cfg_attr` that writes one.
    fn refuse_under_the_rewrite(&mut self, attribute: &syn::Attribute) {
        let Some(attribute_macro) = &self.rewritten_by else {
            return;
        };
        if attribute.path().is_ident("cfg") || cfg_attr_writes_a_cfg(attribute) {
            self.rejections.push(Rejection::CfgUnderAttributeMacro {
                file: self.path.to_path_buf(),
                at: position(attribute.pound_token.span),
                attribute_macro: attribute_macro.clone(),
                attribute: self.text[attribute.span().byte_range()].to_owned(),
            });
        }
    }

    /// Visit an item, an `impl` item, a `trait` item or a foreign item whose own `cfg`s have been
    /// read and keep it in: under its attribute macro, when it carries one and the walk is not
    /// already under another, and as a hazard when one of its attributes may run a macro.
    fn item_with(&mut self, attrs: &[syn::Attribute], at: usize, visit: impl FnOnce(&mut Self)) {
        if attrs
            .iter()
            .any(|attribute| may_run_a_macro(&attribute.meta))
        {
            self.hazards.push(Hazard {
                at,
                scope: self.scope(),
            });
        }
        if self.rewritten_by.is_some() {
            visit(self);
            return;
        }
        let derives = derived(attrs);
        self.rewritten_by = attrs
            .iter()
            .find(|attribute| may_rewrite(&attribute.meta, &derives))
            .map(|attribute| self.text[attribute.meta.span().byte_range()].to_owned());
        visit(self);
        self.rewritten_by = None;
    }

    /// Visit a scope's contents with the scope open.
    fn within(&mut self, scope: Range<usize>, visit: impl FnOnce(&mut Self)) {
        self.scopes.push(scope);
        visit(self);
        self.scopes.pop();
    }

    fn scope(&self) -> Range<usize> {
        self.scopes.last().cloned().unwrap_or(0..self.text.len())
    }

    /// Whether the invocation `path!` starting at `at` is one of a definition whose body this
    /// walk reads (the module's grammar: one segment, the nearest earlier definition of the name
    /// in a scope holding the invocation, and no hazard between them in a scope holding it).
    fn resolves_to_entries(&self, path: &syn::Path, at: usize) -> bool {
        if path.leading_colon.is_some() || path.segments.len() != 1 {
            return false;
        }
        let segment = &path.segments[0];
        if !segment.arguments.is_none() {
            return false;
        }
        let name = segment.ident.to_string();
        let Some(definition) = self.definitions.iter().rev().find(|definition| {
            definition.name == name && definition.end <= at && definition.scope.contains(&at)
        }) else {
            return false;
        };
        let shadowed = self.hazards.iter().any(|hazard| {
            hazard.at >= definition.end && hazard.at < at && hazard.scope.contains(&at)
        });
        definition.reads_entries && !shadowed
    }

    /// A macro invocation anywhere: read its entries when it is one of a read definition,
    /// refuse every `cfg` in its tokens otherwise. `item_position` says its expansion is items
    /// or statements of the current scope, which may define a macro.
    fn invocation(&mut self, mac: &syn::Macro, item_position: bool) {
        let at = mac.path.span().byte_range().start;
        if self.resolves_to_entries(&mac.path, at) {
            self.read_entries(mac);
            return;
        }
        if item_position {
            self.hazards.push(Hazard {
                at,
                scope: self.scope(),
            });
        }
        self.refuse_cfgs(mac.tokens.clone(), &path_spelling(&mac.path));
    }

    /// An invocation of a read definition: `$( $(#[…])* name, )*`, each entry read as a variant.
    fn read_entries(&mut self, mac: &syn::Macro) {
        let list = |input: ParseStream<'_>| -> syn::Result<Vec<(Vec<syn::Attribute>, syn::Ident)>> {
            let mut entries = Vec::new();
            while !input.is_empty() {
                let attrs = input.call(syn::Attribute::parse_outer)?;
                let name = input.call(syn::Ident::parse_any)?;
                input.parse::<syn::Token![,]>()?;
                entries.push((attrs, name));
            }
            Ok(entries)
        };
        match mac.parse_body_with(list) {
            Ok(entries) => {
                for (attrs, name) in &entries {
                    self.gated(attrs, name);
                }
            }
            Err(error) => self.rejections.push(Rejection::UnreadableMacroBody {
                file: self.path.to_path_buf(),
                at: position(mac.path.span()),
                macro_path: path_spelling(&mac.path),
                reason: error.to_string(),
            }),
        }
    }

    /// A `macro_rules! name { … }` carrying `attrs`: read its items when it is a list of
    /// attribute-carrying names that every product build compiles as written, refuse every
    /// `cfg` in its tokens otherwise; then make it visible by name.
    fn definition(&mut self, attrs: &[syn::Attribute], mac: &syn::Macro, name: &syn::Ident) {
        let spelt = format!("macro_rules! {name}");
        // A definition a `cfg` keeps out of some build leaves the name to another definition
        // there; one an attribute macro may rewrite is not the tokens written.
        let as_written = !attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("cfg") || may_run_a_macro(&attribute.meta));
        let read = as_written
            .then(|| entries_definition(mac.tokens.clone(), self.text))
            .flatten();
        let reads_entries = match read {
            Some(items) => {
                for item in items {
                    if !self.gated_range(&item.attrs, item.range) {
                        self.refuse_cfgs(item.rest, &spelt);
                    }
                }
                true
            }
            None => {
                self.refuse_cfgs(mac.tokens.clone(), &spelt);
                false
            }
        };
        self.definitions.push(Definition {
            name: name.to_string(),
            end: mac.delimiter.span().close().byte_range().end,
            scope: self.scope(),
            reads_entries,
        });
    }

    /// Refuse every `#[cfg]`, `#![cfg]` and `cfg`-writing `cfg_attr` in `tokens`, at any depth,
    /// naming the innermost macro written around it (`macro_path` until another opens).
    fn refuse_cfgs(&mut self, tokens: TokenStream, macro_path: &str) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        let mut at = 0;
        while at < trees.len() {
            if let Some((group, next)) = attribute_at(&trees, at) {
                if writes_a_gate(group) {
                    let from = trees[at].span().byte_range().start;
                    let to = group.span().byte_range().end;
                    let file = self.path.to_path_buf();
                    let written_at = position(trees[at].span());
                    let attribute = self.text[from..to].to_owned();
                    self.rejections.push(match &self.rewritten_by {
                        Some(attribute_macro) => Rejection::CfgUnderAttributeMacro {
                            file,
                            at: written_at,
                            attribute_macro: attribute_macro.clone(),
                            attribute,
                        },
                        None => Rejection::CfgInMacroTokens {
                            file,
                            at: written_at,
                            macro_path: macro_path.to_owned(),
                            attribute,
                        },
                    });
                }
                at = next;
                continue;
            }
            if let TokenTree::Group(group) = &trees[at] {
                let inner = macro_opened_by(&trees, at);
                self.refuse_cfgs(group.stream(), inner.as_deref().unwrap_or(macro_path));
            }
            at += 1;
        }
    }
}

impl<'ast> Visit<'ast> for Gates<'_> {
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        let at = attribute.pound_token.span.byte_range().start;
        if self.read.contains(&at) {
            return;
        }
        if self.rewritten_by.is_some() {
            self.refuse_under_the_rewrite(attribute);
            return;
        }
        self.judge(attribute);
        if attribute.path().is_ident("cfg") {
            self.rejections.push(Rejection::UnplacedCfg {
                file: self.path.to_path_buf(),
                at: position(attribute.pound_token.span),
            });
        }
    }

    fn visit_file(&mut self, file: &'ast syn::File) {
        if !self.gated(&file.attrs, file) {
            self.within(0..self.text.len(), |walk| {
                syn::visit::visit_file(walk, file)
            });
        }
    }

    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = item_attrs(item);
        if self.gated(attrs, item) {
            return;
        }
        self.item_with(attrs, item.span().byte_range().start, |walk| {
            syn::visit::visit_item(walk, item);
        });
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        match &module.content {
            Some((brace, _)) => {
                let scope =
                    brace.span.open().byte_range().start..brace.span.close().byte_range().end;
                self.within(scope, |walk| syn::visit::visit_item_mod(walk, module));
            }
            None => syn::visit::visit_item_mod(self, module),
        }
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        let brace = block.brace_token.span;
        let scope = brace.open().byte_range().start..brace.close().byte_range().end;
        self.within(scope, |walk| syn::visit::visit_block(walk, block));
    }

    // The item's own attributes were read by `visit_item`; what is left is the macro.
    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        match &item.ident {
            Some(name) if item.mac.path.is_ident("macro_rules") => {
                self.definition(&item.attrs, &item.mac, name);
            }
            _ => self.invocation(&item.mac, true),
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attrs: &[syn::Attribute] = match item {
            syn::ImplItem::Const(it) => &it.attrs,
            syn::ImplItem::Fn(it) => &it.attrs,
            syn::ImplItem::Type(it) => &it.attrs,
            syn::ImplItem::Macro(it) => &it.attrs,
            _ => &[],
        };
        if !self.gated(attrs, item) {
            self.item_with(attrs, item.span().byte_range().start, |walk| {
                syn::visit::visit_impl_item(walk, item);
            });
        }
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        let attrs: &[syn::Attribute] = match item {
            syn::TraitItem::Const(it) => &it.attrs,
            syn::TraitItem::Fn(it) => &it.attrs,
            syn::TraitItem::Type(it) => &it.attrs,
            syn::TraitItem::Macro(it) => &it.attrs,
            _ => &[],
        };
        if !self.gated(attrs, item) {
            self.item_with(attrs, item.span().byte_range().start, |walk| {
                syn::visit::visit_trait_item(walk, item);
            });
        }
    }

    fn visit_foreign_item(&mut self, item: &'ast syn::ForeignItem) {
        let attrs: &[syn::Attribute] = match item {
            syn::ForeignItem::Fn(it) => &it.attrs,
            syn::ForeignItem::Static(it) => &it.attrs,
            syn::ForeignItem::Type(it) => &it.attrs,
            syn::ForeignItem::Macro(it) => &it.attrs,
            _ => &[],
        };
        if !self.gated(attrs, item) {
            self.item_with(attrs, item.span().byte_range().start, |walk| {
                syn::visit::visit_foreign_item(walk, item);
            });
        }
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if !self.gated(&local.attrs, local) {
            syn::visit::visit_local(self, local);
        }
    }

    // `syn` parses `foo!(…);`, `foo![…];` and `foo! { … }` in a block as a statement macro, so
    // every statement-position invocation is a hazard here.
    fn visit_stmt_macro(&mut self, statement: &'ast syn::StmtMacro) {
        if !self.gated(&statement.attrs, statement) {
            self.invocation(&statement.mac, true);
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.invocation(mac, false);
    }

    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        if !self.gated(expr_attrs(expr), expr) {
            syn::visit::visit_expr(self, expr);
        }
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        if !self.gated(&arm.attrs, arm) {
            syn::visit::visit_arm(self, arm);
        }
    }

    fn visit_field_value(&mut self, value: &'ast syn::FieldValue) {
        if !self.gated(&value.attrs, value) {
            syn::visit::visit_field_value(self, value);
        }
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        if !self.gated(&field.attrs, field) {
            syn::visit::visit_field(self, field);
        }
    }

    fn visit_variant(&mut self, variant: &'ast syn::Variant) {
        if !self.gated(&variant.attrs, variant) {
            syn::visit::visit_variant(self, variant);
        }
    }

    fn visit_fn_arg(&mut self, argument: &'ast syn::FnArg) {
        let attrs = match argument {
            syn::FnArg::Receiver(it) => &it.attrs,
            syn::FnArg::Typed(it) => &it.attrs,
        };
        if !self.gated(attrs, argument) {
            syn::visit::visit_fn_arg(self, argument);
        }
    }

    fn visit_field_pat(&mut self, pattern: &'ast syn::FieldPat) {
        if !self.gated(&pattern.attrs, pattern) {
            syn::visit::visit_field_pat(self, pattern);
        }
    }

    fn visit_generic_param(&mut self, parameter: &'ast syn::GenericParam) {
        let attrs = match parameter {
            syn::GenericParam::Lifetime(it) => &it.attrs,
            syn::GenericParam::Type(it) => &it.attrs,
            syn::GenericParam::Const(it) => &it.attrs,
        };
        if !self.gated(attrs, parameter) {
            syn::visit::visit_generic_param(self, parameter);
        }
    }
}

/// One top-level item of a read definition's transcriber: its attributes, parsed; its bytes,
/// attributes included; and its tokens after the attributes.
struct TranscribedItem {
    attrs: Vec<syn::Attribute>,
    range: Range<usize>,
    rest: TokenStream,
}

/// The top-level items of `tokens` — a `macro_rules!` body — when it is a list of
/// attribute-carrying names (the module's grammar), and `None` for every other body.
fn entries_definition(tokens: TokenStream, text: &str) -> Option<Vec<TranscribedItem>> {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let (matcher, transcriber) = match trees.as_slice() {
        [
            TokenTree::Group(matcher),
            TokenTree::Punct(equals),
            TokenTree::Punct(arrow),
            TokenTree::Group(transcriber),
            rest @ ..,
        ] if equals.as_char() == '='
            && arrow.as_char() == '>'
            && (rest.is_empty()
                || matches!(rest, [TokenTree::Punct(semicolon)] if semicolon.as_char() == ';')) =>
        {
            (matcher, transcriber)
        }
        _ => return None,
    };
    let words = flat(matcher.stream());
    let words: Vec<&str> = words.split(' ').collect();
    let (attribute, name) = match words.as_slice() {
        [
            "$",
            "(",
            "$",
            "(",
            "#",
            "[",
            "$",
            attribute,
            ":",
            "meta",
            "]",
            ")",
            "*",
            "$",
            name,
            ":",
            "ident",
            ",",
            ")",
            "*",
        ] => (*attribute, *name),
        _ => return None,
    };
    let enumerated = format!("$ ( $ ( # [ $ {attribute} ] ) * $ {name} , ) *");
    let items = transcribed_items(transcriber.stream())?;
    for item in &items {
        if item
            .attrs
            .iter()
            .any(|attribute| may_run_a_macro(&attribute.meta))
        {
            return None;
        }
        let gated = item
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("cfg"))
            && cfg_predicates(&item.attrs, text).1 == Compilation::NeverInProduct;
        if !gated
            && !enumerates_the_names(item.rest.clone(), &enumerated)
            && names_a_variable(item.rest.clone(), attribute, name)
        {
            return None;
        }
    }
    Some(items)
}

/// A transcriber's top-level items, cut by the module's grammar; `None` when it is anything else.
fn transcribed_items(tokens: TokenStream) -> Option<Vec<TranscribedItem>> {
    const TO_THE_BODY: [&str; 6] = ["impl", "enum", "struct", "union", "trait", "fn"];
    const TO_THE_SEMICOLON: [&str; 4] = ["const", "static", "type", "use"];
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut items = Vec::new();
    let mut at = 0;
    while at < trees.len() {
        let start = trees[at].span().byte_range().start;
        let mut attrs = Vec::new();
        while let Some(TokenTree::Punct(hash)) = trees.get(at)
            && hash.as_char() == '#'
        {
            let Some(TokenTree::Group(group)) = trees.get(at + 1) else {
                return None;
            };
            if group.delimiter() != Delimiter::Bracket {
                return None;
            }
            let written: TokenStream = trees[at..at + 2].iter().cloned().collect();
            attrs.extend(syn::Attribute::parse_outer.parse2(written).ok()?);
            at += 2;
        }
        let first = at;
        if matches!(trees.get(at), Some(TokenTree::Ident(word)) if word == "pub") {
            at += 1;
            if matches!(trees.get(at), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
            {
                at += 1;
            }
        }
        let Some(TokenTree::Ident(keyword)) = trees.get(at) else {
            return None;
        };
        let keyword = keyword.to_string();
        let to_the_body = TO_THE_BODY.contains(&keyword.as_str());
        if !to_the_body && !TO_THE_SEMICOLON.contains(&keyword.as_str()) {
            return None;
        }
        let end = loop {
            at += 1;
            match trees.get(at)? {
                TokenTree::Punct(semicolon) if semicolon.as_char() == ';' => break at,
                TokenTree::Group(body) if to_the_body && body.delimiter() == Delimiter::Brace => {
                    break at;
                }
                _ => {}
            }
        };
        items.push(TranscribedItem {
            attrs,
            range: start..trees[end].span().byte_range().end,
            rest: trees[first..=end].iter().cloned().collect(),
        });
        at = end + 1;
    }
    Some(items)
}

/// Whether an item's tokens are `[pub] enum Name { … }` whose whole body is `enumerated` —
/// `$( $(#[$a])* $v, )*`, the one place the attributes are sure to gate the names they carry.
fn enumerates_the_names(tokens: TokenStream, enumerated: &str) -> bool {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut at = 0;
    if matches!(trees.get(at), Some(TokenTree::Ident(word)) if word == "pub") {
        at += 1;
        if matches!(trees.get(at), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
        {
            at += 1;
        }
    }
    match &trees[at..] {
        [
            TokenTree::Ident(keyword),
            TokenTree::Ident(_),
            TokenTree::Group(body),
        ] => {
            keyword == "enum"
                && body.delimiter() == Delimiter::Brace
                && flat(body.stream()) == enumerated
        }
        _ => false,
    }
}

/// Whether `tokens` paste `$attribute` or `$name` anywhere.
fn names_a_variable(tokens: TokenStream, attribute: &str, name: &str) -> bool {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    trees.iter().enumerate().any(|(at, tree)| match tree {
        TokenTree::Punct(dollar) if dollar.as_char() == '$' => matches!(
            trees.get(at + 1),
            Some(TokenTree::Ident(variable)) if variable == attribute || variable == name
        ),
        TokenTree::Group(group) => names_a_variable(group.stream(), attribute, name),
        _ => false,
    })
}

/// Whether an attribute may run a macro whose expansion this walk does not see — and so define a
/// `macro_rules!` of any name: an attribute outside the language's own (an attribute macro; the
/// tool namespaces `rustfmt`, `clippy` and `diagnostic` are inert), a `derive` of anything but the
/// language's derives, `macro_use`, or a `cfg_attr` writing any of those. What cannot be read is
/// taken to.
fn may_run_a_macro(meta: &syn::Meta) -> bool {
    let path = meta.path();
    let Some(name) = path.get_ident() else {
        return !names_an_inert_tool(path);
    };
    let name = name.to_string();
    match name.as_str() {
        "derive" => {
            let syn::Meta::List(list) = meta else {
                return true;
            };
            list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .map_or(true, |derived| {
                derived.iter().any(|path| {
                    path.segments.last().is_none_or(|last| {
                        !BUILT_IN_DERIVES
                            .iter()
                            .any(|built_in| last.ident == built_in)
                    })
                })
            })
        }
        "cfg_attr" => {
            let syn::Meta::List(list) = meta else {
                return true;
            };
            list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .map_or(true, |written| written.iter().skip(1).any(may_run_a_macro))
        }
        "macro_use" => true,
        _ => !BUILT_IN_ATTRIBUTES.contains(&name.as_str()),
    }
}

/// The tool namespaces whose attributes are inert: `#[rustfmt::skip]`, `#[clippy::…]`,
/// `#[diagnostic::…]`.
const INERT_TOOLS: [&str; 3] = ["rustfmt", "clippy", "diagnostic"];

/// Whether `path` names an attribute of an inert tool namespace ([`INERT_TOOLS`]). A path with a
/// leading `::` names an extern crate, not a tool, so it never does (`#[::clippy::x]`).
fn names_an_inert_tool(path: &syn::Path) -> bool {
    path.leading_colon.is_none()
        && path
            .segments
            .first()
            .is_some_and(|tool| INERT_TOOLS.iter().any(|inert| tool.ident == inert))
}

/// The derive helpers this walk reads through, each with the derives that declare it, spelt as
/// a `derive` may write them — the bare name and the declaring crate's path (plan row 122, the
/// review's N1): a helper is inert only on an item that carries one of its derives spelt so. A
/// closed table — a helper not in it, or beside a derive spelt any other way
/// (`other_crate::Error`, `::serde::Serialize`), is taken to be an attribute macro, and adding
/// one is a row with a fixture.
const DERIVE_HELPERS: [(&str, &[&str]); 5] = [
    (
        "serde",
        &[
            "Serialize",
            "Deserialize",
            "serde::Serialize",
            "serde::Deserialize",
        ],
    ),
    ("error", &["Error", "thiserror::Error"]),
    ("from", &["Error", "thiserror::Error"]),
    ("source", &["Error", "thiserror::Error"]),
    ("backtrace", &["Error", "thiserror::Error"]),
];

/// Whether an attribute may rewrite the item it stands on — an attribute macro, which receives
/// the item's tokens and may emit anything — given every path the item's `derive`s name, as
/// written (`derives`). Inert: the language's own attributes, the tool namespaces, a
/// `derive` of anything (the item stays as written; the derive's output is beside it), a derive
/// helper beside a derive that declares it ([`DERIVE_HELPERS`]), and a `cfg_attr` writing only
/// those. Everything else may rewrite: any other name, any other path (`tokio::main`),
/// `unsafe(…)`, and what cannot be read.
fn may_rewrite(meta: &syn::Meta, derives: &[String]) -> bool {
    let path = meta.path();
    let Some(name) = path.get_ident() else {
        return !names_an_inert_tool(path);
    };
    let name = name.to_string();
    match name.as_str() {
        "derive" => false,
        "cfg_attr" => {
            let syn::Meta::List(list) = meta else {
                return true;
            };
            list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .map_or(true, |written| {
                written
                    .iter()
                    .skip(1)
                    .any(|meta| may_rewrite(meta, derives))
            })
        }
        built_in if BUILT_IN_ATTRIBUTES.contains(&built_in) => false,
        helper => !DERIVE_HELPERS.iter().any(|(name, declared_by)| {
            *name == helper
                && declared_by
                    .iter()
                    .any(|derive| derives.iter().any(|written| written == derive))
        }),
    }
}

/// Every path the `derive`s in `attrs` name, as written (`Error`, `thiserror::Error`), a
/// `cfg_attr`'s included.
fn derived(attrs: &[syn::Attribute]) -> Vec<String> {
    fn from(meta: &syn::Meta, into: &mut Vec<String>) {
        let syn::Meta::List(list) = meta else {
            return;
        };
        if list.path.is_ident("derive") {
            if let Ok(paths) = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            ) {
                into.extend(paths.iter().map(path_spelling));
            }
        } else if list.path.is_ident("cfg_attr")
            && let Ok(written) = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
        {
            for meta in written.iter().skip(1) {
                from(meta, into);
            }
        }
    }
    let mut derives = Vec::new();
    for attribute in attrs {
        from(&attribute.meta, &mut derives);
    }
    derives
}

/// The macro a group at `trees[at]` is the body of — `path!(…)`, `path! name { … }` for
/// `macro_rules!` — spelt as a refusal names it; `None` when the group is no macro's body.
fn macro_opened_by(trees: &[TokenTree], at: usize) -> Option<String> {
    let mut bang = at.checked_sub(1)?;
    let mut defined = None;
    if let TokenTree::Ident(name) = &trees[bang] {
        defined = Some(name.to_string());
        bang = bang.checked_sub(1)?;
    }
    if !matches!(&trees[bang], TokenTree::Punct(one) if one.as_char() == '!') {
        return None;
    }
    let mut start = bang.checked_sub(1)?;
    let TokenTree::Ident(last) = &trees[start] else {
        return None;
    };
    let mut path = vec![last.to_string()];
    while start >= 3
        && matches!(&trees[start - 1], TokenTree::Punct(colon) if colon.as_char() == ':')
        && matches!(&trees[start - 2], TokenTree::Punct(colon) if colon.as_char() == ':')
        && let TokenTree::Ident(segment) = &trees[start - 3]
    {
        path.insert(0, segment.to_string());
        start -= 3;
    }
    let path = path.join("::");
    match defined {
        Some(name) if path == "macro_rules" => Some(format!("macro_rules! {name}")),
        Some(_) => None,
        None => Some(path),
    }
}

/// The tokens of a stream, one word each and delimiters spelt — a spelling that does not depend
/// on how the source spaced them.
fn flat(tokens: TokenStream) -> String {
    let mut words = Vec::new();
    for tree in tokens {
        match tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::None => ("", ""),
                };
                words.push(open.to_owned());
                words.push(flat(group.stream()));
                words.push(close.to_owned());
            }
            other => words.push(other.to_string()),
        }
    }
    words.retain(|word| !word.is_empty());
    words.join(" ")
}

/// `#[…]` or `#![…]` starting at `trees[at]`: its bracket group, and where the walk resumes.
fn attribute_at(trees: &[TokenTree], at: usize) -> Option<(&proc_macro2::Group, usize)> {
    let TokenTree::Punct(hash) = trees.get(at)? else {
        return None;
    };
    if hash.as_char() != '#' {
        return None;
    }
    let mut next = at + 1;
    if matches!(trees.get(next), Some(TokenTree::Punct(bang)) if bang.as_char() == '!') {
        next += 1;
    }
    match trees.get(next)? {
        TokenTree::Group(group) if group.delimiter() == Delimiter::Bracket => {
            Some((group, next + 1))
        }
        _ => None,
    }
}

/// Whether an attribute's bracket group writes a gate: `cfg`, or a `cfg_attr` that writes one.
fn writes_a_gate(group: &proc_macro2::Group) -> bool {
    let mut inside = group.stream().into_iter();
    match inside.next() {
        Some(TokenTree::Ident(name)) if name == "cfg" => true,
        Some(TokenTree::Ident(name)) if name == "cfg_attr" => names_cfg(inside.collect()),
        _ => false,
    }
}

/// A macro path as written: `thread_local`, `crate::entries`.
fn path_spelling(path: &syn::Path) -> String {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let joined = segments.join("::");
    if path.leading_colon.is_some() {
        format!("::{joined}")
    } else {
        joined
    }
}

/// The attributes an item carries, inner ones included (`syn` keeps an inline module's or a
/// function's `#![…]` with its outer attributes).
fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(it) => &it.attrs,
        syn::Item::Enum(it) => &it.attrs,
        syn::Item::ExternCrate(it) => &it.attrs,
        syn::Item::Fn(it) => &it.attrs,
        syn::Item::ForeignMod(it) => &it.attrs,
        syn::Item::Impl(it) => &it.attrs,
        syn::Item::Macro(it) => &it.attrs,
        syn::Item::Mod(it) => &it.attrs,
        syn::Item::Static(it) => &it.attrs,
        syn::Item::Struct(it) => &it.attrs,
        syn::Item::Trait(it) => &it.attrs,
        syn::Item::TraitAlias(it) => &it.attrs,
        syn::Item::Type(it) => &it.attrs,
        syn::Item::Union(it) => &it.attrs,
        syn::Item::Use(it) => &it.attrs,
        _ => &[],
    }
}

/// Where a token is written, one-based on both, as the declarations report a `mod`.
fn position(span: proc_macro2::Span) -> Position {
    let start = span.start();
    Position {
        line: start.line,
        column: start.column + 1,
    }
}

/// The text between an attribute's own parentheses, as written; the whole attribute when it has
/// none.
fn spelling(attribute: &syn::Attribute, text: &str) -> String {
    match &attribute.meta {
        syn::Meta::List(list) => {
            let span = list.delimiter.span();
            text[span.open().byte_range().end..span.close().byte_range().start].to_owned()
        }
        _ => {
            let range = attribute.span().byte_range();
            text[range].to_owned()
        }
    }
}
