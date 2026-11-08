<div align="center">

# JSONX

**JSON that can point at itself — with a compact, zero-copy binary twin.**

Comments · unquoted keys · real references · scoped labels · typed schemas · random-access binary

[![CI](https://github.com/denizagent29/jsonx/actions/workflows/ci.yml/badge.svg)](https://github.com/denizagent29/jsonx/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Python](https://img.shields.io/badge/Python-3.9%E2%80%933.13-3776AB?logo=python&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.75%2B-000000?logo=rust)
![Go](https://img.shields.io/badge/Go-1.22%2B-00ADD8?logo=go&logoColor=white)
![Node](https://img.shields.io/badge/Node-18%2B-339933?logo=node.js&logoColor=white)

</div>

```jsonx
{
    // comments, unquoted keys, trailing commas
    users: [ { id: 23, name: "Deniz" }, { id: 35, name: "Adil" } ],

    related: @users.1,          // a link to the object — not a copy, not a string path
    uid:     @users.0.id,       // a link to a single field
    <Admin> admin: { <me> id: 1, again: @me.id },          // labels and scoped variables

    lessons: { attendees: [ { user: @users.0 } ] }         // lessons.attendees[0].user.name just works
}
```

JSONX has two forms of one data model — **JSONX text** (`application/jsonx`) for people and **BJSONX** (`application/bjsonx`) for machines — plus a schema language, a compiler,
and four implementations (**Python, JavaScript/TypeScript, Go, Rust**) that pass the same golden test vectors and produce **byte-identical** binary files.

## Why JSONX?

| You want… | JSON | JSONX |
|---|:-:|:-:|
| comments, unquoted keys, trailing commas | ✗ | ✅ |
| shared objects & cross-links **without duplicating data** | ✗ (copy or invent foreign keys) | ✅ `@users.1`, `@..Users.$user.id`, `$prev` / `$next` |
| statically-checked links (a bad link is an error, never `null`) | ✗ | ✅ errors with line/column |
| typed schemas with versions, defaults, enums, unions, constraints | ✗ | ✅ `.jxs` → compiled `.jxsb` |
| read one field of a 500 KB document **without decoding it** | ✗ | ✅ zero-copy views over the bytes |
| binary pointers that survive across languages and CPUs | ✗ | ✅ byte offsets, never memory addresses |
| a hostile-input-safe decoder (every offset and length validated) | – | ✅ bounds, cycles, expansion limits, fuzzed |

It is **not** trying to beat Protobuf at raw throughput; it targets documents with *structure* — links, shared objects, partial reads — where copying and re-parsing hurt.
Numbers, including where JSONX loses, are in [`benchmarks/RESULTS.md`](benchmarks/RESULTS.md).

## Quick start (60 seconds)

You need [uv](https://docs.astral.sh/uv/) for the Python CLI (or any `python3 ≥ 3.9`; the library has no dependencies).

```bash
git clone https://github.com/denizagent29/jsonx && cd jsonx
J="uv run --project implementations/python jsonxc"       # or: compiler/jsonxc/jsonxc

$J compile  examples/schemas/lesson.jxs -o lesson.jxsb                               # schema -> canonical binary schema
$J encode   examples/text/lesson.jsonx -s examples/schemas/lesson.jxs --crc -o lesson.bjsonx
$J inspect  lesson.bjsonx -s lesson.jxsb --hex                                       # header, structure, hex dump
$J decode   lesson.bjsonx -s lesson.jxsb --format json                               # references appear as {"$ref": "/users/1"}
$J validate examples/text/lesson.jsonx -s examples/schemas/lesson.jxs
$J compat   old.jxs new.jxs                                                          # is NEW a compatible evolution of OLD?
$J gen      examples/schemas/lesson.jxs --lang ts|go|rust|python                     # types + embedded schema
```
There is also a native Rust build of the same tool (`cargo run --release -p jsonxc -- --help` in `implementations/rust`), verified byte-for-byte against the Python one.
Exit codes: `0` ok · `1` usage/IO · `2` invalid text, schema or data · `3` malformed binary.

## Using it from code

<details open><summary><b>Python</b></summary>

```python
import jsonx
doc = jsonx.parse(open("examples/text/lesson.jsonx").read())      # JsonxError carries .code .line .col
py = doc.to_python(); assert py["attendees"][0]["user"] is py["users"][1]      # links keep object identity

schema = jsonx.compile_file("examples/schemas/lesson.jxs")        # or jsonx.load_schema(bytes)  (.jxsb)
blob = jsonx.encode(doc, schema, checksum=True)                   # schema-less: jsonx.encode(doc)
v = jsonx.view(blob, schema)                                      # zero-copy
print(v["attendees"][0]["user"]["name"], v["shape"].name)         # follows the reference
owned = jsonx.decode(blob, schema)                                # fully validated copy
```
</details>

<details><summary><b>JavaScript / TypeScript</b> (Node ≥ 18 and browsers, ESM, types included)</summary>

```js
import { parse, encode, decode, view, loadSchema } from "./implementations/js/src/jsonx.mjs";
const sc = loadSchema(new Uint8Array(fs.readFileSync("lesson.jxsb")));
const blob = encode(parse(text), sc);               // Uint8Array
view(blob, sc).get("users").get(0).get("name");     // zero-copy over the ArrayBuffer
```
</details>

<details><summary><b>Go</b></summary>

```go
doc, err := jsonx.Parse(text)                        // *jsonx.Error carries Code/Line/Col
sc, _ := jsonx.LoadSchema(jxsb)
blob, _ := jsonx.Encode(doc, sc, jsonx.EncodeOptions{Checksum: true})
root, _ := jsonx.View(blob, sc, jsonx.ReadOptions{}) // slices alias blob
users, _ := root.(*jsonx.StructView).Get("users")

// typed: structs as generated by `jsonxc gen go` (jx tags, *T = optional, union = struct of variant pointers)
var r Receipt
err = jsonx.Unmarshal(blob, sc, &r)                       // newer fields missing in old blobs take schema defaults
blob2, err := jsonx.Marshal(&r, sc, jsonx.EncodeOptions{}) // byte-identical to the other implementations
```
</details>

<details><summary><b>Rust</b> — generic API and <code>#[derive(Bjsonx)]</code></summary>

```rust
// generic
let sc = jsonx::Schema::from_bytes(&std::fs::read("lesson.jxsb")?)?;      // or jsonx::compile_file(path)
let doc = jsonx::parse(&text)?;
let blob = jsonx::encode(&doc, Some(&sc), &Default::default())?;
let b = jsonx::Blob::open(&blob, Some(&sc), &Default::default())?;       // borrows `blob`; nothing is copied
```
```rust
// typed: the macro compiles the .jxs at build time, checks your struct against it, embeds the schema
use jsonx_derive::Bjsonx;                                // Cargo.toml: jsonx = "0.1", jsonx-derive = "0.1"

#[derive(Bjsonx, Debug, PartialEq)]
#[bjsonx(schema = "schemas/shop.jxs")]
struct Customer { id: u64, name: String, vip: bool, email: Option<String> }
// ...derive the other types of the schema the same way (enums = unit variants, unions = one-field tuple variants)

let blob: Vec<u8> = receipt.to_blob()?;                  // writes straight into the buffer; to_blob_with(Some(1), true) = old layout + CRC
let back = Receipt::from_blob(&blob)?;                   // owned value
let open = Receipt::open(&blob)?;  let view = Receipt::view(&open)?;      // typed zero-copy view
let nick: &str = view.nick()?;                           // points into `blob`, nothing is copied
```
Mismatches (missing/extra field, wrong type, `Option` vs required) are compile errors on the offending field. `ref<T>` fields decode and are navigable in views but cannot be *encoded* from plain
values; `any` fields are rejected. Working example: [`jsonx-derive/tests/derive.rs`](implementations/rust/jsonx-derive/tests/derive.rs) with [`examples/schemas/shop.jxs`](examples/schemas/shop.jxs).
</details>

## Schemas in one minute

```jxs
namespace acme.school
version 2                                   // append-only evolution: old readers/writers keep working

enum Role : u8 { guest = 0  member = 1  admin = 2 }
union Shape { circle = 1: Circle  label = 2: string }

schema User {
    id: u64
    name: string @len(1, 64)                 // constraints are checked on encode
    age: optional<u8> @range(0, 150)
    role: Role = "member"                    // defaults are written into the blob
    email: optional<string> @since(2)        // added in v2 → old data simply lacks it
}
schema Attendee { user: ref<User>  uid: ref<u64>  seat: nullable<u16> }   // refs to objects *and* to single fields
root User
```
Types: `bool u8…u64 i8…i64 f32 f64 string bytes any`, `array<T>`, `ref<T>`, `optional<T>`, `nullable<T>`, enums, unions, nested schemas, `import`.
Full language: [`specification/schema-language.md`](specification/schema-language.md).

## How the binary format works (short version)

* A 28-byte header (magic, schema id + version, root offset, length, optional CRC-32 trailer), then **fixed-layout structs** (presence bits, no per-object tables) and `u32` **absolute offsets**.
* A reference is a **byte location** of the target slot — `users[3].id` is literally an offset to those 8 bytes. No keys, no process addresses; blobs move freely between machines and languages.
* Readers validate every offset, length, alignment-free read, UTF-8 sequence and recursion depth; offsets must point *forward*, so offset cycles are impossible. Details: [`specification/binary-format.md`](specification/binary-format.md).
* Typical results (10,000 records): typed BJSONX is **33 % smaller than JSON**, one field is read in **~1 µs (Go) / ~0.01 ms (Python)** without decoding the rest, and 20,000 links to 200 shared objects take
  **173 KB vs 637 KB** as JSON with foreign keys. It is *larger* than Protobuf/Avro, and pure-Python parsing is ~30× slower than `json` — see [benchmarks](benchmarks/RESULTS.md) for the honest table.

## What is in the box

| | Python | JS / TS | Go | Rust |
|---|:-:|:-:|:-:|:-:|
| text parser + resolver (labels, pointers, `$prev/$next`, errors with line/col) | ✅ | ✅ | ✅ | ✅ |
| schema compiler (`.jxs` → `.jxsb`) | ✅ | loads `.jxsb` | loads `.jxsb` | ✅ |
| BJSONX encode (schema-less + typed) and validating decode | ✅ | ✅ | ✅ | ✅ |
| zero-copy views | ✅ | ✅ | ✅ | ✅ |
| typed mapping to your own types | – | `.d.mts`, `gen ts` | ✅ `Marshal/Unmarshal` | ✅ `#[derive(Bjsonx)]` |
| CLI (`jsonxc`) | ✅ | – | – | ✅ |

Everything is checked against shared vectors: ~390 files in [`tests/vectors/`](tests/vectors) (25 valid documents, 51 text and 17 encoding error cases with exact line/column, 26 malformed binaries, 34 schema-compiler cases,
version-compatibility blobs), plus mutation fuzzing, property tests, an HTTP service test and cross-language interop (`tests/interop.sh`). Run it all: `tests/run_all.sh`.

## Documentation

| | |
|---|---|
| [`specification/text-format.md`](specification/text-format.md) | grammar (EBNF), numbers, strings, identifiers, positions |
| [`specification/references.md`](specification/references.md) | pointers, labels, scopes, `$prev/$next` — with the design decisions explained |
| [`specification/schema-language.md`](specification/schema-language.md) | types, defaults, constraints, versions |
| [`specification/binary-format.md`](specification/binary-format.md) | byte layout, canonical writing order, reader security rules |
| [`specification/compatibility.md`](specification/compatibility.md) · [`errors.md`](specification/errors.md) · [`http.md`](specification/http.md) | evolution rules · every error code · `application/jsonx` over HTTP |
| [`specification/architecture.md`](specification/architecture.md) | alternatives considered, decisions, **known limitations** |
| [`examples/`](examples) | text samples, schemas, a working HTTP server/client |

## Project status

**v0.1.0 — usable, young.** The formats and semantics are specified and cross-tested, but nothing is published to crates.io / PyPI / npm yet. CI (35 jobs: Python 3.9–3.13, Node 18/20/22, Go and Rust on Linux/Windows/macOS, cross-language interop) is green.
Known gaps (all documented): no `map<K,V>`, no f16, no NaN/Infinity in text; `ref` fields are read-only in typed Go/Rust mappings; no JS typed mapping; pure-Python is slow;
FlatBuffers and Cap'n Proto were not benchmarked; blobs are limited to 4 GiB. The Rust derive and the Go mapping are interpreted against the compiled schema at run time, not code-generated.

## Repository layout

```
specification/      the source of truth
implementations/    python/  js/  go/  rust/ (workspace: jsonx, jsonx-derive, jsonxc)
compiler/jsonxc     Python CLI entry point (the Rust CLI lives in implementations/rust/jsonxc)
tests/              golden vectors + generator, per-language suites, interop, CLI parity, run_all.sh
examples/  benchmarks/  tools/  .github/ (ci, bench, release workflows)
```
The Python implementation is the reference used to *generate* the golden vectors; every other implementation must reproduce them exactly. Changing semantics means changing
the spec, the generator inputs and all four implementations — see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).
