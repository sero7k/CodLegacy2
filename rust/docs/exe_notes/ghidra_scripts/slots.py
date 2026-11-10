"""Map writes into the view's frame-constant block (viewInfo + 0x3d0 + 16 * slot) in decompiled text.

usage: slots.py <decompile.txt> [base=0x3d0] [var=param_1]
"""
import re, sys

path = sys.argv[1]
base = int(sys.argv[2], 16) if len(sys.argv) > 2 else 0x3d0
var = sys.argv[3] if len(sys.argv) > 3 else "param_1"
names = {}
for line in open("/tmp/claude-0/-home-claude/f009e6b0-b86e-552a-844d-1282dea90c5d/scratchpad/lit/cnames.txt"):
    k, v = line.split(None, 1)
    names[int(k)] = v.strip()
fn = None
ptr_size = {}
for line in open(path):
    m = re.match(r"// function (\S+)", line)
    if m:
        fn = m.group(1)
        ptr_size = {}
        continue
    m = re.search(r"\b(\w+) \*" + var + r"[,)]", line)
    if m and fn:
        t = m.group(1)
        ptr_size[var] = 8 if "8" in t or t in ("longlong", "ulonglong") else 4 if t in ("undefined4", "float", "int", "uint") else 1
    for m in re.finditer(r"\(" + var + r" \+ (0x[0-9a-f]+|\d+)\)", line):
        off = int(m.group(1), 0)
        if base <= off < base + 350 * 16:
            s = (off - base) // 16
            print(fn, hex(off), "slot", s, names.get(s, "?"), "+%d" % ((off - base) % 16), "|", line.strip()[:110])
    for m in re.finditer(r"\b" + var + r"\[(0x[0-9a-f]+|\d+)\]", line):
        off = int(m.group(1), 0) * ptr_size.get(var, 8)
        if base <= off < base + 350 * 16:
            s = (off - base) // 16
            print(fn, hex(off), "slot", s, names.get(s, "?"), "+%d" % ((off - base) % 16), "|", line.strip()[:110])
