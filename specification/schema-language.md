# JSONX Schema Language (v0.1)

Files use `.jxs`; the compiler `jsonxc` turns them into the canonical binary schema (`.jxsb`, magic `BJXS`).

```jxs
namespace acme.school            // optional, part of the schema id
version 2                        // current schema version (default 1)
import "common.jxs"              // flattens imported types in (no namespaces yet)

enum Role : u8 { guest = 0  member = 1  admin = 2 }      // base u8|u16|u32, default: smallest that fits
union Shape { circle = 1: Circle  label = 2: string }    // name = tag(1..255) : type

schema User {                                              // also accepted: struct
    id: u64
    name: string @len(1, 64)
    age: optional<u8> @range(0, 150)
    role: Role = "member"
    nick: string = "anon" @since(2)
    email: optional<string> @since(2) @doc("free-form annotation")
}
root User                        // type used when a blob does not name another root
```
Fields are separated by whitespace, `,` or `;`.

## Types
`bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 string bytes any`, `array<T>`, `ref<T>`, enums, unions, schemas (nested objects).
* `any` – a dynamic JSONX value (null/bool/int/float/string/bytes/array/object/ref).
* `ref<T>` – a pointer to a **T-typed node elsewhere in the document** (`ref<User>`, `ref<u64>` = a pointer to a field).
* `optional<T>` – the key may be absent (text: absent or `null` both mean absent). `nullable<T>` – key required, value may be `null`. Both are only allowed as the **outermost** type of a field. Binary: one presence bit.
* Text forms: enum = member name string; union = object with exactly one key (the variant name); bytes = canonical base64 string; struct = object.
* Integers are range checked (`E_RANGE`); `f32` overflow is `E_RANGE`; ints are accepted for float fields.

## Defaults, constraints, attributes
* `= literal` (number, string, bool, `null` for nullable, `[]`, enum member name as string). Absent field ⇒ default is *written into the blob*. Optional fields cannot have defaults.
* `@range(lo, hi)` numeric; `@len(lo, hi)` string (code points) / bytes (bytes) / array (count); `_` = unbounded. Checked on **encode** (`E_CONSTRAINT`).
* `@since(n)`, `@deprecated`, and any custom `@name(args)` (kept verbatim in the binary schema; `jsonxc compile --describe` exposes them).

## Declaration order
Types may be used before they are declared. Enums and unions are processed before schemas, so a schema default may name an enum member of an enum declared later in the file.

## Compile-time checks
unknown/duplicate types, reserved names, `optional` nesting, `ref<ref<>>`, union variant types (no `any`, `ref`, `union`), enum collisions/overflow, default type checks, constraint applicability, empty schemas, and the evolution rules below. Struct recursion is allowed (via arrays/optional/ref).

## Versions and evolution (see compatibility.md)
Fields are **append-only**: `@since` must be non-decreasing in declaration order, ≤ `version`, and a field with `@since > 1` must be optional/nullable/have a default. `jsonxc compat old.jxs new.jxs` checks that NEW is a valid evolution of OLD.
