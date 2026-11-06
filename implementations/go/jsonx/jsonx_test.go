package jsonx

import (
	"bytes"
	"fmt"
	"math/rand"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

const V = "../../../tests/vectors/"

func rd(t *testing.T, p string) string {
	b, e := os.ReadFile(V + p)
	if e != nil {
		t.Fatal(e)
	}
	return string(b)
}
func names(t *testing.T, dir, ext string) []string {
	m, _ := filepath.Glob(V + dir + "/*" + ext)
	var out []string
	for _, f := range m {
		out = append(out, strings.TrimSuffix(filepath.Base(f), ext))
	}
	sort.Strings(out)
	if len(out) == 0 {
		t.Fatal("no vectors in " + dir)
	}
	return out
}
func code(err error) string {
	if err == nil {
		return "OK"
	}
	e := err.(*Error)
	if e.Line > 0 {
		return fmt.Sprintf("%s %d:%d", e.Code, e.Line, e.Col)
	}
	return e.Code
}

func TestVectors(t *testing.T) {
	n := 0
	chk := func(c bool, m string) {
		n++
		if !c {
			t.Errorf("FAIL %s", m)
		}
	}
	for _, nm := range names(t, "text/valid", ".jsonx") {
		d, err := Parse(rd(t, "text/valid/"+nm+".jsonx"))
		if err != nil {
			t.Errorf("%s: %v", nm, err)
			continue
		}
		chk(d.Dump() == rd(t, "text/valid/"+nm+".dump"), nm+" dump")
		b, err := Encode(d, nil, EncodeOptions{})
		chk(err == nil && string(b) == rd(t, "text/valid/"+nm+".bjsonx"), nm+" golden bytes")
		d2, err := Decode(b, nil, ReadOptions{})
		chk(err == nil && d2.Dump() == d.Dump(), nm+" roundtrip")
		bc, _ := Encode(d, nil, EncodeOptions{Checksum: true})
		chk(string(bc) == rd(t, "text/valid/"+nm+".crc.bjsonx"), nm+" crc")
	}
	for _, nm := range names(t, "text/errors", ".jsonx") {
		_, err := Parse(rd(t, "text/errors/"+nm+".jsonx"))
		chk(code(err) == strings.TrimSpace(rd(t, "text/errors/"+nm+".err")), "error "+nm+" got "+code(err))
	}
	sc, err := LoadSchema([]byte(rd(t, "schema/lesson.jxsb")))
	if err != nil {
		t.Fatal(err)
	}
	sc1, _ := LoadSchema([]byte(rd(t, "schema/lesson_v1.jxsb")))
	chk(strings.Contains(rd(t, "schema/ids.txt"), fmt.Sprintf("lesson_v2_id=%016x", sc.ID())), "schema id")
	for _, nm := range strings.Fields(rd(t, "typed/manifest.txt")) {
		d, err := Parse(rd(t, "typed/"+nm+".jsonx"))
		if err != nil {
			t.Fatal(err)
		}
		b, err := Encode(d, sc, EncodeOptions{})
		chk(err == nil && string(b) == rd(t, "typed/"+nm+".bjsonx"), nm+" typed golden")
		d2, err := Decode(b, sc, ReadOptions{})
		chk(err == nil && d2.Dump() == rd(t, "typed/"+nm+".dump"), nm+" typed dump")
		bc, _ := Encode(d, sc, EncodeOptions{Checksum: true})
		chk(string(bc) == rd(t, "typed/"+nm+".crc.bjsonx"), nm+" typed crc")
		b3, err := Encode(d2, sc, EncodeOptions{})
		chk(err == nil && bytes.Equal(b3, b), nm+" re-encode identical")
	}
	for _, nm := range names(t, "typed/errors", ".jsonx") {
		d, err := Parse(rd(t, "typed/errors/"+nm+".jsonx"))
		if err == nil {
			_, err = Encode(d, sc, EncodeOptions{})
		}
		chk(code(err) == strings.TrimSpace(rd(t, "typed/errors/"+nm+".err")), "typed error "+nm+" got "+code(err))
	}
	d, err := Decode([]byte(rd(t, "compat/v1_data.bjsonx")), sc, ReadOptions{})
	chk(err == nil && d.Dump() == rd(t, "compat/v1_data_read_by_v2.dump"), "v1 data / v2 reader")
	d, err = Decode([]byte(rd(t, "compat/v2_data.bjsonx")), sc1, ReadOptions{})
	chk(err == nil && d.Dump() == rd(t, "compat/v2_data_read_by_v1.dump"), "v2 data / v1 reader")
	for _, nm := range names(t, "binary/malformed", ".bin") {
		s := (*Schema)(nil)
		if strings.HasPrefix(nm, "typed") {
			s = sc
		}
		_, err := Decode([]byte(rd(t, "binary/malformed/"+nm+".bin")), s, ReadOptions{})
		chk(strings.Fields(code(err))[0] == strings.TrimSpace(rd(t, "binary/malformed/"+nm+".err")), "malformed "+nm+" got "+code(err))
	}
	{ // views and aliasing
		raw := []byte(rd(t, "typed/lesson.bjsonx"))
		v, err := View(raw, sc, ReadOptions{})
		chk(err == nil, "view")
		sv := v.(*StructView)
		us, _ := sv.Get("attendees")
		a0, _ := us.(*ArrayView).At(0)
		u, _ := a0.(*StructView).Get("user")
		nme, _ := u.(*StructView).Get("name")
		chk(nme == "Adil", "view through ref")
		dd, _ := Decode(raw, sc, ReadOptions{})
		chk(dd.Root.Get("attendees").At(0).Get("user") == dd.Root.Get("users").At(1), "alias identity")
	}
	rng := rand.New(rand.NewSource(7))
	var seeds [][]byte
	var ss []*Schema
	for _, p := range []string{"typed/lesson.bjsonx", "typed/lesson_many_users.bjsonx"} {
		seeds = append(seeds, []byte(rd(t, p)))
		ss = append(ss, sc)
	}
	for _, p := range []string{"text/valid/many_refs.bjsonx", "text/valid/scalars.bjsonx"} {
		seeds = append(seeds, []byte(rd(t, p)))
		ss = append(ss, nil)
	}
	for it := 0; it < 5000; it++ {
		k := rng.Intn(len(seeds))
		m := append([]byte(nil), seeds[k]...)
		for j := 0; j <= rng.Intn(4); j++ {
			m[rng.Intn(len(m))] = byte(rng.Intn(256))
		}
		if rng.Intn(5) == 0 {
			m = m[:rng.Intn(len(m))]
		}
		Decode(m, ss[k], ReadOptions{}) // must not panic with a foreign panic
		n++
	}
	t.Logf("go: %d checks", n)
}
