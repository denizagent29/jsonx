# Error Reference (v0.1)

Every implementation reports an error with: **code** (below), **category**, human-readable **message**, **line/column** (text
errors; 1-based, code points), and where useful *expected/actual*. Python: `JsonxError(code, message, line, col, expected, actual, hint)`;
JS: `JsonxError`; Go: `*jsonx.Error`; Rust: `jsonx::Error`. Codes and positions are identical across implementations
(enforced by `tests/vectors/**/*.err`); message wording may differ. CLI exit codes: 0 ok, 1 usage/IO, 2 invalid text/schema/data, 3 malformed binary.

| category | codes |
|---|---|
| syntax | `E_SYNTAX` `E_UNEXPECTED_EOF` `E_INVALID_ESCAPE` `E_INVALID_UTF8` `E_DEPTH_LIMIT` `E_DUPLICATE_KEY` |
| number | `E_INVALID_NUMBER` `E_NUMBER_OVERFLOW` |
| label | `E_INVALID_LABEL_NAME` `E_RESERVED_LABEL` `E_DUPLICATE_LABEL` `E_UNDEFINED_LABEL` `E_LABEL_OUT_OF_SCOPE` `E_AMBIGUOUS_LABEL` `E_AMBIGUOUS_NAME` |
| reference | `E_INVALID_POINTER` `E_POINTER_NO_FIELD` `E_POINTER_TYPE` `E_INVALID_INDEX` `E_INVALID_PARENT` `E_INVALID_CURRENT` `E_CYCLIC_REFERENCE` |
| context | `E_INVALID_PREV` `E_INVALID_NEXT` |
| schema | `E_SCHEMA_SYNTAX` `E_SCHEMA_UNKNOWN_TYPE` `E_SCHEMA_DUPLICATE` `E_SCHEMA_INVALID` `E_SCHEMA_BINARY` `E_SCHEMA_IMPORT` |
| type (encoding) | `E_TYPE_MISMATCH` `E_MISSING_FIELD` `E_UNKNOWN_FIELD` `E_RANGE` `E_CONSTRAINT` `E_REF_TARGET` |
| binary | `E_BIN_MAGIC` `E_BIN_VERSION` `E_BIN_TRUNCATED` `E_BIN_BOUNDS` `E_BIN_CHECKSUM` `E_BIN_SCHEMA_MISMATCH` `E_BIN_INVALID` `E_BIN_UTF8` `E_BIN_LIMIT` `E_BIN_INVALID_REF` |

Examples (actual messages from the Python reference):
```
E_INVALID_PREV at 1:24: '$prev' used on the first element of array '/users': there is no previous element
E_LABEL_OUT_OF_SCOPE at 1:45: label 'mylable' (declared at 1:14) is not visible from here: local labels are only visible inside the object that declares them
  hint: reach it explicitly with a path such as @users.0.$mylable, or make it global with an uppercase name
E_RANGE at 1:78: 300 does not fit in u8 (0..255)
```
