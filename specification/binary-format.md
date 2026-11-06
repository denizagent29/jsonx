# BJSONX Binary Format, version 1

Media type `application/bjsonx`, extension `.bjsonx`. All multi-byte integers are **little-endian**; there are **no
alignment requirements** (readers use unaligned loads); offsets are **absolute `u32` byte positions from the start
of the blob** (so blobs are limited to < 4 GiB; encoders fail with `E_BIN_LIMIT`). No process addresses appear anywhere.
The golden files in `tests/vectors/` are produced by the reference writer and every implementation reproduces
them **byte for byte**.

## 1. Header (28 bytes)
| off | size | field |
|---|---|---|
| 0 | 3 | magic `42 4A 58` ("BJX") |
| 3 | 1 | format version = 1 |
| 4 | 2 | flags: bit0 = CRC-32 trailer present; all other bits must be 0 |
| 6 | 2 | reserved = 0 |
| 8 | 8 | schema id (FNV-1a-64 of `namespace + "\0" + root struct name`); 0 = dynamic (schema-less) |
| 16 | 4 | schema version of the writer (0 for dynamic) |
| 20 | 4 | root offset (≥ 28) – a struct instance (typed) or an `any` slot (dynamic) |
| 24 | 4 | total length in bytes, including the trailer |

With flag bit0 the last 4 bytes are the CRC-32 (IEEE) of everything before them. The length field makes concatenated
blobs self-framing for streaming (`blob_length(first 28 bytes)`); a single blob must have no trailing bytes.
The schema id identifies the *schema family*; compatibility between versions is via the version field (see compatibility.md).

## 2. Slots
A **slot** is the fixed-size representation of a value inside its parent:

| type | slot |
|---|---|
| bool, u8/i8, u16/i16, u32/i32/f32, u64/i64/f64 | 1,1,2,4,8 bytes inline (bool must be 0/1) |
| enum | 1/2/4 bytes (its base) |
| string, bytes | u32 offset → `varint len, bytes` (strings are UTF-8, validated) |
| array<T> | u32 offset → array block |
| struct | u32 offset → struct instance |
| ref<T> | u32 **location** of the target (below) |
| union | u8 tag (1..255) + u32 offset → payload |
| any | u8 tag + u32 payload (5 bytes) |

`varint` = LEB128, at most 5 bytes, value < 2^32, minimal encoding required.
**Array block**: `varint count`, then (only if the element is a struct) `varint stride`, then `count` elements in slot
form; struct elements are stored *inline*, `stride` bytes apart. **Union payload**: the "by-offset" form of the variant
type: string/bytes → `varint len, bytes`; struct → instance; array → array block; scalar/enum → raw bytes.
Variants may not be `any`, `ref` or `union`.

### Struct instance and layout
Fields are laid out in declaration order, considering only fields with `since ≤ layout version`. For each optional/nullable
field with ordinal j (counting optional/nullable fields): if `j % 8 == 0` a **presence byte** is allocated *at that point*
(1 byte), bit `j % 8` = 1 means present/non-null. Then the field's slot. Struct size = end offset (must be ≥ 1). Because a
presence byte is placed where it is first needed, the layout of any prefix of the field list never changes when fields are appended.
Absent/null optional values leave their slot zeroed.

### `any` tags
`0 null, 1 false, 2 true` (payload must be 0) · `3 i32` (payload = the value) · `4 i64`, `5 u64`, `6 f64` (payload → 8 bytes) ·
`7 string` (→ varint+bytes) · `8 array` (→ `varint n` + n×5-byte slots) · `9 object` (→ `varint n` + n×[u32 key offset + 5-byte slot];
keys keep text order, unique) · `10 ref` (payload = location of a non-ref `any` slot) · `11 bytes`.
Integers use the smallest of i32 / i64 / u64; floats are always f64. Object **keys are deduplicated** document-wide.

## 3. Pointers (references)
`ref` payloads/slots hold a **byte location**, never a key. The location of a node is: for a struct instance, the first byte
of the instance; for everything else, the first byte of its slot (a field slot, array element, `any` slot…).
`ref<users[3].id>` is therefore the absolute offset of that u64. Rules:
* A target must be a node of exactly the declared type (`ref<T>`: type-identical; in `any` context: an `any` slot).
* Reference chains are collapsed by the writer; a ref slot may not target another ref (`E_BIN_INVALID_REF`).
* Writers fail with `E_REF_TARGET` if a target has no addressable location (e.g. a non-struct union payload) or the type differs.
* Shared objects are expressed with refs; the physical layout stays a tree, so cycles can only arise through refs, never
  through offsets, and decoders never follow refs implicitly.

## 4. Canonical writing order (needed for byte-identical output)
Pre-order, depth-first: allocate the root block at offset 28 (typed: struct instance; dynamic: the 5-byte root slot, then children).
For a block: allocate its fixed area, then process children **in field/element/entry order**, each child allocating its own
block at the current end of the buffer. In objects, a key string is allocated (if new) *before* its value is processed.
Refs are patched after everything is written. Padding is never inserted. Strings/values are not deduplicated except object keys.

## 5. Reader requirements (security)
Every access is bounds-checked against `[28, end)` (overflow-safe; no unchecked indexing). Additionally:
* **Forward rule**: every non-ref, non-key offset must be `>` the start of the block containing its slot (no backward or self
  pointers ⇒ no offset cycles; recursion depth is bounded: `E_BIN_LIMIT` beyond 256).
* A location may be used by one value only; the owned decoder rejects shared/overlapping *typed* locations (`E_BIN_INVALID`).
* Decoded node count ≤ blob length; decoded string+bytes volume ≤ max(1 MiB, 256 × blob length) (anti-amplification, `E_BIN_LIMIT`).
* Counts × strides are overflow-checked against the payload; array stride ≥ the reader's known struct size.
* Strict UTF-8; bool ∈ {0,1}; unknown any-tags / union tags / reserved header bits / non-zero constant payloads are errors; unknown *enum* values are preserved as integers; CRC verified when present.
* Typed blobs need a schema whose id matches; an older reader accepts newer blobs (unknown trailing fields ignored).
Two read modes: **views** (zero-copy, lazy, per-access checks; Rust `Val<'b,'a>` borrows the buffer, Python/JS/Go slices) and
**owned decode** (validates the whole blob and builds a document graph preserving aliasing).

## 6. Design rationale and alternatives (measured where noted)
* *Absolute vs relative offsets*: relative offsets make sub-blobs relocatable and are slightly smaller with varints, but cost a signed add on every
  hop and complicate the forward rule; absolute `u32` was chosen for simplicity and bounds checking. **Not benchmarked** – analytical choice.
* *Alignment*: none, for compactness and portability (JS needs `DataView`; Python `struct.unpack_from`).
* *Tree vs flat tables / vtables (FlatBuffers)*: fixed layout per schema version + presence bits avoids per-object vtables (smaller for uniform records, see benchmarks).
* *Dynamic mode costs*: 9 bytes per object entry and 5 bytes per value slot (+8 for floats/big ints) – larger than JSON for record-heavy data (measured +5% on dataset A). Typed mode is 33% smaller than JSON there.
  Optimisation opportunities (not implemented): inline f64, varint-packed any slots, key-id tables.
