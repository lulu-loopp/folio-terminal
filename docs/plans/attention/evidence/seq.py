import sys, re
d = open(sys.argv[1], 'rb').read()
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
pat = re.compile(rb'\x1b\](.*?)(\x07|\x1b\\)', re.S)
for m in pat.finditer(d):
    print(f'{m.start():>7}  OSC {m.group(1)[:120]!r}  term={"BEL" if m.group(2)==b"\\x07" else "ST"}')
print('--- private mode sets ---')
for m in re.finditer(rb'\x1b\[\?([0-9;]+)([hl])', d):
    code = m.group(1).decode()
    if code.split(';')[0] in ('1049', '2004', '1004', '1000', '1002', '1003', '1006', '9001'):
        print(f'{m.start():>7}  ?{code}{m.group(2).decode()}')
print('--- bare BEL ---')
osc_term = set(m.end()-1 for m in pat.finditer(d) if m.group(2) == b'\x07')
for m in re.finditer(b'\x07', d):
    if m.start() not in osc_term:
        print(f'{m.start():>7}  BEL  ctx={d[max(0,m.start()-80):m.start()+10]!r}')
