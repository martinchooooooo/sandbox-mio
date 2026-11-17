# Median and range of a JSONL field or of a plain number file. Usage: python summarize.py <file> [field]
import sys, json, statistics
f = sys.argv[1]
if len(sys.argv) > 2:
    v = [json.loads(l)[sys.argv[2]] for l in open(f) if l.strip()]
else:
    v = [float(l) for l in open(f) if l.strip()]
v = [x for x in v if x is not None]
print(f"{f} {sys.argv[2] if len(sys.argv) > 2 else ''}: n={len(v)} median={statistics.median(v):.0f} min={min(v):.0f} max={max(v):.0f}")
