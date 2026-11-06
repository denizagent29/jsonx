package jsonx

import (
	"encoding/binary"
	"math"
	"math/big"
	"strconv"
	"strings"
)

// Type is a schema type descriptor.
type Type struct {
	Kind string
	Idx  int
	Elem *Type
	k    string
}

func (t *Type) key() string {
	if t.k == "" {
		t.k = t.computeKey()
	}
	return t.k
}
func (t *Type) computeKey() string {
	switch t.Kind {
	case "struct", "enum", "union":
		return t.Kind + ":" + strconv.Itoa(t.Idx)
	case "array", "ref":
		return t.Kind + "<" + t.Elem.key() + ">"
	}
	return t.Kind
}

var anyT = &Type{Kind: "any"}

type Default struct {
	Kind string
	Int  *big.Int
	F    float64
	S    string
	B    bool
}
type Bound struct{ Lo, Hi *float64 }
type Field struct {
	Name               string
	Type               *Type
	Optional, Nullable bool
	Since              int
	Default            *Default
	Cons               map[string]Bound
}
type EnumVal struct {
	Name string
	V    int
}
type Variant struct {
	Name string
	Tag  int
	T    *Type
}
type TypeDef struct {
	Kind, Name string
	Fields     []*Field
	By         map[string]*Field
	Base       int
	Values     []EnumVal
	Variants   []Variant
}
type fieldLayout struct {
	F     *Field
	Off   int
	PByte int // -1 if none
	Bit   int
}
type layout struct {
	Lay  []fieldLayout
	Size int
}

// Schema is a compiled (canonical binary) schema.
type Schema struct {
	Namespace string
	Version   int
	Root      int // -1 if none
	Types     []*TypeDef
	lay       map[[2]int]*layout
}

func FNV1a64(b []byte) uint64 {
	h := uint64(0xcbf29ce484222325)
	for _, x := range b {
		h = (h ^ uint64(x)) * 0x100000001b3
	}
	return h
}

func (s *Schema) idFor(name string) uint64 { return FNV1a64([]byte(s.Namespace + "\x00" + name)) }
func (s *Schema) ID() uint64 {
	if s.Root < 0 {
		return 0
	}
	return s.idFor(s.Types[s.Root].Name)
}
func (s *Schema) structIndex(name string) int {
	for i, t := range s.Types {
		if t.Kind == "struct" && t.Name == name {
			return i
		}
	}
	fail("E_SCHEMA_UNKNOWN_TYPE", "no schema named '"+name+"'")
	return -1
}
func (s *Schema) typeName(t *Type) string {
	switch t.Kind {
	case "struct", "enum", "union":
		return s.Types[t.Idx].Name
	case "array", "ref":
		return t.Kind + "<" + s.typeName(t.Elem) + ">"
	}
	return t.Kind
}

var scalarSize = map[string]int{"bool": 1, "u8": 1, "i8": 1, "u16": 2, "i16": 2, "u32": 4, "i32": 4, "f32": 4, "u64": 8, "i64": 8, "f64": 8}

func (s *Schema) slotSize(t *Type) int {
	if n, ok := scalarSize[t.Kind]; ok {
		return n
	}
	switch t.Kind {
	case "enum":
		return s.Types[t.Idx].Base
	case "string", "bytes", "array", "struct", "ref":
		return 4
	}
	return 5
}

func (s *Schema) layoutOf(idx, ver int) *layout {
	k := [2]int{idx, ver}
	if l, ok := s.lay[k]; ok {
		return l
	}
	l := &layout{}
	off, nopt := 0, 0
	var pres []int
	for _, f := range s.Types[idx].Fields {
		if f.Since > ver {
			continue
		}
		pb, bit := -1, 0
		if f.Optional || f.Nullable {
			if nopt%8 == 0 {
				pres = append(pres, off)
				off++
			}
			pb, bit = pres[nopt/8], nopt%8
			nopt++
		}
		l.Lay = append(l.Lay, fieldLayout{f, off, pb, bit})
		off += s.slotSize(f.Type)
	}
	l.Size = off
	s.lay[k] = l
	return l
}

var codeT = map[byte]string{1: "bool", 2: "u8", 3: "u16", 4: "u32", 5: "u64", 6: "i8", 7: "i16", 8: "i32", 9: "i64", 10: "f32", 11: "f64", 12: "string", 13: "bytes", 14: "any"}

// LoadSchema parses the canonical binary schema (BJXS) produced by jsonxc.
func LoadSchema(d []byte) (sc *Schema, err error) {
	defer catch(&err)
	p := 0
	bad := func(m string) { fail("E_SCHEMA_BINARY", m) }
	u8 := func() byte {
		if p >= len(d) {
			bad("truncated schema")
		}
		p++
		return d[p-1]
	}
	vi := func() uint64 {
		var v uint64
		var sh uint
		for i := 0; i < 10; i++ {
			b := u8()
			v |= uint64(b&0x7f) << sh
			sh += 7
			if b&0x80 == 0 {
				return v
			}
		}
		bad("varint too long")
		return 0
	}
	vn := func() int { return int(vi()) }
	st := func() string {
		n := vn()
		if n < 0 || p+n > len(d) {
			bad("truncated string")
		}
		s := string(d[p : p+n])
		p += n
		return s
	}
	var td func(dp int) *Type
	td = func(dp int) *Type {
		if dp > 32 {
			bad("type nesting too deep")
		}
		c := u8()
		if k, ok := codeT[c]; ok {
			return &Type{Kind: k}
		}
		switch c {
		case 20:
			return &Type{Kind: "struct", Idx: vn()}
		case 21:
			return &Type{Kind: "enum", Idx: vn()}
		case 22:
			return &Type{Kind: "union", Idx: vn()}
		case 23:
			return &Type{Kind: "array", Elem: td(dp + 1)}
		case 24:
			return &Type{Kind: "ref", Elem: td(dp + 1)}
		}
		bad("unknown type code")
		return nil
	}
	if len(d) < 5 || string(d[:4]) != "BJXS" || d[4] != 1 {
		bad("not a BJXS v1 schema")
	}
	p = 5
	s := &Schema{lay: map[[2]int]*layout{}}
	s.Namespace = st()
	s.Version = vn()
	s.Root = vn() - 1
	n := vn()
	if n > 100000 {
		bad("too many types")
	}
	for i := 0; i < n; i++ {
		k := u8()
		name := st()
		t := &TypeDef{Name: name}
		switch k {
		case 1:
			t.Kind = "struct"
			t.By = map[string]*Field{}
			nf := vn()
			for j := 0; j < nf; j++ {
				f := &Field{Name: st(), Type: td(0), Cons: map[string]Bound{}}
				fl := u8()
				f.Optional, f.Nullable = fl&1 != 0, fl&2 != 0
				f.Since = vn()
				if fl&8 != 0 {
					c := u8()
					switch c {
					case 0:
						f.Default = &Default{Kind: "null"}
					case 1, 2:
						f.Default = &Default{Kind: "bool", B: c == 2}
					case 3:
						z := vi()
						f.Default = &Default{Kind: "int", Int: new(big.Int).SetInt64(int64(z>>1) ^ -int64(z&1))}
					case 4:
						f.Default = &Default{Kind: "int", Int: new(big.Int).SetUint64(vi())}
					case 5:
						if p+8 > len(d) {
							bad("truncated default")
						}
						f.Default = &Default{Kind: "float", F: math.Float64frombits(binary.LittleEndian.Uint64(d[p:]))}
						p += 8
					case 6:
						f.Default = &Default{Kind: "string", S: st()}
					case 7:
						f.Default = &Default{Kind: "array"}
					default:
						bad("bad default tag")
					}
				}
				na := vn()
				for a := 0; a < na; a++ {
					an, aa := st(), st()
					if an == "range" || an == "len" {
						q := strings.Split(aa, ",")
						if len(q) != 2 {
							bad("bad constraint")
						}
						var b Bound
						for qi, x := range q {
							x = strings.TrimSpace(x)
							if x == "_" {
								continue
							}
							v, e := strconv.ParseFloat(x, 64)
							if e != nil {
								bad("bad constraint number")
							}
							if qi == 0 {
								b.Lo = &v
							} else {
								b.Hi = &v
							}
						}
						f.Cons[an] = b
					}
				}
				t.Fields = append(t.Fields, f)
				t.By[f.Name] = f
			}
		case 2:
			t.Kind = "enum"
			b := u8()
			if b != 1 && b != 2 && b != 4 {
				bad("bad enum base")
			}
			t.Base = int(b)
			nv := vn()
			for j := 0; j < nv; j++ {
				t.Values = append(t.Values, EnumVal{st(), vn()})
			}
		case 3:
			t.Kind = "union"
			nv := vn()
			for j := 0; j < nv; j++ {
				vname := st()
				tag := u8()
				t.Variants = append(t.Variants, Variant{vname, int(tag), td(0)})
			}
		default:
			bad("bad type kind")
		}
		s.Types = append(s.Types, t)
	}
	if p != len(d) {
		bad("trailing bytes in schema")
	}
	return s, nil
}
