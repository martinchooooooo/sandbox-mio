# Renders a byte dump captured by herdr-probe into screen text with pyte.
# Usage: PYTHONPATH=<dir holding pyte> python render.py <dump> [cols] [rows] [upto-byte-offset]
import sys, pyte
data = open(sys.argv[1], 'rb').read()
cols = int(sys.argv[2]) if len(sys.argv) > 2 else 120
rows = int(sys.argv[3]) if len(sys.argv) > 3 else 40
if len(sys.argv) > 4:
    data = data[:int(sys.argv[4])]
# pyte cannot handle terminal queries (DSR with private marker, DECRQM, kitty keyboard, DA), so strip them.
import re
data = re.sub(rb'\[\?[0-9;]*n|\[[0-9;?]*\$p|\[\?u|\[>[0-9]*u|\[c|\[[0-9]*t', b'', data)
screen = pyte.Screen(cols, rows)
stream = pyte.ByteStream(screen)
stream.feed(data)
sys.stdout.reconfigure(encoding='utf-8')
for line in screen.display:
    print(line.rstrip())
