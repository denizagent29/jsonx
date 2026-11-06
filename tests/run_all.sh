#!/usr/bin/env bash
# Runs every test suite. Requires python3 (>=3.9), node (>=18), go (>=1.22), cargo (>=1.75); tsc optional.
set -euo pipefail
R="$(cd "$(dirname "$0")/.." && pwd)"; cd "$R"
echo "== vector determinism"; before=$(find tests/vectors -type f | sort | xargs sha256sum | sha256sum); python3 tests/generate_vectors.py >/dev/null
after=$(find tests/vectors -type f | sort | xargs sha256sum | sha256sum); [ "$before" = "$after" ] && echo "vectors reproducible" || { echo "VECTORS CHANGED"; exit 1; }
echo "== python"; python3 tests/test_python.py
echo "== http";   python3 tests/test_http.py
echo "== js";     node tests/test_js.mjs
echo "== go";     (cd implementations/go && go test -count=1 ./... )
echo "== rust (workspace: jsonx, jsonx-derive, jsonxc)"; (cd implementations/rust && cargo test --release --workspace 2>&1 | grep -E "^test result|FAILED|error" )
echo "== rust cli parity"; tests/cli_parity.sh
echo "== interop"; tests/interop.sh
command -v tsc >/dev/null && { echo "== typescript definitions"; tsc --noEmit --strict --module nodenext --moduleResolution nodenext --target es2022 tests/typecheck.ts && echo ok; } || true
echo "ALL TESTS PASSED"
