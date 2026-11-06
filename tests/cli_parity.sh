#!/usr/bin/env bash
# The Rust `jsonxc` must produce exactly what the Python `jsonxc` produces: compiled schemas, encoded blobs, decoded dumps, generated code.
set -euo pipefail
R="$(cd "$(dirname "$0")/.." && pwd)"; V="$R/tests/vectors"; T="$(mktemp -d)"
( cd "$R/implementations/rust" && cargo build -q --release -p jsonxc )
RS="$R/implementations/rust/target/release/jsonxc"; PY="$R/compiler/jsonxc/jsonxc"; n=0
for f in "$V"/schema/ok/*.jxs "$V/schema/lesson.jxs" "$R/examples/schemas/shop.jxs" "$R/benchmarks/users.jxs"; do
  "$RS" compile "$f" -o "$T/rs.jxsb" 2>/dev/null; "$PY" compile "$f" -o "$T/py.jxsb" 2>/dev/null; cmp "$T/rs.jxsb" "$T/py.jxsb" || { echo "compile differs: $f"; exit 1; }; n=$((n+1))
  "$RS" compile "$f" --describe >/dev/null; for l in ts go rust python; do "$RS" gen "$f" --lang $l > "$T/rs.$l" ; "$PY" gen "$f" --lang $l > "$T/py.$l"; cmp "$T/rs.$l" "$T/py.$l" || { echo "gen $l differs: $f"; exit 1; }; n=$((n+1)); done
done
S="$V/schema/lesson.jxs"
for t in lesson lesson_minimal lesson_union_label lesson_many_users; do
  "$RS" encode "$V/typed/$t.jsonx" -s "$S" --crc -o "$T/rs.b"; "$PY" encode "$V/typed/$t.jsonx" -s "$S" --crc -o "$T/py.b"; cmp "$T/rs.b" "$T/py.b"; n=$((n+1))
  "$RS" decode "$T/py.b" -s "$S" > "$T/rs.d"; "$PY" decode "$T/py.b" -s "$S" > "$T/py.d"; cmp "$T/rs.d" "$T/py.d"; n=$((n+1))
  "$RS" decode "$T/py.b" -s "$S" --format json > "$T/rs.j"; "$PY" decode "$T/py.b" -s "$S" --format json > "$T/py.j"; python3 -c "import json,sys;assert json.load(open('$T/rs.j'))==json.load(open('$T/py.j'))"; n=$((n+1))
done
for f in "$V"/text/valid/*.jsonx; do [[ "$f" == *.crc.* ]] && continue; "$RS" encode "$f" -o "$T/rs.b"; "$PY" encode "$f" -o "$T/py.b"; cmp "$T/rs.b" "$T/py.b"; n=$((n+1)); done
for f in "$V"/text/errors/*.jsonx; do exp=$(cut -d' ' -f1 "${f%.jsonx}.err"); set +e; "$RS" validate "$f" 2>"$T/e"; rc=$?; set -e; [ $rc -eq 2 ] && grep -q "^$exp at" "$T/e" || { echo "validate error mismatch $f"; exit 1; }; n=$((n+1)); done
"$RS" compat "$V/schema/lesson_v1.jxs" "$S" >/dev/null; n=$((n+1))
echo "cli parity: $n comparisons identical (Rust jsonxc == Python jsonxc)"; rm -rf "$T"
