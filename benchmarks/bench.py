"""Python benchmark: JSONX vs JSON, MessagePack, CBOR, BSON, Avro, Protocol Buffers on identical data.
Pure-Python JSONX is compared against C-extension libraries; see RESULTS.md for interpretation."""
import gc, io, json, os, platform, statistics, subprocess, sys, tempfile, time, tracemalloc
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, os.path.join(HERE, "..", "implementations", "python"))
import jsonx, msgpack, cbor2, bson, fastavro
D = os.path.join(HERE, "data")
if not os.path.exists(D + "/users.json"): subprocess.check_call([sys.executable, HERE + "/gen_data.py"])
REPS = int(os.environ.get("BENCH_REPS", 5))

def t(f, reps=REPS, min_total=0.2):
    """median seconds per call (each sample runs enough iterations to take >= min_total/reps)"""
    f(); samples = []
    for _ in range(reps):
        gc.collect(); n = 1
        while True:
            s = time.perf_counter()
            for _ in range(n): f()
            e = time.perf_counter() - s
            if e >= min_total / reps or n >= 1 << 14: break
            n *= 2
        samples.append(e / n)
    return statistics.median(samples)
def mem(f):
    gc.collect(); tracemalloc.start(); r = f(); cur, peak = tracemalloc.get_traced_memory(); tracemalloc.stop(); return peak
ms = lambda x: f"{x * 1e3:.2f}"
rows = []

# ---------------- dataset 1: flat array of records
users_json = open(D + "/users.json").read(); users = json.loads(users_json)["users"]
sc = jsonx.compile_file(HERE + "/users.jxs"); sc = jsonx.load_schema(sc.to_bytes())
jx_text = open(D + "/users.jsonx").read()
doc = jsonx.parse(jx_text)
tmp = tempfile.mkdtemp()
open(tmp + "/u.proto", "w").write('syntax="proto3"; message User{uint64 id=1; string name=2; bool active=3; double score=4; repeated string tags=5;} message Users{repeated User users=1;}')
subprocess.check_call([sys.executable, "-m", "grpc_tools.protoc", f"-I{tmp}", f"--python_out={tmp}", tmp + "/u.proto"]); sys.path.insert(0, tmp)
import u_pb2
avro_schema = fastavro.parse_schema({"type": "record", "name": "Users", "fields": [{"name": "users", "type": {"type": "array", "items": {"type": "record", "name": "User", "fields": [
    {"name": "id", "type": "long"}, {"name": "name", "type": "string"}, {"name": "active", "type": "boolean"}, {"name": "score", "type": "double"}, {"name": "tags", "type": {"type": "array", "items": "string"}}]}}}]})
def pb_build():
    m = u_pb2.Users()
    for u in users: x = m.users.add(); x.id = u["id"]; x.name = u["name"]; x.active = u["active"]; x.score = u["score"]; x.tags.extend(u["tags"])
    return m
pbm = pb_build()
def avro_enc():
    b = io.BytesIO(); fastavro.schemaless_writer(b, avro_schema, {"users": users}); return b.getvalue()
fmts = {
 "JSON (json)": (lambda: json.dumps(users and {"users": users}, separators=(",", ":")).encode(), lambda b: json.loads(b), lambda b: json.loads(b)["users"][5000]["name"]),
 "MessagePack": (lambda: msgpack.packb({"users": users}), lambda b: msgpack.unpackb(b), lambda b: msgpack.unpackb(b)["users"][5000]["name"]),
 "CBOR (cbor2)": (lambda: cbor2.dumps({"users": users}), lambda b: cbor2.loads(b), lambda b: cbor2.loads(b)["users"][5000]["name"]),
 "BSON (pymongo)": (lambda: bson.encode({"users": users}), lambda b: bson.decode(b), lambda b: bson.decode(b)["users"][5000]["name"]),
 "Avro (fastavro)": (avro_enc, lambda b: fastavro.schemaless_reader(io.BytesIO(b), avro_schema), lambda b: fastavro.schemaless_reader(io.BytesIO(b), avro_schema)["users"][5000]["name"]),
 "Protobuf (upb)": (lambda: pbm.SerializeToString(), lambda b: (lambda m: (m.ParseFromString(b), m)[1])(u_pb2.Users()), lambda b: (lambda m: (m.ParseFromString(b), m.users[5000].name)[1])(u_pb2.Users())),
 "BJSONX dynamic": (lambda: jsonx.encode(doc), lambda b: jsonx.decode(b), lambda b: jsonx.view(b)["users"][5000]["name"]),
 "BJSONX typed": (lambda: jsonx.encode(doc, sc), lambda b: jsonx.decode(b, sc), lambda b: jsonx.view(b, sc)["users"][5000]["name"]),
}
print(f"## Dataset A: {len(users)} user records (id, name, active, score, 3 tags)\n")
print("| format | size (bytes) | encode ms | full decode ms | 1-field access ms | decode peak mem (KiB) |\n|---|---:|---:|---:|---:|---:|")
for name, (enc, dec, one) in fmts.items():
    blob = enc(); size = len(blob)
    print(f"| {name} | {size:,} | {ms(t(enc))} | {ms(t(lambda: dec(blob)))} | {ms(t(lambda: one(blob)))} | {mem(lambda: dec(blob)) // 1024:,} |", flush=True)
print("\nText formats:\n\n| operation | ms |\n|---|---:|")
print(f"| JSON text size | {len(users_json):,} bytes |\n| JSONX text size | {len(jx_text):,} bytes |")
print(f"| json.loads (C) | {ms(t(lambda: json.loads(users_json)))} |")
print(f"| jsonx.parse (pure Python, incl. resolver) | {ms(t(lambda: jsonx.parse(jx_text), reps=3, min_total=0.1))} |")
print(f"| json.dumps (C) | {ms(t(lambda: json.dumps({'users': users})))} |")
print(f"| jsonx.dumps (pure Python) | {ms(t(lambda: jsonx.dumps({'users': users}), reps=3, min_total=0.1))} |")

# ---------------- dataset 2: shared references
cj = json.load(open(D + "/class.json")); ctext = open(D + "/class.jsonx").read(); cdoc = jsonx.parse(ctext)
cjson = json.dumps(cj, separators=(",", ":")).encode(); cmp_ = msgpack.packb(cj)
cb_dyn = jsonx.encode(cdoc); cb_typ = jsonx.encode(cdoc, sc, root="Class")
embedded = json.dumps({"users": cj["users"], "attendees": [{"user": cj["users"][a["user_index"]], "seat": a["seat"]} for a in cj["attendees"]]}, separators=(",", ":")).encode()
print("\n## Dataset B: 200 users, 20,000 attendees each pointing at one user\n")
print("| representation | size (bytes) | notes |\n|---|---:|---|")
print(f"| JSON, user as integer index | {len(cjson):,} | application-level foreign key; consumer must resolve it |")
print(f"| JSON, user embedded (denormalised) | {len(embedded):,} | what a naive serializer produces for shared objects |")
print(f"| MessagePack, integer index | {len(cmp_):,} | |")
print(f"| JSONX text with @users.N references | {len(ctext):,} | real references |")
print(f"| BJSONX dynamic (refs = 5-byte slots) | {len(cb_dyn):,} | |")
print(f"| BJSONX typed (ref<User> = 4-byte offset) | {len(cb_typ):,} | |")
v = jsonx.view(cb_typ, sc, ) if False else None
from jsonx.binary import view as bview
def nav(): return bview(cb_typ, sc)["attendees"][12345]["user"]["name"]
print(f"\nPointer navigation `attendees[12345].user.name` on the typed blob (zero-copy, through a ref): {ms(t(nav))} ms per access")

# ---------------- dataset 3: deep nesting
deep = open(D + "/deep.jsonx").read()
print("\n## Dataset C: nesting depth 200\n\n| operation | ms |\n|---|---:|")
print(f"| json.loads | {ms(t(lambda: json.loads(deep)))} |\n| jsonx.parse | {ms(t(lambda: jsonx.parse(deep)))} |")
dd = jsonx.parse(deep); db = jsonx.encode(dd)
print(f"| jsonx.encode (dynamic) | {ms(t(lambda: jsonx.encode(dd)))} |\n| jsonx.decode | {ms(t(lambda: jsonx.decode(db)))} |")
# schema compile time
src = open(HERE + "/users.jxs").read()
print(f"\nSchema compile time (`users.jxs`, 4 types): {ms(t(lambda: jsonx.compile_schema(src)))} ms; load canonical binary schema: {ms(t(lambda: jsonx.load_schema(sc.to_bytes())))} ms")
print(f"\nEnvironment: Python {platform.python_version()}, {platform.machine()}, {os.cpu_count()} CPU(s), {platform.system()}; median of {REPS} samples.")
