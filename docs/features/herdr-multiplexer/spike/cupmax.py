# Largest row and column among CUP sequences in a byte range of a dump. Usage: python cupmax.py <dump> <from> <to>
import re, sys
d = open(sys.argv[1], 'rb').read()[int(sys.argv[2]):int(sys.argv[3])]
r = c = 0
for m in re.finditer(rb'\x1b\[(\d+);(\d+)H', d):
    r = max(r, int(m.group(1))); c = max(c, int(m.group(2)))
print(f"bytes={len(d)} max_row={r} max_col={c}")
