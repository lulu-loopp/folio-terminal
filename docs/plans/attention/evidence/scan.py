import sys, re, collections

path = sys.argv[1]
d = open(path, 'rb').read()
print('file', path, 'bytes', len(d))
print('BEL 0x07 total count:', d.count(b'\x07'))

# OSC sequences: ESC ] ... (BEL | ESC \)
osc = re.findall(rb'\x1b\](.*?)(?:\x07|\x1b\\)', d, re.S)
counts = collections.Counter()
samples = {}
for body in osc:
    head = body.split(b';')[0][:8]
    counts[head] += 1
    samples.setdefault(head, []).append(body[:160])
print('--- OSC families ---')
for head, n in counts.most_common():
    print(repr(head), 'x', n)
    for s in samples[head][:4]:
        print('    ', repr(s))

# BELs not part of an OSC terminator
bel_positions = [m.start() for m in re.finditer(b'\x07', d)]
osc_term = set()
for m in re.finditer(rb'\x1b\](.*?)(\x07|\x1b\\)', d, re.S):
    if m.group(2) == b'\x07':
        osc_term.add(m.end() - 1)
bare = [p for p in bel_positions if p not in osc_term]
print('--- bare BEL count:', len(bare))
for p in bare[:20]:
    print('  at', p, 'ctx', repr(d[max(0,p-60):p+20]))

# CSI private modes of interest
for pat in [rb'\x1b\[\?1049[hl]', rb'\x1b\[\?2004[hl]', rb'\x1b\[\?1004[hl]', rb'\x1b\[\?25[hl]']:
    print(pat, len(re.findall(pat, d)))

# DCS / APC
print('APC count', len(re.findall(rb'\x1b_', d)))
print('DCS count', len(re.findall(rb'\x1bP', d)))
