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
//! * every generic parameter.
//!
//! A gated node's bytes are the gate, attributes included, and nothing inside it is read again
//! (no product build compiles any of it). A `cfg` anywhere else is refused
//! ([`Rejection::UnplacedCfg`]), so a position this walk does not know is a refusal and never a
//! silent product count; so is a `cfg` whose predicate the three-valued reading cannot read
//! ([`Rejection::UnreadableCfg`]) and a `cfg_attr` that writes a `cfg`
//! ([`Rejection::ConditionalCfg`]).

use std::collections::BTreeSet;
use std::path::Path;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::declarations::{
    Compilation, cfg_attr_writes_a_cfg, cfg_is_readable, cfg_predicates, expr_attrs,
};
use crate::index::Span;
use crate::lower::at_union;
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
}

impl Gates<'_> {
    /// Read the gate `attrs` write on `node`: refuse what does not read, and when the node is
    /// kept out of every product build, record its bytes and say so — the caller then does not
    /// descend into it.
    fn gated<T: Spanned>(&mut self, attrs: &[syn::Attribute], node: &T) -> bool {
        let mut cfg = false;
        for attribute in attrs {
            let at = attribute.pound_token.span.byte_range().start;
            self.read.insert(at);
            self.judge(attribute);
            cfg |= attribute.path().is_ident("cfg");
        }
        if !cfg || cfg_predicates(attrs, self.text).1 != Compilation::NeverInProduct {
            return false;
        }
        let range = node.span().byte_range();
        let start = attrs
            .iter()
            .map(|attribute| attribute.pound_token.span.byte_range().start)
            .chain([range.start])
            .min()
            .unwrap_or(range.start);
        let end = range.end.max(start);
        self.gates.push(Span::new(at_union(self.base, start), at_union(self.base, end)));
        true
    }

    /// The refusals one attribute can earn wherever it stands.
    fn judge(&mut self, attribute: &syn::Attribute) {
        if attribute.path().is_ident("cfg") && !cfg_is_readable(attribute) {
            self.rejections.push(Rejection::UnreadableCfg {
                file: self.path.to_path_buf(),
                at: position(attribute),
                spelling: spelling(attribute, self.text),
            });
        }
        if cfg_attr_writes_a_cfg(attribute) {
            self.rejections.push(Rejection::ConditionalCfg {
                file: self.path.to_path_buf(),
                at: position(attribute),
                spelling: spelling(attribute, self.text),
            });
        }
    }
}

impl<'ast> Visit<'ast> for Gates<'_> {
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        let at = attribute.pound_token.span.byte_range().start;
        if self.read.contains(&at) {
            return;
        }
        self.judge(attribute);
        if attribute.path().is_ident("cfg") {
            self.rejections.push(Rejection::UnplacedCfg {
                file: self.path.to_path_buf(),
                at: position(attribute),
            });
        }
    }

    fn visit_file(&mut self, file: &'ast syn::File) {
        if !self.gated(&file.attrs, file) {
            syn::visit::visit_file(self, file);
        }
    }

    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !self.gated(item_attrs(item), item) {
            syn::visit::visit_item(self, item);
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
            syn::visit::visit_impl_item(self, item);
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
            syn::visit::visit_trait_item(self, item);
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
            syn::visit::visit_foreign_item(self, item);
        }
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if !self.gated(&local.attrs, local) {
            syn::visit::visit_local(self, local);
        }
    }

    fn visit_stmt_macro(&mut self, statement: &'ast syn::StmtMacro) {
        if !self.gated(&statement.attrs, statement) {
            syn::visit::visit_stmt_macro(self, statement);
        }
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

/// Where an attribute is written, one-based on both, as the declarations report a `mod`.
fn position(attribute: &syn::Attribute) -> Position {
    let start = attribute.pound_token.span.start();
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
