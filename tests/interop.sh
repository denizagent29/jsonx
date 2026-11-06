#!/usr/bin/env bash
# Cross-language interop: each implementation encodes the same text; every other implementation must decode it
# to the same canonical dump. (Golden-byte identity is also asserted by each language's own test-suite.)
set -euo pipefail
R="$(cd "$(dirname "$0")/.." && pwd)"; T="$(mktemp -d)"; V="$R/tests/vectors"
SRC="$V/typed/lesson.jsonx"; SCB="$V/schema/lesson.jxsb"
python3 - "$R" "$SRC" "$SCB" "$T" <<'PY'
import sys; r,src,scb,t=sys.argv[1:]; sys.path.insert(0,r+"/implementations/python"); import jsonx
sc=jsonx.load_schema(open(scb,"rb").read()); d=jsonx.parse(open(src).read())
open(t+"/py.bjsonx","wb").write(jsonx.encode(d,sc)); open(t+"/py.dyn","wb").write(jsonx.encode(d))
PY
cat > "$T/js.mjs" <<JS
import fs from "node:fs"; import * as J from "$R/implementations/js/src/jsonx.mjs";
const sc=J.loadSchema(new Uint8Array(fs.readFileSync("$SCB"))), d=J.parse(fs.readFileSync("$SRC","utf8"));
fs.writeFileSync("$T/js.bjsonx",J.encode(d,sc)); fs.writeFileSync("$T/js.dyn",J.encode(d));
const dump=(f,s)=>J.decode(new Uint8Array(fs.readFileSync(f)),s).dump();
fs.writeFileSync("$T/js_reads_py.txt",dump("$T/py.bjsonx",sc)+"\n"+dump("$T/py.dyn",null));
JS
node "$T/js.mjs"
( cd "$R/implementations/go" && mkdir -p cmd/interop && cat > cmd/interop/main.go <<GO
package main

import (
	"fmt"
	"os"

	"github.com/jsonx/jsonx/implementations/go/jsonx"
)

func must(e error) {
	if e != nil {
		panic(e)
	}
}
func main() {
	scb, _ := os.ReadFile(os.Args[1])
	sc, e := jsonx.LoadSchema(scb)
	must(e)
	src, _ := os.ReadFile(os.Args[2])
	d, e := jsonx.Parse(string(src))
	must(e)
	t := os.Args[3]
	b, e := jsonx.Encode(d, sc, jsonx.EncodeOptions{})
	must(e)
	os.WriteFile(t+"/go.bjsonx", b, 0o644)
	b2, e := jsonx.Encode(d, nil, jsonx.EncodeOptions{})
	must(e)
	os.WriteFile(t+"/go.dyn", b2, 0o644)
	pb, _ := os.ReadFile(t + "/py.bjsonx")
	pd, _ := os.ReadFile(t + "/py.dyn")
	x, e := jsonx.Decode(pb, sc, jsonx.ReadOptions{})
	must(e)
	y, e := jsonx.Decode(pd, nil, jsonx.ReadOptions{})
	must(e)
	os.WriteFile(t+"/go_reads_py.txt", []byte(x.Dump()+"\n"+y.Dump()), 0o644)
	fmt.Print("")
}
GO
go run ./cmd/interop "$SCB" "$SRC" "$T" )
( cd "$R/implementations/rust/jsonx" && mkdir -p examples && cat > examples/interop.rs <<RS
use jsonx::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = Schema::from_bytes(&std::fs::read(&a[1]).unwrap()).unwrap();
    let d = parse(&std::fs::read_to_string(&a[2]).unwrap()).unwrap();
    let (o, ro, t) = (EncodeOptions::default(), ReadOptions::default(), &a[3]);
    std::fs::write(format!("{}/rs.bjsonx", t), encode(&d, Some(&sc), &o).unwrap()).unwrap();
    std::fs::write(format!("{}/rs.dyn", t), encode(&d, None, &o).unwrap()).unwrap();
    let x = decode(&std::fs::read(format!("{}/py.bjsonx", t)).unwrap(), Some(&sc), &ro).unwrap();
    let y = decode(&std::fs::read(format!("{}/py.dyn", t)).unwrap(), None, &ro).unwrap();
    std::fs::write(format!("{}/rs_reads_py.txt", t), format!("{}\n{}", x.dump(), y.dump())).unwrap();
}
RS
cargo run -q --release --example interop -- "$SCB" "$SRC" "$T" )
python3 - "$R" "$T" "$SCB" <<'PY'
import sys; r,t,scb=sys.argv[1:]; sys.path.insert(0,r+"/implementations/python"); import jsonx
sc=jsonx.load_schema(open(scb,"rb").read()); rd=lambda f: open(f,"rb").read()
want=jsonx.decode(rd(t+"/py.bjsonx"),sc).dump()+"\n"+jsonx.decode(rd(t+"/py.dyn")).dump()
for lang in ("js","go","rs"):
    assert rd(f"{t}/{lang}.bjsonx")==rd(t+"/py.bjsonx"), lang+" typed bytes differ from Python"
    assert rd(f"{t}/{lang}.dyn")==rd(t+"/py.dyn"), lang+" dynamic bytes differ from Python"
    assert open(f"{t}/{lang}_reads_py.txt").read()==want, lang+" read Python blob differently"
    assert jsonx.decode(rd(f"{t}/{lang}.bjsonx"),sc).dump()+"\n"+jsonx.decode(rd(f"{t}/{lang}.dyn")).dump()==want, "Python read "+lang+" blob differently"
print("interop: Python<->JS<->Go<->Rust typed+dynamic blobs identical and mutually readable")
PY
rm -rf "$T"
