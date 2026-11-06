"""HTTP client examples (stdlib)."""
import os, sys, urllib.request, urllib.error
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "implementations", "python"))
import jsonx

def post(url, body: bytes, content_type: str, accept: str):
    req = urllib.request.Request(url, data=body, method="POST", headers={"Content-Type": content_type, "Accept": accept})
    try:
        with urllib.request.urlopen(req) as r: return r.status, r.headers, r.read()
    except urllib.error.HTTPError as e: return e.code, e.headers, e.read()

def post_binary(url, doc, schema):
    ct = f'application/bjsonx; schema="{schema.id:016x}"; v={schema.version}'
    status, hdr, data = post(url, jsonx.encode(doc, schema), ct, "application/bjsonx")
    return status, (jsonx.decode(data, schema) if status == 200 else data)
