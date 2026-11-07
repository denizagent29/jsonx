# Architecture Research and Decisions

## Compared designs (Phase A)
| format | model | refs/sharing | zero-copy | schema | notes for JSONX |
|---|---|---|---|---|---|
| JSON | text tree | none (copies) | no | optional | baseline; no comments/refs |
| MessagePack / CBOR / BSON | binary tree, self-describing | none (CBOR tags 28/29 optional) | no | none | compact, but must decode whole tree to read a field |
| Protocol Buffers | tag-length-value, schema | none | no (parse to objects) | required | very small & fast with C/upb; no random access, no sharing |
| Avro | schema-ordered, no tags | none | no | required at both ends | smallest for records; sequential |
| FlatBuffers | vtables + relative offsets | shared via offsets (DAG) | yes | required | closest to BJSONX typed mode |
| Cap'n Proto | pointer-based, aligned | yes | yes | required | alignment/pointers heavier than needed here |
(Public specifications were used as references; FlatBuffers and Cap'n Proto were **not benchmarked** – their compilers were unavailable in the build environment.)

## Chosen architecture
* Custom recursive-descent parser (no JSON dependency), AST with parent links, **separate resolver pass** (forward refs, static validation).
* One document model for text and binary: a node graph where references hold the target node.
* BJSONX: schema-driven fixed layouts with presence bits, absolute u32 offsets, byte-location pointers, forward-only offsets for safety, plus a built-in `any` type so schema-less documents use the same machinery.
* Runtime schema interpretation (compiled `.jxsb`, loaded once) in all four languages; `jsonxc gen` emits typed definitions + embedded schema.
  **Generated specialised codecs were not implemented**, so "generated vs generic" was not benchmarked; the measured numbers are for the interpreted codecs.

## Implemented beyond the core (v0.1)
* **Schema compiler in two languages**: Python (reference) and Rust (`jsonx::compile_str`, `jsonxc`); they agree byte-for-byte on `.jxsb` output and on schema error codes/positions (vectors `schema/ok`, `schema/errors`).
* **Rust `#[derive(Bjsonx)]`** (`jsonx-derive`): reads the schema at build time, type-checks the struct against it, embeds the canonical schema, generates `to_blob/from_blob/open/view` and `<Name>View` zero-copy accessors.
  Encoding is direct (`rt::Writer` + `BjEncode`, schema layout resolved at run time, no intermediate document; differential-tested against the generic encoder); decoding into structs reads only the needed fields.
  These are the only *specialised* paths; everything else interprets the compiled schema at run time.

## Known limitations (v0.1)
* Schema *text* compilers exist in Python and Rust only; Go and JS load the compiled `.jxsb`. Go/JS have no derive-style typed mapping: `jsonxc gen go|ts` emits type definitions + the embedded schema, decoding yields views or a generic document.
* The derive macro: no `any` fields; `ref<T>` fields decode (copy of the target) but cannot be encoded from a Rust value tree (`E_REF_TARGET`); Rust types must be named like the schema types; every derived type embeds its own copy of the schema bytes;
  unknown enum values from newer writers are an error in owned decode (views of raw documents keep the integer).
* Generated *specialised* codecs for Go/JS/Python, and the "generated vs interpreted" benchmark for them, do not exist.
* No `map<K,V>` type, no f16, no NaN/Infinity in text, imports are flattened (no namespacing), `ref` inside `any` can only target `any` slots, refs to non-struct union payloads are rejected.
* Python implementation is pure Python (slow for bulk work; see benchmarks); Go owned decode allocates heavily; Rust generic `encode` clones the document first.
* Blobs < 4 GiB; the decoder's expansion limits (see binary-format.md §5) can reject legitimate documents with extreme string sharing.
* Constraint ranges in Go/Rust are compared as float64 (exact for |values| < 2^53).
* GitHub Actions workflows are linted (`actionlint`) but have not run on GitHub; FlatBuffers/Cap'n Proto were not benchmarked.
