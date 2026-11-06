#!/usr/bin/env bash
# Runs every benchmark and writes benchmarks/results/*.md plus a combined RESULTS.md. Needs the comparison libraries
# for Python (pip install msgpack cbor2 pymongo fastavro grpcio-tools). Env: BENCH_N (records, default 10000), BENCH_REPS.
set -euo pipefail
R="$(cd "$(dirname "$0")/.." && pwd)"; cd "$R"; O=benchmarks/results; mkdir -p "$O"
python3 benchmarks/gen_data.py
python3 compiler/jsonxc/jsonxc compile benchmarks/users.jxs -o benchmarks/data/users.jxsb 2>/dev/null
python3 benchmarks/bench.py > "$O/python.md"
node benchmarks/bench.mjs > "$O/node.md"
( cd implementations/go && go test -run xxx -bench . -benchtime 1s ./jsonx | grep -E "^(Benchmark|cpu|goos|goarch)" ) > "$O/go.txt"
( cd implementations/rust && cargo run -q --release -p jsonx --example bench | grep "^|" ) > "$O/rust.md"
( cd implementations/rust && cargo run -q --release -p jsonx-derive --example derive_bench | grep "^|" ) > "$O/rust-derive.md"
{ echo "# Benchmark run $(date -u +%FT%TZ)"; echo; echo "Runner: $(uname -sm), $(nproc) CPU(s). Shared hardware: compare ratios inside this run only."
  echo; echo "## Python"; cat "$O/python.md"; echo; echo "## Node.js"; cat "$O/node.md"; echo; echo '## Go'; echo '```'; cat "$O/go.txt"; echo '```'
  echo; echo "## Rust (generic API)"; cat "$O/rust.md"; echo; echo "## Rust (derive)"; cat "$O/rust-derive.md"; } > "$O/RESULTS.md"
echo "wrote $O/RESULTS.md"
