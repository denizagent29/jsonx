"""Deterministic datasets shared by every benchmark (written to benchmarks/data/)."""
import json, os, random, sys
D = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data"); os.makedirs(D, exist_ok=True)
random.seed(42)
N = int(os.environ.get("BENCH_N", 10000)); NU, NA = 200, 20000
def user(i): return {"id": i * 7919, "name": "user_%06d" % i, "active": i % 3 != 0, "score": round(random.random() * 1000, 3), "tags": ["t%d" % (i % 17), "grp%d" % (i % 5), "x" * (i % 9 + 1)]}
users = [user(i) for i in range(N)]
cls_users = [user(i) for i in range(NU)]
attendees = [{"user_index": random.randrange(NU), "seat": i} for i in range(NA)]
def jx(o, ind=None):
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "implementations", "python")); import jsonx
    return jsonx.dumps(o)
json.dump({"users": users}, open(f"{D}/users.json", "w"), separators=(",", ":"))
open(f"{D}/users.jsonx", "w").write(jx({"users": users}))
json.dump({"users": cls_users, "attendees": attendees}, open(f"{D}/class.json", "w"), separators=(",", ":"))
# JSONX form of the shared-reference dataset: attendees point at user objects with @users.N
txt = jx({"users": cls_users})[:-1] + ",attendees:[" + ",".join("{user:@users.%d,seat:%d}" % (a["user_index"], a["seat"]) for a in attendees) + "]}"
open(f"{D}/class.jsonx", "w").write(txt)
deep = "[" * 200 + "1" + "]" * 200
open(f"{D}/deep.json", "w").write(deep); open(f"{D}/deep.jsonx", "w").write(deep)
print("generated", N, "users;", NU, "users +", NA, "attendees")
