import os, sys, random, glob, struct
HERE = os.path.dirname(os.path.abspath(__file__)); V = os.path.join(HERE, "vectors")
sys.path.insert(0, os.path.join(HERE, "..", "implementations", "python"))
import jsonx
from jsonx import JsonxError
rd = lambda p, m="r": open(os.path.join(V, p), m + ("" if "b" in m else ""), **({} if "b" in m else {"encoding": "utf-8", "newline": ""})).read()
n = 0
def ok(c, msg):
    global n; n += 1
    if not c: print("FAIL", msg); sys.exit(1)
names = sorted(os.path.basename(p)[:-6] for p in glob.glob(V + "/text/valid/*.jsonx"))
for nm in names:
    d = jsonx.parse(rd(f"text/valid/{nm}.jsonx"))
    ok(d.dump() == rd(f"text/valid/{nm}.dump"), nm + " dump")
    b = jsonx.encode(d); ok(b == rd(f"text/valid/{nm}.bjsonx", "rb"), nm + " golden bytes")
    ok(jsonx.decode(b).dump() == d.dump(), nm + " roundtrip")
    ok(jsonx.encode(d, checksum=True) == rd(f"text/valid/{nm}.crc.bjsonx", "rb"), nm + " crc")
for p in sorted(glob.glob(V + "/text/errors/*.jsonx")):
    nm = os.path.basename(p)[:-6]; exp = rd(f"text/errors/{nm}.err").split()
    try: jsonx.parse(rd(f"text/errors/{nm}.jsonx")); ok(False, nm + " should fail")
    except JsonxError as e: ok(f"{e.code} {e.line}:{e.col}" == " ".join(exp), nm)
sc = jsonx.load_schema(rd("schema/lesson.jxsb", "rb")); sc1 = jsonx.load_schema(rd("schema/lesson_v1.jxsb", "rb"))
for nm in rd("typed/manifest.txt").split():
    d = jsonx.parse(rd(f"typed/{nm}.jsonx")); b = jsonx.encode(d, sc)
    ok(b == rd(f"typed/{nm}.bjsonx", "rb"), nm + " typed golden")
    ok(jsonx.decode(b, sc).dump() == rd(f"typed/{nm}.dump"), nm + " typed dump")
    ok(jsonx.encode(d, sc, checksum=True) == rd(f"typed/{nm}.crc.bjsonx", "rb"), nm + " typed crc")
    ok(jsonx.encode(jsonx.decode(b, sc), sc) == b, nm + " re-encode identical")
for p in sorted(glob.glob(V + "/typed/errors/*.jsonx")):
    nm = os.path.basename(p)[:-6]; exp = rd(f"typed/errors/{nm}.err").strip()
    try: jsonx.encode(jsonx.parse(rd(f"typed/errors/{nm}.jsonx")), sc); ok(False, nm)
    except JsonxError as e: ok(f"{e.code} {e.line}:{e.col}" == exp, nm + " " + str(e))
# compat
ok(jsonx.decode(rd("compat/v1_data.bjsonx", "rb"), sc).dump() == rd("compat/v1_data_read_by_v2.dump"), "v1 data / v2 reader")
ok(jsonx.decode(rd("compat/v2_data.bjsonx", "rb"), sc1).dump() == rd("compat/v2_data_read_by_v1.dump"), "v2 data / v1 reader")
ok(jsonx.decode(rd("compat/v2_data.bjsonx", "rb"), sc).dump() == rd("compat/v2_data_read_by_v2.dump"), "v2/v2")
v = jsonx.view(rd("compat/v2_data.bjsonx", "rb"), sc1); ok(v["users"][1]["name"] == "b", "v1 view over v2 blob (strided arrays)")
# schema compiler vectors
for p_ in sorted(glob.glob(V + "/schema/ok/*.jxs")):
    nm = os.path.basename(p_)[:-4]
    ok(jsonx.compile_schema(rd(f"schema/ok/{nm}.jxs"), nm + ".jxs").to_bytes() == rd(f"schema/ok/{nm}.jxsb", "rb"), "schema ok " + nm)
    ok(jsonx.load_schema(rd(f"schema/ok/{nm}.jxsb", "rb")).to_bytes() == rd(f"schema/ok/{nm}.jxsb", "rb"), "schema reload " + nm)
ldr = lambda p, f: (open(os.path.join(V, "schema", "ok", p)).read(), p)
for p_ in sorted(glob.glob(V + "/schema/errors/*.jxs")):
    nm = os.path.basename(p_)[:-4]
    try: jsonx.compile_schema(rd(f"schema/errors/{nm}.jxs"), nm + ".jxs", ldr); ok(False, "schema error " + nm)
    except JsonxError as e: ok(f"{e.code} {e.line}:{e.col}" == rd(f"schema/errors/{nm}.err").strip(), "schema error " + nm + " got " + str(e))
# malformed
for p in sorted(glob.glob(V + "/binary/malformed/*.bin")):
    nm = os.path.basename(p)[:-4]; exp = rd(f"binary/malformed/{nm}.err").strip()
    s = sc if nm.startswith("typed") else None
    try: jsonx.decode(rd(f"binary/malformed/{nm}.bin", "rb"), s); ok(False, nm + " accepted")
    except JsonxError as e: ok(e.code == exp, f"{nm}: got {e.code} want {exp}")
# aliasing semantics
d = jsonx.parse(rd("text/valid/refs_basic.jsonx")); py = d.to_python()
ok(py["user_related_to"] is py["users"][1], "alias identity")
d2 = jsonx.decode(jsonx.encode(d)).to_python(); ok(d2["user_related_to"] is d2["users"][1], "alias identity after binary")
cy = jsonx.decode(jsonx.encode(jsonx.parse(rd("text/valid/structure_cycle.jsonx")))).to_python()
ok(cy["node"]["up"] is cy and cy["node"]["self"] is cy["node"], "cycle preserved")
# fuzz: mutated blobs must raise JsonxError only (or decode), never crash/hang
random.seed(1); seeds = [(rd(f"typed/lesson.bjsonx", "rb"), sc), (rd("typed/lesson_many_users.bjsonx", "rb"), sc), (rd("text/valid/many_refs.bjsonx", "rb"), None), (rd("text/valid/scalars.bjsonx", "rb"), None)]
for it in range(6000):
    b, s = random.choice(seeds); m = bytearray(b)
    for _ in range(random.randint(1, 4)): m[random.randrange(len(m))] = random.randrange(256)
    if random.random() < .2: m = m[:random.randrange(len(m))]
    try: jsonx.decode(bytes(m), s)
    except JsonxError: pass
ok(True, "fuzz")
# property: random plain data roundtrip text and binary
def gen(dp=0):
    r = random.random()
    if dp > 3 or r < .5: return random.choice([None, True, False, random.randint(-2**63, 2**64 - 1), random.random() * 1e10, "s\u00e9\n", ""])
    if r < .75: return [gen(dp + 1) for _ in range(random.randint(0, 4))]
    return {random.choice(["a", "b c", "k", "ключ"]) + str(i): gen(dp + 1) for i in range(random.randint(0, 4))}
for _ in range(300):
    o = gen(); t = jsonx.dumps(o); d = jsonx.parse(t)
    ok(d.to_python() == o, "text roundtrip"); ok(jsonx.decode(jsonx.encode(d)).to_python() == o, "binary roundtrip")
print(f"python: {n} checks passed")
