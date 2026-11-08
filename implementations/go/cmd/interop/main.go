package main

import (
	"fmt"
	"os"

	"github.com/denizagent29/jsonx/implementations/go/jsonx"
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
