# Compatibility and Versioning (v0.1)

## Format versions
Byte 3 of a blob is the **format version** (1). Readers reject other values (`E_BIN_VERSION`). Reserved header bits must be zero so
that a future version can only add features under a new format version.

## Schema evolution (typed blobs)
The header carries the writer's **schema version** `w`. The layout used for a struct is "all fields with `since ≤ w`" (see binary-format.md §2).
Rules enforced by `jsonxc` (and checked between two files by `jsonxc compat old new`):
1. Add fields only at the end, with `@since(new version)`; they must be optional, nullable or have a default.
2. Never remove or reorder fields (mark `@deprecated` – the slot stays). Never change a field's type or its optional/nullable status.
3. Enum members: add only; never change a value. Union variants: add only; never change a tag.
Behaviour:
* **Backward** (new reader, old data): fields with `since > w` are absent in the blob; readers return the default (or null). Verified by `compat/v1_data*` vectors in every language.
* **Forward** (old reader, new data): the reader uses its own layout (a prefix of the writer's); arrays of structs carry their writer's *stride*, so
  unknown trailing fields are skipped; unknown enum values surface as integers; an unknown union tag fails only when that value is accessed
  (view) or decoded (`E_BIN_INVALID`). Verified by `compat/v2_data*` vectors, including strided array traversal.
* An encoder can target an older layout (`version=` / `--version`) to feed old readers; the header then carries that version.
* Schema identity = `namespace + root name`; two unrelated schemas with the same names would collide (use distinct namespaces). Exact structural
  fingerprints are available via `jsonxc compile --describe` (`fingerprint`) for registries that want to pin them.

## Text format
`.jsonx` has no version marker; v0.1 grammar additions would be backward compatible only if they do not change the meaning of existing valid documents.
