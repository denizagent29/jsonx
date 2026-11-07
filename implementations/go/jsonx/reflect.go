package jsonx

import (
	"math/big"
	"reflect"
	"sort"
	"strings"
)

// Typed mapping between Go structs and BJSONX, in the shape produced by `jsonxc gen go`:
//   - schema fields map to struct fields by the `jx:"name"` tag (fallback: case-insensitive CamelCase of the schema name);
//   - optional/nullable fields are pointers (or slices/interfaces, whose nil is "absent"); required struct fields are pointers to structs;
//   - schema enums are string-based named types; unions are structs with one pointer field per variant (exactly one non-nil);
//   - `bytes` is []byte, `array<T>` is []T, `any` is interface{} (a view that aliases the blob when unmarshalling).
// ref<T> fields are readable (the target is copied) but cannot be marshalled (E_REF_TARGET).

func camelName(s string) string {
	var b strings.Builder
	for _, p := range strings.Split(s, "_") {
		if p != "" {
			b.WriteString(strings.ToUpper(p[:1]) + p[1:])
		}
	}
	return b.String()
}

func fieldIndex(t reflect.Type, name string) (int, bool) {
	for i := 0; i < t.NumField(); i++ {
		sf := t.Field(i)
		if tag, ok := sf.Tag.Lookup("jx"); ok {
			if tag == name {
				return i, true
			}
			continue
		}
		if sf.IsExported() && strings.EqualFold(sf.Name, camelName(name)) {
			return i, true
		}
	}
	return 0, false
}

func mismatchErr(what string, v interface{}) {
	fail("E_TYPE_MISMATCH", "cannot store "+reflect.TypeOf(v).String()+" value in "+what)
}
func toBig(v interface{}) *big.Int {
	switch x := v.(type) {
	case int64:
		return big.NewInt(x)
	case uint64:
		return new(big.Int).SetUint64(x)
	}
	return nil
}

// Unmarshal decodes a typed blob into *v (a pointer to a struct named like the blob's root schema).
func Unmarshal(data []byte, sc *Schema, v interface{}) (err error) {
	defer catch(&err)
	rv := reflect.ValueOf(v)
	if rv.Kind() != reflect.Ptr || rv.IsNil() || rv.Elem().Kind() != reflect.Struct {
		fail("E_TYPE_MISMATCH", "Unmarshal needs a non-nil pointer to a struct")
	}
	r := newReader(data, sc, ReadOptions{})
	if r.dynamic {
		fail("E_BIN_SCHEMA_MISMATCH", "dynamic blobs have no typed root; use Decode or View")
	}
	if want := sc.Types[r.ridx].Name; want != rv.Elem().Type().Name() {
		fail("E_TYPE_MISMATCH", "blob root is schema "+want+", destination is "+rv.Elem().Type().Name())
	}
	fillStruct(r.structView(r.ridx, r.root), rv.Elem())
	return nil
}

func fillStruct(sv *StructView, dst reflect.Value) {
	t := dst.Type()
	for _, f := range sv.r.sc.Types[sv.idx].Fields {
		fi, ok := fieldIndex(t, f.Name)
		if !ok {
			continue
		}
		assign(dst.Field(fi), sv.get(f.Name))
	}
}

func assign(dst reflect.Value, val interface{}) {
	if val == nil {
		return
	}
	switch dst.Kind() {
	case reflect.Ptr:
		n := reflect.New(dst.Type().Elem())
		assign(n.Elem(), val)
		dst.Set(n)
	case reflect.Interface:
		dst.Set(reflect.ValueOf(val))
	case reflect.Bool:
		b, ok := val.(bool)
		if !ok {
			mismatchErr("bool", val)
		}
		dst.SetBool(b)
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		b := toBig(val)
		if b == nil || !b.IsInt64() || dst.OverflowInt(b.Int64()) {
			mismatchErr(dst.Type().String(), val)
		}
		dst.SetInt(b.Int64())
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		b := toBig(val)
		if b == nil || !b.IsUint64() || dst.OverflowUint(b.Uint64()) {
			mismatchErr(dst.Type().String(), val)
		}
		dst.SetUint(b.Uint64())
	case reflect.Float32, reflect.Float64:
		switch x := val.(type) {
		case float32:
			dst.SetFloat(float64(x))
		case float64:
			dst.SetFloat(x)
		default:
			mismatchErr(dst.Type().String(), val)
		}
	case reflect.String:
		s, ok := val.(string)
		if !ok {
			mismatchErr(dst.Type().String(), val)
		}
		dst.SetString(s)
	case reflect.Slice:
		if b, ok := val.([]byte); ok && dst.Type().Elem().Kind() == reflect.Uint8 {
			dst.SetBytes(append([]byte(nil), b...))
			return
		}
		switch a := val.(type) {
		case *ArrayView:
			sl := reflect.MakeSlice(dst.Type(), a.n, a.n)
			for i := 0; i < a.n; i++ {
				assign(sl.Index(i), a.at(i))
			}
			dst.Set(sl)
		case emptyArray:
			dst.Set(reflect.MakeSlice(dst.Type(), 0, 0))
		default:
			mismatchErr(dst.Type().String(), val)
		}
	case reflect.Struct:
		switch x := val.(type) {
		case *StructView:
			fillStruct(x, dst)
		case *UnionView:
			name := x.Name()
			fi, ok := fieldIndex(dst.Type(), name)
			if !ok {
				fail("E_TYPE_MISMATCH", "union variant '"+name+"' has no field in "+dst.Type().String())
			}
			assign(dst.Field(fi), x.value())
		default:
			mismatchErr(dst.Type().String(), val)
		}
	default:
		mismatchErr(dst.Type().String(), val)
	}
}

// Marshal encodes a Go struct (named like a schema in sc) at the schema's current version (or o.Version).
func Marshal(v interface{}, sc *Schema, o EncodeOptions) (out []byte, err error) {
	defer catch(&err)
	rv := reflect.ValueOf(v)
	for rv.Kind() == reflect.Ptr {
		if rv.IsNil() {
			fail("E_TYPE_MISMATCH", "Marshal of a nil pointer")
		}
		rv = rv.Elem()
	}
	if rv.Kind() != reflect.Struct {
		fail("E_TYPE_MISMATCH", "Marshal needs a struct")
	}
	if o.Root == "" {
		o.Root = rv.Type().Name()
	}
	ver := sc.Version
	if o.Version != 0 {
		ver = o.Version
	}
	root := structNode(sc, sc.structIndex(o.Root), ver, rv)
	return Encode(&Document{Root: root}, sc, o)
}

func structNode(sc *Schema, idx, ver int, rv reflect.Value) *Node {
	obj := newObj()
	obj.Pos = -1
	for _, f := range sc.Types[idx].Fields {
		if f.Since > ver {
			continue
		}
		fi, ok := fieldIndex(rv.Type(), f.Name)
		if !ok {
			continue // the encoder reports a missing required field precisely
		}
		fv := rv.Field(fi)
		if f.Optional || f.Nullable {
			if isNilish(fv) {
				if f.Nullable {
					setMember(obj, f.Name, &Node{Kind: "null", Pos: -1})
				}
				continue
			}
		} else if isNilish(fv) && f.Type.Kind != "any" && f.Type.Kind != "array" && f.Type.Kind != "bytes" {
			fail("E_MISSING_FIELD", sc.Types[idx].Name+"."+f.Name+" is required but the Go value is nil")
		}
		setMember(obj, f.Name, toNode(sc, f.Type, ver, fv))
	}
	return obj
}
func isNilish(v reflect.Value) bool {
	switch v.Kind() {
	case reflect.Ptr, reflect.Interface, reflect.Map:
		return v.IsNil()
	}
	return false
}

func toNode(sc *Schema, t *Type, ver int, fv reflect.Value) *Node {
	for fv.Kind() == reflect.Ptr || (fv.Kind() == reflect.Interface && t.Kind != "any") {
		if fv.IsNil() {
			fail("E_TYPE_MISMATCH", "nil value for a required "+sc.typeName(t))
		}
		fv = fv.Elem()
	}
	n := &Node{Pos: -1}
	switch t.Kind {
	case "bool":
		n.Kind, n.Bool = "bool", fv.Bool()
	case "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64":
		n.Kind = "int"
		switch fv.Kind() {
		case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
			n.Int = big.NewInt(fv.Int())
		case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
			n.Int = new(big.Int).SetUint64(fv.Uint())
		default:
			fail("E_TYPE_MISMATCH", "expected a Go integer for "+t.Kind+", got "+fv.Kind().String())
		}
	case "f32", "f64":
		if fv.Kind() != reflect.Float32 && fv.Kind() != reflect.Float64 {
			fail("E_TYPE_MISMATCH", "expected a Go float for "+t.Kind+", got "+fv.Kind().String())
		}
		n.Kind, n.Float = "float", fv.Float()
	case "string", "enum":
		if fv.Kind() != reflect.String {
			fail("E_TYPE_MISMATCH", "expected a Go string for "+sc.typeName(t)+", got "+fv.Kind().String())
		}
		n.Kind, n.Str = "string", fv.String()
	case "bytes":
		if fv.Kind() != reflect.Slice || fv.Type().Elem().Kind() != reflect.Uint8 {
			fail("E_TYPE_MISMATCH", "expected []byte for bytes, got "+fv.Type().String())
		}
		n.Kind, n.Bytes = "bytes", append([]byte(nil), fv.Bytes()...)
	case "array":
		if fv.Kind() != reflect.Slice {
			fail("E_TYPE_MISMATCH", "expected a slice for "+sc.typeName(t)+", got "+fv.Kind().String())
		}
		n.Kind = "array"
		for i := 0; i < fv.Len(); i++ {
			c := toNode(sc, t.Elem, ver, fv.Index(i))
			c.Parent, c.Index = n, i
			n.Items = append(n.Items, c)
		}
	case "struct":
		if fv.Kind() != reflect.Struct {
			fail("E_TYPE_MISMATCH", "expected a struct for "+sc.typeName(t)+", got "+fv.Kind().String())
		}
		return structNode(sc, t.Idx, ver, fv)
	case "union":
		un := sc.Types[t.Idx]
		var found *Variant
		var fval reflect.Value
		for i := range un.Variants {
			fi, ok := fieldIndex(fv.Type(), un.Variants[i].Name)
			if ok && !fv.Field(fi).IsNil() {
				if found != nil {
					fail("E_TYPE_MISMATCH", "union "+un.Name+" has more than one variant set")
				}
				found, fval = &un.Variants[i], fv.Field(fi)
			}
		}
		if found == nil {
			fail("E_TYPE_MISMATCH", "union "+un.Name+" has no variant set")
		}
		n = newObj()
		n.Pos = -1
		setMember(n, found.Name, toNode(sc, found.T, ver, fval))
	case "ref":
		fail("E_REF_TARGET", "marshalling ref<T> fields is not supported; build the document from JSONX text")
	case "any":
		return anyNode(fv)
	}
	return n
}

func anyNode(fv reflect.Value) *Node {
	for fv.Kind() == reflect.Interface || fv.Kind() == reflect.Ptr {
		if fv.IsNil() {
			return &Node{Kind: "null", Pos: -1}
		}
		fv = fv.Elem()
	}
	n := &Node{Pos: -1}
	switch fv.Kind() {
	case reflect.Bool:
		n.Kind, n.Bool = "bool", fv.Bool()
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		n.Kind, n.Int = "int", big.NewInt(fv.Int())
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		n.Kind, n.Int = "int", new(big.Int).SetUint64(fv.Uint())
	case reflect.Float32, reflect.Float64:
		n.Kind, n.Float = "float", fv.Float()
	case reflect.String:
		n.Kind, n.Str = "string", fv.String()
	case reflect.Slice:
		if fv.Type().Elem().Kind() == reflect.Uint8 {
			n.Kind, n.Bytes = "bytes", append([]byte(nil), fv.Bytes()...)
			return n
		}
		n.Kind = "array"
		for i := 0; i < fv.Len(); i++ {
			c := anyNode(fv.Index(i))
			c.Parent, c.Index = n, i
			n.Items = append(n.Items, c)
		}
	case reflect.Map:
		if fv.Type().Key().Kind() != reflect.String {
			fail("E_TYPE_MISMATCH", "any maps need string keys")
		}
		n = newObj()
		n.Pos = -1
		keys := fv.MapKeys()
		sort.Slice(keys, func(i, j int) bool { return keys[i].String() < keys[j].String() })
		for _, k := range keys {
			setMember(n, k.String(), anyNode(fv.MapIndex(k)))
		}
	default:
		fail("E_TYPE_MISMATCH", "cannot store a Go "+fv.Kind().String()+" in an any field")
	}
	return n
}
