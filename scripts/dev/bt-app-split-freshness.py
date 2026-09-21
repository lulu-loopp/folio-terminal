"""Read-only source audit; no Cargo. Run from the worktree root.

python scripts/dev/bt-app-split-freshness.py --base 76ca0788 --commit 1f1d2daa \
    --date 2026-09-21 --report docs/plans/bt-app-split-inventory-2026-09-21.md
Requires target/review-python: tree_sitter==0.25.2, tree_sitter_rust==0.24.2.
The retained graph/table scripts are run separately; this supplements their AST
inventory with destinations, caller witnesses, and transitive source consumers.
Source inputs are git blobs; target/bt-app-graph.json must first be generated
by the retained script on the same source tree (the audit checks that tree).
Results remain reproducible after this docs commit.
Names/calls are syntax evidence, NOT Rust type resolution. See the report.
"""
import argparse
import csv
import io
import json
import re
import subprocess
import sys
from collections import Counter, defaultdict
from datetime import date
from difflib import SequenceMatcher
from pathlib import Path

sys.path.insert(0, str(Path('target/review-python').resolve()))
from tree_sitter import Language, Parser
import tree_sitter_rust

PARSER = Parser(Language(tree_sitter_rust.language()))
PREFIX = 'crates/bt-app/'
OUT = Path('docs/plans')
STEM = 'bt-app-split-'

def git(*args):
    return subprocess.check_output(['git', *args])

def text(n):
    return n.text.decode('utf-8') if n else ''

def walk(n):
    yield n
    for c in n.named_children:
        yield from walk(c)

def extent(n):
    return [n.start_point.row + 1, n.end_point.row + 1]

def strings(n):
    return [text(x) for x in walk(n) if x.type in ('string_literal', 'raw_string_literal')]

def decoded(s):
    try: return json.loads(s)
    except (ValueError, TypeError): return s.replace('\\n','\n').replace('\\"','"')

def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n', encoding='utf-8', newline='\n')

def tsv(path, rows, fields):
    out = io.StringIO(newline='')
    w = csv.DictWriter(out, fields, delimiter='\t', lineterminator='\n', extrasaction='ignore')
    w.writeheader()
    for r in rows:
        w.writerow({k: json.dumps(v, ensure_ascii=False) if isinstance(v, (dict, list)) else v for k,v in r.items()})
    path.write_text(out.getvalue(), encoding='utf-8', newline='\n')

def snapshot(commit):
    paths = git('ls-tree', '-r', '--name-only', commit, '--', PREFIX).decode().splitlines()
    paths = [p for p in paths if p.endswith('.rs')]
    # One git batch, no checkout or temporary product tree.
    proc = subprocess.run(['git', 'cat-file', '--batch'], input=''.join(f'{commit}:{p}\n' for p in paths).encode(), stdout=subprocess.PIPE, check=True)
    stream = io.BytesIO(proc.stdout)
    result = {}
    for p in paths:
        header = stream.readline().split()
        data = stream.read(int(header[2])); stream.read(1)
        result[p[len(PREFIX):]] = data
    return result

def analyse(files):
    result = {'items': [], 'methods': [], 'includes': [], 'attributes': [], 'platform': [], 'calls': [], 'tests': [], 'defs': [], 'errors': []}
    for p,b in files.items():
        tree = PARSER.parse(b)
        if tree.root_node.has_error: result['errors'].append(p)
        initial = () if p == 'src/main.rs' else tuple(p.removeprefix('src/').removesuffix('.rs').split('/'))
        initial={'src/preview_viewport_tests.rs':('preview_viewport','tests'),'src/focus_thumb_restore_tests.rs':('focus_thumb','restore_tests'),'src/attention_words/mod.rs':('attention_words',)}.get(p,initial)
        def visit(n, ctx, attrs=(), enclosing=None, runtime=False):
            name = text(n.child_by_field_name('name'))
            isimpl = n.type == 'impl_item'
            ty = text(n.child_by_field_name('type')) if isimpl else ''
            trait = text(n.child_by_field_name('trait')) if isimpl else ''
            rt = isimpl and ty == "Runtime<'_>" and not trait and p == 'src/main.rs'
            ident = '::'.join(ctx + ((name,) if name else ()))
            row = {'file':p, 'symbol':ident, 'kind':n.type, 'start':extent(n)[0], 'end':extent(n)[1], 'attributes':list(attrs)}
            if n.type in ('mod_item','impl_item','function_item','struct_item','enum_item','const_item','static_item'):
                if isimpl: row['symbol'] = '::'.join(ctx + (f'impl {trait + " for " if trait else ""}{ty}',))
                result['items'].append(row.copy())
            if rt: row['runtime_block'] = True; result.setdefault('blocks', []).append(row.copy())
            own = enclosing
            if n.type in ('function_item','const_item','static_item'):
                own = len(result['defs'])
                row.update({'id':own, 'context':list(ctx), 'name':name, 'body':text(n), 'strings':strings(n), 'refs': sorted({text(x) for x in walk(n) if x.type in ('identifier','field_identifier')}), 'includes':[]})
                result['defs'].append(row)
                if any(re.search(r'#\[test\]', a) for a in attrs): result['tests'].append(own)
                if runtime and n.type == 'function_item':
                    result['methods'].append(dict(row, lines=row['end']-row['start']+1))
            if n.type == 'macro_invocation' and text(n.child_by_field_name('macro')) in ('include_str','include_bytes','include','file','line','module_path'):
                rec = dict(row, macro=text(n.child_by_field_name('macro')), text=text(n), owner=own)
                binding=n.parent
                while binding and binding.type not in ('let_declaration','const_item','static_item','function_item','source_file'):
                    binding=binding.parent
                rec['binding_kind']=binding.type if binding else ''
                rec['binding_name']=text(binding.child_by_field_name('name') or binding.child_by_field_name('pattern')) if binding else ''
                result['includes'].append(rec)
                if own is not None: result['defs'][own]['includes'].append(rec)
            # Rust macro arguments are opaque token trees. An include inside
            # assert!/vec!/another macro is not a macro_invocation AST node.
            if n.type=='identifier' and text(n) in ('include_str','include_bytes','include','file','line','module_path') and n.parent and n.parent.type=='token_tree':
                siblings=n.parent.named_children
                pos=next(i for i,c in enumerate(siblings) if c.start_byte==n.start_byte)
                tail=siblings[pos+1] if pos+1<len(siblings) else None
                if tail and tail.type=='token_tree' and b[n.end_byte:tail.start_byte].strip()==b'!':
                    rec=dict(row,macro=text(n),text=b[n.start_byte:tail.end_byte].decode(),owner=own,binding_kind='macro-token',binding_name='',nested=True)
                    result['includes'].append(rec)
                    if own is not None: result['defs'][own]['includes'].append(rec)
            if n.type in ('attribute_item','inner_attribute_item'):
                if 'path' in text(n) or 'cfg' in text(n): result['attributes'].append(dict(row,text=text(n)))
                if 'cfg' in text(n) and re.search(r'\b(?:windows|unix|target_os|target_family|target_env|target_arch)\b',text(n)):
                    result['platform'].append(dict(row,text=text(n),runtime=runtime))
            if n.type=='macro_invocation' and text(n.child_by_field_name('macro'))=='cfg' and re.search(r'\b(?:windows|unix|target_os|target_family|target_env|target_arch)\b',text(n)):
                result['platform'].append(dict(row,text=text(n),runtime=runtime))
            if n.type == 'call_expression':
                f = n.child_by_field_name('function')
                if f and f.type=='generic_function': f=f.child_by_field_name('function')
                if f and f.type == 'field_expression':
                    result['calls'].append(dict(row, name=text(f.child_by_field_name('field')), expression=text(f), owner=own, runtime=runtime, kind='method'))
                elif f and f.type in ('scoped_identifier','generic_function'):
                    result['calls'].append(dict(row, name=text(f).split('::')[-1], expression=text(f), owner=own, runtime=runtime, kind='associated'))
            if n.type == 'scoped_identifier' and n.parent and n.parent.type != 'call_expression' and (text(n).startswith('Runtime::') or runtime and text(n).startswith('Self::')):
                result['calls'].append(dict(row,name=text(n).split('::')[-1],expression=text(n),owner=own,runtime=runtime,kind='associated'))
            if n.type == 'token_tree' and n.parent and n.parent.type == 'macro_invocation':
                clean=bytearray(n.text)
                for leaf in walk(n):
                    if leaf.type in ('string_literal','raw_string_literal','char_literal','line_comment','block_comment'):
                        lo=leaf.start_byte-n.start_byte; hi=leaf.end_byte-n.start_byte
                        clean[lo:hi]=b' '*(hi-lo)
                for hit in re.finditer(rb'\b([A-Za-z_]\w*)\s*(\.|::)\s*([A-Za-z_]\w*)\s*\(',bytes(clean)):
                    result['calls'].append(dict(row,name=hit[3].decode(),expression=hit[1].decode()+hit[2].decode()+hit[3].decode(),owner=own,runtime=runtime,kind='associated' if hit[2]==b'::' else 'method',macro=True))
            childctx = ctx
            if n.type == 'mod_item': childctx = ctx + (name,)
            elif isimpl: childctx = ctx + (f'impl {trait + " for " if trait else ""}{ty}',)
            elif n.type == 'function_item': childctx = ctx + (name,)
            pending=[]
            for c in n.named_children:
                if c.type == 'attribute_item':
                    visit(c, childctx, (), own, runtime or rt); pending.append(text(c)); continue
                visit(c, childctx, tuple(pending), own, runtime or rt)
                if c.type not in ('line_comment','block_comment'): pending=[]
        visit(tree.root_node, initial)
    return result

THEMES = [
('settings',r'settings|advanced_row|editor_choice|scheme|colour|palette_colour'),
('profiles',r'profile|root_menu'),
('first_run',r'first_run|psreadline|invite|powershell_integration|context_menu_install|explorer_package|claude_hook|codex_notify|copilot'),
('git',r'git|graph|checkout|repo|commit_message|branch'),
('math',r'math|formula|equation|tex'),
('preview',r'preview|markdown|document|pdf|image|picture|video|animation|hex|table_block|linebreak|wrap'),
('files',r'files|file_row|file_index|folder|directory|dir_news|locate|recent_folder'),
('peek',r'peek'),('focus',r'focus|card|thumbnail|thumb'),('web',r'web|favicon|browser|address_bar'),
('tabs',r'tab|rename|blank_page|strip'),('panes',r'pane|split|divider|seat|leaf|rail|layout|dock|geometry|reflow|refit'),
('quake',r'quake|summon'),('search',r'search'),('palette',r'palette'),
('attention',r'attention|notif|notice|agent|taskbar|toast'),
('windows',r'window|session|restore|quit|reopen|monitor|work_area|dirty_gate|handover|activate_window'),
('dpi',r'dpi|resize|rescale|scale|size|lawful|settle_size'),
('keyboard',r'keyboard|key_|_key|keys|ime|preedit|chord|shortcut|modifier|text_input|caret|blink'),
('mouse',r'mouse|pointer|wheel|click|hover|press|drag|drop|tear|foreign|cursor'),
('clipboard',r'clipboard|paste|copy|cut_'),('floats',r'float|popup|popover|menu|overlay|chevron|context'),
('tooltips',r'tooltip|hint'),('frame',r'publish|redraw|present|chrome|draw|paint|compose|frame|^turn$|dump|refresh_overlay|ink$'),
('terminal',r'pty|terminal|term_|shell|selection|hyperlink|mark|cmdrail|command_rail|scroll|screen|host'),
('diagnostics',r'trace|diagnostic|hang|perf|log_'),('launch',r'launch|cli|seed|arrival|startup|create'),
('i18n',r'i18n|lang|translat|localis|localiz')]

def manifest(a):
    methods = {m['name']:m for m in a['methods']}
    byid = {m['id']:m for m in a['methods']}
    for m in methods.values():
        theme = next((theme for theme,pattern in THEMES if re.search(pattern,m['name'])),None)
        m['destination'] = f'runtime/{theme}.rs' if theme else 'unassigned'
        m['callers'] = []
    for c in a['calls']:
        if c['name'] not in methods: continue
        if c['kind'] == 'associated' and not (c['expression'].startswith('Runtime::') or c['expression'].startswith("Runtime::") or c['runtime'] and c['expression'].startswith('Self::')): continue
        caller = byid.get(c['owner'])
        owner=a['defs'][c['owner']] if c['owner'] is not None else None
        if owner and re.fullmatch(r'self\.\w+',c['expression']):
            impls=[s for s in owner['context'] if s.startswith('impl ')]
            if impls and 'Runtime' not in impls[-1]: continue
        destination = caller['destination'] if caller else c['file']
        # Self dispatch within a Runtime method is an exact syntactic witness.
        certainty = 'direct-self' if caller and (re.fullmatch(r'self\.\w+',c['expression']) or c['expression'].startswith('Self::')) else 'receiver-unresolved'
        methods[c['name']]['callers'].append({'file':c['file'],'line':c['start'],'symbol':a['defs'][c['owner']]['symbol'] if c['owner'] is not None else c['symbol'],'destination':destination,'expression':c['expression'],'evidence':certainty})
    rows=[]
    for m in methods.values():
        external = [c for c in m['callers'] if not c['destination'].startswith('runtime/') and c['destination']!='unassigned']
        sibling = [c for c in m['callers'] if c['destination'] != m['destination'] or c['destination']=='unassigned' and c['symbol']!=m['symbol']]
        visibility = 'pub(crate)' if external else 'pub(in crate::runtime)' if sibling else 'private'
        rows.append({'commit':ARGS.commit,'name':m['name'],'start':m['start'],'end':m['end'],'lines':m['lines'],'destination':m['destination'],'visibility':visibility,'evidence':'inferred; minimum for listed candidate callers','unresolved_callers':sum(c['evidence']=='receiver-unresolved' for c in m['callers']),'callers':m['callers']})
    return rows

def census(a, base, moves):
    defs=a['defs']; names=defaultdict(list)
    for d in defs: names[(d['file'],d['name'])].append(d['id'])
    def resolve(d, ref):
        context=d['context']+([d['name']] if d['kind']=='function_item' else [])
        candidates=[defs[i] for i in names[(d['file'],ref)] if defs[i]['context'] == context[:len(defs[i]['context'])]]
        return max(candidates,key=lambda x:len(x['context']))['id'] if candidates else None
    deps={d['id']:{i for r in d['refs'] if (i:=resolve(d,r)) is not None and i!=d['id']} for d in defs}
    old={(base['defs'][i]['file'],base['defs'][i]['symbol']):base['defs'][i] for i in base['tests']}
    rows=[]
    for idx in a['tests']:
        d=defs[idx]; seen=set(); todo=[idx]
        while todo:
            i=todo.pop()
            if i in seen: continue
            seen.add(i); todo.extend(deps[i]-seen)
        closure=[defs[i] for i in sorted(seen)]
        includes=[r for x in closure for r in x['includes']]
        bodies='\n'.join(x['body'] for x in closure)
        source_includes=[r for r in includes if re.search(r'\.(?:rs|ps1|bash|zsh|toml|json|md|html)\b',r['text'])]
        pathpin=bool(re.search(r'--exact|file!\(|line!\(|\.rs(?::\d|\\"|\")',d['body']))
        refs_in_closure={r for x in closure for r in x['refs']}
        dynamic_input=bool(refs_in_closure & {'read_to_string','read_dir'} and re.search(r'CARGO_MANIFEST_DIR|source file',bodies))
        dynamic=bool(dynamic_input and re.search(r'"src"|"src/[^\"]+\.rs"|source file',bodies))
        if not (source_includes or pathpin or dynamic_input): continue
        literals=[decoded(s) for s in d['strings']]
        # Signature arrays/constants read by a test are subjects too.
        literals.extend(decoded(s) for x in closure if x['kind'] in ('const_item','static_item') for s in x['strings'])
        # Pins deliberately split signatures so their own test literals cannot
        # satisfy the lookup. Fold literal arrays, without executing Rust.
        for match in re.finditer(r'\[((?:\s*"(?:\\.|[^"\\])*"\s*,?)+)\s*\]\.concat\(\)',d['body']):
            literals.append(''.join(decoded(s) for s in re.findall(r'"(?:\\.|[^"\\])*"',match[1])))
        fnnames={x['name'] for x in defs if x['file']=='src/main.rs' and x['kind']=='function_item'}
        subjects=sorted(set(re.findall(r'\bfn\s+(\w+)', '\n'.join(literals))) | {s for s in literals if s in fnnames})
        # Read-audited tests select an arm, field block, or exact source list
        # rather than spelling a function header. Keep these explicit.
        overrides={
          'layer_shape_tests::the_window_layer_owns_nothing_the_application_owns':(['WindowRuntime'],'retained subject: no 2a move',None),
          'layer_shape_tests::the_device_is_the_applications_and_the_surface_is_the_windows':(['App','WindowRuntime'],'retained subject: no 2a move',None),
          'layer_shape_tests::the_session_file_is_written_by_the_application_and_never_by_a_window':(['App','WindowRuntime'],'retained subject: no 2a move',None),
          'layer_shape_tests::the_application_layer_owns_nothing_one_window_owns':(['App'],'retained subject: no 2a move',None),
          'tab_identity_tests::no_window_keeps_a_tab_counter_of_its_own':(['App','WindowRuntime','NewWindowParts'],'retained subject: no 2a move',None),
          'pointer_chord_site_tests::no_other_door_reads_the_raw_control_key_for_a_gesture':(['preview_browse_key'],'source-list loses moved occurrence',['arity / uniqueness']),
          'floated_page_tests::the_gear_on_the_summoned_terminal_opens_the_page_about_it':(['chrome_mouse_input'], 'arm moves: retarget atomically',None),
          'tests::a_remembered_line_comes_only_from_a_mark_the_reader_was_at':(['TabState::term_leaf'], 'retained subject: no 2a move',None),
          'tests::the_speaker_is_a_second_channel_and_takes_you_to_the_sound':(['chrome_mouse_input'], 'arm moves: retarget atomically',['named body / positive requirement','scoped negative']),
          'field_command_tests::the_default_menu_does_not_own_command_q':(['main'],'retained subject: no 2a move',['named body / positive requirement']),
          'tests::hostile_math_is_refused_and_the_real_decoration_worker_survives':([], 'test identity retained in 2a',['path / selector (§6.2(c))']),
        }
        override=overrides.get(d['symbol']) if d['file']=='src/main.rs' else None
        if override: subjects=sorted(set(subjects+override[0]))
        # Include literal signatures passed through a local helper by this test.
        srcs=sorted(set(r['text'] for r in source_includes))
        reads_main=any('"main.rs"' in s for s in srcs)
        witnesses={}
        for s in subjects:
            candidates=[x for x in defs if x['file']=='src/main.rs' and x['kind']=='function_item' and x['name']==s]
            explicit=[v.lstrip() for v in literals if re.match(r'(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+'+re.escape(s)+r'\b',v.lstrip())]
            if explicit:
                matched=[x for x in candidates if any(x['body'].startswith(v) for v in explicit)]
                if matched: candidates=matched
            # The current helpers use first textual find, not type resolution.
            # Retain all candidates to expose duplicate-subject evidence needs.
            witnesses[s]=[{'symbol':x['symbol'],'start':x['start'],'end':x['end'],'moves':x['id'] in {m['id'] for m in a['methods']}} for x in candidates]
        moved=[s for s in subjects if s in moves and (not witnesses[s] or min(witnesses[s],key=lambda x:x['start'])['moves'])]
        whole_neg=bool(re.search(r'!\s*(?:SOURCE|MAIN|before_this_fixture)\s*\.\s*contains',d['body']) or re.search(r'!\s*source\s*\.\s*contains',d['body']) and re.search(r'let source[^=]*=\s*include_str!',d['body']))
        neg=bool(re.search(r'!\s*\w+\s*\.\s*contains',d['body']))
        arity=bool(re.search(r'\.matches\([\s\S]*?\.count\(\)',d['body']))
        whole_arity=bool(re.search(r'\b(?:SOURCE|MAIN|before_this_fixture)\s*\.\s*matches\(',d['body']))
        classes=[]
        if subjects or re.search(r'\.(?:contains|find|split|starts_with)\(',d['body']): classes.append('named body / positive requirement')
        if whole_neg or dynamic: classes.append('whole-source prohibition')
        elif neg: classes.append('scoped negative')
        if arity: classes.append('arity / uniqueness')
        if pathpin and not source_includes: classes.append('path / selector (§6.2(c))')
        if not classes: classes=['source fixture / path (§6.2(c))']
        impact = 'subject moves: retarget atomically' if reads_main and moved else 'coverage changes: preserve union / audit needles' if reads_main and (whole_neg or arity or dynamic) else 'read-main: inspect subject/needle' if reads_main else 'no 2a subject move identified'
        if reads_main and subjects and not moved and not (whole_neg or whole_arity or dynamic): impact='retained subject: no 2a move'
        if dynamic: impact='enumeration: audit recursion and original scope' if 'read_dir' in refs_in_closure else 'retained source file read dynamically: no 2a move'
        if override:
            impact=override[1]
            if override[2]: classes=override[2]
        if d['symbol']=='diagnostics::bt_environment_doc_tests::every_bt_name_in_the_source_is_in_the_document_and_the_reverse':
            classes=['whole-source prohibition','named body / positive requirement']; impact='recursive enumeration: coverage retained in 2a'
        if d['symbol']=='tests::the_shell_page_is_gone' and d['file']=='src/main.rs':
            impact='silent coverage loss: nonrecursive source walk'; classes=['whole-source prohibition','path / selector (§6.2(c))']
        if d['symbol']=='platform_gate_tests::only_the_named_files_decide_what_platform_this_is':
            impact='recursive enumeration: coverage retained in 2a'; classes=['whole-source prohibition','arity / uniqueness']
        if not source_includes and dynamic_input and not dynamic and not pathpin:
            classes=['source fixture / path (§6.2(c))']; impact='fixture/manifest input retained in 2a'
        before=old.get((d['file'],d['symbol']))
        rows.append({'commit':ARGS.commit,'file':d['file'],'test':d['symbol'],'start':d['start'],'end':d['end'],'new':'NEW' if before is None else 'CHANGED' if before['body']!=d['body'] else 'existing','classes':classes,'sources':srcs,'subjects':subjects,'subject_witnesses':witnesses,'destinations':{s:moves[s]['destination'] for s in moved},'impact':impact,'evidence':'read override + parsed dependencies' if override else 'parsed dependency closure; inferred classification; see test body','bindings':[f"{x['file']}:{x['start']} {x['symbol']}" for x in closure if x['includes']],'needles':d['strings'],'_body':d['body']})
    return rows

def reference_maps(oldfiles,newfiles,new):
    """The inputs are revision-5 locators, not hand-written new line numbers.

    Section 5 was refreshed at ee771130; section 6 retained the earlier
    main.rs coordinates, before the 22-line insertion recorded in section 5.
    Each row retains actual source text so this normalization is reviewable.
    Both the locator table and the +22 normalization belong to that one base,
    so this map is only asked for with `--references` and `--base ee771130`.
    """
    refs={
      '5': {'i18n.rs':[249,253,259,2678,2679,2689,2693,2696,3248,3462,3469,6359,6378,6379,6380,6391,6551,6553,6554,6579,6695,8848,8849], 'settings.rs':[4083], 'profiles.rs':[22990,23009,23013], 'preview.rs':[4238,4320], 'main.rs':[22723,36750,51453,69720], 'file_peek.rs':[2852], 'marks.rs':[72]},
      '6': {'main.rs':[42046,39271,36598,61746,62188,100525,16477,112433,112442,114252,114261,114272,114293,114545,114547,116199,116223,163322,163122,163012,163103,163045,163086,161410,125096,157543,15772,105705,113789,113779,103570,103555,103557,14201,14217,103965,104432,104975,105163,105784,131933,151992,152393,165696,165697,116322,116328]}}
    mappings={}; rows=[]
    for section,byfile in refs.items():
        for file,lines in byfile.items():
            p='src/'+file; old=oldfiles[p].decode().splitlines(); current=newfiles[p].decode().splitlines()
            if p not in mappings:
                # Git's diff algorithm makes the large-file alignment cheap.
                diff=git('diff','--no-ext-diff','--unified=0',ARGS.base,ARGS.commit,'--',PREFIX+p).decode()
                line_map={}; a=b=1
                for hit in re.finditer(r'^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@',diff,re.M):
                    x,n,y,m=int(hit[1]),int(hit[2] or 1),int(hit[3]),int(hit[4] or 1)
                    if n==0: x+=1
                    if m==0: y+=1
                    for i in range(a,x): line_map[i]=b+i-a
                    for tag,aa,az,bb,bz in SequenceMatcher(None,old[x-1:x-1+n],current[y-1:y-1+m],autojunk=False).get_opcodes():
                        if tag=='equal':
                            for i in range(aa,az): line_map[x+i]=y+bb+i-aa
                    a=x+n; b=y+m
                for i in range(a,len(old)+1): line_map[i]=b+i-a
                mappings[p]=line_map
            for line in lines:
                actual=line+(22 if section=='6' and line>=69720 else 0)
                now=mappings[p].get(actual)
                symbol=min((x for x in new['items'] if x['file']==p and now and x['start']<=now<=x['end']),key=lambda x:x['end']-x['start'],default={}).get('symbol','')
                rows.append({'section':section,'file':p,'was':line,'actual_baseline_line':actual,'is':now or 'changed/deleted','symbol':symbol,'baseline_text':old[actual-1].strip(),'current_text':current[now-1].strip() if now else 'inspect changed item','evidence':'parsed line correspondence; read normalization of mixed-base plan'})
    return rows

def main():
    global ARGS
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--base',default='ee771130'); ap.add_argument('--commit',default='dc99be53')
    # The run's own date names its artefacts; pass it so a re-run reproduces them.
    ap.add_argument('--date',default=date.today().isoformat())
    # The revision-5 locator map is tied to one base; ask for it explicitly.
    ap.add_argument('--references',action='store_true')
    ap.add_argument('--report',default=None,help='the document carrying the GENERATED 2A SUMMARY block')
    ARGS=ap.parse_args()
    DATE=ARGS.date
    oldfiles=snapshot(ARGS.base); newfiles=snapshot(ARGS.commit)
    old=analyse(oldfiles); new=analyse(newfiles)
    assert not old['errors'] and not new['errors'], (old['errors'],new['errors'])
    for inventory in (old,new):
        for site in inventory['platform']:
            if site['kind'] not in ('attribute_item','inner_attribute_item'): continue
            subjects=[x for x in inventory['items'] if x['file']==site['file'] and x['start']>site['start'] and site['text'] in x['attributes']]
            if subjects: site['symbol']=min(subjects,key=lambda x:x['start'])['symbol']
    rows=manifest(new); moves={m['name']:m for m in rows}
    # The same sort run over the baseline's blocks, so the inventory's delta is
    # generated rather than eyeballed. A rename shows as one gone and one added.
    oldrows={r['name']:r for r in manifest(old)}
    delta=[]
    for name in sorted(set(oldrows)|set(moves)):
        was=oldrows.get(name); now=moves.get(name)
        delta.append({'name':name,'status':'retained' if was and now else 'added' if now else 'gone',
                      'was_destination':was['destination'] if was else '','is_destination':now['destination'] if now else '',
                      'was_lines':was['lines'] if was else '','is_lines':now['lines'] if now else '',
                      'moved_topic':bool(was and now and was['destination']!=now['destination'])})
    references=reference_maps(oldfiles,newfiles,new) if ARGS.references else []
    if references:
        tsv(OUT/f'{STEM}references-{DATE}.tsv',references,['section','file','was','actual_baseline_line','is','symbol','baseline_text','current_text','evidence'])
    tsv(OUT/f'{STEM}2a-manifest-{DATE}.tsv', rows, ['commit','name','start','end','lines','destination','visibility','evidence','unresolved_callers','callers'])
    tsv(OUT/f'{STEM}method-delta-{DATE}.tsv', delta, ['name','status','was_destination','is_destination','was_lines','is_lines','moved_topic'])
    pins=census(new,old,moves)
    tsv(OUT/f'{STEM}pins-{DATE}.tsv',pins,['commit','file','test','start','end','new','classes','sources','subjects','subject_witnesses','destinations','impact','evidence','bindings','needles'])
    Path('target').mkdir(exist_ok=True)
    Path('target/bt-app-pin-bodies.txt').write_text('\n\n'.join(f"{r['file']}:{r['start']} {r['test']} [{r['impact']}]\n{r['_body']}" for r in pins),encoding='utf-8',newline='\n')
    def key(x): return x['file'],x['kind'],x['symbol']
    olditems={key(x) for x in old['items']}
    added=[x for x in new['items'] if key(x) not in olditems]
    tsv(OUT/f'{STEM}changes-{DATE}.tsv',[x for x in added if x['file'].startswith('src/') and x['kind'] in ('mod_item','impl_item')],['file','symbol','kind','start','end','attributes'])
    counts={}
    for label,a,files in [('baseline',old,oldfiles),('candidate',new,newfiles)]:
        blocks=a['blocks']; total=len(files['src/main.rs'].splitlines())
        # Anchor each block by the name of its first and last method: a line
        # number stops being an anchor the moment anything above it moves.
        for b in blocks:
            inside=sorted((m for m in a['methods'] if b['start']<=m['start']<=b['end']),key=lambda m:m['start'])
            b.update(methods=len(inside),first_method=inside[0]['name'],last_method=inside[-1]['name'])
        inc=[r for r in a['includes'] if r['file']=='src/main.rs' and r['text']=='include_str!("main.rs")']
        function_paths={tuple(d['context']+[d['name']]) for d in a['defs'] if d['kind']=='function_item' and d['file']=='src/main.rs'}
        def inscope(i):
            d=a['defs'][i['owner']]; ctx=d['context']
            return 'function' if d['kind']=='function_item' or any(tuple(ctx[:n]) in function_paths for n in range(1,len(ctx)+1)) else 'module'
        counts[label]={'main_lines':total,'blocks':blocks,'block_lines':sum(b['end']-b['start']+1 for b in blocks),'methods':len(a['methods']),'method_lines':sum(m['lines'] for m in a['methods']),'residue':total-sum(b['end']-b['start']+1 for b in blocks),'main_self_includes':len(inc),'main_include_bindings':dict(Counter(i['binding_kind']+':'+i['binding_name'] for i in inc)),'main_include_scopes':dict(Counter(inscope(i) for i in inc)),'test_attributes':len(a['tests']),'main_platform': [r for r in a['platform'] if r['file']=='src/main.rs'],'main_self_include_sites':inc}
    theme=defaultdict(lambda:{'methods':0,'lines':0})
    for r in rows: theme[r['destination']]['methods']+=1; theme[r['destination']]['lines']+=r['lines']
    oldinc={(r['file'], old['defs'][r['owner']]['symbol'] if r['owner'] is not None else r['symbol'],r['text']) for r in old['includes']}
    newinc=[r for r in new['includes'] if (r['file'], new['defs'][r['owner']]['symbol'] if r['owner'] is not None else r['symbol'],r['text']) not in oldinc]
    moved_inputs=[r for r in new['includes'] if r['file']=='src/main.rs' and any(b['start']<=r['start']<=b['end'] for b in new['blocks'])]
    mainlines=newfiles['src/main.rs'].decode().splitlines()
    platform_lexical=[{'line':i,'text':line.strip()} for i,line in enumerate(mainlines,1) if (code:=line.split('//')[0]) and any(s in code for s in ('cfg(','cfg!(','cfg_attr(')) and any(s in code for s in ('windows','unix','target_os','target_family','target_env','target_arch'))]
    graph_path=Path('target/bt-app-graph.json')
    graph=json.loads(graph_path.read_text(encoding='utf-8'))
    # The retained script reads the working tree, this audit reads Git blobs.
    # Refuse an accidentally stale graph, including same-length source edits.
    assert all(Path(PREFIX+p).read_bytes().replace(b'\r\n',b'\n')==b.replace(b'\r\n',b'\n') for p,b in newfiles.items() if p.startswith('src/'))
    assert graph['stats']['main.rs']['lines']==counts['candidate']['main_lines']
    assert len(graph['methods'])==len(rows)
    assert {(m['name'],m['line'],m['lines']) for m in graph['methods']}=={(m['name'],m['start'],m['lines']) for m in rows}
    cited_external={}
    for path,patterns in {
      'scripts/check-portable-core.ps1':['$list = Read-RustStringArray $pinText','if ($allowed.Count -lt','foreach ($file in Get-ChildItem -Path $appSource'],
      '.github/workflows/ci.yml':['cargo check --locked --all-targets -p bt-app','bt-app` stays off','cargo check --locked --all-targets'],
      'scripts/ci/ignored-tests.txt':['direct_crt_receives_the_exact_literal_after_the_program_token','five in `bt-app`']}.items():
        lines=git('show',f'{ARGS.commit}:{path}').decode().splitlines()
        cited_external[path]=[{'line':i,'text':s.strip()} for i,s in enumerate(lines,1) if any(p in s for p in patterns)]
    main_commits=git('rev-list',f'{ARGS.base}..{ARGS.commit}','--','crates/bt-app/src/main.rs').decode().splitlines()
    summary_extra={'cited_external':cited_external,'main_touch_commits':len(main_commits),'root_prod_ratio':graph['cuts']['root_prod']['i18n']['free_lines']/graph['cuts']['root_prod']['baseline']['free_lines'], 'source_directories':sorted({str(Path(p).parent).replace('\\','/') for p in newfiles if p.startswith('src/')}),'whole_source_count_sites':[],'method_delta':delta,'method_delta_counts':dict(Counter(d['status'] for d in delta),moved_topic=sum(d['moved_topic'] for d in delta))}
    # AST-selected function bodies, then the guard's own lexical counter form.
    for d in new['defs']:
        if d['file']!='src/main.rs' or d['kind']!='function_item': continue
        for hit in re.finditer(r'\b(SOURCE|MAIN)\s*\.\s*matches\([^;]*?\.count\(\)',d['body']):
            summary_extra['whole_source_count_sites'].append({'test':d['symbol'],'line':d['start']+d['body'][:hit.start()].count('\n'),'expression':hit[0]})
    summary={'source_commit':git('rev-parse',ARGS.commit).decode().strip(),'base_commit':git('rev-parse',ARGS.base).decode().strip(),'counts':counts,'themes':dict(sorted(theme.items())),'visibility':dict(Counter(r['visibility'] for r in rows)),'new_files':sorted(set(newfiles)-set(oldfiles)),'added_items':added,'new_platform_attributes':[r for r in new['platform'] if not any(r['file']==o['file'] and r['text']==o['text'] and r['symbol']==o['symbol'] for o in old['platform'])],'pin_rows':len(pins),'pin_new_status':dict(Counter(p['new'] for p in pins)),'pin_impacts':dict(Counter(p['impact'] for p in pins)),'includes':new['includes'],'new_includes':newinc,'moved_inputs':moved_inputs,'platform_lexical':platform_lexical,'attributes':new['attributes'],'retained_root_prod':{k:graph['cuts']['root_prod'][k] for k in ('baseline','i18n')},'retained_stats':graph['stats'],'references':references}
    summary.update(summary_extra)
    write_json(OUT/f'{STEM}freshness-data-{DATE}.json',summary)
    report=Path(ARGS.report) if ARGS.report else OUT/'review'/f'{STEM}freshness-{DATE}.md'
    if report.exists():
        contents=report.read_text(encoding='utf-8')
        generated=['| Proposed destination | Methods | Method extent lines |','| --- | ---: | ---: |']
        generated += [f"| `{name}` | {value['methods']:,} | {value['lines']:,} |" for name,value in sorted(theme.items())]
        generated += [f"| **Total** | **{len(rows):,}** | **{sum(r['lines'] for r in rows):,}** |",'',
          'Visibility proposals: '+', '.join(f"**{count:,}** `{name}`" for name,count in sorted(summary['visibility'].items()))+'.',
          f"Thus **{sum(v for k,v in summary['visibility'].items() if k!='private'):,}** methods have proposed widening; **{theme['unassigned']['methods']}** destinations remain unassigned.", '',
          f"Census output: **{len(pins)}** consumer rows; **{sum(p['new']=='NEW' for p in pins)}** new test identities and **{sum(p['new']=='CHANGED' for p in pins)}** changed bodies. These include non-guard fixtures and are not a structural-pin total."]
        blocks=['| Block | First method | Last method | Methods | Block lines |','| --- | --- | --- | ---: | ---: |']
        for label,c in counts.items():
            for i,b in enumerate(c['blocks'],1):
                blocks.append(f"| {label} block {i} | `{b['first_method']}` | `{b['last_method']}` | {b['methods']:,} | {b['end']-b['start']+1:,} |")
        blocks += ['',
          f"`main.rs` is **{counts['candidate']['main_lines']:,}** lines; the two blocks are **{counts['candidate']['block_lines']:,}** of them "
          f"(**{counts['candidate']['methods']:,}** methods, **{counts['candidate']['method_lines']:,}** lines of method extent), "
          f"leaving a strict residue of **{counts['candidate']['residue']:,}** before any scaffolding.",
          f"Baseline: **{counts['baseline']['main_lines']:,}** / **{counts['baseline']['block_lines']:,}** / "
          f"**{counts['baseline']['methods']:,}** methods / residue **{counts['baseline']['residue']:,}**."]
        unplaced=sorted(r['name'] for r in rows if r['destination']=='unassigned')
        unassigned=[f"**{len(unplaced)}** of the **{len(rows):,}** methods match no topic expression. In name order:",'']
        unassigned += ['- '+', '.join(f'`{n}`' for n in unplaced[i:i+6]) for i in range(0,len(unplaced),6)]
        classes=Counter(c for p in pins for c in p['classes'])
        readers=['| Reader class (a row may carry several) | Rows |','| --- | ---: |']
        readers += [f"| {name} | {count:,} |" for name,count in sorted(classes.items(),key=lambda kv:-kv[1])]
        readers += ['','| What Step 2a does to the row | Rows |','| --- | ---: |']
        readers += [f"| {name} | {count:,} |" for name,count in sorted(summary['pin_impacts'].items(),key=lambda kv:-kv[1])]
        readers += ['','| Against the baseline | Rows |','| --- | ---: |']
        readers += [f"| {name} | {count:,} |" for name,count in sorted(summary['pin_new_status'].items(),key=lambda kv:-kv[1])]
        readers += ['',f"**{len(pins):,}** rows in all, over **{len({p['file'] for p in pins})}** files.",
          f"`include_str!(\"main.rs\")` invocations in `main.rs`: **{counts['candidate']['main_self_includes']}** "
          f"(baseline **{counts['baseline']['main_self_includes']}**). Parsed `#[test]` attributes under `crates/bt-app`: "
          f"**{counts['candidate']['test_attributes']:,}** (baseline **{counts['baseline']['test_attributes']:,}**)."]
        status=Counter(d['status'] for d in delta); topic=[d for d in delta if d['moved_topic']]
        gone=sorted(d['name'] for d in delta if d['status']=='gone'); new_names=sorted(d['name'] for d in delta if d['status']=='added')
        deltalines=[f"**{status['added']}** method names added, **{status['gone']}** gone, **{status['retained']:,}** retained; "
          f"**{len(topic)}** retained names land in a different topic than they did at the baseline. "
          f"A rename appears here as one gone and one added, which is why the two are listed rather than netted.",'',
          '| Gone since the baseline | Was |','| --- | --- |']
        deltalines += [f"| `{n}` | `{oldrows[n]['destination']}` |" for n in gone]
        deltalines += ['','| Added since the baseline | Is |','| --- | --- |']
        deltalines += [f"| `{n}` | `{moves[n]['destination']}` |" for n in new_names]
        if topic:
            deltalines += ['','| Retained, different topic | Was | Is |','| --- | --- | --- |']
            deltalines += [f"| `{d['name']}` | `{d['was_destination']}` | `{d['is_destination']}` |" for d in sorted(topic,key=lambda d:d['name'])]
        for marker,body in [('2A SUMMARY',generated),('BLOCK EXTENT',blocks),('UNASSIGNED',unassigned),('READERS',readers),('DELTA',deltalines)]:
            contents=re.sub(rf'<!-- GENERATED {marker} -->[\s\S]*?<!-- END GENERATED {marker} -->',
                            lambda m: f'<!-- GENERATED {marker} -->\n'+'\n'.join(body)+f'\n<!-- END GENERATED {marker} -->',contents)
        report.write_text(contents,encoding='utf-8',newline='\n')
    # Complete item index supports refreshed plan references without grep counts.
    tsv(Path('target/bt-app-item-index.tsv'),new['items'],['file','symbol','kind','start','end','attributes'])
    print(json.dumps({k:v for k,v in summary.items() if k in ('source_commit','themes','visibility','new_files','pin_rows','pin_new_status','pin_impacts','method_delta_counts')},indent=2))
    for label,c in counts.items(): print(label, {k:v for k,v in c.items() if k in ('main_lines','block_lines','methods','method_lines','residue','main_self_includes','test_attributes')}, [(b['start'],b['end'],b['first_method'],b['last_method'],b['methods']) for b in c['blocks']])

if __name__=='__main__': main()
