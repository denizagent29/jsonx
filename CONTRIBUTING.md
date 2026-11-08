# Contributing

Thanks for looking! A few things make changes land quickly.

## Setup
* Python via [uv](https://docs.astral.sh/uv/): `uv sync --project implementations/python` (dependency-free library; `--group bench` adds benchmark comparators).
* Node ≥ 18, Go ≥ 1.22, Rust ≥ 1.75 (MSRV; CI also tests stable). Nothing needs network access except fetching `syn/quote` for the Rust derive macro.
* Run everything: `tests/run_all.sh` (vectors reproducibility, all four test suites, HTTP test, cross-language interop, Rust↔Python CLI parity, TypeScript definitions).

## The one rule: the spec leads, vectors prove it
1. Behaviour is defined in `specification/`. If you change semantics, change the spec first.
2. `tests/generate_vectors.py` (inputs inline) produces the golden files from the Python reference implementation; commit the regenerated `tests/vectors/` (CI fails if they differ from a fresh generation).
3. Every implementation must reproduce the vectors **byte for byte** (binary output) and **code + line:column** (errors). Fix all four, not just one.
4. New error cases and malformed-binary cases are cheap and valuable — add them to the generator.

## Style
* Keep libraries dependency-free (the Rust derive crate is the only exception: `syn`, `quote`).
* Security-relevant decoder code: every offset/length must be validated before use; add a malformed-input vector for each new rule.
* Don't claim performance without a measurement from `benchmarks/` (and say where JSONX loses).

## Reporting issues
Please include the JSONX text or the blob (hex is fine), the schema, the implementation and version. For suspected decoder vulnerabilities see [SECURITY.md](SECURITY.md).
