"""Minimal JSONX/BJSONX HTTP service (stdlib only). Run: python3 server.py [port]

POST /lessons   body: application/jsonx | application/bjsonx; schema="<id>"; v=<n>
                Accept: application/jsonx | application/bjsonx  -> echoes the validated, normalised document.
Schemas are compiled ONCE at start-up and looked up by id for every request (no per-request schema parsing).
Errors are returned as application/problem+json with the JSONX error code, line and column.
"""
import json, os, sys, zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "implementations", "python"))
import jsonx
from jsonx import JsonxError

SCHEMA = jsonx.compile_file(os.path.join(HERE, "..", "schemas", "lesson.jxs"))
REGISTRY = {f"{SCHEMA.id:016x}": SCHEMA}          # schema id -> compiled schema
JSONX, BJSONX = "application/jsonx", "application/bjsonx"

def params(header: str):
    parts = [p.strip() for p in header.split(";")]
    return parts[0].lower(), {k.lower(): v.strip('"') for k, _, v in (p.partition("=") for p in parts[1:])}

def negotiate(accept: str) -> str:
    """Pick the best supported media type from an Accept header (q-values honoured, default JSONX)."""
    best, bq = JSONX, -1.0
    for item in (accept or "*/*").split(","):
        mt, p = params(item)
        q = float(p.get("q", 1))
        if mt in (JSONX, BJSONX) and q > bq: best, bq = mt, q
        elif mt in ("*/*", "application/*") and q > bq and bq < 0: best, bq = JSONX, q
    return best

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def problem(self, status, e: JsonxError):
        body = json.dumps({"type": f"urn:jsonx:error:{e.code}", "title": e.category, "status": status, "detail": e.message, "code": e.code, "line": e.line, "column": e.col}).encode()
        self.send_response(status); self.send_header("Content-Type", "application/problem+json"); self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        ctype, p = params(self.headers.get("Content-Type", JSONX))
        try:
            if ctype == BJSONX:
                sid = p.get("schema")
                if sid and sid not in REGISTRY:       # schema negotiation: tell the client which schemas we know
                    self.send_response(415); self.send_header("Accept-Schema", ",".join(REGISTRY)); self.end_headers(); return
                doc = jsonx.decode(body, REGISTRY.get(sid) if sid else SCHEMA)
            elif ctype == JSONX:
                doc = jsonx.parse(body); jsonx.encode(doc, SCHEMA)       # validate against the schema
            else:
                self.send_response(415); self.send_header("Accept", f"{JSONX}, {BJSONX}"); self.end_headers(); return
            out_type = negotiate(self.headers.get("Accept", ""))
            if out_type == BJSONX:
                out = jsonx.encode(doc, SCHEMA, checksum=True)
                ct = f'{BJSONX}; schema="{SCHEMA.id:016x}"; v={SCHEMA.version}'
            else:
                out = jsonx.dumps(doc.to_python(), indent=2).encode(); ct = JSONX
        except JsonxError as e:
            return self.problem(400 if e.category != "binary" else 422, e)
        self.send_response(200); self.send_header("Content-Type", ct); self.send_header("Content-Length", str(len(out)))
        self.send_header("Vary", "Accept"); self.send_header("ETag", f'"{zlib.crc32(out):08x}"'); self.send_header("Cache-Control", "no-cache"); self.end_headers(); self.wfile.write(out)

def serve(port=0):
    srv = ThreadingHTTPServer(("127.0.0.1", port), H); return srv

if __name__ == "__main__":
    s = serve(int(sys.argv[1]) if len(sys.argv) > 1 else 8080); print("listening on", s.server_address); s.serve_forever()
