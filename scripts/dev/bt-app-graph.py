# bt-app-graph.py - rebuild the bt-app module dependency graph.
#
# Retained in the tree because `docs/plans/bt-app-split.md` is ticketed off its
# numbers and a plan whose measurements cannot be re-run is a plan nobody can
# check (R-BT-APP-SPLIT finding 2). Originally written for that review as
# `target/review_graph.py`; moved here unchanged except for this header and the
# dependency lookup below.
#
# Read-only. It parses; it never builds, checks or tests.
#
#   python scripts/dev/bt-app-graph.py > target/bt-app-graph-output.txt
#
# writes `target/bt-app-graph.json`, which `scripts/dev/bt-app-split-table.py`
# turns into the row tables in the plan.
#
# Needs three packages that are NOT workspace dependencies and never become
# ones - this is a documentation tool, not part of the product:
#
#   pip install tree_sitter tree_sitter_rust networkx
#
# It looks in `target/review-python` first, so a vendored copy made for the
# review keeps working without a global install.
#
# Six graph variants are emitted and they mean different things. `regex_full`
# reproduces the first inventory's `crate::(\w+)` convention. **`root_prod` is
# the one the plan uses**: it adds a `@root` node for items owned by `main.rs`,
# so a module that names a root-owned type is correctly shown as un-extractable.
# `*_all` include test-only edges; `*_prod` exclude them.

import sys, re, json, gc
gc.disable()
from pathlib import Path
sys.path.insert(0, str(Path('target/review-python').resolve()))  # optional vendored copy
from tree_sitter import Language, Parser
import tree_sitter_rust
import networkx as nx

BASE=Path('crates/bt-app/src')
parser=Parser(Language(tree_sitter_rust.language()))
files={p.relative_to(BASE).as_posix():p.read_bytes() for p in BASE.rglob('*.rs')}
trees={p:parser.parse(b) for p,b in files.items()}
def txt(n): return n.text.decode('utf8')
def walk(n):
    yield n
    for c in n.named_children: yield from walk(c)
def uses(n,prefix=()):
    if n.type=='use_declaration': return uses(n.child_by_field_name('argument'),prefix)
    if n.type=='scoped_use_list':
        path=n.child_by_field_name('path')
        return uses(n.child_by_field_name('list'),prefix+tuple(txt(path).split('::')) if path else prefix)
    if n.type=='use_list': return [x for c in n.named_children for x in uses(c,prefix)]
    if n.type=='use_as_clause':
        p=n.child_by_field_name('path'); a=n.child_by_field_name('alias')
        return [(prefix+tuple(txt(p).split('::')),txt(a))]
    val=txt(n).replace(' ','').split('::'); path=prefix+tuple(val)
    return [(path, path[-2] if path[-1]=='self' and len(path)>1 else path[-1])]

# Source paths are physical-file nodes for the reproduction. Semantic contexts
# follow external #[path] test modules, not their incidental file names, and
# both they and the wholly-test set are DERIVED from the module declarations
# rather than listed by hand: a hand-written list goes stale the first time a
# test module moves into a file of its own, and it did - five files were named
# where twelve are wholly test (inventory 2026-09-21 §5.4).
def declarations(p):
    """(context path, #[path] value, cfg(test)) per bodyless `mod x;` in `p`."""
    out=[]
    def visit(n,ctx,test):
        pending=[]
        for c in n.named_children:
            if c.type=='attribute_item': pending.append(txt(c)); continue
            if c.type in {'line_comment','block_comment'}: continue
            ct=test or any(re.search(r'cfg\s*\(\s*test\s*\)',a) for a in pending)
            if c.type=='mod_item' and c.child_by_field_name('name'):
                name=txt(c.child_by_field_name('name')); body=c.child_by_field_name('body')
                if body is None:
                    rel=next((m[1] for a in pending if (m:=re.search(r'path\s*=\s*"([^"]+)"',a))),None)
                    out.append((ctx+(name,),rel,ct))
                else: visit(body,ctx+(name,),ct)
            pending=[]
    visit(trees[p].root_node,(),False)
    return out

# `#[path]` on a module outside an inline block is relative to the directory of
# the declaring file; a plain `mod x;` looks in that file's own module directory
# (`` for the crate root, `foo/` for `foo.rs`, `a/` for `a/mod.rs`).
contexts={'main.rs':()}; whole_test=set(); queue=[('main.rs',False)]
while queue:
    p,test=queue.pop()
    here=p.rsplit('/',1)[0]+'/' if '/' in p else ''
    moddir='' if p=='main.rs' else here if p.endswith('/mod.rs') else p[:-3]+'/'
    for ctx,rel,ct in declarations(p):
        child=next((q for q in (here+rel,) if rel and q in files),None) if rel else \
              next((q for q in (moddir+ctx[-1]+'.rs',moddir+ctx[-1]+'/mod.rs') if q in files),None)
        if child is None or child in contexts: continue
        contexts[child]=contexts[p]+ctx
        if test or ct: whole_test.add(child)
        queue.append((child,test or ct))
unreached=sorted(set(files)-set(contexts))
for p in unreached: contexts[p]=tuple(p[:-3].split('/'))
nodes={p.split('/')[0].removesuffix('.rs') for p in files if p!='main.rs'}
def physical(p): return p.split('/')[0].removesuffix('.rs')
def owner(path): return path[0] if path and path[0] in nodes else '@root'
def node_of(p): return owner(contexts[p])
# A file whose physical node is not its semantic owner is a phantom node in the
# module graph; a node all of whose files are wholly test is not production.
phantom={p for p in files if p!='main.rs' and physical(p)!=node_of(p)}
test_only={n for n in nodes if all(p in whole_test for p in files if physical(p)==n)}

records=[]; stats={}; stripped={}; prod={}; items={}; imports=[]
skip={'line_comment','block_comment','string_literal','raw_string_literal','char_literal'}
for p,b in files.items():
    clean=bytearray(b); tests=[]; rec=[]
    def visit(n,ctx,test=False):
        pending=[]
        for c in n.named_children:
            if c.type=='attribute_item': pending.append(txt(c)); continue
            ct=test or any(re.search(r'cfg\s*\(\s*test\s*\)',a) for a in pending)
            pending=[]
            if c.type in skip:
                clean[c.start_byte:c.end_byte]=bytes(10 if v==10 else 32 for v in b[c.start_byte:c.end_byte]); continue
            if ct and not test: tests.append((c.start_byte,c.end_byte,c.start_point.row+1,c.end_point.row+1))
            rec.append((c,ctx,ct))
            if c.type=='use_declaration': imports.append((p,c,ctx,ct,uses(c)))
            childctx=ctx+(txt(c.child_by_field_name('name')),) if c.type=='mod_item' and c.child_by_field_name('body') else ctx
            visit(c,childctx,ct)
    if p in whole_test: tests=[(0,len(b),1,len(b.splitlines()))]
    visit(trees[p].root_node,contexts[p],p in whole_test)
    records += [(p,*r) for r in rec]
    stripped[p]=bytes(clean)
    pc=bytearray(clean)
    for start,end,_,_ in tests: pc[start:end]=bytes(10 if v==10 else 32 for v in b[start:end])
    prod[p]=bytes(pc)
    testlines=set(i for _,_,a,z in tests for i in range(a,z+1))
    stats[p]={'lines':len(b.splitlines()),'test':len(testlines),'code':sum(bool(l.strip()) for l in clean.splitlines()),'parse_errors':trees[p].root_node.has_error}
    items[p]=rec

# Resolve root imports (including external-crate aliases) and root definitions.
aliases={}
for p,n,ctx,test,uu in imports:
    if p=='main.rs' and n.parent.type=='source_file':
        for path,alias in uu: aliases[alias]=path
rootdefs={txt(n.child_by_field_name('name')) for n,ctx,t in items['main.rs'] if not ctx and n.type in {'struct_item','enum_item','function_item','const_item','static_item','type_item','trait_item'} and n.child_by_field_name('name')}
def resolve(path,ctx):
    path=list(path)
    if not path: return None
    if path[0]=='crate': path=path[1:]
    elif path[0]=='super':
        scope=list(ctx)
        while path and path[0]=='super':
            scope=scope[:-1]; path=path[1:]
        path=scope+path
    elif path[0]=='self': path=list(ctx)+path[1:]
    else: return None
    if not path: return '@root'
    if path[0] in aliases:
        q=aliases[path[0]]
        if q[0] not in nodes and q[0] not in ('crate','super','self'): return None
        return resolve(q,()) if q[0] in ('crate','super','self') else owner(q)
    return owner(path)

graphs={k:nx.DiGraph() for k in ['regex','regex_full','syntax_prod','syntax_all','root_prod','root_all']}
for g in graphs.values(): g.add_nodes_from(nodes)
evidence={k:[] for k in graphs}
def edge(kind,a,z,p,line,source):
    if z is None or a==z: return
    if not kind.startswith('root') and z=='@root': return
    graphs[kind].add_edge(a,z); evidence[kind].append([a,z,p,line,source])
for p,b in prod.items():
    if p=='main.rs': continue
    # Exact inventory-style match, without inherited cfg on external files.
    bb=stripped[p] if p in whole_test else b
    for m in re.finditer(rb'crate::(\w+)::',bb):
        z=m[1].decode()
        if z in nodes: edge('regex',physical(p),z,p,bb[:m.start()].count(b'\n')+1,m[0].decode())
    for m in re.finditer(rb'crate::(\w+)\b',bb):
        z=m[1].decode()
        if z in nodes: edge('regex_full',physical(p),z,p,bb[:m.start()].count(b'\n')+1,m[0].decode())
for p,n,ctx,test in records:
    a='@root' if p=='main.rs' else owner(contexts[p])
    paths=[]
    if n.type=='use_declaration': paths=[path for path,alias in uses(n)]
    elif n.type in {'scoped_identifier','scoped_type_identifier'}:
        if n.parent and n.parent.type in {'scoped_identifier','scoped_type_identifier','use_declaration','scoped_use_list','use_as_clause'}: continue
        paths=[tuple(txt(n).replace(' ','').split('::'))]
    elif n.type=='token_tree' and n.parent.type!='token_tree':
        code=stripped[p][n.start_byte:n.end_byte].decode()
        paths=[tuple(re.sub(r'\s+','',m[0]).split('::')) for m in re.finditer(r'\b(?:crate|super)\s*::\s*\w+(?:\s*::\s*\w+)*',code)]
    elif n.type=='mod_item' and p=='main.rs' and n.child_by_field_name('body') is None:
        z=txt(n.child_by_field_name('name'))
        for k in ['root_all']+([] if test else ['root_prod']): edge(k,a,z,p,n.start_point.row+1,txt(n))
    for path in paths:
        z=resolve(path,ctx)
        for k in ['syntax_all','root_all']+([] if test else ['syntax_prod','root_prod']):
            if not k.startswith('root') and a=='@root': continue
            edge(k,a,z,p,n.start_point.row+1,txt(n)[:250])

weights={n:sum(s['lines']+1 for p,s in stats.items() if physical(p)==n) for n in nodes}
# Physical-file graph uses 99 nodes for comparison; semantic graph folds the two
# #[path] test files into their owners and removes phantom physical nodes.
for kind,g in graphs.items():
    if not kind.startswith('regex'):
        for n in sorted({physical(p) for p in phantom}):
            if n in g: g.remove_node(n)
    if kind.endswith('prod'):
        for n in sorted(test_only):
            if n in g: g.remove_node(n)
def report(g,kind):
    mass=weights.copy()
    mass['@root']=stats['main.rs']['lines']+1
    if not kind.startswith('regex'):
        for p in sorted(phantom): mass[node_of(p)]+=mass[physical(p)]
    scc=sorted(nx.strongly_connected_components(g),key=lambda s:(len(s),sum(mass.get(n,0) for n in s)),reverse=True)
    cycles=set().union(*(s for s in scc if len(s)>1))
    blocked=set(cycles)
    for n in cycles: blocked |= nx.ancestors(g,n)
    if '@root' in g: blocked.add('@root'); blocked |= nx.ancestors(g,'@root')
    free=set(g)-blocked
    blocked_large=set(scc[0])
    for n in scc[0]: blocked_large |= nx.ancestors(g,n)
    avoiding=set(g)-blocked_large
    return {'nodes':len(g),'edges':g.number_of_edges(),'largest_count':len(scc[0]),'largest_lines':sum(mass.get(n,0) for n in scc[0]),'largest':sorted(scc[0]),'cycles':[sorted(s) for s in scc if len(s)>1], 'free_count':len(free),'free_lines':sum(mass.get(n,0) for n in free),'free':sorted(free),'avoid_largest_count':len(avoiding),'avoid_largest_lines':sum(mass.get(n,0) for n in avoiding),'avoid_largest':sorted(avoiding)}
cuts={}
for k,g in graphs.items():
    cases=[('baseline',[],[]),('i18n',['i18n'],[]),('plus_two',['i18n'],[('quit','webhost'),('attention','attention_wire')]),('plus_preview',['i18n','preview'],[('quit','webhost'),('attention','attention_wire')]),('plus_seats',['i18n','seats'],[('quit','webhost'),('attention','attention_wire')]),('all_sinks',['i18n','seats','settings','preview','profiles'],[('quit','webhost'),('attention','attention_wire')])]
    cuts[k]={}
    for label,sinks,ee in cases:
        gg=g.copy()
        for n in sinks: gg.remove_edges_from(list(gg.out_edges(n)))
        gg.remove_edges_from(ee); cuts[k][label]=report(gg,k)
methods=[]
for n,ctx,test in items['main.rs']:
    if n.type=='impl_item' and n.child_by_field_name('trait') is None and n.child_by_field_name('type') and txt(n.child_by_field_name('type')).startswith('Runtime'):
        for m in n.child_by_field_name('body').named_children:
            if m.type=='function_item':
                code=prod['main.rs'][m.start_byte:m.end_byte].decode()
                methods.append({'name':txt(m.child_by_field_name('name')),'line':m.start_point.row+1,'lines':m.end_point.row-m.start_point.row+1,'window':sorted(set(re.findall(r'self\s*\.\s*window\s*\.\s*(\w+)',code))),'app':sorted(set(re.findall(r'self\s*\.\s*app\s*\.\s*(\w+)',code)))})
out={'stats':stats,'cuts':cuts,'edges':evidence,'methods':methods,'root_aliases':aliases,'whole_test':sorted(whole_test),'phantom_nodes':sorted({physical(p) for p in phantom}),'test_only_nodes':sorted(test_only),'contexts':{p:list(c) for p,c in sorted(contexts.items())},'unreached':unreached}
Path('target/bt-app-graph.json').write_text(json.dumps(out,indent=2),encoding='utf8')
for k,cc in cuts.items():
    print(k)
    for label,v in cc.items(): print(label, 'nodes/edges',v['nodes'],v['edges'],'SCC',v['largest_count'],v['largest_lines'],'free',v['free_count'],v['free_lines'],'avoid-largest',v['avoid_largest_count'],v['avoid_largest_lines'])
print('parse errors',[p for p,s in stats.items() if s['parse_errors']])
print('wholly test',len(whole_test),sorted(whole_test))
print('phantom nodes',sorted({physical(p) for p in phantom}),'test-only nodes',sorted(test_only),'unreached',unreached)
print('source',len(files),sum(s['lines'] for s in stats.values()),'test lines',sum(s['test'] for s in stats.values()),'code',sum(s['code'] for s in stats.values()))
print('main',stats['main.rs'],'methods',len(methods),'lines',sum(m['lines'] for m in methods),'tabs',sum('tabs' in m['window'] for m in methods))
for kind in ['regex','syntax_prod','root_prod']:
    print(kind,'i18n',sorted(graphs[kind].successors('i18n')))
    for n in ['seats','settings','preview','profiles','shortcuts','marks','icons']:
        print(kind,n,sorted(graphs[kind].successors(n)))
