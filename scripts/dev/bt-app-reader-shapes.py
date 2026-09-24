#!/usr/bin/env python3
"""The shapes a source reader takes, counted.

`docs/plans/MIGRATION-DEBT.tsv` carries every reader in this workspace that
still names a file. `docs/plans/bt-app-split-prep.md` §6.3 and §6.5 hand the
body pins to tickets P3 and P14 and the `file_reads_doors.txt` ledger keys to
P6 and P16, and the plan's hypothesis is that most of those readers are two or
three repeated shapes a converter could rewrite mechanically.

This script measures that hypothesis instead of asserting it. For every P3, P14
and P16 row it locates the reading, reads the helper that does the slicing,
resolves each selector against the file the reading actually names, and
classifies the row into a shape with a verdict of `yes`, `partial` or `no` on
whether a tool could rewrite the site without a person reading it.

It changes nothing. It reads the debt list, the sources the debt list names,
and the sources those readings name, and it writes one TSV and a summary.

    python scripts/dev/bt-app-reader-shapes.py

Determinism: every set that reaches the output is sorted, the rows come out in
the debt list's own order, and nothing is read from the environment but the
repository the script lives in.

**It is a classifier, not an oracle.** A shape is a claim about the text at a
call site, and the three `X-`/`S-` shapes are its own report of where it could
not place a reading. Those are findings to read, not rows to delete.
"""

from __future__ import annotations

import argparse
import re
import sys
from bisect import bisect_right
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

# ── the lexer ─────────────────────────────────────────────────────────────
#
# Enough of Rust to tell a comment and a string from code. Every question this
# script asks — where a brace matches, where an item starts, which arguments a
# call was given — is asked of code positions only, because `main.rs` carries
# hundreds of doc comments that spell the very selectors being searched for.

_TOKEN = re.compile(
    r"""
    (?P<line>//)
  | (?P<block>/\*)
  | (?P<raw>b?r\#*")
  | (?P<text>b?")
  | (?P<char>'(?:\\.|[^\\'])')
    """,
    re.VERBOSE,
)


def lex(text: str) -> bytearray:
    """A mask over `text`: 1 where the byte is code, 0 inside a comment, a
    string or a character literal. A delimiter is masked with its contents, so
    a quote never reads as code."""
    mask = bytearray(b"\x01") * len(text)
    limit = len(text)
    at = 0
    while at < limit:
        found = _TOKEN.search(text, at)
        if not found:
            break
        start = found.start()
        group = found.lastgroup
        if group == "line":
            stop = text.find("\n", start)
            stop = limit if stop < 0 else stop
        elif group == "block":
            depth, stop = 1, start + 2
            while stop < limit and depth:
                if text.startswith("/*", stop):
                    depth, stop = depth + 1, stop + 2
                elif text.startswith("*/", stop):
                    depth, stop = depth - 1, stop + 2
                else:
                    stop += 1
        elif group == "raw":
            closing = '"' + "#" * found.group().count("#")
            end = text.find(closing, found.end())
            stop = limit if end < 0 else end + len(closing)
        elif group == "text":
            walk = found.end()
            while walk < limit:
                if text[walk] == "\\":
                    walk += 2
                    continue
                if text[walk] == '"':
                    break
                walk += 1
            stop = min(walk + 1, limit)
        else:  # a character literal, which is not a lifetime
            stop = found.end()
        mask[start:stop] = b"\x00" * (stop - start)
        at = max(stop, start + 1)
    return mask


def brace_pairs(text: str, mask: bytearray) -> dict[int, int]:
    """Every matching `{`/`}` at a code position, both directions."""
    stack: list[int] = []
    pairs: dict[int, int] = {}
    for found in re.finditer(r"[{}]", text):
        here = found.start()
        if not mask[here]:
            continue
        if text[here] == "{":
            stack.append(here)
        elif stack:
            opened = stack.pop()
            pairs[opened] = here
            pairs[here] = opened
    return pairs


def body_opener(text: str, mask: bytearray, after: int) -> int | None:
    """The `{` that opens an item declared at `after`, or None for a
    declaration that ends in `;` — a `mod x;`, a trait method signature."""
    depth, walk, limit = 0, after, len(text)
    while walk < limit:
        if not mask[walk]:
            walk += 1
            continue
        here = text[walk]
        if here in "([":
            depth += 1
        elif here in ")]":
            depth -= 1
        elif depth == 0:
            if here == ";":
                return None
            if here == "{":
                return walk
        walk += 1
    return None


def indent_of(text: str, at: int) -> int:
    line = text.rfind("\n", 0, at) + 1
    return len(text[line:at]) - len(text[line:at].lstrip())


def unescape(literal: str) -> str:
    """A Rust string literal's spelling, decoded far enough to compare against
    a terminator. §2.1 is emphatic that spelling is not value; this decodes
    only in order to recognise `\\n    fn `, and nothing is asserted from it."""
    return (
        literal.replace("\\r", "\r")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace('\\"', '"')
        .replace("\\\\", "\\")
    )


# ── the items of one file ─────────────────────────────────────────────────


@dataclass
class Item:
    kind: str  # "fn" | "mod" | "impl"
    name: str
    head: int
    open_at: int
    close_at: int
    indent: int
    owner: str | None = None  # the self type of the enclosing `impl`
    trait: str | None = None
    module: str = ""


_ITEM = re.compile(r"\b(?:fn|mod|impl)\b")
_SELF_TYPE = re.compile(r"([A-Za-z_]\w*)\s*(?:<[^{]*>)?\s*$")
_INCLUDE_CONST = re.compile(
    r"\b(?:static|const)\s+([A-Za-z_0-9]+)\s*:\s*&(?:'static\s+)?str\s*=\s*"
    r"include_str!\(\s*\"([^\"]+)\"\s*\)"
)
_INCLUDE_ANY = re.compile(r"include_str!\(\s*\"([^\"]+)\"\s*\)")
_MANIFEST_READ = re.compile(r"CARGO_MANIFEST_DIR")
_JOINED_RS = re.compile(r"\.join\(\s*\"([^\"]*\.rs)\"")


@dataclass
class SourceFile:
    path: Path
    relative: str
    text: str
    mask: bytearray = field(repr=False, default_factory=bytearray)
    pairs: dict[int, int] = field(repr=False, default_factory=dict)
    items: list[Item] = field(repr=False, default_factory=list)
    by_name: dict[str, list[Item]] = field(repr=False, default_factory=dict)
    consts: list[tuple[int, str, str]] = field(repr=False, default_factory=list)
    modules: list[Item] = field(repr=False, default_factory=list)
    _code: str | None = field(repr=False, default=None)
    _scope_cache: dict[int, tuple[int, int]] = field(repr=False, default_factory=dict)

    @classmethod
    def read(cls, path: Path, relative: str) -> "SourceFile":
        text = path.read_text(encoding="utf-8", errors="replace")
        file = cls(path=path, relative=relative, text=text)
        file.mask = lex(text)
        file.pairs = brace_pairs(text, file.mask)
        file.collect()
        return file

    @property
    def code(self) -> str:
        """The text with every non-code byte replaced by a space, so a regex
        cannot match inside a doc comment or a string."""
        if self._code is None:
            self._code = "".join(
                character if keep else " "
                for character, keep in zip(self.text, self.mask)
            )
        return self._code

    def collect(self) -> None:
        text, mask = self.text, self.mask
        found_items: list[Item] = []
        for found in _ITEM.finditer(text):
            start = found.start()
            if not mask[start]:
                continue
            keyword, after, name = found.group(), found.end(), ""
            if keyword in ("fn", "mod"):
                named = re.match(r"\s+([A-Za-z_]\w*)", text[after:])
                if not named:
                    continue
                name, after = named.group(1), after + named.end()
            opened = body_opener(text, mask, after)
            if opened is None or opened not in self.pairs:
                continue
            trait_name = None
            if keyword == "impl":
                header = text[start:opened]
                if " for " in header:
                    trait_part, self_part = header.rsplit(" for ", 1)
                    trait_found = _SELF_TYPE.search(trait_part.strip())
                    trait_name = (
                        trait_found.group(1) if trait_found else trait_part.strip()
                    )
                else:
                    self_part = header[len("impl") :]
                self_part = re.sub(r"\bwhere\b.*$", "", self_part, flags=re.S).strip()
                typed = _SELF_TYPE.search(self_part)
                name = typed.group(1) if typed else self_part
            found_items.append(
                Item(
                    kind=keyword,
                    name=name,
                    head=start,
                    open_at=opened,
                    close_at=self.pairs[opened],
                    indent=indent_of(text, start),
                    trait=trait_name,
                )
            )
        found_items.sort(key=lambda item: (item.head, -item.close_at))
        stack: list[Item] = []
        for item in found_items:
            while stack and item.head > stack[-1].close_at:
                stack.pop()
            item.module = "::".join(
                outer.name for outer in stack if outer.kind == "mod"
            )
            for outer in reversed(stack):
                if outer.kind == "impl":
                    item.owner, item.trait = outer.name, outer.trait
                    break
                if outer.kind in ("fn", "mod"):
                    break
            stack.append(item)
        self.items = found_items
        index: dict[str, list[Item]] = defaultdict(list)
        for item in found_items:
            if item.kind == "fn":
                index[item.name].append(item)
        self.by_name = dict(index)
        self.consts = [
            (found.start(), found.group(1), found.group(2))
            for found in _INCLUDE_CONST.finditer(text)
        ]
        self.modules = sorted(
            (item for item in found_items if item.kind == "mod"),
            key=lambda item: item.open_at,
        )
        self._module_starts = [item.open_at for item in self.modules]

    def scope(self, at: int) -> tuple[int, int]:
        """The innermost `mod` braces holding `at`, or the whole file.

        Only `mod` blocks count: a source const declared in one module is not
        the const next door, and forty modules of `main.rs` declare one called
        `SOURCE`."""
        cached = self._scope_cache.get(at)
        if cached is not None:
            return cached
        best = (0, len(self.text))
        cut = bisect_right(self._module_starts, at)
        for item in self.modules[:cut]:
            if item.open_at < at < item.close_at and (
                item.close_at - item.open_at < best[1] - best[0]
            ):
                best = (item.open_at, item.close_at)
        self._scope_cache[at] = best
        return best

    def scope_chain(self, at: int) -> list[tuple[int, int]]:
        """Every `mod` scope holding `at`, innermost first, then the file."""
        holding = [
            (item.open_at, item.close_at)
            for item in self.modules
            if item.open_at < at < item.close_at
        ]
        holding.sort(key=lambda span: span[1] - span[0])
        return holding + [(0, len(self.text))]

    def consts_in_scope(self, at: int) -> dict[str, str]:
        seen: dict[str, str] = {}
        for start, stop in self.scope_chain(at):
            for position, declared, target in self.consts:
                if start <= position <= stop and declared not in seen:
                    seen[declared] = target
        return seen


# ── the local body finders ────────────────────────────────────────────────
#
# Forty-odd functions in this tree take a selector and hand back a slice of an
# `include_str!` const. They are not one function repeated: their end-of-body
# rules differ, and that difference is the whole of the conversion risk.

#: What a finder's terminator literal means for the span it hands back.
TERMINATORS = {
    "\n    fn ": "next-method",
    "\n    pub fn ": "next-method",
    "\n    pub(crate) fn ": "next-method",
    "\n    async fn ": "next-method",
    "\n    }\n": "own-close",
    "\n    }": "own-close",
    "\n}\n": "own-close",
    "\n}": "own-close",
    "\n    #[": "next-attribute",
    "\nimpl ": "next-impl",
    "\nfn ": "next-free-fn",
    "\n    /// ": "next-doc-comment",
}

_CUSTOM_OPS = (
    ".lines(",
    ".split(",
    ".splitn(",
    ".replace(",
    ".rfind(",
    ".to_lowercase(",
    ".chars(",
    ".trim(",
    ".filter(",
    ".windows(",
    ".rsplit(",
)


@dataclass
class Reader:
    """A local function a test reads its own source through."""

    item: Item
    role: str  # "finder" | "fixed" | "whole"
    scope: tuple[int, int]
    target: str  # the file its const includes
    arity: int
    selector_kind: str  # "signature" | "name" | "list" | "fixed" | "unknown"
    end_semantics: str
    custom_ops: tuple[str, ...]
    fixed_selector: str = ""
    through: str = ""  # the finder a fixed reading calls

    @property
    def name(self) -> str:
        return self.item.name

    @property
    def standard(self) -> bool:
        """A reading one `Index::body_of` call replaces."""
        return (
            self.role in ("finder", "fixed")
            and self.arity <= 1
            and not self.custom_ops
            and self.end_semantics in ("next-method", "own-close")
        )


def string_literals(fragment: str) -> list[str]:
    """Every string literal spelled in a fragment, escapes left as written."""
    return re.findall(r'"((?:[^"\\]|\\.)*)"', fragment, re.S)


def read_readers(file: SourceFile) -> list[Reader]:
    """Every function in `file` that hands a test a reading of its own source."""
    readers: list[Reader] = []
    for item in file.items:
        if item.kind != "fn" or item.close_at - item.open_at > 4000:
            continue
        body = file.text[item.open_at : item.close_at]
        code = file.code[item.open_at : item.close_at]
        visible = file.consts_in_scope(item.head)
        used = sorted(name for name in visible if re.search(rf"\b{name}\b", code))
        inline = _INCLUDE_ANY.search(body)
        manifest = _MANIFEST_READ.search(code) and _JOINED_RS.search(body)
        if not used and not inline and not manifest:
            continue
        if used:
            target = visible[used[0]]
        elif inline:
            target = inline.group(1)
        else:
            target = _JOINED_RS.search(body).group(1)

        signature = file.text[item.head : item.open_at]
        params = signature[signature.find("(") + 1 : signature.rfind(")")]
        arity = 0 if not params.strip() else params.count(",") + 1
        literals = string_literals(body)
        terminators = [piece for piece in literals if unescape(piece) in TERMINATORS]
        heads = [piece for piece in literals if "{" in piece and "}" in piece]
        selectors = [
            piece
            for piece in literals
            if piece not in terminators and piece not in heads and piece.strip()
        ]
        semantics = "no-slice"
        if terminators:
            semantics = TERMINATORS[unescape(terminators[0])]
        elif ".find(" in code:
            semantics = "custom-terminator"
        if arity >= 2:
            semantics = "caller-supplied-end"

        if "&[&str]" in params or "&[&'static str]" in params:
            selector_kind = "list"
        elif heads:
            selector_kind = "name"
        elif re.search(r"\b(signature|opener|head|declaration|sig|line)\s*:", params):
            selector_kind = "signature"
        elif re.search(r"\b(name|method|function)\s*:", params):
            selector_kind = "name"
        elif arity == 0:
            selector_kind = "fixed" if selectors and terminators else "none"
        else:
            selector_kind = "signature" if "&str" in params else "unknown"

        # A helper that *asserts* as well as reads is one edit that converts
        # every one of its call sites, which is a different amount of work
        # from a finder, and a different risk.
        asserting = bool(re.search(r"\bassert(_eq|_ne)?!", code)) and (
            arity >= 2 or item.name.startswith("assert")
        )
        if asserting:
            role = "asserting"
        elif arity == 0 and not terminators:
            role = "whole"
        elif arity == 0:
            role = "fixed"
        else:
            role = "finder"

        readers.append(
            Reader(
                item=item,
                role=role,
                scope=file.scope(item.head),
                target=target,
                arity=arity,
                selector_kind=selector_kind,
                end_semantics=semantics,
                custom_ops=tuple(op for op in _CUSTOM_OPS if op in code),
                fixed_selector=";".join(sorted(set(selectors)))[:200]
                if role == "fixed"
                else "",
            )
        )

    # One level of indirection, which this tree writes two ways:
    # `fn router() -> &'static str { body("…") }` fixes a selector, and
    # `fn struct_fields(name) { struct_body(name).lines().filter(…) }` narrows
    # the slice. The second is the dangerous one: it reads as a body finder at
    # the call site and is not one.
    by_scope: dict[tuple[int, int], list[Reader]] = defaultdict(list)
    for reader in readers:
        by_scope[reader.scope].append(reader)
    known = {reader.item.head for reader in readers}
    wrappers: list[Reader] = []
    for item in file.items:
        if item.kind != "fn" or item.head in known:
            continue
        if item.close_at - item.open_at > 1500:
            continue
        signature = file.text[item.head : item.open_at]
        params = signature[signature.find("(") + 1 : signature.rfind(")")]
        arity = 0 if not params.strip() else params.count(",") + 1
        code = file.code[item.open_at : item.close_at]
        asserting = bool(re.search(r"\bassert(_eq|_ne)?!", code)) and (
            arity >= 2 or item.name.startswith("assert")
        )
        if arity > 1 and not asserting:
            continue
        scope = file.scope(item.head)
        for inner in by_scope.get(scope, []):
            if inner.role == "whole":
                continue
            if not re.search(rf"\b{re.escape(inner.name)}\s*\(", code):
                continue
            literals = string_literals(file.text[item.open_at : item.close_at])
            if arity == 0 and not literals and not asserting:
                continue
            wrappers.append(
                Reader(
                    item=item,
                    role="asserting"
                    if asserting
                    else ("fixed" if arity == 0 else "finder"),
                    scope=scope,
                    target=inner.target,
                    arity=arity,
                    selector_kind="fixed" if arity == 0 else inner.selector_kind,
                    end_semantics=inner.end_semantics,
                    custom_ops=tuple(
                        sorted(
                            set(inner.custom_ops)
                            | {op for op in _CUSTOM_OPS if op in code}
                        )
                    ),
                    fixed_selector="".join(literals)[:200] if arity == 0 else "",
                    through=inner.name,
                )
            )
            break
    return readers + wrappers


# ── one call site ─────────────────────────────────────────────────────────

_LITERAL = re.compile(r'^\s*&?\s*"((?:[^"\\]|\\.)*)"\s*$', re.S)
_ASSEMBLED = re.compile(r"\.concat\(\)|\bformat!\(|\bconcat!\(")


def call_arguments(file: SourceFile, paren: int) -> str | None:
    """The text between the parentheses of a call whose `(` is at `paren`."""
    text, mask = file.text, file.mask
    depth, walk, start = 0, paren, paren + 1
    while walk < len(text):
        if mask[walk]:
            if text[walk] == "(":
                depth += 1
            elif text[walk] == ")":
                depth -= 1
                if depth == 0:
                    return text[start:walk]
        walk += 1
    return None


_FOR_IN = re.compile(r"\bfor\s+([^\n{]{1,90}?)\s+in\s*(?:&\s*)?\[")


def loop_literals(file: SourceFile, test: Item) -> dict[str, list[str]]:
    """Every `for … in [ … ]` in a test, as {bound name: the array's literals}.

    A selector that arrives through a loop variable is not decidable at the
    call site, but it *is* decidable when the array is written out in full —
    and that is a different amount of work for a tool, so it is a different
    shape."""
    bound: dict[str, list[str]] = defaultdict(list)
    code = file.code[test.open_at : test.close_at]
    for found in _FOR_IN.finditer(code):
        opening = test.open_at + found.end() - 1
        depth, walk = 0, opening
        while walk < test.close_at:
            if file.mask[walk]:
                if file.text[walk] == "[":
                    depth += 1
                elif file.text[walk] == "]":
                    depth -= 1
                    if depth == 0:
                        break
            walk += 1
        literals = string_literals(file.text[opening : walk + 1])
        for name in re.findall(r"[A-Za-z_]\w*", found.group(1)):
            bound[name].extend(literals)
    return {name: sorted(set(values)) for name, values in bound.items() if values}


def classify_argument(argument: str) -> tuple[str, str]:
    """(kind, selector) for one reader argument."""
    literal = _LITERAL.match(argument)
    if literal:
        return "literal", literal.group(1)
    if _ASSEMBLED.search(argument):
        pieces = string_literals(argument)
        if pieces:
            return "assembled", "".join(pieces)
        return "assembled", " ".join(argument.split())[:120]
    if re.match(r"^\s*&?\s*[A-Za-z_]\w*\s*$", argument):
        return "variable", argument.strip().lstrip("&").strip()
    return "expression", " ".join(argument.split())[:120]


# ── the assertions ────────────────────────────────────────────────────────


def negated(code: str, at: int) -> bool:
    """Whether the call at `at` is written under a `!`.

    The receiver is walked backwards with brackets balanced, because the `!`
    of `!body(signature).contains(needle)` stands in front of the whole
    expression and not in front of the dot. Reading it as "the character
    before `.contains` is not a `!`" is how a negative reads as a positive,
    which is the one mistake this script must not make: a negative converted
    as a positive is the plan's quietly-green failure, in the tooling."""
    walk, depth = at - 1, 0
    while walk >= 0:
        here = code[walk]
        if here in ")]":
            depth += 1
        elif here in "([":
            if depth == 0:
                break
            depth -= 1
        elif depth == 0 and not (here.isalnum() or here in "_.'& \t\n:?"):
            break
        walk -= 1
    while walk >= 0 and code[walk].isspace():
        walk -= 1
    return walk >= 0 and code[walk] == "!" and (walk == 0 or code[walk - 1] != "=")


def assertion_kinds(code: str) -> tuple[str, ...]:
    """Which assertion shapes a test body uses over its reading."""
    kinds: set[str] = set()
    for found in re.finditer(r"\.contains\(", code):
        kinds.add("!contains" if negated(code, found.start()) else "contains")
    if ".matches(" in code and ".count()" in code:
        kinds.add("count")
    if ".find(" in code or ".rfind(" in code:
        ordered = re.search(r"\.find\([^;]*?\)[^;]{0,150}?[<>]", code, re.S)
        kinds.add("order" if ordered else "position")
    if ".starts_with(" in code or ".ends_with(" in code:
        kinds.add("edge")
    if ".lines(" in code or ".split(" in code:
        kinds.add("slice")
    if "assert_eq!" in code and "count" not in kinds:
        kinds.add("equals")
    return tuple(sorted(kinds))


# ── selector resolution ───────────────────────────────────────────────────

_SIGNATURE = re.compile(
    r"(?:^|\\n|\s)(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_]\w*)"
)
_BARE_NAME = re.compile(r"^[A-Za-z_]\w*$")


def selector_identity(selector: str, target: SourceFile | None) -> tuple[str, str]:
    """(identity, note) — the `ItemQuery` a selector resolves to in the file the
    reading names. `Type::name` for a method, `name` for a free function; an
    empty identity is a selector no tool may convert on its own."""
    named = _SIGNATURE.search(selector)
    if named:
        name = named.group(1)
    elif _BARE_NAME.match(selector.strip()):
        name = selector.strip()
    else:
        return "", "the selector spells no single `fn`"
    if target is None:
        return "", "the file this reading names was not read"
    declarations = target.by_name.get(name, [])
    if not declarations:
        return "", f"`{name}` is not declared in {target.path.name}"
    if len(declarations) > 1:
        owners = sorted({item.owner or "(free)" for item in declarations})
        return (
            "",
            f"`{name}` is declared {len(declarations)} times here "
            f"({', '.join(owners)})",
        )
    only = declarations[0]
    return (f"{only.owner}::{name}" if only.owner else only.name), (
        "method" if only.owner else "free function"
    )


# ── the debt list ─────────────────────────────────────────────────────────


@dataclass
class DebtRow:
    line: int
    file: str
    owner: str
    mechanism: str
    subject: str
    ticket: str
    disturbed: str


def read_debt(path: Path) -> list[DebtRow]:
    rows: list[DebtRow] = []
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if raw.startswith("#") or not raw.strip():
            continue
        cells = raw.split("\t")
        if len(cells) < 6 or cells[4] == "ticket":
            continue
        rows.append(DebtRow(number, *cells[:6]))
    return rows


# ── the shapes ────────────────────────────────────────────────────────────


@dataclass
class Verdict:
    shape: str
    reader: str
    selectors: str
    assertion: str
    mechanical: str
    reason: str


SHAPE_NAMES = {
    "B1-body-contains": "a named body contains / does not contain a spelling",
    "B2-body-order": "two spellings in a named body, in order",
    "B3-body-count": "occurrences counted inside a named body",
    "B4-body-other": "a named body, asserted some other way",
    "B5-loop-over-literals": "a named body, selector from a written-out array",
    "A-assembled-needle": "a named body, needle written in halves",
    "M-mixed-readers": "one test, several finders",
    "M2-file-and-body": "one test, the whole file and a named body",
    "P-assertion-helper": "the reading is inside a shared assertion helper",
    "H-custom-helper": "the finder does its own slicing",
    "R-runtime-selector": "the selector is not decidable at the call site",
    "U-unresolved-selector": "the selector names no single declaration",
    "W-whole-file": "the whole file is the scope",
    "N-whole-file-negative": "the whole file is the scope of a negative",
    "C-include-binding": "the `include_str!` binding itself",
    "F-fixture": "the subject is not Rust source",
    "Q-runtime-file": "a file the test writes and reads back at run time",
    "L-ledger-key": "a ledger key naming a file",
    "S-no-source-read": "no source reading found at the site",
    "X-not-found": "the named item was not found",
    "X-unread": "the file was not read",
}

BODY_PIN_SHAPES = (
    "B1-body-contains",
    "B2-body-order",
    "B3-body-count",
    "B4-body-other",
    "B5-loop-over-literals",
    "A-assembled-needle",
    "M-mixed-readers",
    "M2-file-and-body",
    "P-assertion-helper",
    "H-custom-helper",
    "R-runtime-selector",
    "U-unresolved-selector",
)


def shape_of_row(
    row: DebtRow,
    files: dict[str, SourceFile],
    readers_of: dict[str, list[Reader]],
) -> Verdict:
    if row.ticket == "P16" or row.mechanism == "ledger key naming a file":
        return Verdict(
            "L-ledger-key",
            "file_reads_doors.txt",
            row.owner,
            "ledger key equality",
            "no",
            "not a reader with a test function: a key in a plain-text ledger "
            "whose first field is a file name. Re-keying it is P6/P16's own "
            "edit and there is no expression to rewrite",
        )

    file = files.get(row.file)
    if file is None:
        return Verdict("X-unread", "", "", "", "no", f"{row.file} was not read")

    parts = row.owner.split("::")
    test_name = parts[-1]
    module_name = parts[-2] if len(parts) > 1 else ""

    # A row whose last segment is a constant is the `include_str!` binding, not
    # a reader: P3 says the bindings go with the batches that convert them.
    if test_name.isupper():
        target = next(
            (target for _, name, target in file.consts if name == test_name), ""
        )
        return Verdict(
            "C-include-binding",
            test_name,
            target,
            "none",
            "partial",
            "a binding, not an assertion. The const is deleted by whichever "
            "batch converts the last of its consumers (P3's own note), so the "
            "row is mechanical only in the order its consumers impose",
        )

    candidates = list(file.by_name.get(test_name, []))
    if module_name:
        narrowed = [
            item for item in candidates if module_name in item.module.split("::")
        ]
        candidates = narrowed or candidates
    if not candidates:
        return Verdict(
            "X-not-found",
            "",
            "",
            "",
            "no",
            f"no `fn {test_name}` and no const of that name in "
            f"{Path(row.file).name}: the row names an item this scan cannot place",
        )
    test = candidates[0]
    code = file.code[test.open_at : test.close_at]
    body = file.text[test.open_at : test.close_at]
    kinds = assertion_kinds(code)
    assertion = ",".join(kinds) or "none"
    scope = file.scope(test.head)

    # Which readings this test uses: a local finder, a source const, an inline
    # `include_str!`, or a manifest-relative read of a `.rs` file.
    calls: list[tuple[Reader, str, str]] = []
    used: list[Reader] = []
    # A reading is visible to the test when the test stands inside the module
    # the reading is written in — its own, or any module around it. Where two
    # modules declare the same name, the innermost wins, which is Rust's rule
    # and the reason `main.rs` can hold twenty-five functions called `body`.
    visible: dict[str, Reader] = {}
    for reader in readers_of[row.file]:
        if reader.item.head == test.head:
            continue
        if not reader.scope[0] <= test.head <= reader.scope[1]:
            continue
        standing = visible.get(reader.name)
        if standing is None or (
            reader.scope[1] - reader.scope[0] < standing.scope[1] - standing.scope[0]
        ):
            visible[reader.name] = reader
    for reader in visible.values():
        for found in re.finditer(rf"\b{re.escape(reader.name)}\s*\(", code):
            if reader.role != "finder":
                if reader not in used:
                    used.append(reader)
                    calls.append((reader, "literal", reader.fixed_selector))
                continue
            argument = call_arguments(file, test.open_at + found.end() - 1)
            if argument is None:
                continue
            if reader not in used:
                used.append(reader)
            calls.append((reader, *classify_argument(argument)))

    visible_consts = file.consts_in_scope(test.head)
    direct = sorted(name for name in visible_consts if re.search(rf"\b{name}\b", code))
    inline = sorted({found.group(1) for found in _INCLUDE_ANY.finditer(body)})
    manifest = sorted(
        {found.group(1) for found in _JOINED_RS.finditer(body)}
        if _MANIFEST_READ.search(code)
        else set()
    )

    if not used and not direct and not inline and not manifest:
        # The census's own answer, where it names files and none of them is
        # Rust: an `include_str!` of a bundled asset is on the debt list
        # because it names a file, and `bt-source` is not what replaces it.
        named = [piece for piece in row.subject.split(";") if "/" in piece]
        if named and not any(piece.endswith(".rs") for piece in named):
            return Verdict(
                "F-fixture",
                "",
                ";".join(sorted(set(named)))[:400],
                assertion,
                "no",
                "the row's subject is a bundled asset, not Rust source ("
                + str(len(set(named)))
                + " file(s)): `bt-source` enumerates a crate's declarations "
                "and has no answer for it",
            )
        if row.mechanism in ("fixture / manifest read", "runtime fs read"):
            return Verdict(
                "Q-runtime-file",
                "",
                row.subject,
                assertion,
                "no",
                "the test reads a file it created at run time, not source text. "
                "`bt-source` enumerates a crate's declarations and has no "
                "answer for it: the row belongs to §6.1's file-scoped "
                "allowlist or to a decision that it never was a source reader",
            )
        return Verdict(
            "S-no-source-read",
            "",
            "",
            assertion,
            "no",
            "no source-reading construction in this test: no const in scope, "
            "no `include_str!`, no manifest-relative read, no local finder. "
            "A candidate hit of the debt list's lexical seed on a `.rs` path "
            "spelled as data — read the site before the row is removed",
        )

    reader_names = ";".join(
        sorted({reader.name for reader in used} | set(direct) | set(inline))
    )
    literal = sorted({value for _, kind, value in calls if kind == "literal"})
    assembled = sorted({value for _, kind, value in calls if kind == "assembled"})
    dynamic = sorted(
        {
            (kind, value)
            for _, kind, value in calls
            if kind in ("variable", "expression")
        }
    )
    selectors = ";".join(literal + assembled)[:400]

    subjects = sorted(
        {reader.target for reader in used if reader.target}
        | {visible_consts[name] for name in direct}
        | set(inline)
        | set(manifest)
    )
    if subjects and not any(name.endswith(".rs") for name in subjects):
        return Verdict(
            "F-fixture",
            reader_names,
            ";".join(subjects),
            assertion,
            "no",
            "the reading's subject is not Rust source ("
            + ", ".join(subjects)
            + "): `bt-source` enumerates a crate's declarations and has no "
            "answer for a document or a fixture",
        )

    # A reading that takes no selector and slices nothing hands back the file.
    slicing = [reader for reader in used if reader.role != "whole"]

    if not slicing:
        return Verdict(
            "N-whole-file-negative" if "!contains" in kinds else "W-whole-file",
            reader_names or ";".join(inline + manifest),
            ";".join(sorted(set(string_literals(body))))[:400],
            assertion,
            "no",
            "the reading's scope is the file itself. §4.1 and §4.2 rule 2 make "
            "the replacement scope (`Everything`, `Module`, `Item`) a decision "
            "per site, and a tool that guessed would widen a negative",
        )
    used = slicing

    if all(reader.role == "asserting" for reader in used):
        return Verdict(
            "P-assertion-helper",
            reader_names,
            selectors,
            assertion,
            "partial",
            "the reading and the assertion are both inside "
            + ", ".join(sorted({reader.name for reader in used}))
            + ", so converting that one function converts every call site and "
            "the sites themselves are not edited. §6.0 rule 4 serialises it: "
            "one shared helper, one ticket, before its consumers",
        )

    if direct or inline or manifest:
        return Verdict(
            "M2-file-and-body",
            reader_names,
            selectors,
            assertion,
            "no",
            "the test asserts over the whole file *and* over a named body "
            "(the file half reads "
            + ", ".join(sorted(set(direct) | set(inline) | set(manifest)))
            + "). Converting only the body half leaves the row on the debt "
            "list, and the file half is §4.1's scope decision",
        )

    if len({reader.name for reader in used}) > 1:
        return Verdict(
            "M-mixed-readers",
            reader_names,
            selectors,
            assertion,
            "partial",
            "the test reads through "
            + str(len({reader.name for reader in used}))
            + " readings whose end-of-body rules are "
            + ", ".join(sorted({reader.end_semantics for reader in used}))
            + "; each call site takes its own decision",
        )

    reader = used[0]
    if not reader.standard:
        why = []
        if reader.arity >= 2:
            why.append("the caller supplies the end of the slice")
        if reader.custom_ops:
            why.append("it slices further (" + ", ".join(reader.custom_ops) + ")")
            if ".split(" in reader.custom_ops and "#[cfg(test)]" in file.text[
                reader.item.open_at : reader.item.close_at
            ]:
                why.append(
                    "cutting the file at the text `#[cfg(test)]` — the "
                    "text-separator scope §2.3 and P16 replace with a named one"
                )
        if reader.end_semantics not in ("next-method", "own-close"):
            why.append(f"its end-of-body rule is `{reader.end_semantics}`")
        return Verdict(
            "H-custom-helper",
            reader_names,
            selectors,
            assertion,
            "no",
            f"`{reader.name}` is not a plain body finder: " + "; ".join(why),
        )

    if dynamic:
        bound = loop_literals(file, test)
        carried = sorted(
            {
                name
                for kind, name in dynamic
                if kind == "variable" and name in bound
            }
        )
        if carried and len(carried) == len({name for kind, name in dynamic}):
            selected = sorted({value for name in carried for value in bound[name]})
            return Verdict(
                "B5-loop-over-literals",
                reader_names,
                ";".join(selected)[:400],
                assertion,
                "partial",
                "the selector arrives through a loop variable whose array is "
                "written out in full ("
                + ", ".join(carried)
                + f", {len(selected)} literal(s)): a tool must rewrite the "
                "array as well as the call, in two places rather than one",
            )
        return Verdict(
            "R-runtime-selector",
            reader_names,
            selectors,
            assertion,
            "no",
            "a selector arrives as "
            + ", ".join(sorted({kind for kind, _ in dynamic}))
            + " ("
            + "; ".join(sorted({value for _, value in dynamic}))[:150]
            + "): the item it names is not decidable from the call site",
        )

    target_file = files.get((Path(row.file).parent / reader.target).as_posix())
    identities, notes = [], []
    for selector in literal + assembled:
        identity, note = selector_identity(selector, target_file)
        identities.append(identity)
        if not identity:
            notes.append(f"{selector.strip()[:60]} — {note}")

    over_read = reader.end_semantics == "next-method"
    span_note = (
        f"`{reader.name}` reads from the signature to the next item, so its "
        "slice carries the closing brace and whatever prose stands before the "
        "next declaration; `body_of` does not"
        if over_read
        else f"`{reader.name}` ends at the first `}}` at the item's own "
        "indentation, which is `body_of`'s span unless a nested block closes "
        "there first"
    )

    if not identities or any(not identity for identity in identities):
        return Verdict(
            "U-unresolved-selector",
            reader_names,
            selectors,
            assertion,
            "partial",
            "the finder is standard but "
            + str(sum(1 for identity in identities if not identity))
            + " of "
            + str(len(identities))
            + " selectors do not resolve to one declaration: "
            + "; ".join(notes)[:220],
        )

    if assembled:
        return Verdict(
            "A-assembled-needle",
            reader_names,
            selectors,
            assertion,
            "partial",
            "the selector is written in halves so the test does not match "
            "itself (plan P18); §2.6 provenance removes the need, but joining "
            "it is a hand edit at every site — " + span_note,
        )

    if kinds and set(kinds) <= {"contains", "!contains", "position", "equals"}:
        shape = "B1-body-contains"
    elif "count" in kinds:
        shape = "B3-body-count"
    elif "order" in kinds:
        shape = "B2-body-order"
    else:
        shape = "B4-body-other"

    verdict, reason = "yes", span_note
    if shape == "B4-body-other":
        verdict = "partial"
        reason = f"the slice is asserted with `{assertion}`; " + span_note
    elif "!contains" in kinds:
        # **A negative is never `yes`.** It is the assertion that goes quietly
        # green when a reading narrows, and §4.2 rule 2 requires an injection
        # in each destination class whoever writes the conversion. The span
        # changes at every one of these sites, in one direction or the other.
        verdict = "partial"
        reason = (
            "a negative over a slice whose width changes on conversion: §4.2 "
            "rule 2's injection is required here whoever writes the edit — "
            + span_note
        )
    elif "count" in kinds:
        verdict = "partial"
        reason = (
            "a count over a slice whose width changes on conversion; §4.2 "
            "rule 1 gives every count its own mutation — " + span_note
        )
    return Verdict(
        shape,
        reader_names,
        selectors,
        assertion,
        verdict,
        reason + "; identities: " + ", ".join(sorted(set(identities)))[:140],
    )


# ── the run ───────────────────────────────────────────────────────────────


def main() -> int:
    root = Path(__file__).resolve().parent.parent.parent
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=root)
    parser.add_argument("--debt", default="docs/plans/MIGRATION-DEBT.tsv")
    parser.add_argument(
        "--out", default="docs/plans/bt-app-reader-shapes-2026-09-21.tsv"
    )
    parser.add_argument("--tickets", default="P3,P14,P16")
    options = parser.parse_args()
    root = options.root.resolve()
    tickets = options.tickets.split(",")

    rows = [row for row in read_debt(root / options.debt) if row.ticket in tickets]

    files: dict[str, SourceFile] = {}
    for relative in sorted({row.file for row in rows if row.file.endswith(".rs")}):
        if (root / relative).exists():
            files[relative] = SourceFile.read(root / relative, relative)
    for relative in sorted(list(files)):
        for _, _, target in files[relative].consts:
            named = (Path(relative).parent / target).as_posix()
            if target.endswith(".rs") and named not in files and (root / named).exists():
                files[named] = SourceFile.read(root / named, named)
    readers_of = {
        relative: read_readers(file) for relative, file in sorted(files.items())
    }

    verdicts = [(row, shape_of_row(row, files, readers_of)) for row in rows]

    header = [
        "# The shapes a source reader takes — the "
        + ", ".join(tickets)
        + " rows of docs/plans/MIGRATION-DEBT.tsv, classified.",
        "# Generated by: python scripts/dev/bt-app-reader-shapes.py"
        + ("" if options.tickets == "P3,P14,P16" else f" --tickets {options.tickets}"),
        "# Read-only, re-runnable; the rows come out in the debt list's order.",
        "# mechanical: yes = a converter can rewrite the site and a person need "
        "not read it; partial = a converter can propose the rewrite and a "
        "person must read the result; no = a person decides.",
        "# row: the line of MIGRATION-DEBT.tsv this came from.",
        "\t".join(
            "row ticket file test shape reader selectors assertion mechanical "
            "reason".split()
        ),
    ]
    body = [
        "\t".join(
            cell.replace("\t", " ").replace("\n", " ")
            for cell in (
                f"D{row.line:04d}",
                row.ticket,
                row.file,
                row.owner,
                verdict.shape,
                verdict.reader,
                verdict.selectors,
                verdict.assertion,
                verdict.mechanical,
                verdict.reason,
            )
        )
        for row, verdict in verdicts
    ]
    (root / options.out).write_text(
        "\n".join(header + body) + "\n", encoding="utf-8", newline="\n"
    )

    shapes = Counter(verdict.shape for _, verdict in verdicts)
    per_shape: dict[str, Counter] = defaultdict(Counter)
    for _, verdict in verdicts:
        per_shape[verdict.shape][verdict.mechanical] += 1
    print(f"{len(rows)} rows of {options.debt} in tickets {', '.join(tickets)}")
    print(f"written to {options.out}")
    print()
    print(f"{'shape':<24}{'rows':>6}{'yes':>6}{'part':>6}{'no':>6}  what it is")
    for shape, total in sorted(shapes.items(), key=lambda pair: (-pair[1], pair[0])):
        counts = per_shape[shape]
        print(
            f"{shape:<24}{total:>6}{counts['yes']:>6}{counts['partial']:>6}"
            f"{counts['no']:>6}  {SHAPE_NAMES.get(shape, '')}"
        )
    counted = Counter(verdict.mechanical for _, verdict in verdicts)
    total = max(len(rows), 1)
    print()
    print(
        f"all {len(rows)} rows: fully mechanical {counted['yes']} "
        f"({counted['yes'] * 100 // total}%), partial {counted['partial']} "
        f"({counted['partial'] * 100 // total}%), a person decides "
        f"{counted['no']} ({counted['no'] * 100 // total}%)"
    )
    families = {
        "body pins (B/A/M/P/H/R/U)": BODY_PIN_SHAPES,
        "whole-file readings (W/N)": ("W-whole-file", "N-whole-file-negative"),
        "not a source reading (S/F/Q/L)": (
            "S-no-source-read",
            "F-fixture",
            "Q-runtime-file",
            "L-ledger-key",
        ),
        "bindings (C)": ("C-include-binding",),
        "unplaced (X)": ("X-not-found", "X-unread"),
    }
    print()
    for label, group in families.items():
        count = sum(1 for _, verdict in verdicts if verdict.shape in group)
        print(f"  {label:<32}{count:>5}  ({count * 100 // total}%)")
    pins = [pair for pair in verdicts if pair[1].shape in BODY_PIN_SHAPES]
    pin_yes = sum(1 for _, verdict in pins if verdict.mechanical == "yes")
    pin_part = sum(1 for _, verdict in pins if verdict.mechanical == "partial")
    print(
        f"of the {len(pins)} rows that really are body pins: {pin_yes} fully "
        f"mechanical ({pin_yes * 100 // max(len(pins), 1)}%), {pin_part} partial "
        f"({pin_part * 100 // max(len(pins), 1)}%), "
        f"{len(pins) - pin_yes - pin_part} a person decides"
    )
    print()
    print("every reading these files declare, by its end-of-body rule:")
    semantics: Counter = Counter()
    for relative in sorted(readers_of):
        for reader in readers_of[relative]:
            semantics[(reader.role, reader.end_semantics, reader.arity)] += 1
    for (role, rule, arity), count in sorted(
        semantics.items(), key=lambda pair: (-pair[1], pair[0])
    ):
        print(f"  {role:<10}{rule:<21}arity {arity}{count:>6}")
    print()
    print("the readings the classified rows actually call, by name and rule:")
    named: Counter = Counter()
    for row, verdict in verdicts:
        if verdict.shape in BODY_PIN_SHAPES and verdict.reader:
            named[(Path(row.file).name, verdict.reader)] += 1
    for (where, name), count in sorted(
        named.items(), key=lambda pair: (-pair[1], pair[0])
    )[:20]:
        print(f"  {where:<28}{name:<28}{count:>5}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
