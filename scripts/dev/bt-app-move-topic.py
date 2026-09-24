"""Move one 2a topic out of `main.rs`'s `impl Runtime<'_>` blocks, as a pure move.

Read-only unless `--write`. Every run prints its plan first, and `--write` does
exactly the plan it printed. No Cargo; the tree is read and rewritten as text.

    python scripts/dev/bt-app-move-topic.py --plan quake
    python scripts/dev/bt-app-move-topic.py --plan quake --write
    python scripts/dev/bt-app-move-topic.py --check quake --base <rev>

`--plan` prints the eleven steps of the move with their findings and, at the
end, the reviewer's table `docs/plans/bt-app-split.md` §6.1 asks for: one row
per item with its destination, the visibility it needs, and the callers that
change. `--check` is the §7.4 proof on its own, over a topic that has already
moved: it reads the pre-move text out of Git and compares it with what stands
in the topic file today.

Requires `target/review-python` (`tree_sitter==0.25.2`, `tree_sitter_rust==0.24.2`),
the same parsers the retained inventory scripts vendor:

    python -m pip install --target target/review-python \\
        tree_sitter==0.25.2 tree_sitter_rust==0.24.2

The destinations, the caller witnesses and the visibility column come from the
2a manifest TSV in `docs/plans/`; the newest one is used unless `--manifest`
names another. Names and calls there are syntax evidence, not Rust type
resolution, and this script inherits that limit: **it decides where text goes,
never whether the program still means the same thing.** `cargo build`, the
crate's tests and `cargo clippy -- -D warnings` decide that.

WHAT THE DRY RUN (P11, 2026-09-22) ESTABLISHED, AND THIS SCRIPT ENFORCES

* **`pub(crate)`, not the manifest's column, for any method still called from
  the crate root.** The manifest computes its visibility against a tree where
  every caller has also moved. A topic-at-a-time move does not have that tree:
  a caller still written in `main.rs` is at the crate root, and
  `pub(in crate::runtime)` does not reach the crate root — `error[E0624]`. So
  the visibility is computed against *the set that actually moves*, and a
  method every one of whose callers moves with it stays private. Of the
  twenty-three methods of the `profiles` topic, twenty-two needed `pub(crate)`
  and one — `adopt_profile_table` — did not. When every topic has landed, a
  closing pass may narrow the widened set back to the manifest's column; that
  pass is not this script's job and must be reviewed on its own.

* **The direction that breaks is root-to-child, and only for methods.** A
  private *item* of the crate root — a free function, a `struct`, a `mod` — is
  visible in every descendant module, so a topic file may name
  `crate::native_window` with nothing widened. Private inherent *methods* do
  not work that way, which is the whole of the visibility problem.

* **`runtime/mod.rs` declares modules and imports nothing.** Twelve of the
  twenty-eight topic stems — attention, diagnostics, files, first_run, git,
  i18n, palette, preview, profiles, quake, search, settings — are already the
  name of a crate-root module. Inside a topic file that is harmless
  (`use crate::profiles;` binds a name the module does not otherwise hold, and
  `runtime/profiles.rs` names it twenty-seven times). Inside `runtime/mod.rs`
  it is `error[E0255]`, so this script refuses to write a `use` there.

* **A moved item may not carry a relative input.** `include_str!`,
  `include_bytes!`, `#[path]`, `file!()` and `module_path!` resolve against the
  file that declares them, so their meaning changes when the file changes
  (§7.4's fourth audit). Any of them in a moved item stops the run.

* **A moved item may not carry a platform `cfg`.** Strict 2a moves none —
  every one of `main.rs`'s ten stands outside both `impl` blocks — and an
  entry appearing in a `runtime/*.rs` file turns both
  `platform_gate_tests::only_the_named_files_decide_what_platform_this_is` and
  `scripts/check-portable-core.ps1` red by design. Either the item does not
  move or `FILES_THAT_MAY_NAME_A_PLATFORM` gains the new path in the same
  commit, which `--allow-platform-cfg` acknowledges and this script does not do
  for you.

* **`rustfmt` may re-wrap a declaration that the visibility prefix pushed past
  the width limit** — two of the twenty-three `profiles` signatures. The body
  is untouched, so §7.4's body comparison still passes byte for byte; the
  declaration comparison has to allow that one re-wrap, and `--check` reports
  every occurrence by name rather than passing it over.

WHAT STEP 2a ITSELF ADDED (2026-09-22)

* **The import finder reads four shapes**: path heads, types, macros, and
  bare values (constants, statics, functions named as values), skipping what
  the item binds itself, a `fn`'s own name, anything after a `.`, and the
  anonymous `const _`. `launch`, `clipboard` and `settings` each found a gap.

* **Two import edits stay the compiler's, not this script's.** A trait
  imported for its methods (`.context(…)` names no trait) is found from
  rustc's E0599 suggestion, taking the one candidate the crate root itself
  imports. And a crate-root import whose last user moved out goes unused in
  `main.rs`; rustc's warning names it, and it is removed only when no other
  unqualified spelling of the name remains in `main.rs`. Each such edit is
  named in its topic's commit.

Written for `docs/plans/bt-app-split-prep.md` §6.4 P11 and
`docs/plans/bt-app-split.md` §6.1/§6.5.
"""

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path('target/review-python').resolve()))
from tree_sitter import Language, Parser  # noqa: E402
import tree_sitter_rust  # noqa: E402

PARSER = Parser(Language(tree_sitter_rust.language()))

# A relative input resolves against the file that declares it (§7.4).
RELATIVE_INPUTS = ('include_str!', 'include_bytes!', '#[path', 'file!()', 'module_path!')
# The lexical rule `platform_gate_tests` and `check-portable-core.ps1` share.
PLATFORM_WORDS = ('windows', 'unix', 'macos', 'target_os', 'target_family')
PLATFORM_CFG = re.compile(r'cfg\(|cfg!\(|cfg_attr\(')


# --------------------------------------------------------------------------
# reading


def text(src, node):
    return src[node.start_byte:node.end_byte].decode('utf-8')


def walk(node):
    yield node
    for child in node.named_children:
        yield from walk(child)


def parse(src, what):
    if b'\r\n' in src:
        raise SystemExit(f'{what} has CRLF line endings; this tree is LF')
    return PARSER.parse(src).root_node


def git_show(rev, path):
    return subprocess.check_output(['git', 'show', f'{rev}:{path}'])


def code_only(body):
    """The lines that are not whole-line comments — the reading the portable
    gate and `the_shell_page_is_gone` both take."""
    return '\n'.join(l for l in body.split('\n') if not l.lstrip().startswith('//'))


# --------------------------------------------------------------------------
# step 2 — the impl blocks


def impl_blocks(src, root, type_name):
    """Every top-level inherent `impl <type_name>…` block, trait impls skipped."""
    blocks = []
    for node in root.named_children:
        if node.type != 'impl_item':
            continue
        if node.child_by_field_name('trait') is not None:
            continue
        self_ty = node.child_by_field_name('type')
        if self_ty is None:
            continue
        spelling = text(src, self_ty)
        # `impl Runtime<'_>` and `impl crate::Runtime<'_>` are the same block.
        if spelling.split('<')[0].strip().rsplit('::', 1)[-1] != type_name:
            continue
        blocks.append(node)
    return blocks


# --------------------------------------------------------------------------
# step 3 — one item's full extent


def full_start(src, fn_node, floor):
    """The first byte of the item: its attributes, then back over the
    contiguous `///` / `//` / `#[…]` lines directly above it, stopping at a
    blank line. A doc comment left behind is not a pure move."""
    start = fn_node.start_byte
    for child in fn_node.children:
        if child.type == 'attribute_item':
            start = min(start, child.start_byte)
    at = src.rfind(b'\n', 0, start) + 1
    while at > floor:
        previous_end = at - 1
        previous_start = src.rfind(b'\n', 0, previous_end) + 1
        line = src[previous_start:previous_end].strip()
        if line.startswith(b'///') or line.startswith(b'//') or line.startswith(b'#['):
            at = previous_start
            continue
        break
    return at


def collect(src, blocks, names):
    """Steps 3 and 4: the full extent of each named method, and a hard failure
    on a name that is missing or declared twice."""
    found, seen = {}, {}
    for block in blocks:
        body = block.child_by_field_name('body')
        for node in body.named_children:
            if node.type != 'function_item':
                continue
            name = text(src, node.child_by_field_name('name'))
            seen[name] = seen.get(name, 0) + 1
            if name in names:
                if name in found:
                    raise SystemExit(
                        f'`{name}` is declared {seen[name]} times in these blocks; '
                        'the manifest row is ambiguous and the move is not mechanical')
                found[name] = (node, body.start_byte + 1)
    missing = [n for n in names if n not in found]
    if missing:
        raise SystemExit('not declared in an `impl` block of this type: ' + ', '.join(missing))

    cuts = []
    for name, (node, floor) in found.items():
        start = full_start(src, node, floor)
        end = node.end_byte
        tail = end
        # take the one blank line after the item, so the residue keeps its shape
        for _ in range(2):
            if src[tail:tail + 1] == b'\n':
                tail += 1
        body = node.child_by_field_name('body')
        cuts.append({
            'name': name,
            'start': start,
            'end': end,
            'cut_end': tail,
            'line': src[:start].count(b'\n') + 1,
            'item': src[start:end].decode('utf-8'),
            'body': src[body.start_byte:body.end_byte].decode('utf-8'),
            'body_sha': hashlib.sha256(src[body.start_byte:body.end_byte]).hexdigest(),
            'declaration': src[start:body.start_byte].decode('utf-8'),
        })
    cuts.sort(key=lambda cut: cut['start'])
    return cuts


# --------------------------------------------------------------------------
# step 6 — what the moved text needs bound at its new depth


def root_bindings(src, root):
    """The flat table of every name the crate root binds: its `use` trees, its
    `mod` declarations and its own top-level items."""
    modules, items, uses = set(), set(), {}

    def from_use(node, prefix):
        kind = node.type
        if kind == 'scoped_use_list':
            path = node.child_by_field_name('path')
            head = text(src, path) if path is not None else ''
            for child in node.named_children:
                if child.type == 'use_list':
                    for leaf in child.named_children:
                        from_use(leaf, (prefix + '::' + head).strip(':'))
        elif kind == 'use_list':
            for leaf in node.named_children:
                from_use(leaf, prefix)
        elif kind == 'use_as_clause':
            alias = text(src, node.child_by_field_name('alias'))
            uses[alias] = (prefix + '::' + text(src, node.child_by_field_name('path'))).strip(':')
        elif kind in ('scoped_identifier', 'identifier'):
            full = (prefix + '::' + text(src, node)).strip(':')
            uses[full.split('::')[-1]] = full

    carriers = ('function_item', 'struct_item', 'enum_item', 'const_item',
                'static_item', 'type_item', 'union_item', 'trait_item')
    for node in root.named_children:
        if node.type == 'mod_item':
            modules.add(text(src, node.child_by_field_name('name')))
        elif node.type == 'use_declaration':
            from_use(node.child_by_field_name('argument'), '')
        elif node.type in carriers:
            name = node.child_by_field_name('name')
            # `const _: … = …;` binds nothing: `_` is the anonymous item, and
            # `use crate::{_};` does not parse.
            if name is not None and text(src, name) != '_':
                items.add(text(src, name))
    return modules, items, uses


# The fields under which an identifier is a binding the item makes, not a name
# it reads: `let x`, `|x|`, `fn f(x: T)`, `for x in`, `if let Some(x)`, a match
# arm's pattern, and the name of a `fn` nested in a body.
BINDING_FIELDS = {
    'let_declaration': 'pattern', 'parameter': 'pattern', 'for_expression': 'pattern',
    'let_condition': 'pattern', 'match_arm': 'pattern', 'function_item': 'name',
}


def bound_names(src, tree):
    bound = set()
    for node in walk(tree):
        field = BINDING_FIELDS.get(node.type)
        # The moved method's own name binds nothing in its body: a method is
        # reached through `self`, so a bare call of the same spelling is a
        # crate-root free function (`copy_selection` is both).
        if (node.type == 'function_item' and node.parent is not None
                and node.parent.type == 'declaration_list'
                and node.parent.parent is not None
                and node.parent.parent.type == 'impl_item'):
            continue
        if field is not None:
            target = node.child_by_field_name(field)
            if target is not None:
                bound.update(text(src, n) for n in walk(target) if n.type == 'identifier')
        elif node.type == 'closure_parameters':
            bound.update(text(src, n) for n in walk(node) if n.type == 'identifier')
    return bound


def named_by(cuts):
    """Every name the moved items write, as a set. Over-approximate on purpose:
    the filter below keeps only the ones written unqualified, and `cargo build`
    is the authority on what is left over.

    Four shapes: the head of a path (`settings::Choice`), a type
    (`LogicalSize`), a macro (`anyhow!`), and a bare value — a constant, a
    static or a function named as a value (`INITIAL_WIDTH`, `.map(revive_plan)`),
    including inside a macro's arguments. A bare identifier the item itself
    binds (a local, a parameter, a closure argument, a pattern) is its own and
    is not asked of the crate root. An import of a root name can never make a
    name resolve differently from how it resolved at the root; an extra one is
    a warning and a missing one an error, so either way the build says so."""
    wanted = set()
    for cut in cuts:
        src = ("impl X {\n" + cut['item'] + "\n}\n").encode('utf-8')
        tree = parse(src, cut['name'])
        bound = bound_names(src, tree)
        for node in walk(tree):
            if node.type in ('scoped_identifier', 'scoped_type_identifier'):
                wanted.add(text(src, node).split('::')[0].strip())
            elif node.type == 'macro_invocation':
                name = node.child_by_field_name('macro')
                if name is not None and name.type == 'identifier':
                    wanted.add(text(src, name))
            elif node.type == 'identifier':
                spelling = text(src, node)
                parent = node.parent
                declared = (parent is not None and parent.type == 'function_item'
                            and parent.child_by_field_name('name').start_byte == node.start_byte)
                # a `fn` item's own name is a declaration, never a use
                if spelling not in bound and not declared:
                    wanted.add(spelling)
            elif node.type in ('type_identifier', 'generic_type'):
                spelling = text(src, node).split('<')[0].strip()
                if re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', spelling):
                    wanted.add(spelling)
    return wanted


def imports(cuts, modules, items, uses, type_name):
    """Step 6. A name earns a `use` line when the crate root binds it AND the
    moved code writes it unqualified at least once outside a comment. After
    a `.` it is a field or a method (inside a macro's arguments the parser
    hands those over as bare identifiers), not a name the module binds."""
    code = code_only('\n'.join(cut['item'] for cut in cuts))
    unqualified = set()
    for name in named_by(cuts):
        if re.search(r'(?<![\w:.])' + re.escape(name) + r'(?![\w])', code):
            unqualified.add(name)

    from_crate, from_elsewhere, unresolved = {type_name}, {}, set()
    for name in sorted(unqualified):
        if name in modules or name in items:
            from_crate.add(name)
        elif name in uses:
            from_elsewhere.setdefault(uses[name].rsplit('::', 1)[0], set()).add(name)
        else:
            unresolved.add(name)

    lines = []
    for owner in sorted(from_elsewhere):
        names = sorted(from_elsewhere[owner])
        if len(names) == 1:
            lines.append(f'use {owner}::{names[0]};')
        else:
            lines.append(f'use {owner}::{{{", ".join(names)}}};')
    lines.append('use crate::{' + ', '.join(
        sorted(from_crate, key=lambda s: (s[0].islower(), s))) + '};')
    return lines, sorted(unresolved)


# --------------------------------------------------------------------------
# step 7 — the visibility the set that actually moves needs


def visibility(rows, moving):
    """`pub(crate)` for any method with a caller outside the moving set — during
    a topic-at-a-time move that caller is at the crate root, which
    `pub(in crate::runtime)` does not reach. Private when every caller moves too.
    No witness at all is read as "a caller this pass cannot see"."""
    decided = {}
    for row in rows:
        callers = json.loads(row['callers']) if row.get('callers') else []
        outside = []
        for caller in callers:
            symbol = re.sub(r'^impl (.*?::)?', '', caller['symbol'])
            if symbol not in moving:
                outside.append(caller)
        decided[row['name']] = {
            'needed': 'pub(crate)' if (outside or not callers) else '',
            'manifest': row.get('visibility', ''),
            'outside': outside,
            'inside': len(callers) - len(outside),
        }
    return decided


# --------------------------------------------------------------------------
# steps 8 and 9 — the two refusals


def refuse_relative_inputs(cuts):
    offences = [(cut['name'], token) for cut in cuts
                for token in RELATIVE_INPUTS if token in cut['item']]
    if offences:
        raise SystemExit(
            'these items resolve an input against the file that declares them, so moving '
            'the file changes what they read (prep §7.4, fourth audit):\n  '
            + '\n  '.join(f'{name}: {token}' for name, token in offences))


def refuse_platform_cfg(cuts, allowed):
    offences = []
    for cut in cuts:
        for number, line in enumerate(cut['item'].split('\n'), 1):
            code = line.split('//')[0]
            if PLATFORM_CFG.search(code) and any(w in code for w in PLATFORM_WORDS):
                offences.append(f'{cut["name"]}+{number}: {line.strip()}')
    if offences and not allowed:
        raise SystemExit(
            'these items decide what platform they are on, and a `runtime/*.rs` file is not on '
            '`FILES_THAT_MAY_NAME_A_PLATFORM`; both the Rust gate and check-portable-core.ps1 '
            'will name the new path. Either the item does not move, or the list grows in the '
            'same commit and this run is repeated with --allow-platform-cfg:\n  '
            + '\n  '.join(offences))
    return offences


def refuse_import_in_mod_rs(mod_rs):
    """Step 8's other half. Twelve topic stems are also crate-root module names,
    and `mod settings;` beside `use crate::settings;` is `error[E0255]`."""
    for line in mod_rs.split('\n'):
        if line.strip().startswith('use '):
            raise SystemExit(
                f'{line.strip()}\n`runtime/mod.rs` declares modules and imports nothing - '
                'twelve topic stems are also crate-root module names and the two would collide')


# --------------------------------------------------------------------------
# step 10 — the proof


def prove(cuts, topic_path):
    """§7.4's first audit, over every item rather than a sample: each body is
    byte-identical, and each item stands verbatim apart from the one visibility
    prefix the move is allowed — or, where the prefix pushed the signature past
    the width limit, apart from `rustfmt`'s re-wrap of that declaration."""
    src = topic_path.read_bytes()
    root = parse(src, str(topic_path))
    after = {}
    for node in walk(root):
        if node.type != 'function_item':
            continue
        name = text(src, node.child_by_field_name('name'))
        body = node.child_by_field_name('body')
        after[name] = src[body.start_byte:body.end_byte]

    whole = src.decode('utf-8')
    same, changed, reflowed, missing = [], [], [], []
    for cut in cuts:
        arrived = after.get(cut['name'])
        if arrived is None:
            missing.append(cut['name'])
            continue
        (same if hashlib.sha256(arrived).hexdigest() == cut['body_sha']
         else changed).append(cut['name'])
        if not any(cut['item'].replace('    fn ' + cut['name'],
                                       '    ' + prefix + 'fn ' + cut['name'], 1) in whole
                   for prefix in ('', 'pub(crate) ', 'pub(in crate::runtime) ')):
            reflowed.append(cut['name'])
    return {'same': same, 'changed': changed, 'reflowed': reflowed, 'missing': missing}


# --------------------------------------------------------------------------
# the manifest


def newest_manifest(plans):
    found = sorted(plans.glob('bt-app-split-2a-manifest-*.tsv'))
    if not found:
        raise SystemExit(f'no 2a manifest under {plans}; run bt-app-split-freshness.py first')
    return found[-1]


def manifest_rows(path, destination):
    import csv
    with path.open(encoding='utf-8', newline='') as handle:
        rows = [r for r in csv.DictReader(handle, delimiter='\t')
                if r['destination'] == destination]
    if not rows:
        raise SystemExit(f'{path.name} files no method under `{destination}`')
    return rows


# --------------------------------------------------------------------------
# the run


def report(cuts, decided, use_lines, unresolved, platform, args, manifest, blocks):
    print(f'# {args.plan or args.check} - {len(cuts)} methods from {len(blocks)} '
          f'`impl {args.type_name}` blocks of {args.root}')
    print(f'# manifest: {manifest.name}')
    print()
    print('1. root parsed, LF')
    print(f'2. {len(blocks)} top-level inherent `impl {args.type_name}` blocks')
    print('3. full extents taken, doc comments and attributes included')
    print(f'4. every requested name found exactly once ({len(cuts)})')
    total = sum(cut['cut_end'] - cut['start'] for cut in cuts)
    print(f'5. {total} bytes to cut, in reverse byte order')
    print('6. the `use` lines the moved names need at their new depth:')
    for line in use_lines:
        print(f'       {line}')
    if unresolved:
        print('   not bound at the crate root (extern crate or prelude, no line needed):')
        print('       ' + ', '.join(unresolved))
    widened = [n for n, d in decided.items() if d['needed'] and d['needed'] != d['manifest']]
    kept = [n for n, d in decided.items() if not d['needed']]
    print(f'7. visibility against the set that moves: {len(cuts) - len(kept)} `pub(crate)`, '
          f'{len(kept)} private')
    if widened:
        print(f'   wider than the manifest column, because their callers have not moved yet '
              f'({len(widened)}):')
        print('       ' + ', '.join(sorted(widened)))
    if kept:
        print('   private - every caller moves with them:')
        print('       ' + ', '.join(sorted(kept)))
    print('8. `runtime/mod.rs` declares and imports nothing - checked')
    print(f'9. relative inputs: none; platform `cfg`: {len(platform)}')
    print()
    print('11. the reviewer\'s table')
    print(f'    {"item":34}{"visibility":26}{"line":>8}  {"bytes":>7}  callers that change')
    for cut in cuts:
        d = decided.get(cut['name'], {'needed': 'pub(crate)', 'outside': []})
        names = sorted({re.sub(r"^impl (.*?::)?", "", c['symbol']) for c in d['outside']})
        shown = ', '.join(names[:3]) + (f' (+{len(names) - 3})' if len(names) > 3 else '')
        print(f'    {cut["name"]:34}{d["needed"] or "private":26}{cut["line"]:>8}  '
              f'{cut["end"] - cut["start"]:>7}  {shown}')


def do_check(args, plans):
    manifest = Path(args.manifest) if args.manifest else newest_manifest(plans)
    destination = f'{args.package_dir}/{args.check}.rs'
    rows = manifest_rows(manifest, destination)
    names = [r['name'] for r in rows]
    before = git_show(args.base, args.root)
    cuts = collect(before, impl_blocks(before, parse(before, f'{args.base}:{args.root}'),
                                       args.type_name), names)
    topic = Path(args.root).parent / f'{args.package_dir}/{args.check}.rs'
    if not topic.exists():
        raise SystemExit(f'{topic} does not exist; nothing has moved there')
    result = prove(cuts, topic)
    print(f'# --check {args.check} against {args.base}')
    print(f'  bodies byte-identical      {len(result["same"])}/{len(cuts)}')
    print(f'  declarations verbatim      {len(cuts) - len(result["reflowed"])}/{len(cuts)}')
    for name in result['changed']:
        print(f'  BODY DIFFERS               {name}')
    for name in result['missing']:
        print(f'  NOT IN THE TOPIC FILE      {name}')
    for name in result['reflowed']:
        print(f'  declaration re-wrapped     {name}  (body untouched; rustfmt after the prefix)')
    ok = not result['changed'] and not result['missing']
    print('\nRESULT:', 'pure move' if ok else 'NOT A PURE MOVE')
    return 0 if ok else 1


def do_plan(args, plans):
    manifest = Path(args.manifest) if args.manifest else newest_manifest(plans)
    destination = f'{args.package_dir}/{args.plan}.rs'
    rows = manifest_rows(manifest, destination)
    names = [r['name'] for r in rows]

    root_path = Path(args.root)
    src = root_path.read_bytes()
    root = parse(src, args.root)
    blocks = impl_blocks(src, root, args.type_name)
    if not blocks:
        raise SystemExit(f'no top-level inherent `impl {args.type_name}` block in {args.root}')

    cuts = collect(src, blocks, names)
    # Every refusal fires here, while the tree is still the tree that was read:
    # a run that stops must leave nothing half-written behind it.
    refuse_relative_inputs(cuts)
    platform = refuse_platform_cfg(cuts, args.allow_platform_cfg)
    directory = root_path.parent / args.package_dir
    mod_rs = directory / 'mod.rs'
    existing = mod_rs.read_text(encoding='utf-8') if mod_rs.exists() else None
    if existing is not None:
        refuse_import_in_mod_rs(existing)

    modules, items, uses = root_bindings(src, root)
    use_lines, unresolved = imports(cuts, modules, items, uses, args.type_name)
    decided = visibility(rows, set(names))

    report(cuts, decided, use_lines, unresolved, platform, args, manifest, blocks)

    if args.record:
        Path(args.record).write_text(json.dumps(cuts, indent=2), encoding='utf-8', newline='\n')
    if not args.write:
        print('\n(read-only; pass --write to do exactly this)')
        return 0

    # 5. cut in reverse byte order, so the earlier offsets stay valid
    residue = src
    for cut in sorted(cuts, key=lambda c: c['start'], reverse=True):
        residue = residue[:cut['start']] + residue[cut['cut_end']:]

    # the declaration, at a unique textual anchor — never at a line number
    holder = f'mod {Path(args.package_dir).name};\n'.encode('utf-8')
    if holder not in residue:
        anchor = args.declare_after.encode('utf-8') + b'\n'
        if residue.count(anchor) != 1:
            raise SystemExit(f'--declare-after {args.declare_after!r} is not a unique line')
        at = residue.index(anchor) + len(anchor)
        residue = residue[:at] + holder + residue[at:]
    root_path.write_bytes(residue)

    directory.mkdir(parents=True, exist_ok=True)
    declarations = set()
    if existing is not None:
        declarations = {l.strip() for l in existing.split('\n') if l.strip().startswith('mod ')}
        header = existing.split('\nmod ')[0].rstrip('\n')
    else:
        header = MOD_RS_HEADER.rstrip('\n')
    declarations.add(f'mod {args.plan};')
    mod_rs.write_text(header + '\n\n' + '\n'.join(sorted(declarations)) + '\n',
                      encoding='utf-8', newline='\n')

    pieces = []
    for cut in cuts:
        item = cut['item']
        prefix = decided[cut['name']]['needed']
        if prefix:
            at = item.index('    fn ')
            item = item[:at] + f'    {prefix} fn ' + item[at + len('    fn '):]
        pieces.append(item)
    banner = args.header and Path(args.header).read_text(encoding='utf-8')
    if not banner:
        banner = (f'//! `{args.plan}` — moved out of `{root_path.name}`\'s '
                  f'`impl {args.type_name}` blocks by\n'
                  f'//! `scripts/dev/bt-app-move-topic.py`. Bodies unchanged.\n\n'
                  + '\n'.join(use_lines) + f'\n\nimpl {args.type_name}<\'_> {{\n')
    topic = directory / f'{args.plan}.rs'
    topic.write_text(banner + '\n\n'.join(pieces) + '\n}\n', encoding='utf-8', newline='\n')

    print(f'\nwrote {topic} ({len(pieces)} items) and rewrote {root_path}')
    result = prove(cuts, topic)
    print(f'10. bodies byte-identical {len(result["same"])}/{len(cuts)}; '
          f'declarations verbatim {len(cuts) - len(result["reflowed"])}/{len(cuts)}')
    print('    next: `cargo fmt -p bt-app`, then `--check` again - rustfmt may re-wrap a '
          'signature the prefix pushed past the width limit.')
    return 0


MOD_RS_HEADER = """//! **`Runtime`'s inherent methods, by topic** (`docs/plans/bt-app-split.md` §6.1).
//!
//! One file per topic, each holding one `impl Runtime<'_>` block and the
//! methods of that topic byte for byte as `main.rs` wrote them.
//!
//! **This file declares modules and imports nothing.** Twelve of the topic
//! stems are also the name of a crate-root module, and `mod settings;` beside
//! `use crate::settings;` is `error[E0255]`. A topic file writes its own
//! imports, where the two names cannot collide.
"""


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--plan', help='the topic to move, by its manifest stem (e.g. quake)')
    ap.add_argument('--check', help='prove an already-moved topic against --base')
    ap.add_argument('--write', action='store_true', help='do exactly the plan that was printed')
    ap.add_argument('--root', default='crates/bt-app/src/main.rs')
    ap.add_argument('--type', dest='type_name', default='Runtime')
    ap.add_argument('--package-dir', default='runtime', help='the directory under src/')
    ap.add_argument('--manifest', default=None, help='a 2a manifest TSV; newest by default')
    ap.add_argument('--plans', default='docs/plans')
    ap.add_argument('--base', default='HEAD', help='--check: the revision the topic moved from')
    ap.add_argument('--declare-after', default='mod restore;',
                    help='the unique line `mod <package-dir>;` is inserted after')
    ap.add_argument('--header', default=None, help='a file whose text opens the topic file')
    ap.add_argument('--record', default=None, help='write the per-item record as JSON')
    ap.add_argument('--allow-platform-cfg', action='store_true')
    args = ap.parse_args()

    if bool(args.plan) == bool(args.check):
        ap.error('name exactly one of --plan and --check')
    plans = Path(args.plans)
    return do_check(args, plans) if args.check else do_plan(args, plans)


if __name__ == '__main__':
    raise SystemExit(main())
