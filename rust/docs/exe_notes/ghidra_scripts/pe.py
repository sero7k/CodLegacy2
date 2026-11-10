import struct, sys

EXE = "/mnt/user-data/attachments/f009e6b0-b86e-552a-844d-1282dea90c5d/44fad54f-1793352485678_h1_mp64_zt.exe"
data = open(EXE, "rb").read()
pe = struct.unpack_from("<I", data, 0x3c)[0]
nsec = struct.unpack_from("<H", data, pe + 6)[0]
optsz = struct.unpack_from("<H", data, pe + 20)[0]
base = struct.unpack_from("<Q", data, pe + 24 + 24)[0]
secs = []
for i in range(nsec):
    o = pe + 24 + optsz + i * 40
    name = data[o:o + 8].rstrip(b"\0").decode()
    vsz, va, rsz, raw = struct.unpack_from("<IIII", data, o + 8)
    secs.append((name, base + va, vsz, raw, rsz))


def off(v):
    for n, va, vsz, raw, rsz in secs:
        if va <= v < va + rsz:
            return raw + v - va
    return None


def va_of(o):
    for n, va, vsz, raw, rsz in secs:
        if raw <= o < raw + rsz:
            return va + o - raw
    return None


def f32(v):
    return struct.unpack_from("<f", data, off(v))[0]


def q(v):
    return struct.unpack_from("<Q", data, off(v))[0]


def cstr(v):
    o = off(v)
    if o is None:
        return None
    e = data.find(b"\0", o)
    return data[o:e].decode("latin1")


def find_qword(x):
    pat = struct.pack("<Q", x)
    res = []
    i = data.find(pat)
    while i >= 0:
        res.append(va_of(i))
        i = data.find(pat, i + 1)
    return res


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "secs":
        for s in secs:
            print(s[0], hex(s[1]), hex(s[2]))
    elif cmd == "qref":
        for a in sys.argv[2:]:
            print(a, [hex(r) for r in find_qword(int(a, 16))])
    elif cmd == "qwords":
        t, n = int(sys.argv[2], 16), int(sys.argv[3])
        for i in range(n):
            print(i, hex(t + 8 * i), hex(q(t + 8 * i)))
    elif cmd == "f32":
        for a in sys.argv[2:]:
            print(a, f32(int(a, 16)))
    elif cmd == "table":
        # table start, entry size, count: dump {ptr->str, then remaining u32s}
        t, sz, n = int(sys.argv[2], 16), int(sys.argv[3]), int(sys.argv[4])
        for i in range(n):
            p = q(t + i * sz)
            rest = struct.unpack_from("<%dI" % ((sz - 8) // 4), data, off(t + i * sz + 8))
            print(i, hex(t + i * sz), cstr(p) if off(p) else hex(p), [hex(r) for r in rest])
