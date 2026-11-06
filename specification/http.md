# HTTP Integration (v0.1)

| | |
|---|---|
| Text | `Content-Type: application/jsonx` (UTF-8) |
| Binary | `Content-Type: application/bjsonx; schema="<16-hex id>"; v=<schema version>` (params optional; the header inside the blob is authoritative) |

**Content negotiation**: `Accept: application/bjsonx, application/jsonx;q=0.5`; servers answer with `Vary: Accept`. `*/*` ⇒ JSONX.
**Schema negotiation**: a client sends the schema id it used; an unknown id gets **415** with `Accept-Schema: <id>,<id>…` listing the ids the server holds.
Servers load compiled schemas (`.jxsb`) once into an id → schema registry; decoding never re-parses schema text.
**Errors**: `application/problem+json` with `type: urn:jsonx:error:<code>`, `code`, `line`, `column`, `detail`; HTTP 400 for text/validation errors, 422 for malformed binary, 415 for unsupported media type or schema.
**Caching**: representations are deterministic (canonical writer) so a strong `ETag` over the bytes is stable; use `Vary: Accept`.
**Content-Encoding**: gzip/br apply to either type as usual (BJSONX has no built-in compression); the CRC-32 trailer (`checksum=true`) protects non-TLS storage, not transport.
**Streaming**: one blob is a random-access structure and must be complete before reading. A sequence of blobs can be streamed back-to-back: read 28 bytes, take `length` (offset 24), read the rest (`blob_length`).
Reference implementation and tests: `examples/http/server.py`, `client.py`, `tests/test_http.py` (text↔binary negotiation, 400/415/422 paths).

```js
// browser / Node
import { parse, encode, decode, loadSchema } from "jsonx";
const res = await fetch("/lessons", { method: "POST", headers: { "Content-Type": "application/bjsonx", Accept: "application/bjsonx" },
                                       body: encode(parse(text), schema) });
const doc = decode(new Uint8Array(await res.arrayBuffer()), schema);
```
