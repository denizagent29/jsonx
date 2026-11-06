# Benchmark Results (measured, not estimated)

Produced by `benchmarks/run_all.sh` in a single run on one machine (1 vCPU Intel Xeon @ 2.1 GHz, shared sandbox, Linux; Python 3.12.3, Node 22, Go 1.22,
Rust 1.75 `--release`) with identical generated datasets (`gen_data.py`, seed 42). Timings are medians of 3 samples. **Run-to-run noise on this shared
machine is up to ~30 %**, so compare ratios *within* one table, not absolute numbers across runs. The same workflow runs on GitHub Actions
(`.github/workflows/bench.yml`, manual or weekly) and publishes its own numbers to the job summary.

Comparators: JSON, MessagePack, CBOR (cbor2), BSON (pymongo), Avro (fastavro), Protocol Buffers (upb C runtime). **FlatBuffers and Cap'n Proto were not
measured** (compilers unavailable here). Comparator libraries are C extensions, the Python JSONX implementation is pure Python: Python speed numbers
compare *implementations* as much as *formats*.

## What the numbers say
* **Where JSONX loses.** Bulk encode/decode and text parsing.
  Python (pure): full decode of 10,000 records is ~26x slower than `json`, ~28x slower than MessagePack and ~250x slower than Protobuf-upb (223 ms typed
  vs 8.5 / 8.1 / 0.9 ms); text parsing (with the resolver) is ~32x slower than `json.loads` (249 ms vs 7.9 ms).
  Node: `JSON.parse` is ~11x faster than JSONX text parsing (4.7 ms vs 53.7 ms). Go: `encoding/json` is ~1.8x faster than `jsonx.Parse` (19.6 ms vs 34.5 ms).
  Rust parses the same file in ~17 ms (no Rust JSON baseline measured). Dynamic (schema-less) BJSONX is **5 % larger than JSON** on this record-heavy data
  (9 bytes per object entry, 13 per float); typed BJSONX is **33 % smaller than JSON and 7 % smaller than MessagePack**, but still 35 % larger than Protobuf and
  56 % larger than Avro, which store neither field names nor offsets/presence structure.
* **Where JSONX wins.** Random access and shared references. Reading one field through a zero-copy view costs ~0.01 ms in Python (vs 8-12 ms for every format that
  decodes the whole document first, 22 ms for Avro), ~1 us in Go, and is below the 0.01 ms resolution of the Node/Rust tables. Shared objects are stored once: with 20,000
  references to 200 users the typed blob is 173 KB vs 637 KB for JSON with integer foreign keys (3.7x), 441 KB for MessagePack with integer keys (2.5x), and 2.3 MB for denormalised JSON.
* **Typed vs generic decoding (Rust, measured):** `#[derive(Bjsonx)]` `from_blob` into a struct takes 3.7 ms vs 15.7 ms for the generic validated `jsonx::decode` (4.2x faster), because it
  reads only what the struct needs and skips alias bookkeeping. `to_blob` (18 ms) is *slower* than decode: the derive currently builds an intermediate `Doc` then encodes it; a direct
  struct-to-bytes encoder is the obvious next optimisation and is **not implemented**.
* **Why owned decode is slow elsewhere:** it builds a validated node graph with alias tracking (location memo, per-node budget checks). Views are the fast path. In Go the remaining cost is per-node
  allocation (a `big.Int` and a wide `Node` per value); replacing string memo keys with integer keys and cached type keys gave only ~10 %.
* **Not measured:** generated *specialised* codecs vs the interpreted codec (the derive's typed decode above is the only specialised path), relative-vs-absolute offsets, FlatBuffers, Cap'n Proto.

---


## Python
## Dataset A: 10000 user records (id, name, active, score, 3 tags)

| format | size (bytes) | encode ms | full decode ms | 1-field access ms | decode peak mem (KiB) |
|---|---:|---:|---:|---:|---:|
| JSON (json) | 963,892 | 8.93 | 8.54 | 9.76 | 5,965 |
| MessagePack | 694,102 | 3.76 | 8.09 | 8.63 | 4,939 |
| CBOR (cbor2) | 694,102 | 16.80 | 9.89 | 11.79 | 7,229 |
| BSON (pymongo) | 1,113,019 | 5.02 | 7.69 | 7.79 | 7,268 |
| Avro (fastavro) | 413,980 | 22.61 | 22.35 | 21.81 | 5,061 |
| Protobuf (upb) | 477,173 | 0.58 | 0.88 | 0.88 | 0 |
| BJSONX dynamic | 1,014,189 | 90.18 | 275.48 | 0.01 | 32,632 |
| BJSONX typed | 644,147 | 111.18 | 222.52 | 0.01 | 31,429 |

Text formats:

| operation | ms |
|---|---:|
| JSON text size | 963,892 bytes |
| JSONX text size | 863,890 bytes |
| json.loads (C) | 7.90 |
| jsonx.parse (pure Python, incl. resolver) | 249.12 |
| json.dumps (C) | 9.04 |
| jsonx.dumps (pure Python) | 67.20 |

## Dataset B: 200 users, 20,000 attendees each pointing at one user

| representation | size (bytes) | notes |
|---|---:|---|
| JSON, user as integer index | 636,784 | application-level foreign key; consumer must resolve it |
| JSON, user embedded (denormalised) | 2,342,370 | what a naive serializer produces for shared objects |
| MessagePack, integer index | 440,647 | |
| JSONX text with @users.N references | 574,780 | real references |
| BJSONX dynamic (refs = 5-byte slots) | 500,382 | |
| BJSONX typed (ref<User> = 4-byte offset) | 172,916 | |

Pointer navigation `attendees[12345].user.name` on the typed blob (zero-copy, through a ref): 0.01 ms per access

## Dataset C: nesting depth 200

| operation | ms |
|---|---:|
| json.loads | 0.01 |
| jsonx.parse | 0.27 |
| jsonx.encode (dynamic) | 0.18 |
| jsonx.decode | 0.46 |

Schema compile time (`users.jxs`, 4 types): 0.10 ms; load canonical binary schema: 0.04 ms

Environment: Python 3.12.3, x86_64, 1 CPU(s), Linux; median of 3 samples.

## Node.js
| operation (Node v22.23.4) | ms | size (bytes) |
|---|---:|---:|
| JSON.stringify | 3.38 | 963892 |
| JSON.parse | 4.70 |  |
| JSONX parse (text, with resolver) | 53.69 | 863890 |
| BJSONX dynamic encode | 44.08 | 1014189 |
| BJSONX dynamic decode (to Document) | 85.17 |  |
| BJSONX typed encode | 55.53 | 644147 |
| BJSONX typed decode (to Document) | 101.61 |  |
| JSON.parse + read users[5000].name | 4.63 |  |
| BJSONX typed view: users[5000].name (zero-copy) | 0.00 |  |
| BJSONX dynamic view: users[5000].name (zero-copy) | 0.00 |  |

## Go
```
goos: linux
goarch: amd64
cpu: Intel(R) Xeon(R) Processor @ 2.10GHz
BenchmarkJSONUnmarshal          	      64	  19610219 ns/op	  49.15 MB/s
BenchmarkJSONXParse             	      34	  34525394 ns/op	  25.02 MB/s
BenchmarkEncodeTyped            	      40	  25799232 ns/op
BenchmarkDecodeTyped            	      15	  72839541 ns/op	   8.84 MB/s
BenchmarkViewField              	 1025661	      1033 ns/op
BenchmarkJSONUnmarshalThenIndex 	      42	  27328413 ns/op
```

## Rust (generic API)
| operation (Rust, --release) | ms | size (bytes) |
|---|---:|---:|
| JSONX parse (text, with resolver) | 16.87 | 863890 |
| BJSONX dynamic encode | 12.99 | 1014189 |
| BJSONX dynamic decode (to Doc) | 16.85 |  |
| BJSONX typed encode | 14.44 | 644147 |
| BJSONX typed decode (to Doc) | 18.96 |  |
| BJSONX typed view: users[5000].name (zero-copy) | 0.00 |  |

## Rust (derive)
| operation (Rust --release; 10,000 items + 1,000 customers; blob 558705 bytes) | ms |
|---|---:|
| derive: to_blob (struct -> Doc -> bytes) | 17.987 |
| derive: from_blob (bytes -> owned struct) | 3.739 |
| generic: jsonx::decode (bytes -> validated Doc) | 15.701 |
| derive view: items[5000].sku (zero-copy) | 0.000 |
