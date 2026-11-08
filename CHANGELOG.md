# Changelog

## 0.1.0 — first public version
* JSONX text format: comments, unquoted/Unicode keys, references (`@a.b`, `@!x`, `@.!.N`, `@..Global.$local`), labels (local/global/variable), `$prev`/`$next`, static validation with line/column errors.
* Schema language (`.jxs`): enums, unions, defaults, constraints, `@since` evolution, imports; canonical binary schema (`.jxsb`).
* BJSONX v1: schema-less and typed layouts, byte-location pointers, presence bits, CRC-32 trailer, hardened zero-copy readers.
* Implementations: Python (reference), JavaScript/TypeScript, Go (+ `Marshal`/`Unmarshal`), Rust (+ schema compiler, `#[derive(Bjsonx)]`, `jsonxc` CLI).
* ~460 shared test vectors, fuzzing, cross-language byte-identity and CLI-parity tests, benchmarks, HTTP example, CI/bench/release workflows.
