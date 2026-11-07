package jsonx

import (
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"hash/crc32"
	"math"
	"math/big"
	"unicode/utf8"
)

const (
	hdr                                                                          = 28
	tNull, tFalse, tTrue, tI32, tI64, tU64, tF64, tStr, tArr, tObj, tRef, tBytes = 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11
)

var le = binary.LittleEndian

func vlen(n int) int {
	switch {
	case n < 0x80:
		return 1
	case n < 0x4000:
		return 2
	case n < 0x200000:
		return 3
	case n < 0x10000000:
		return 4
	}
	return 5
}

var intBits = map[string][2]int{"u8": {0, 8}, "u16": {0, 16}, "u32": {0, 32}, "u64": {0, 64}, "i8": {1, 8}, "i16": {1, 16}, "i32": {1, 32}, "i64": {1, 64}}

// ---------------------------------------------------------------- encoder
type locEnt struct {
	loc int
	t   *Type
}
type refEnt struct {
	pos  int
	node *Node
	exp  *Type
}
type enc struct {
	sc   *Schema
	ver  int
	src  string
	buf  []byte
	refs []refEnt
	locs map[*Node]locEnt
	keys map[string]int
}

func (e *enc) fail(code, msg string, n *Node) {
	pe := &Error{Code: code, Message: msg, Pos: -1}
	if n != nil && n.Pos >= 0 && e.src != "" {
		pe.Pos = n.Pos
		pe.Line, pe.Col = lineCol(e.src, n.Pos)
	}
	panic(pe)
}
func kd(n *Node) string {
	if n.Kind == "int" {
		return "integer"
	}
	if n.Kind == "f32" {
		return "float"
	}
	return n.Kind
}
func (e *enc) tname(t *Type) string {
	if e.sc != nil {
		return e.sc.typeName(t)
	}
	return t.Kind
}
func (e *enc) mismatch(t *Type, n *Node) {
	e.fail("E_TYPE_MISMATCH", "expected "+e.tname(t)+", got "+kd(n), n)
}
func (e *enc) alloc(n int) int {
	p := len(e.buf)
	if p+n >= 0xFFFFFFFF {
		fail("E_BIN_LIMIT", "document exceeds the 4 GiB offset limit")
	}
	e.buf = append(e.buf, make([]byte, n)...)
	return p
}
func (e *enc) varint(p, v int) int {
	for v >= 0x80 {
		e.buf[p] = byte(v&0x7f) | 0x80
		v >>= 7
		p++
	}
	e.buf[p] = byte(v)
	return p + 1
}
func (e *enc) u32(p, v int)                  { le.PutUint32(e.buf[p:], uint32(v)) }
func (e *enc) reg(n *Node, loc int, t *Type) { e.locs[n] = locEnt{loc, t} }
func (e *enc) putStr(b []byte) int {
	p := e.alloc(vlen(len(b)) + len(b))
	q := e.varint(p, len(b))
	copy(e.buf[q:], b)
	return p
}

func (e *enc) anySlot(n *Node, pos, depth int) {
	if depth > MaxDepth {
		fail("E_DEPTH_LIMIT", "nesting too deep")
	}
	e.reg(n, pos, anyT)
	switch n.Kind {
	case "null":
	case "bool":
		if n.Bool {
			e.buf[pos] = tTrue
		} else {
			e.buf[pos] = tFalse
		}
	case "int":
		if n.Int.IsInt64() && n.Int.Int64() >= math.MinInt32 && n.Int.Int64() <= math.MaxInt32 {
			e.buf[pos] = tI32
			le.PutUint32(e.buf[pos+1:], uint32(int32(n.Int.Int64())))
		} else if n.Int.IsInt64() {
			p := e.alloc(8)
			le.PutUint64(e.buf[p:], uint64(n.Int.Int64()))
			e.buf[pos] = tI64
			e.u32(pos+1, p)
		} else {
			p := e.alloc(8)
			le.PutUint64(e.buf[p:], n.Int.Uint64())
			e.buf[pos] = tU64
			e.u32(pos+1, p)
		}
	case "float", "f32":
		p := e.alloc(8)
		le.PutUint64(e.buf[p:], math.Float64bits(n.Float))
		e.buf[pos] = tF64
		e.u32(pos+1, p)
	case "string":
		e.buf[pos] = tStr
		e.u32(pos+1, e.putStr([]byte(n.Str)))
	case "bytes":
		e.buf[pos] = tBytes
		e.u32(pos+1, e.putStr(n.Bytes))
	case "array":
		c := len(n.Items)
		hl := vlen(c)
		blk := e.alloc(hl + 5*c)
		e.varint(blk, c)
		e.buf[pos] = tArr
		e.u32(pos+1, blk)
		for i, ch := range n.Items {
			e.anySlot(ch, blk+hl+5*i, depth+1)
		}
	case "object":
		c := len(n.Keys)
		hl := vlen(c)
		blk := e.alloc(hl + 9*c)
		e.varint(blk, c)
		e.buf[pos] = tObj
		e.u32(pos+1, blk)
		for i, k := range n.Keys {
			en := blk + hl + 9*i
			ko, ok := e.keys[k]
			if !ok {
				ko = e.putStr([]byte(k))
				e.keys[k] = ko
			}
			e.u32(en, ko)
			e.anySlot(n.Map[k], en+4, depth+1)
		}
	case "ref":
		e.buf[pos] = tRef
		e.refs = append(e.refs, refEnt{pos + 1, n, anyT})
	default:
		fail("E_TYPE_MISMATCH", "cannot encode "+n.Kind)
	}
}

func (e *enc) scalar(t *Type, n *Node, pos int) {
	k := t.Kind
	switch {
	case k == "bool":
		if n.Kind != "bool" {
			e.mismatch(t, n)
		}
		if n.Bool {
			e.buf[pos] = 1
		}
	case intBits[k][1] != 0:
		if n.Kind != "int" {
			e.mismatch(t, n)
		}
		sg, bits := intBits[k][0], intBits[k][1]
		var lo, hi *big.Int
		if sg == 1 {
			lo = new(big.Int).Neg(new(big.Int).Lsh(big.NewInt(1), uint(bits-1)))
			hi = new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), uint(bits-1)), big.NewInt(1))
		} else {
			lo = big.NewInt(0)
			hi = new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), uint(bits)), big.NewInt(1))
		}
		if n.Int.Cmp(lo) < 0 || n.Int.Cmp(hi) > 0 {
			e.fail("E_RANGE", n.Int.String()+" does not fit in "+k, n)
		}
		var u uint64
		if n.Int.Sign() < 0 {
			u = uint64(n.Int.Int64())
		} else {
			u = n.Int.Uint64()
		}
		for i := 0; i < bits/8; i++ {
			e.buf[pos+i] = byte(u >> (8 * i))
		}
	case k == "f32" || k == "f64":
		var v float64
		switch n.Kind {
		case "int":
			v, _ = new(big.Float).SetInt(n.Int).Float64()
		case "float", "f32":
			v = n.Float
		default:
			e.mismatch(t, n)
		}
		if k == "f32" {
			f := float32(v)
			if math.IsInf(float64(f), 0) && !math.IsInf(v, 0) {
				e.fail("E_RANGE", "value overflows f32", n)
			}
			le.PutUint32(e.buf[pos:], math.Float32bits(f))
		} else {
			le.PutUint64(e.buf[pos:], math.Float64bits(v))
		}
	case k == "enum":
		en := e.sc.Types[t.Idx]
		var v int
		if n.Kind == "string" {
			found := false
			for _, ev := range en.Values {
				if ev.Name == n.Str {
					v, found = ev.V, true
				}
			}
			if !found {
				e.fail("E_TYPE_MISMATCH", "'"+n.Str+"' is not a member of enum "+en.Name, n)
			}
		} else if n.Kind == "int" && n.Int.Sign() >= 0 && n.Int.Cmp(new(big.Int).Lsh(big.NewInt(1), uint(8*en.Base))) < 0 {
			v = int(n.Int.Int64())
		} else {
			e.mismatch(t, n)
		}
		for i := 0; i < en.Base; i++ {
			e.buf[pos+i] = byte(v >> (8 * i))
		}
	}
}

func (e *enc) bytesOf(n *Node) []byte {
	if n.Kind == "bytes" {
		return n.Bytes
	}
	if n.Kind != "string" {
		e.mismatch(&Type{Kind: "bytes"}, n)
	}
	b, err := base64.StdEncoding.Strict().DecodeString(n.Str)
	if err != nil {
		e.fail("E_TYPE_MISMATCH", "bytes fields take a base64 string", n)
	}
	return b
}

func (e *enc) heapPut(t *Type, n *Node) int {
	switch t.Kind {
	case "string":
		if n.Kind != "string" {
			e.mismatch(t, n)
		}
		return e.putStr([]byte(n.Str))
	case "bytes":
		return e.putStr(e.bytesOf(n))
	case "array":
		return e.arrayBlock(t, n)
	case "struct":
		if n.Kind != "object" {
			e.mismatch(t, n)
		}
		p := e.alloc(e.sc.layoutOf(t.Idx, e.ver).Size)
		e.structBlock(t.Idx, n, p)
		return p
	}
	p := e.alloc(e.sc.slotSize(t))
	e.scalar(t, n, p)
	return p
}

func (e *enc) slot(t *Type, n *Node, pos int) {
	k := t.Kind
	if _, ok := scalarSize[k]; ok || k == "enum" {
		e.scalar(t, n, pos)
		e.reg(n, pos, t)
		return
	}
	switch k {
	case "string", "bytes", "array", "struct":
		off := e.heapPut(t, n)
		e.u32(pos, off)
		if k != "struct" {
			e.reg(n, pos, t)
		}
	case "union":
		if n.Kind != "object" || len(n.Keys) != 1 {
			e.fail("E_TYPE_MISMATCH", "union value must be an object with exactly one key naming a variant", n)
		}
		un := e.sc.Types[t.Idx]
		name := n.Keys[0]
		child := n.Map[name]
		var v *Variant
		for i := range un.Variants {
			if un.Variants[i].Name == name {
				v = &un.Variants[i]
			}
		}
		if v == nil {
			e.fail("E_UNKNOWN_FIELD", "'"+name+"' is not a variant of union "+un.Name, child)
		}
		off := e.heapPut(v.T, child)
		e.buf[pos] = byte(v.Tag)
		e.u32(pos+1, off)
		e.reg(n, pos, t)
	case "any":
		e.anySlot(n, pos, 0)
	case "ref":
		if n.Kind != "ref" {
			e.fail("E_TYPE_MISMATCH", "expected "+e.tname(t)+", got "+kd(n), n)
		}
		e.refs = append(e.refs, refEnt{pos, n, t.Elem})
	}
}

func (e *enc) arrayBlock(t *Type, n *Node) int {
	if n.Kind != "array" {
		e.mismatch(t, n)
	}
	elem, c := t.Elem, len(n.Items)
	var blk int
	if elem.Kind == "struct" {
		size := e.sc.layoutOf(elem.Idx, e.ver).Size
		hl := vlen(c) + vlen(size)
		blk = e.alloc(hl + c*size)
		p := e.varint(blk, c)
		e.varint(p, size)
		for i, ch := range n.Items {
			if ch.Kind != "object" {
				e.mismatch(elem, ch)
			}
			e.structBlock(elem.Idx, ch, blk+hl+i*size)
		}
	} else {
		es := e.sc.slotSize(elem)
		hl := vlen(c)
		blk = e.alloc(hl + c*es)
		e.varint(blk, c)
		for i, ch := range n.Items {
			e.slot(elem, ch, blk+hl+i*es)
		}
	}
	return blk
}

func mkDefault(d *Default) *Node {
	switch d.Kind {
	case "null":
		return &Node{Kind: "null", Pos: -1}
	case "bool":
		return &Node{Kind: "bool", Bool: d.B, Pos: -1}
	case "int":
		return &Node{Kind: "int", Int: d.Int, Pos: -1}
	case "float":
		return &Node{Kind: "float", Float: d.F, Pos: -1}
	case "string":
		return &Node{Kind: "string", Str: d.S, Pos: -1}
	}
	return &Node{Kind: "array", Pos: -1}
}

func (e *enc) structBlock(idx int, n *Node, base int) {
	sc := e.sc
	st := sc.Types[idx]
	e.reg(n, base, &Type{Kind: "struct", Idx: idx})
	lay := sc.layoutOf(idx, e.ver)
	known := map[string]bool{}
	for _, fl := range lay.Lay {
		known[fl.F.Name] = true
	}
	for _, k := range n.Keys {
		if !known[k] {
			e.fail("E_UNKNOWN_FIELD", "schema "+st.Name+" has no field '"+k+"'", n.Map[k])
		}
	}
	for _, fl := range lay.Lay {
		f := fl.F
		child := n.Map[f.Name]
		opt := f.Optional || f.Nullable
		if child == nil {
			switch {
			case f.Default != nil:
				child = mkDefault(f.Default)
			case f.Optional:
				continue
			case f.Nullable:
				e.fail("E_MISSING_FIELD", st.Name+"."+f.Name+" is nullable but not optional: write null explicitly", n)
			default:
				e.fail("E_MISSING_FIELD", st.Name+"."+f.Name+" is required", n)
			}
		}
		if opt {
			if child.Kind == "null" {
				continue
			}
			e.buf[base+fl.PByte] |= 1 << fl.Bit
		} else if child.Kind == "null" && f.Type.Kind != "any" {
			e.fail("E_TYPE_MISMATCH", st.Name+"."+f.Name+" is not nullable", child)
		}
		e.slot(f.Type, child, base+fl.Off)
		if len(f.Cons) > 0 {
			e.constrain(st, f, child)
		}
	}
}

func (e *enc) constrain(st *TypeDef, f *Field, n *Node) {
	for name, b := range f.Cons {
		var v float64
		var ok bool
		if name == "range" {
			switch n.Kind {
			case "int":
				v, _ = new(big.Float).SetInt(n.Int).Float64()
				ok = true
			case "float", "f32":
				v, ok = n.Float, true
			}
		} else {
			switch {
			case n.Kind == "string" && f.Type.Kind == "bytes":
				v, ok = float64(len(e.bytesOf(n))), true
			case n.Kind == "string":
				v, ok = float64(utf8.RuneCountInString(n.Str)), true
			case n.Kind == "bytes":
				v, ok = float64(len(n.Bytes)), true
			case n.Kind == "array":
				v, ok = float64(len(n.Items)), true
			}
		}
		if ok && ((b.Lo != nil && v < *b.Lo) || (b.Hi != nil && v > *b.Hi)) {
			e.fail("E_CONSTRAINT", st.Name+"."+f.Name+" violates @"+name, n)
		}
	}
}

// EncodeOptions control Encode.
type EncodeOptions struct {
	Root     string // schema name; default is the schema's root
	Version  int    // layout version; default is the schema's version
	Checksum bool
}

// Encode serialises a document to BJSONX. With a nil schema the dynamic (any) layout is used.
func Encode(doc *Document, sc *Schema, o EncodeOptions) (out []byte, err error) {
	defer catch(&err)
	e := &enc{sc: sc, src: doc.Src, buf: make([]byte, hdr), locs: map[*Node]locEnt{}, keys: map[string]int{}}
	var rootOff, ver int
	var sid uint64
	if sc == nil {
		p := e.alloc(5)
		e.anySlot(doc.Root, p, 0)
		rootOff = p
	} else {
		ver = sc.Version
		if o.Version != 0 {
			ver = o.Version
		}
		if ver < 1 || ver > sc.Version {
			fail("E_SCHEMA_INVALID", "cannot encode that version with this schema")
		}
		ridx := sc.Root
		sid = sc.ID()
		if o.Root != "" {
			ridx = sc.structIndex(o.Root)
			sid = sc.idFor(o.Root)
		}
		if ridx < 0 {
			fail("E_SCHEMA_INVALID", "schema declares no root")
		}
		e.ver = ver
		if doc.Root.Kind != "object" {
			e.mismatch(&Type{Kind: "struct", Idx: ridx}, doc.Root)
		}
		rootOff = e.alloc(sc.layoutOf(ridx, ver).Size)
		e.structBlock(ridx, doc.Root, rootOff)
	}
	for _, r := range e.refs {
		tg, ok := e.locs[r.node.Target]
		if !ok {
			e.fail("E_REF_TARGET", "reference target is not addressable in the binary layout", r.node)
		}
		if tg.t.key() != r.exp.key() {
			e.fail("E_REF_TARGET", "reference of type ref<"+r.exp.key()+"> points at a value of type "+tg.t.key(), r.node)
		}
		e.u32(r.pos, tg.loc)
	}
	flags := 0
	if o.Checksum {
		flags = 1
		e.alloc(4)
	}
	b := e.buf
	copy(b, "BJX")
	b[3] = 1
	le.PutUint16(b[4:], uint16(flags))
	le.PutUint16(b[6:], 0)
	le.PutUint64(b[8:], sid)
	le.PutUint32(b[16:], uint32(ver))
	le.PutUint32(b[20:], uint32(rootOff))
	le.PutUint32(b[24:], uint32(len(b)))
	if o.Checksum {
		le.PutUint32(b[len(b)-4:], crc32.ChecksumIEEE(b[:len(b)-4]))
	}
	return b, nil
}

// ---------------------------------------------------------------- reader
type fieldInfo struct {
	by   map[string]*fieldLayout
	lay  []fieldLayout
	size int
}
type reader struct {
	d       []byte
	n       int
	sc      *Schema
	sid     uint64
	sver    int
	root    int
	dynamic bool
	ridx    int
	w       int
	f       map[int]*fieldInfo
}

// ReadOptions control Decode/View.
type ReadOptions struct {
	SkipChecksum  bool
	StrictVersion bool
}

func newReader(data []byte, sc *Schema, o ReadOptions) *reader {
	n := len(data)
	if n < hdr {
		fail("E_BIN_TRUNCATED", "blob is shorter than the 28-byte header")
	}
	if string(data[:3]) != "BJX" {
		fail("E_BIN_MAGIC", "not a BJSONX blob (bad magic)")
	}
	if data[3] != 1 {
		fail("E_BIN_VERSION", "unsupported BJSONX format version")
	}
	flags, res := le.Uint16(data[4:]), le.Uint16(data[6:])
	sid, sver, root, length := le.Uint64(data[8:]), int(le.Uint32(data[16:])), int(le.Uint32(data[20:])), int(le.Uint32(data[24:]))
	if flags&^1 != 0 || res != 0 {
		fail("E_BIN_INVALID", "reserved header bits are set")
	}
	if length > n {
		fail("E_BIN_TRUNCATED", "header length exceeds the data")
	}
	if length < n {
		fail("E_BIN_INVALID", "trailing bytes after the blob")
	}
	r := &reader{d: data, n: n, sc: sc, sid: sid, sver: sver, root: root, f: map[int]*fieldInfo{}}
	if flags&1 != 0 {
		if n < hdr+4 {
			fail("E_BIN_TRUNCATED", "missing checksum")
		}
		if !o.SkipChecksum && crc32.ChecksumIEEE(data[:n-4]) != le.Uint32(data[n-4:]) {
			fail("E_BIN_CHECKSUM", "CRC-32 mismatch")
		}
		r.n = n - 4
	}
	r.dynamic = sid == 0 && sver == 0
	if !r.dynamic {
		if sc == nil {
			fail("E_BIN_SCHEMA_MISMATCH", "blob requires a schema; none supplied")
		}
		if sver < 1 {
			fail("E_BIN_INVALID", "typed blob with schema version 0")
		}
		r.ridx = -1
		for i, t := range sc.Types {
			if t.Kind == "struct" && sc.idFor(t.Name) == sid {
				r.ridx = i
				break
			}
		}
		if r.ridx < 0 {
			fail("E_BIN_SCHEMA_MISMATCH", "schema id does not match any schema in the supplied definition")
		}
		if o.StrictVersion && sver > sc.Version {
			fail("E_BIN_SCHEMA_MISMATCH", "blob is newer than the reader's schema")
		}
	}
	r.w = sver
	if root < hdr || root >= r.n {
		fail("E_BIN_BOUNDS", "root offset is outside the payload")
	}
	return r
}

func (r *reader) need(p, k int) {
	if p < hdr || k < 0 || p+k > r.n {
		fail("E_BIN_BOUNDS", "access outside the payload")
	}
}
func (r *reader) needMul(p, cnt, sz int) {
	if cnt == 0 {
		return
	}
	if sz > 0 && cnt > r.n/sz+1 {
		fail("E_BIN_BOUNDS", "array larger than the payload")
	}
	r.need(p, cnt*sz)
}
func (r *reader) u8(p int) int  { r.need(p, 1); return int(r.d[p]) }
func (r *reader) u32(p int) int { r.need(p, 4); return int(le.Uint32(r.d[p:])) }
func (r *reader) varint(p int) (int, int) {
	v := 0
	for i := 0; i < 5; i++ {
		r.need(p+i, 1)
		b := r.d[p+i]
		v |= int(b&0x7f) << (7 * i)
		if b&0x80 == 0 {
			if i > 0 && b == 0 {
				fail("E_BIN_INVALID", "non-minimal varint")
			}
			if v > 0xFFFFFFFF {
				fail("E_BIN_INVALID", "varint exceeds u32")
			}
			return v, p + i + 1
		}
	}
	fail("E_BIN_INVALID", "varint longer than 5 bytes")
	return 0, 0
}
func (r *reader) fwd(off, ms int, what string) int {
	if off <= ms {
		fail("E_BIN_INVALID", what+" offset does not point forward of its container")
	}
	return off
}
func (r *reader) hbytes(off int) []byte {
	n, p := r.varint(off)
	r.need(p, n)
	return r.d[p : p+n : p+n]
}
func (r *reader) hstr(off int) string {
	b := r.hbytes(off)
	if !utf8.Valid(b) {
		fail("E_BIN_UTF8", "invalid UTF-8 in string")
	}
	return string(b)
}

// scalar returns bool, int64, uint64, float32 or float64.
func (r *reader) scalar(k string, p int) interface{} {
	r.need(p, scalarSize[k])
	b := r.d[p:]
	switch k {
	case "bool":
		if b[0] > 1 {
			fail("E_BIN_INVALID", "invalid bool byte")
		}
		return b[0] == 1
	case "u8":
		return uint64(b[0])
	case "i8":
		return int64(int8(b[0]))
	case "u16":
		return uint64(le.Uint16(b))
	case "i16":
		return int64(int16(le.Uint16(b)))
	case "u32":
		return uint64(le.Uint32(b))
	case "i32":
		return int64(int32(le.Uint32(b)))
	case "u64":
		return le.Uint64(b)
	case "i64":
		return int64(le.Uint64(b))
	case "f32":
		return math.Float32frombits(le.Uint32(b))
	}
	return math.Float64frombits(le.Uint64(b))
}
func (r *reader) enumRaw(t *Type, p int) (int, *TypeDef) {
	en := r.sc.Types[t.Idx]
	r.need(p, en.Base)
	v := 0
	for i := en.Base - 1; i >= 0; i-- {
		v = v<<8 | int(r.d[p+i])
	}
	return v, en
}
func (r *reader) fields(idx int) *fieldInfo {
	if f, ok := r.f[idx]; ok {
		return f
	}
	l := r.sc.layoutOf(idx, r.w)
	f := &fieldInfo{by: map[string]*fieldLayout{}, lay: l.Lay, size: l.Size}
	for i := range f.lay {
		f.by[f.lay[i].F.Name] = &f.lay[i]
	}
	r.f[idx] = f
	return f
}

// Value kinds returned by views: nil, bool, int64, uint64, float32, float64, string, []byte, *StructView,
// *ArrayView, *UnionView, *AnyArray, *AnyObject.
func (r *reader) slotGet(t *Type, pos, ms int) interface{} {
	k := t.Kind
	if _, ok := scalarSize[k]; ok {
		return r.scalar(k, pos)
	}
	switch k {
	case "enum":
		v, en := r.enumRaw(t, pos)
		for _, ev := range en.Values {
			if ev.V == v {
				return ev.Name
			}
		}
		return int64(v)
	case "any":
		return r.anyGet(pos, ms)
	case "union":
		r.need(pos, 5)
		return &UnionView{r, t.Idx, pos, ms, r.u8(pos)}
	case "ref":
		return r.valueAt(t.Elem, r.u32(pos))
	}
	return r.heapGet(t, r.fwd(r.u32(pos), ms, k), ms)
}
func (r *reader) heapGet(t *Type, off, ms int) interface{} {
	k := t.Kind
	switch k {
	case "string":
		return r.hstr(off)
	case "bytes":
		return r.hbytes(off)
	case "array":
		return r.arrayView(t, off)
	case "struct":
		return r.structView(t.Idx, off)
	case "enum":
		return r.slotGet(t, off, ms)
	}
	if _, ok := scalarSize[k]; ok {
		return r.scalar(k, off)
	}
	fail("E_BIN_INVALID", "type "+k+" cannot be stored by offset")
	return nil
}
func (r *reader) valueAt(t *Type, loc int) interface{} {
	if t.Kind == "struct" {
		return r.structView(t.Idx, loc)
	}
	r.need(loc, r.sc.slotSize(t))
	return r.slotGet(t, loc, 0)
}
func (r *reader) structView(idx, loc int) *StructView {
	r.need(loc, r.fields(idx).size)
	return &StructView{r, idx, loc}
}
func (r *reader) arrayView(t *Type, off int) *ArrayView {
	n, p := r.varint(off)
	elem := t.Elem
	if elem.Kind == "struct" {
		var stride int
		stride, p = r.varint(p)
		if stride < r.fields(elem.Idx).size {
			fail("E_BIN_INVALID", "array stride smaller than struct size")
		}
		r.needMul(p, n, stride)
		return &ArrayView{r, elem, off, n, p, stride}
	}
	es := r.sc.slotSize(elem)
	r.needMul(p, n, es)
	return &ArrayView{r, elem, off, n, p, es}
}
func (r *reader) anyGet(pos, ms int) interface{} {
	tag, pl := r.u8(pos), r.u32(pos+1)
	if tag <= tTrue {
		if pl != 0 {
			fail("E_BIN_INVALID", "nonzero payload on constant tag")
		}
		if tag == tNull {
			return nil
		}
		return tag == tTrue
	}
	if tag == tI32 {
		return int64(int32(le.Uint32(r.d[pos+1:])))
	}
	if tag == tRef {
		r.need(pl, 5)
		if r.d[pl] == tRef {
			fail("E_BIN_INVALID_REF", "reference targets another reference")
		}
		return r.anyGet(pl, 0)
	}
	r.fwd(pl, ms, "value")
	switch tag {
	case tI64:
		return r.scalar("i64", pl)
	case tU64:
		return r.scalar("u64", pl)
	case tF64:
		return r.scalar("f64", pl)
	case tStr:
		return r.hstr(pl)
	case tBytes:
		return r.hbytes(pl)
	case tArr:
		n, p := r.varint(pl)
		r.needMul(p, n, 5)
		return &AnyArray{r, pl, n, p}
	case tObj:
		n, p := r.varint(pl)
		r.needMul(p, n, 9)
		return &AnyObject{r, pl, n, p}
	}
	fail("E_BIN_INVALID", "unknown any-tag")
	return nil
}

// StructView is a zero-copy view of a struct instance.
type StructView struct {
	r        *reader
	idx, loc int
}

func (v *StructView) Keys() []string {
	var out []string
	for _, fl := range v.r.fields(v.idx).lay {
		out = append(out, fl.F.Name)
	}
	return out
}

// Get reads a field (nil for an absent optional/null value).
func (v *StructView) Get(name string) (val interface{}, err error) {
	defer catch(&err)
	return v.get(name), nil
}

type emptyArray struct{}

func defaultValue(f *Field) interface{} {
	d := f.Default
	if d == nil {
		return nil
	}
	switch d.Kind {
	case "bool":
		return d.B
	case "int":
		if d.Int.IsInt64() {
			return d.Int.Int64()
		}
		return d.Int.Uint64()
	case "float":
		if f.Type.Kind == "f32" {
			return float32(d.F)
		}
		return d.F
	case "string":
		return d.S
	case "array":
		return emptyArray{}
	}
	return nil
}

func (v *StructView) get(name string) interface{} {
	r := v.r
	fl := r.fields(v.idx).by[name]
	if fl == nil {
		f := r.sc.Types[v.idx].By[name]
		if f == nil {
			fail("E_UNKNOWN_FIELD", "no field "+name)
		}
		return defaultValue(f)
	}
	if fl.PByte >= 0 && (r.u8(v.loc+fl.PByte)>>fl.Bit)&1 == 0 {
		return nil
	}
	return r.slotGet(fl.F.Type, v.loc+fl.Off, v.loc)
}

type ArrayView struct {
	r                  *reader
	elem               *Type
	blk, n, start, str int
}

func (a *ArrayView) Len() int { return a.n }
func (a *ArrayView) At(i int) (val interface{}, err error) {
	defer catch(&err)
	return a.at(i), nil
}
func (a *ArrayView) at(i int) interface{} {
	if i < 0 || i >= a.n {
		fail("E_BIN_BOUNDS", "index out of range")
	}
	p := a.start + i*a.str
	if a.elem.Kind == "struct" {
		return a.r.structView(a.elem.Idx, p)
	}
	return a.r.slotGet(a.elem, p, a.blk)
}

type UnionView struct {
	r            *reader
	idx, pos, ms int
	Tag          int
}

func (u *UnionView) Name() string {
	for _, v := range u.r.sc.Types[u.idx].Variants {
		if v.Tag == u.Tag {
			return v.Name
		}
	}
	return ""
}
func (u *UnionView) Value() (val interface{}, err error) {
	defer catch(&err)
	return u.value(), nil
}
func (u *UnionView) value() interface{} {
	for _, v := range u.r.sc.Types[u.idx].Variants {
		if v.Tag == u.Tag {
			return u.r.heapGet(v.T, u.r.fwd(u.r.u32(u.pos+1), u.ms, "union payload"), u.ms)
		}
	}
	return nil
}

type AnyArray struct {
	r             *reader
	blk, n, start int
}

func (a *AnyArray) Len() int { return a.n }
func (a *AnyArray) At(i int) (val interface{}, err error) {
	defer catch(&err)
	if i < 0 || i >= a.n {
		fail("E_BIN_BOUNDS", "index out of range")
	}
	return a.r.anyGet(a.start+5*i, a.blk), nil
}

type AnyObject struct {
	r             *reader
	blk, n, start int
}

func (o *AnyObject) Len() int { return o.n }
func (o *AnyObject) Keys() (ks []string, err error) {
	defer catch(&err)
	for i := 0; i < o.n; i++ {
		ks = append(ks, o.r.hstr(o.r.u32(o.start+9*i)))
	}
	return
}
func (o *AnyObject) Get(key string) (val interface{}, err error) {
	defer catch(&err)
	for i := 0; i < o.n; i++ {
		if o.r.hstr(o.r.u32(o.start+9*i)) == key {
			return o.r.anyGet(o.start+9*i+4, o.blk), nil
		}
	}
	return nil, nil
}

// View opens a blob without copying: the returned values alias data. Root is *StructView (typed) or an any-view.
func View(data []byte, sc *Schema, o ReadOptions) (root interface{}, err error) {
	defer catch(&err)
	r := newReader(data, sc, o)
	if r.dynamic {
		return r.anyGet(r.root, r.root), nil
	}
	return r.structView(r.ridx, r.root), nil
}

// ---------------------------------------------------------------- owned decoder
type dec struct {
	r       *reader
	memo    map[[2]int]*Node
	tids    map[string]int
	pending []pend
	visited int
	budget  int
	used    int
	stypes  map[int]*Type
}
type pend struct {
	n   *Node
	loc int
	t   *Type
}

func (d *dec) node(n *Node, parent *Node, key interface{}, loc int, t *Type, depth int) *Node {
	if depth > MaxDepth {
		fail("E_BIN_LIMIT", "nesting too deep")
	}
	d.visited++
	if d.visited > d.r.n {
		fail("E_BIN_LIMIT", "decoded node count exceeds the blob size")
	}
	n.Pos = -1
	n.Parent = parent
	switch k := key.(type) {
	case string:
		n.Key = k
	case int:
		n.Index = k
	}
	if loc >= 0 {
		mk := [2]int{loc, d.tid(t)}
		if _, dup := d.memo[mk]; dup {
			fail("E_BIN_INVALID", "location is used by two values")
		}
		d.memo[mk] = n
	}
	return n
}
func (d *dec) tid(t *Type) int {
	k := t.key()
	id, ok := d.tids[k]
	if !ok {
		id = len(d.tids)
		d.tids[k] = id
	}
	return id
}
func (d *dec) charge(k int) {
	d.used += k
	if d.used > d.budget {
		fail("E_BIN_LIMIT", "decoded string/bytes volume exceeds the expansion limit")
	}
}
func (d *dec) run() *Node {
	r := d.r
	var root *Node
	if r.dynamic {
		root = d.anyNode(r.root, r.root, nil, nil, 0)
	} else {
		root = d.structNode(r.ridx, r.root, nil, nil, 0)
	}
	for _, p := range d.pending {
		tg := d.memo[[2]int{p.loc, d.tid(p.t)}]
		if tg == nil {
			fail("E_BIN_INVALID_REF", "reference does not hit a value of the expected type")
		}
		p.n.Target = tg
	}
	return root
}
func newObj() *Node { return &Node{Kind: "object", Map: map[string]*Node{}} }
func setMember(o *Node, k string, c *Node) {
	if _, dup := o.Map[k]; dup {
		fail("E_BIN_INVALID", "duplicate object key")
	}
	c.Parent, c.Key = o, k
	o.Map[k] = c
	o.Keys = append(o.Keys, k)
	o.Items = append(o.Items, c)
}

func (d *dec) anyNode(pos, ms int, parent *Node, key interface{}, depth int) *Node {
	r := d.r
	tag, pl := r.u8(pos), r.u32(pos+1)
	if tag <= tTrue {
		if pl != 0 {
			fail("E_BIN_INVALID", "nonzero payload on constant tag")
		}
		n := &Node{Kind: "null"}
		if tag != tNull {
			n = &Node{Kind: "bool", Bool: tag == tTrue}
		}
		return d.node(n, parent, key, pos, anyT, depth)
	}
	if tag == tI32 {
		return d.node(&Node{Kind: "int", Int: big.NewInt(int64(int32(le.Uint32(r.d[pos+1:]))))}, parent, key, pos, anyT, depth)
	}
	if tag == tRef {
		n := d.node(&Node{Kind: "ref"}, parent, key, -1, anyT, depth)
		d.pending = append(d.pending, pend{n, pl, anyT})
		r.need(pl, 5)
		return n
	}
	r.fwd(pl, ms, "value")
	switch tag {
	case tI64:
		return d.node(&Node{Kind: "int", Int: big.NewInt(r.scalar("i64", pl).(int64))}, parent, key, pos, anyT, depth)
	case tU64:
		return d.node(&Node{Kind: "int", Int: new(big.Int).SetUint64(r.scalar("u64", pl).(uint64))}, parent, key, pos, anyT, depth)
	case tF64:
		return d.node(&Node{Kind: "float", Float: r.scalar("f64", pl).(float64)}, parent, key, pos, anyT, depth)
	case tStr:
		s := r.hstr(pl)
		d.charge(len(s))
		return d.node(&Node{Kind: "string", Str: s}, parent, key, pos, anyT, depth)
	case tBytes:
		b := append([]byte(nil), r.hbytes(pl)...)
		d.charge(len(b))
		return d.node(&Node{Kind: "bytes", Bytes: b}, parent, key, pos, anyT, depth)
	case tArr:
		n, p := r.varint(pl)
		r.needMul(p, n, 5)
		node := d.node(&Node{Kind: "array"}, parent, key, pos, anyT, depth)
		for i := 0; i < n; i++ {
			node.Items = append(node.Items, d.anyNode(p+5*i, pl, node, i, depth+1))
		}
		return node
	case tObj:
		n, p := r.varint(pl)
		r.needMul(p, n, 9)
		node := d.node(newObj(), parent, key, pos, anyT, depth)
		for i := 0; i < n; i++ {
			e := p + 9*i
			k := r.hstr(r.u32(e))
			d.charge(len(k))
			if _, dup := node.Map[k]; dup {
				fail("E_BIN_INVALID", "duplicate object key")
			}
			c := d.anyNode(e+4, pl, node, k, depth+1)
			node.Map[k] = c
			node.Keys = append(node.Keys, k)
			node.Items = append(node.Items, c)
		}
		return node
	}
	fail("E_BIN_INVALID", "unknown any-tag")
	return nil
}

func (d *dec) structType(idx int) *Type {
	if d.stypes == nil {
		d.stypes = map[int]*Type{}
	}
	t, ok := d.stypes[idx]
	if !ok {
		t = &Type{Kind: "struct", Idx: idx}
		d.stypes[idx] = t
	}
	return t
}
func (d *dec) structNode(idx, base int, parent *Node, key interface{}, depth int) *Node {
	r := d.r
	fi := r.fields(idx)
	r.need(base, fi.size)
	node := d.node(newObj(), parent, key, base, d.structType(idx), depth)
	have := map[string]bool{}
	for i := range fi.lay {
		fl := &fi.lay[i]
		f := fl.F
		have[f.Name] = true
		if fl.PByte >= 0 && (r.u8(base+fl.PByte)>>fl.Bit)&1 == 0 {
			if f.Nullable {
				setMember(node, f.Name, &Node{Kind: "null", Pos: -1})
			}
			continue
		}
		setMember(node, f.Name, d.slotNode(f.Type, base+fl.Off, base, node, f.Name, depth+1))
	}
	for _, f := range r.sc.Types[idx].Fields {
		if !have[f.Name] && f.Default != nil {
			setMember(node, f.Name, mkDefault(f.Default))
		}
	}
	return node
}

func (d *dec) slotNode(t *Type, pos, ms int, parent *Node, key interface{}, depth int) *Node {
	r := d.r
	k := t.Kind
	switch {
	case k == "bool":
		return d.node(&Node{Kind: "bool", Bool: r.scalar("bool", pos).(bool)}, parent, key, pos, t, depth)
	case intBits[k][1] != 0:
		v := r.scalar(k, pos)
		var bi *big.Int
		switch x := v.(type) {
		case int64:
			bi = big.NewInt(x)
		case uint64:
			bi = new(big.Int).SetUint64(x)
		}
		return d.node(&Node{Kind: "int", Int: bi}, parent, key, pos, t, depth)
	case k == "f64":
		return d.node(&Node{Kind: "float", Float: r.scalar("f64", pos).(float64)}, parent, key, pos, t, depth)
	case k == "f32":
		return d.node(&Node{Kind: "f32", Float: float64(r.scalar("f32", pos).(float32))}, parent, key, pos, t, depth)
	case k == "enum":
		v, en := r.enumRaw(t, pos)
		for _, ev := range en.Values {
			if ev.V == v {
				return d.node(&Node{Kind: "string", Str: ev.Name}, parent, key, pos, t, depth)
			}
		}
		return d.node(&Node{Kind: "int", Int: big.NewInt(int64(v))}, parent, key, pos, t, depth)
	case k == "any":
		return d.anyNode(pos, ms, parent, key, depth)
	case k == "ref":
		n := d.node(&Node{Kind: "ref"}, parent, key, -1, t, depth)
		loc := r.u32(pos)
		r.need(loc, 1)
		d.pending = append(d.pending, pend{n, loc, t.Elem})
		return n
	case k == "union":
		un := r.sc.Types[t.Idx]
		tag := r.u8(pos)
		off := r.fwd(r.u32(pos+1), ms, "union payload")
		var v *Variant
		for i := range un.Variants {
			if un.Variants[i].Tag == tag {
				v = &un.Variants[i]
			}
		}
		if v == nil {
			fail("E_BIN_INVALID", "unknown union tag")
		}
		node := d.node(newObj(), parent, key, pos, t, depth)
		setMember(node, v.Name, d.heapNode(v.T, off, ms, node, v.Name, depth+1, -1))
		return node
	}
	off := r.fwd(r.u32(pos), ms, k)
	return d.heapNode(t, off, ms, parent, key, depth, pos)
}

func (d *dec) heapNode(t *Type, off, ms int, parent *Node, key interface{}, depth, slotpos int) *Node {
	r := d.r
	k := t.Kind
	switch k {
	case "string":
		s := r.hstr(off)
		d.charge(len(s))
		return d.node(&Node{Kind: "string", Str: s}, parent, key, slotpos, t, depth)
	case "bytes":
		b := append([]byte(nil), r.hbytes(off)...)
		d.charge(len(b))
		return d.node(&Node{Kind: "bytes", Bytes: b}, parent, key, slotpos, t, depth)
	case "struct":
		return d.structNode(t.Idx, off, parent, key, depth)
	case "array":
		av := r.arrayView(t, off)
		node := d.node(&Node{Kind: "array"}, parent, key, slotpos, t, depth)
		for i := 0; i < av.n; i++ {
			p := av.start + i*av.str
			var c *Node
			if t.Elem.Kind == "struct" {
				c = d.structNode(t.Elem.Idx, p, node, i, depth+1)
			} else {
				c = d.slotNode(t.Elem, p, off, node, i, depth+1)
			}
			node.Items = append(node.Items, c)
		}
		return node
	}
	if _, ok := scalarSize[k]; ok || k == "enum" {
		return d.slotNode(t, off, ms, parent, key, depth)
	}
	fail("E_BIN_INVALID", "type "+k+" cannot be stored by offset")
	return nil
}

// Decode fully validates a blob and builds an owned Document (aliasing preserved through Node.Target).
func Decode(data []byte, sc *Schema, o ReadOptions) (doc *Document, err error) {
	defer catch(&err)
	r := newReader(data, sc, o)
	b := 1 << 20
	if 256*r.n > b {
		b = 256 * r.n
	}
	d := &dec{r: r, memo: map[[2]int]*Node{}, tids: map[string]int{}, budget: b}
	return &Document{Root: d.run()}, nil
}

// BlobLength returns the total blob length from a header, for stream framing.
func BlobLength(h []byte) (int, error) {
	if len(h) < hdr {
		return 0, &Error{Code: "E_BIN_TRUNCATED", Message: "need 28 header bytes"}
	}
	if !bytes.Equal(h[:3], []byte("BJX")) {
		return 0, &Error{Code: "E_BIN_MAGIC", Message: "bad magic"}
	}
	return int(le.Uint32(h[24:])), nil
}
