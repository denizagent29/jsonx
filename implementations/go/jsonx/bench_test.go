package jsonx

import (
	"encoding/json"
	"os"
	"testing"
)

func benchData(b *testing.B) (jtext, xtext string, sc *Schema) {
	d := "../../../benchmarks/data/"
	j, e1 := os.ReadFile(d + "users.json")
	x, e2 := os.ReadFile(d + "users.jsonx")
	s, e3 := os.ReadFile(d + "users.jxsb")
	if e1 != nil || e2 != nil || e3 != nil {
		b.Skip("run benchmarks/gen_data.py and jsonxc compile first")
	}
	sc, _ = LoadSchema(s)
	return string(j), string(x), sc
}
func BenchmarkJSONUnmarshal(b *testing.B) {
	j, _, _ := benchData(b)
	b.SetBytes(int64(len(j)))
	for i := 0; i < b.N; i++ {
		var v interface{}
		json.Unmarshal([]byte(j), &v)
	}
}
func BenchmarkJSONXParse(b *testing.B) {
	_, x, _ := benchData(b)
	b.SetBytes(int64(len(x)))
	for i := 0; i < b.N; i++ {
		Parse(x)
	}
}
func BenchmarkEncodeTyped(b *testing.B) {
	_, x, sc := benchData(b)
	d, _ := Parse(x)
	for i := 0; i < b.N; i++ {
		Encode(d, sc, EncodeOptions{})
	}
}
func BenchmarkDecodeTyped(b *testing.B) {
	_, x, sc := benchData(b)
	d, _ := Parse(x)
	blob, _ := Encode(d, sc, EncodeOptions{})
	b.SetBytes(int64(len(blob)))
	for i := 0; i < b.N; i++ {
		Decode(blob, sc, ReadOptions{})
	}
}
func BenchmarkViewField(b *testing.B) {
	_, x, sc := benchData(b)
	d, _ := Parse(x)
	blob, _ := Encode(d, sc, EncodeOptions{})
	for i := 0; i < b.N; i++ {
		r, _ := View(blob, sc, ReadOptions{})
		us, _ := r.(*StructView).Get("users")
		u, _ := us.(*ArrayView).At(5000)
		u.(*StructView).Get("name")
	}
}
func BenchmarkJSONUnmarshalThenIndex(b *testing.B) {
	j, _, _ := benchData(b)
	for i := 0; i < b.N; i++ {
		var v map[string][]map[string]interface{}
		json.Unmarshal([]byte(j), &v)
		_ = v["users"][5000]["name"]
	}
}
