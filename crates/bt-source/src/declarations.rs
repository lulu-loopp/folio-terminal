//! The walk from a crate root through the declarations that build the crate.
//!
//! Three things make this different from the two resolvers already in the tree.
//!
//! **Inline ancestry is kept.** `mod outer { mod leaf; }` written in `lib.rs`
//! does not name `src/leaf.rs`; it names `src/outer/leaf.rs`, because the file
//! path mirrors the *logical* module path and an inline module is a component of
//! it. `bt_app::file_reads_source_tests::scan` resolves such a declaration
//! against the declaring file's directory as though it were written at the top
//! of the file, and `vendor/mitex/tests/cvt.rs` is that exact shape — sixteen
//! files reached only through an inline `mod cvt { … }`.
//!
//! **Ambiguity and non-resolution are refusals.** See [`crate::Rejection`].
//!
//! **What "test code" is belongs to the declaration, transitively** (plan §2.3).
//! An inline `#[cfg(test)] mod tests { … }` and an out-of-line
//! `#[cfg(test)] mod tests;` are the same statement written two ways, and both
//! make everything under them test code — whether or not the files under them
//! carry a gate of their own, because there is no build in which the parent is
//! compiled and the child is not. The rule has one half the first design did not
//! have: a file can be reached through a product declaration *and* a test one,
//! and being reachable from a test declaration must not remove its product
//! reachability. Reachability is a property of a **path to the file**, so every
//! path is kept and [`crate::FileFacts`] answers over the set of them.
//!
//! The reading of a `cfg` predicate evaluates the `test` bit and nothing else —
//! `bt_app::file_reads_source_tests::product_cfg`'s three-valued rule, taken as
//! written, because it is already correct over `not`/`all`/`any`. A platform or
//! a feature leaves the answer [`Compilation::DecidedElsewhere`], which permits
//! product compilation, and the walk follows the declaration either way.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use syn::visit::Visit;

use crate::manifest::{TargetId, TargetRoot};
use crate::paths::normalized;
use crate::reject::{Position, Rejection};

/// What the `test` predicate says about a declaration, three-valued.
///
/// Three values and not two because `any(test, windows)` is neither: it is
/// compiled in a product build on Windows and not off it, and a reading that
/// collapsed that to "test" would exempt a platform door from every guard that
/// skips test code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Compilation {
    /// No build of the shipped program contains this — `cfg(test)`, or an `all`
    /// that holds one.
    NeverInProduct,
    /// Nothing about `test` keeps this out of a product build.
    AlwaysInProduct,
    /// A platform, a feature or something else decides. The walk follows it, and
    /// a product build may well contain it.
    DecidedElsewhere,
}

impl Compilation {
    /// The two combined: a path is as test-gated as its most gated step.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::NeverInProduct, _) | (_, Self::NeverInProduct) => Self::NeverInProduct,
            (Self::DecidedElsewhere, _) | (_, Self::DecidedElsewhere) => Self::DecidedElsewhere,
            (Self::AlwaysInProduct, Self::AlwaysInProduct) => Self::AlwaysInProduct,
        }
    }

    /// Whether a build of the shipped program can contain this.
    #[must_use]
    pub fn permits_product(self) -> bool {
        !matches!(self, Self::NeverInProduct)
    }
}

/// Where a module's body is written.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModuleBody {
    /// `mod x { … }` — the body is in the declaring file.
    Inline,
    /// `mod x;` — the body is the whole of the named file.
    File(PathBuf),
}

/// One `mod` declaration on the way to a module.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeclarationStep {
    /// The name the declaration gives the module.
    pub module: String,
    /// The file the declaration is written in.
    pub declared_in: PathBuf,
    /// Where in that file.
    pub at: Position,
    pub body: ModuleBody,
    /// The text inside each `#[cfg(…)]` standing on the declaration, in the
    /// order they are written and **byte for byte as they are written** — the
    /// bytes between the parentheses, spacing included, not the parser's
    /// re-printing of the tokens. `#[cfg(all(test, windows))]` is
    /// `all(test, windows)` here and not `all (test , windows)`.
    ///
    /// That is the contract because P1c's queries are about what the code
    /// *says* (plan §2.1: spelling is not value), and because a comparison
    /// against a re-printing is a comparison against a parser version. See
    /// [`spelling`].
    pub predicates: Vec<String>,
    /// What those predicates say about a product build.
    pub compilation: Compilation,
}

/// One module, reached one way.
///
/// "One way" is the load-bearing part: the same module path reached from two
/// targets is two entries, and so is one file reached through two declarations.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReachedModule {
    pub target: TargetId,
    /// `crate`, `crate::a`, `crate::a::b` — the ownership path of plan §2.3.
    pub module_path: String,
    /// The file whose text holds this module's body.
    pub file: PathBuf,
    pub body: ModuleBody,
    /// Every declaration between the crate root and here, in order.
    pub steps: Vec<DeclarationStep>,
    /// The steps combined with the target's own kind.
    pub compilation: Compilation,
}

/// Where the walk is: one file, and how deep inside it.
#[derive(Clone, Debug)]
struct Frame {
    file: PathBuf,
    /// The file's own text, kept so that a `cfg` predicate can be read back in
    /// the spelling it was written in rather than in the parser's printing of
    /// it. Shared, because a frame is rebuilt for every inline module.
    source: Rc<str>,
    /// The directory the file itself sits in. A `#[path]` written at the top
    /// level of a file is relative to **this**, which is the one place the two
    /// bases differ.
    file_parent: PathBuf,
    /// The directory a `mod x;` written here resolves against: the file's own
    /// directory for a file that owns the directory it sits in, a directory
    /// named after the file otherwise, with one component per enclosing inline
    /// module.
    directory: PathBuf,
    /// How many inline module blocks enclose the current position in this file.
    inline_depth: usize,
}

impl Frame {
    /// The frame a file opens with.
    ///
    /// **`owns_its_directory` is not "the file is called `main.rs`."** Three
    /// different files own the directory they sit in, and rustc's rule is the
    /// union of them rather than a rule about names:
    ///
    /// * a target's **root** file, whatever it is called — `tests/cvt.rs` is one,
    ///   which is why its `mod cvt { mod arg_parse; }` names `tests/cvt/arg_parse.rs`
    ///   and not `tests/cvt/cvt/arg_parse.rs`;
    /// * a file called **`mod.rs`**;
    /// * a file reached through **`#[path]`**. `rustc_expand::module` puts it
    ///   plainly — *"All `#[path]` files are treated as though they are a
    ///   `mod.rs` file"* — so a plain `mod q;` written in a file reached by
    ///   `#[path = "p.rs"]` from `src/lib.rs` names `src/q.rs`, and rustc's
    ///   E0583 for a missing one says to create `src/q.rs` or `src/q/mod.rs`.
    ///   Looking under `src/p/` instead is the worse half of the defect this
    ///   crate exists to remove: it is a refusal for a legal declaration, or — if
    ///   a directory of that name happens to exist — a silent resolution to a
    ///   file rustc never compiles.
    ///
    /// `bt_platform::…::module_file` keys this on the file's stem being `lib`,
    /// `main` or `mod`, and so is wrong about all three of the above.
    fn opening(file: &Path, owns_its_directory: bool, source: Rc<str>) -> Self {
        let file = normalized(file);
        let parent = file.parent().unwrap_or(Path::new("")).to_path_buf();
        let owned = owns_its_directory || file.file_name().is_some_and(|name| name == "mod.rs");
        let directory = if owned {
            parent.clone()
        } else {
            let stem = file.file_stem().unwrap_or_default().to_owned();
            parent.join(stem)
        };
        Self {
            file,
            source,
            file_parent: parent,
            directory,
            inline_depth: 0,
        }
    }

    /// The directory a declaration written at this point resolves against.
    fn base_for(&self, has_path_attribute: bool) -> &Path {
        if has_path_attribute && self.inline_depth == 0 {
            &self.file_parent
        } else {
            &self.directory
        }
    }
}

/// What a declaration was read to say.
struct Declaration {
    module: String,
    at: Position,
    predicates: Vec<String>,
    compilation: Compilation,
    path_attribute: Option<String>,
    /// The spelling of a `#[cfg_attr(…)]` standing on this declaration that
    /// carries a `path` or a `cfg`. See
    /// [`Rejection::ConditionalDeclarationAttribute`]: it is refused rather
    /// than read, because it decides the file or the gate by a predicate this
    /// walk is blind to on purpose.
    conditional_attribute: Option<String>,
}

/// The walk over one target.
struct Walk {
    target: TargetId,
    frames: Vec<Frame>,
    module_path: Vec<String>,
    steps: Vec<DeclarationStep>,
    modules: Vec<ReachedModule>,
    rejections: Vec<Rejection>,
}

impl Walk {
    fn frame(&self) -> &Frame {
        self.frames
            .last()
            .expect("the walk always stands in a file")
    }

    fn path_now(&self) -> String {
        let mut path = String::from("crate");
        for name in &self.module_path {
            path.push_str("::");
            path.push_str(name);
        }
        path
    }

    fn combined(&self) -> Compilation {
        let mut combined = if self.target.kind.permits_product() {
            Compilation::AlwaysInProduct
        } else {
            Compilation::NeverInProduct
        };
        for step in &self.steps {
            combined = combined.and(step.compilation);
        }
        combined
    }

    /// Read a file, parse it, and walk its items in a frame of its own.
    ///
    /// The frame is pushed only once both readings succeed, so a file the walk
    /// cannot read is a refusal and never a frame with nothing in it.
    fn enter_file(&mut self, file: &Path, owns_its_directory: bool) {
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) => {
                self.rejections.push(Rejection::UnreadableFile {
                    file: file.to_path_buf(),
                    reason: error.to_string(),
                });
                return;
            }
        };
        match syn::parse_file(&text) {
            Ok(parsed) => {
                self.frames.push(Frame::opening(
                    file,
                    owns_its_directory,
                    Rc::from(text.as_str()),
                ));
                syn::visit::visit_file(self, &parsed);
                self.frames.pop();
            }
            Err(error) => self.rejections.push(Rejection::UnparsableFile {
                file: file.to_path_buf(),
                reason: error.to_string(),
            }),
        }
    }

    /// Follow `mod name;` to the file that holds it.
    fn resolve(&mut self, declaration: &Declaration) -> Option<PathBuf> {
        let frame = self.frame();
        let base = frame.base_for(declaration.path_attribute.is_some());
        let declared_in = frame.file.clone();
        let candidates = if let Some(named) = &declaration.path_attribute {
            vec![normalized(&base.join(named))]
        } else {
            vec![
                normalized(&base.join(format!("{}.rs", declaration.module))),
                normalized(&base.join(&declaration.module).join("mod.rs")),
            ]
        };
        let present: Vec<PathBuf> = candidates
            .iter()
            .filter(|candidate| candidate.is_file())
            .cloned()
            .collect();
        match present.len() {
            0 => {
                self.rejections.push(Rejection::UnresolvedModule {
                    declared_in,
                    at: declaration.at,
                    module: declaration.module.clone(),
                    tried: candidates,
                });
                None
            }
            1 => Some(present[0].clone()),
            _ => {
                self.rejections.push(Rejection::AmbiguousModule {
                    declared_in,
                    at: declaration.at,
                    module: declaration.module.clone(),
                    candidates: present,
                });
                None
            }
        }
    }
}

impl<'ast> Visit<'ast> for Walk {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let source = Rc::clone(&self.frame().source);
        let declaration = read_declaration(item, &source);
        let declared_in = self.frame().file.clone();

        if let Some(spelling) = declaration.conditional_attribute {
            self.rejections
                .push(Rejection::ConditionalDeclarationAttribute {
                    declared_in,
                    at: declaration.at,
                    module: declaration.module,
                    spelling,
                });
            return;
        }

        if let Some((_, items)) = &item.content {
            let step = DeclarationStep {
                module: declaration.module.clone(),
                declared_in,
                at: declaration.at,
                body: ModuleBody::Inline,
                predicates: declaration.predicates,
                compilation: declaration.compilation,
            };
            let frame = self.frame();
            let base = frame.base_for(declaration.path_attribute.is_some());
            let component = declaration
                .path_attribute
                .as_deref()
                .unwrap_or(&declaration.module);
            let next = Frame {
                file: frame.file.clone(),
                source: Rc::clone(&frame.source),
                file_parent: frame.file_parent.clone(),
                directory: normalized(&base.join(component)),
                inline_depth: frame.inline_depth + 1,
            };
            let file = next.file.clone();

            self.module_path.push(declaration.module);
            self.steps.push(step);
            self.frames.push(next);
            self.modules.push(ReachedModule {
                target: self.target.clone(),
                module_path: self.path_now(),
                file,
                body: ModuleBody::Inline,
                steps: self.steps.clone(),
                compilation: self.combined(),
            });
            for child in items {
                self.visit_item(child);
            }
            self.frames.pop();
            self.steps.pop();
            self.module_path.pop();
            return;
        }

        let Some(file) = self.resolve(&declaration) else {
            return;
        };
        if self.frames.iter().any(|frame| frame.file == file) {
            self.rejections.push(Rejection::ModuleCycle {
                declared_in,
                at: declaration.at,
                module: declaration.module,
                file,
            });
            return;
        }
        let step = DeclarationStep {
            module: declaration.module.clone(),
            declared_in,
            at: declaration.at,
            body: ModuleBody::File(file.clone()),
            predicates: declaration.predicates,
            compilation: declaration.compilation,
        };
        // **A `#[path]` file is a `mod.rs` to its own children** — see
        // [`Frame::opening`]. This is the argument rustc's own resolver makes
        // and the one both readers in this tree get wrong.
        let owns_its_directory = declaration.path_attribute.is_some();
        self.module_path.push(declaration.module);
        self.steps.push(step);
        self.modules.push(ReachedModule {
            target: self.target.clone(),
            module_path: self.path_now(),
            file: file.clone(),
            body: ModuleBody::File(file.clone()),
            steps: self.steps.clone(),
            compilation: self.combined(),
        });
        self.enter_file(&file, owns_its_directory);
        self.steps.pop();
        self.module_path.pop();
    }
}

/// Every module `root` reaches, and everything the walk refused on the way.
pub(crate) fn walk_target(root: &TargetRoot) -> (Vec<ReachedModule>, Vec<Rejection>) {
    let mut walk = Walk {
        target: root.id.clone(),
        frames: Vec::new(),
        module_path: Vec::new(),
        steps: Vec::new(),
        modules: Vec::new(),
        rejections: Vec::new(),
    };
    let file = normalized(&root.file);
    walk.modules.push(ReachedModule {
        target: root.id.clone(),
        module_path: "crate".to_owned(),
        file: file.clone(),
        body: ModuleBody::File(file.clone()),
        steps: Vec::new(),
        compilation: walk.combined(),
    });
    walk.enter_file(&file, true);
    (walk.modules, walk.rejections)
}

/// Every `#[cfg(…)]` standing on an item, in the spelling it is written in, and
/// what the conjunction of them says about a product build.
///
/// The one reading of a `cfg` attribute in this crate. P1b's index carries the
/// same spellings on an item's conditional variant that P1a carries on a module
/// declaration (plan §2.4), and two readings of one attribute would be two
/// answers waiting to disagree.
pub(crate) fn cfg_predicates(attrs: &[syn::Attribute], source: &str) -> (Vec<String>, Compilation) {
    let mut predicates = Vec::new();
    let mut compilation = Compilation::AlwaysInProduct;
    for attribute in attrs {
        if !attribute.path().is_ident("cfg") {
            continue;
        }
        if let syn::Meta::List(list) = &attribute.meta {
            predicates.push(spelling(&list.delimiter, source));
        }
        compilation = compilation.and(cfg_compilation(attribute));
    }
    (predicates, compilation)
}

/// What stands on a `mod` declaration, read against the text of the file it is
/// written in.
fn read_declaration(item: &syn::ItemMod, source: &str) -> Declaration {
    let span = item.mod_token.span.start();
    let (predicates, compilation) = cfg_predicates(&item.attrs, source);
    let mut path_attribute = None;
    let mut conditional_attribute = None;
    for attribute in &item.attrs {
        if attribute.path().is_ident("path")
            && let syn::Meta::NameValue(value) = &attribute.meta
            && let syn::Expr::Lit(literal) = &value.value
            && let syn::Lit::Str(named) = &literal.lit
        {
            path_attribute = Some(named.value());
        }
        if attribute.path().is_ident("cfg_attr")
            && let syn::Meta::List(list) = &attribute.meta
            && decides_a_file_or_a_gate(list.tokens.clone())
        {
            conditional_attribute = Some(spelling(&list.delimiter, source));
        }
    }
    Declaration {
        module: item.ident.to_string(),
        at: Position {
            line: span.line,
            column: span.column + 1,
        },
        predicates,
        compilation,
        path_attribute,
        conditional_attribute,
    }
}

/// Whether a `cfg_attr`'s body names `path` or `cfg` anywhere inside it.
///
/// Those two are the only attributes that change what this walk answers — which
/// file a declaration names, and whether that file is test code — so they are
/// the two a `cfg_attr` may not smuggle. Anything else it carries is followed
/// without a word, because it cannot move either answer.
fn decides_a_file_or_a_gate(tokens: proc_macro2::TokenStream) -> bool {
    tokens.into_iter().any(|tree| match tree {
        proc_macro2::TokenTree::Ident(name) => name == "path" || name == "cfg",
        proc_macro2::TokenTree::Group(group) => decides_a_file_or_a_gate(group.stream()),
        _ => false,
    })
}

/// The text between a delimiter pair, exactly as it is written.
///
/// **Spelling and not a re-printing** (plan §2.1, "spelling is not value"). The
/// obvious `list.tokens.to_string()` hands back the parser's own rendering —
/// `#[cfg(all(test, windows))]` comes out as `all (test , windows)` — and a
/// query about what the code *says* cannot be answered from that. `span-locations`
/// is on, so the open and close delimiters carry byte ranges into the text this
/// file was parsed from, and the bytes between them are the predicate.
///
/// # Panics
///
/// If the byte range the parser reports is not a range of the text it was given.
/// It is the same text, passed down the frame the file opened, so this cannot
/// happen; it panics rather than substituting a second-best answer.
fn spelling(delimiter: &syn::MacroDelimiter, source: &str) -> String {
    let span = delimiter.span();
    let from = span.open().byte_range().end;
    let to = span.close().byte_range().start;
    source[from..to].to_owned()
}

/// What one `#[cfg(…)]` says about a product build.
fn cfg_compilation(attribute: &syn::Attribute) -> Compilation {
    let Ok(meta) = attribute.parse_args::<syn::Meta>() else {
        return Compilation::DecidedElsewhere;
    };
    match product_cfg(&meta) {
        Some(false) => Compilation::NeverInProduct,
        Some(true) => Compilation::AlwaysInProduct,
        None => Compilation::DecidedElsewhere,
    }
}

/// Evaluate only the test bit; all platform and feature choices remain possible.
///
/// Lifted as written from `bt_app::file_reads_source_tests::product_cfg`, which
/// is already a correct three-valued evaluator over `not`, `all` and `any`. In
/// particular `cfg(not(test))` must be walked, and `any(test, windows)` is not a
/// test-only item. A textual search for "test" would silently exempt both.
fn product_cfg(meta: &syn::Meta) -> Option<bool> {
    if meta.path().is_ident("test") {
        return Some(false);
    }
    let syn::Meta::List(list) = meta else {
        return None;
    };
    let children = list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .ok()?;
    let values: Vec<_> = children.iter().map(product_cfg).collect();
    if list.path.is_ident("not") && values.len() == 1 {
        return values[0].map(|value| !value);
    }
    if list.path.is_ident("all") {
        if values.contains(&Some(false)) {
            Some(false)
        } else if values.iter().all(|value| *value == Some(true)) {
            Some(true)
        } else {
            None
        }
    } else if list.path.is_ident("any") {
        if values.contains(&Some(true)) {
            Some(true)
        } else if values.iter().all(|value| *value == Some(false)) {
            Some(false)
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaration_of(source: &str) -> Declaration {
        let file: syn::File = syn::parse_str(source).expect("the fixture parses");
        let syn::Item::Mod(item) = &file.items[0] else {
            panic!("the fixture declares a module");
        };
        read_declaration(item, source)
    }

    fn compilation_of(source: &str) -> Compilation {
        declaration_of(source).compilation
    }

    /// PIN — the `test` bit is the only one evaluated, and the other two answers
    /// are not the same answer.
    #[test]
    fn a_predicate_that_is_not_about_test_leaves_the_door_open() {
        assert_eq!(
            compilation_of("#[cfg(test)] mod tests;"),
            Compilation::NeverInProduct
        );
        assert_eq!(
            compilation_of("#[cfg(all(test, windows))] mod tests;"),
            Compilation::NeverInProduct
        );
        assert_eq!(
            compilation_of("#[cfg(not(test))] mod shipped;"),
            Compilation::AlwaysInProduct
        );
        assert_eq!(
            compilation_of("#[cfg(any(test, windows))] mod either;"),
            Compilation::DecidedElsewhere,
            "a door that is open on Windows is not test-only"
        );
        assert_eq!(
            compilation_of("#[cfg(windows)] mod platform;"),
            Compilation::DecidedElsewhere
        );
        assert_eq!(compilation_of("mod plain;"), Compilation::AlwaysInProduct);
        // Two gates on one declaration are one conjunction.
        assert_eq!(
            compilation_of("#[cfg(windows)] #[cfg(test)] mod both;"),
            Compilation::NeverInProduct
        );
    }

    /// PIN — **a `cfg` predicate is kept in the spelling it was written in.**
    ///
    /// The field is what P1c queries and what P1b's index lowers, so the
    /// contract is the exact bytes and not the parser's rendering of them.
    /// Every line below would be a different string under
    /// `TokenStream::to_string`, which is what this crate used to store.
    ///
    /// MUTATION: go back to `list.tokens.to_string()` and every assertion here
    /// goes red with a re-printed predicate beside the written one.
    #[test]
    fn a_predicate_is_kept_in_the_spelling_it_was_written_in() {
        assert_eq!(declaration_of("#[cfg(test)] mod t;").predicates, ["test"]);
        assert_eq!(
            declaration_of("#[cfg(all(test, windows))] mod t;").predicates,
            ["all(test, windows)"],
            "not `all (test , windows)`"
        );
        assert_eq!(
            declaration_of("#[cfg(any(test,   windows))] mod t;").predicates,
            ["any(test,   windows)"],
            "the spacing somebody wrote is part of the spelling"
        );
        assert_eq!(
            declaration_of("#[cfg(feature = \"x\")] mod t;").predicates,
            ["feature = \"x\""]
        );
        // One string per gate, in the order they stand on the declaration.
        assert_eq!(
            declaration_of("#[cfg(windows)] #[cfg(test)] mod t;").predicates,
            ["windows", "test"]
        );
        assert!(declaration_of("mod t;").predicates.is_empty());
    }

    /// PIN — **three different files own the directory they sit in**: a target
    /// root whatever it is called, a `mod.rs`, and a file reached through
    /// `#[path]`. The rule is never about being called `main.rs`.
    ///
    /// The third arm is the one this crate got wrong until 2026-09-21, and it is
    /// wrong in `bt_platform::…::module_file` today.
    #[test]
    fn a_target_root_owns_the_directory_it_sits_in() {
        let empty = || Rc::from("");
        let root = Frame::opening(Path::new("tests/cvt.rs"), true, empty());
        assert_eq!(root.directory, PathBuf::from("tests"));
        let reached = Frame::opening(Path::new("tests/cvt.rs"), false, empty());
        assert_eq!(reached.directory, Path::new("tests").join("cvt"));
        let module = Frame::opening(Path::new("src/video/mod.rs"), false, empty());
        assert_eq!(module.directory, Path::new("src").join("video"));
        let by_path = Frame::opening(Path::new("src/p.rs"), true, empty());
        assert_eq!(
            by_path.directory,
            PathBuf::from("src"),
            "a `#[path]` file is a `mod.rs` to its children, so a plain `mod q;` in it names \
             src/q.rs — which is what rustc's own E0583 tells you to create"
        );
    }
}
