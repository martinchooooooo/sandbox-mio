# Lists OSC 52 clipboard sequences in a dump. Usage: python osc52.py <dump>
import base64, sys
d = open(sys.argv[1], 'rb').read()
i = 0
while True:
    i = d.find(b'\x1b]52;', i)
    if i < 0:
        break
    j = min(x for x in (d.find(b'\x07', i), d.find(b'\x1b' + bytes([92]), i)) if x >= 0)
    body = d[i + 5:j]
    sel, _, payload = body.partition(b';')
    shown = payload if payload == b'?' else base64.b64decode(payload)
    print(f"offset={i} selection={sel!r} terminator={'BEL' if d[j] == 7 else 'ST'} payload={shown!r}")
    i = j
