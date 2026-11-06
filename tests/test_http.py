import os, sys, threading, json
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "examples", "http")); sys.path.insert(0, os.path.join(HERE, "..", "implementations", "python"))
import server, client, jsonx
srv = server.serve(0); threading.Thread(target=srv.serve_forever, daemon=True).start()
url = f"http://127.0.0.1:{srv.server_address[1]}/lessons"
text = open(os.path.join(HERE, "..", "examples", "text", "lesson.jsonx"), "rb").read()
n = 0
def ok(c, m):
    global n; n += 1
    if not c: print("FAIL", m); sys.exit(1)
# 1. text in, text out
s, h, b = client.post(url, text, "application/jsonx", "application/jsonx"); ok(s == 200 and h["Content-Type"] == "application/jsonx" and h["Vary"] == "Accept", "text roundtrip")
ok(jsonx.parse(b).to_python()["users"][0]["name"] == "Deniz", "text body parses")
# 2. text in, binary out (content negotiation) and decode with the shared compiled schema
s, h, b = client.post(url, text, "application/jsonx", "application/bjsonx;q=1, application/jsonx;q=0.5")
ok(s == 200 and h["Content-Type"].startswith("application/bjsonx") and f'schema="{server.SCHEMA.id:016x}"' in h["Content-Type"], "binary negotiated")
d = jsonx.decode(b, server.SCHEMA); ok(d.dump().startswith('{"id":7'), "binary body decodes"); ok(h["ETag"], "etag")
# 3. binary in, binary out
st, doc = client.post_binary(url, jsonx.parse(text), server.SCHEMA); ok(st == 200 and doc.dump() == d.dump(), "binary roundtrip")
# 4. errors: invalid text -> problem+json with line/col; unknown schema -> 415 + Accept-Schema; corrupt binary -> 422
s, h, b = client.post(url, b"{ a: @nothing }", "application/jsonx", "*/*"); ok(s == 400 and h["Content-Type"] == "application/problem+json" and json.loads(b)["code"] == "E_UNDEFINED_LABEL" and json.loads(b)["line"] == 1, "problem+json")
s, h, b = client.post(url, jsonx.encode(jsonx.parse(text), server.SCHEMA), 'application/bjsonx; schema="deadbeefdeadbeef"; v=1', "*/*"); ok(s == 415 and "Accept-Schema" in h, "unknown schema 415")
s, h, b = client.post(url, b"BJX\x01" + b"\0" * 30, "application/bjsonx", "*/*"); ok(s == 422, "corrupt binary 422")
s, h, b = client.post(url, b"x", "text/plain", "*/*"); ok(s == 415, "unsupported media type")
print(f"http: {n} checks passed"); srv.shutdown()
