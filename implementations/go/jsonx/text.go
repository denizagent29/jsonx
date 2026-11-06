// Package jsonx implements the JSONX text format and the BJSONX binary format.
package jsonx

import (
	"encoding/base64"
	"fmt"
	"math"
	"math/big"
	"strconv"
	"strings"
	"unicode/utf8"
)

// Error is the single error type of the library; Code values are shared with all implementations.
type Error struct {
	Code, Message string
	Pos           int
	Line, Col     int // 0 when unknown
}

func (e *Error) Error() string {
	if e.Line > 0 {
		return fmt.Sprintf("%s at %d:%d: %s", e.Code, e.Line, e.Col, e.Message)
	}
	return e.Code + ": " + e.Message
}

func fail(code, msg string)            { panic(&Error{Code: code, Message: msg, Pos: -1}) }
func failAt(code, msg string, pos int) { panic(&Error{Code: code, Message: msg, Pos: pos}) }

// catch converts a library panic into an error; foreign panics propagate.
func catch(err *error) {
	if r := recover(); r != nil {
		if e, ok := r.(*Error); ok {
			*err = e
			return
		}
		panic(r)
	}
}

func lineCol(src string, pos int) (int, int) {
	line, ls := 1, 0
	for i := 0; i < pos && i < len(src); i++ {
		switch src[i] {
		case '\n':
			line++
			ls = i + 1
		case '\r':
			if i+1 < len(src) && src[i+1] == '\n' {
				i++
			}
			line++
			ls = i + 1
		}
	}
	if pos > len(src) {
		pos = len(src)
	}
	return line, utf8.RuneCountInString(src[ls:pos]) + 1
}

const MaxDepth = 256

func isIDStart(c rune) bool {
	if c < 0x80 {
		return c == '_' || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
	}
	return nonASCIIOK(c)
}
func isIDChar(c rune) bool { return isIDStart(c) || (c >= '0' && c <= '9') }
func nonASCIIOK(o rune) bool {
	return !((o >= 0x80 && o <= 0xA0) || o == 0x1680 || (o >= 0x2000 && o <= 0x200F) || (o >= 0x2028 && o <= 0x202F) ||
		(o >= 0x205F && o <= 0x206F) || o == 0x3000 || o == 0xFEFF || o == 0xFFFE || o == 0xFFFF)
}
func isDigit(b byte) bool { return b >= '0' && b <= '9' }

// Node is a JSONX value. Kind: null bool int float f32 string bytes array object ref.
type Node struct {
	Kind   string
	Bool   bool
	Int    *big.Int
	Float  float64
	Str    string
	Bytes  []byte
	Pos    int
	Parent *Node
	Key    string // member name when Parent is an object
	Index  int    // element index when Parent is an array
	Items  []*Node
	Keys   []string
	Map    map[string]*Node
	Labels []*label
	lmap   map[string]*label
	expr   *expr
	Target *Node
}

type label struct {
	name, kind string
	expr       *expr
	pos        int
	owner      *Node
	target     *Node
}
type seg struct {
	kind string
	s    string
	i    int
}
type expr struct {
	start, name string
	segs        []seg
	text        string
	pos         int
}

// Path returns the canonical structural path (JSON-pointer style).
func (n *Node) Path() string {
	var parts []string
	for c := n; c.Parent != nil; c = c.Parent {
		var k string
		if c.Parent.Kind == "array" {
			k = strconv.Itoa(c.Index)
		} else {
			k = c.Key
		}
		parts = append(parts, strings.ReplaceAll(strings.ReplaceAll(k, "~", "~0"), "/", "~1"))
	}
	var b strings.Builder
	for i := len(parts) - 1; i >= 0; i-- {
		b.WriteString("/" + parts[i])
	}
	return b.String()
}

// Deref follows a reference node to its (non-reference) target.
func (n *Node) Deref() *Node {
	if n.Kind == "ref" {
		return n.Target
	}
	return n
}

// Get returns an object member (dereferencing references), or nil.
func (n *Node) Get(k string) *Node {
	n = n.Deref()
	if c, ok := n.Map[k]; ok {
		return c.Deref()
	}
	return nil
}

// At returns an array element (dereferencing references), or nil.
func (n *Node) At(i int) *Node {
	n = n.Deref()
	if n.Kind != "array" || i < 0 || i >= len(n.Items) {
		return nil
	}
	return n.Items[i].Deref()
}

// Document is a parsed and resolved JSONX document.
type Document struct {
	Root *Node
	Src  string
}

func (d *Document) Dump() string { return Dump(d.Root) }

type parser struct {
	s    string
	n, i int
}

func (p *parser) fail(code, msg string)            { failAt(code, msg, p.i) }
func (p *parser) failAt(code, msg string, pos int) { failAt(code, msg, pos) }
func (p *parser) cp(i int) rune {
	if i >= p.n {
		return -1
	}
	r, _ := utf8.DecodeRuneInString(p.s[i:])
	return r
}
func (p *parser) skip() {
	s, n := p.s, p.n
	i := p.i
	for i < n {
		c := s[i]
		if c == ' ' || c == '\t' || c == '\r' || c == '\n' {
			i++
		} else if c == '/' && i+1 < n && s[i+1] == '/' {
			i += 2
			for i < n && s[i] != '\r' && s[i] != '\n' {
				i++
			}
		} else if c == '/' && i+1 < n && s[i+1] == '*' {
			j := strings.Index(s[i+2:], "*/")
			if j < 0 {
				p.failAt("E_SYNTAX", "unterminated block comment", i)
			}
			i = i + 2 + j + 2
		} else {
			break
		}
	}
	p.i = i
}
func (p *parser) ident() string {
	st := p.i
	for p.i < p.n {
		r, sz := utf8.DecodeRuneInString(p.s[p.i:])
		if !isIDChar(r) {
			break
		}
		p.i += sz
	}
	return p.s[st:p.i]
}
func (p *parser) asciiIdent(at int) string {
	j := at
	if j < p.n && (p.s[j] == '_' || (p.s[j] >= 'a' && p.s[j] <= 'z') || (p.s[j] >= 'A' && p.s[j] <= 'Z')) {
		j++
		for j < p.n && (p.s[j] == '_' || isDigit(p.s[j]) || (p.s[j] >= 'a' && p.s[j] <= 'z') || (p.s[j] >= 'A' && p.s[j] <= 'Z')) {
			j++
		}
		return p.s[at:j]
	}
	return ""
}

func (p *parser) document() *Node {
	if strings.HasPrefix(p.s, "\ufeff") {
		p.i = 3
	}
	p.skip()
	if p.i >= p.n {
		p.fail("E_UNEXPECTED_EOF", "empty document: expected a value")
	}
	v := p.value(0)
	p.skip()
	if p.i < p.n {
		p.fail("E_SYNTAX", "unexpected content after the root value")
	}
	return v
}

func (p *parser) value(depth int) *Node {
	p.skip()
	i := p.i
	if i >= p.n {
		p.fail("E_UNEXPECTED_EOF", "unexpected end of input: expected a value")
	}
	c := p.s[i]
	switch {
	case c == '{':
		return p.obj(depth)
	case c == '[':
		return p.arr(depth)
	case c == '"':
		return &Node{Kind: "string", Str: p.str(), Pos: i}
	case c == '@':
		return p.ref()
	case c == '-' || isDigit(c):
		return p.number()
	case c == '+' || c == '.':
		p.fail("E_INVALID_NUMBER", "numbers cannot start with '"+string(c)+"'")
	}
	if isIDStart(p.cp(i)) {
		w := p.ident()
		switch w {
		case "true":
			return &Node{Kind: "bool", Bool: true, Pos: i}
		case "false":
			return &Node{Kind: "bool", Pos: i}
		case "null":
			return &Node{Kind: "null", Pos: i}
		}
		p.failAt("E_SYNTAX", "unexpected identifier '"+w+"': expected a value", i)
	}
	if c == '$' {
		p.fail("E_SYNTAX", "unexpected '$': contextual variables are only valid inside references")
	}
	p.fail("E_SYNTAX", "unexpected character")
	return nil
}

var (
	minInt = new(big.Int).Neg(new(big.Int).Lsh(big.NewInt(1), 63))
	maxInt = new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), 64), big.NewInt(1))
)

func (p *parser) number() *Node {
	s, i, n := p.s, p.i, p.n
	j := i
	if s[j] == '-' {
		j++
	}
	if j >= n || !isDigit(s[j]) {
		p.fail("E_INVALID_NUMBER", "'-' must be followed by a digit")
	}
	if s[j] == '0' {
		j++
	} else {
		for j < n && isDigit(s[j]) {
			j++
		}
	}
	isF := false
	if j+1 < n && s[j] == '.' && isDigit(s[j+1]) {
		j++
		for j < n && isDigit(s[j]) {
			j++
		}
		isF = true
	}
	if j < n && (s[j] == 'e' || s[j] == 'E') {
		k := j + 1
		if k < n && (s[k] == '+' || s[k] == '-') {
			k++
		}
		if k < n && isDigit(s[k]) {
			for k < n && isDigit(s[k]) {
				k++
			}
			j = k
			isF = true
		}
	}
	if j < n {
		c := p.cp(j)
		if c == '.' || (c >= '0' && c <= '9') || isIDChar(c) {
			p.failAt("E_INVALID_NUMBER", "malformed number", i)
		}
	}
	p.i = j
	txt := s[i:j]
	if !isF {
		v, _ := new(big.Int).SetString(txt, 10)
		if v.Cmp(minInt) < 0 || v.Cmp(maxInt) > 0 {
			p.failAt("E_NUMBER_OVERFLOW", "integer "+txt+" does not fit in [-2^63, 2^64-1]", i)
		}
		return &Node{Kind: "int", Int: v, Pos: i}
	}
	f, _ := strconv.ParseFloat(txt, 64)
	if math.IsInf(f, 0) {
		p.failAt("E_NUMBER_OVERFLOW", "float "+txt+" overflows IEEE-754 binary64", i)
	}
	return &Node{Kind: "float", Float: f, Pos: i}
}

func isHex(s string) bool {
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !(isDigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F')) {
			return false
		}
	}
	return true
}
func (p *parser) hex4(i int) int {
	if i+4 > p.n || !isHex(p.s[i:i+4]) {
		p.failAt("E_INVALID_ESCAPE", "\\u needs exactly 4 hex digits", i-2)
	}
	v, _ := strconv.ParseInt(p.s[i:i+4], 16, 32)
	return int(v)
}
func (p *parser) str() string {
	s, n := p.s, p.n
	i := p.i + 1
	var out strings.Builder
	start := i
	for {
		if i >= n {
			p.failAt("E_UNEXPECTED_EOF", "unterminated string", p.i)
		}
		c := s[i]
		if c == '"' {
			out.WriteString(s[start:i])
			p.i = i + 1
			return out.String()
		}
		if c == '\\' {
			out.WriteString(s[start:i])
			i++
			if i >= n {
				p.failAt("E_UNEXPECTED_EOF", "unterminated escape", i)
			}
			e := s[i]
			switch e {
			case '"', '\\', '/':
				out.WriteByte(e)
				i++
			case 'b':
				out.WriteByte('\b')
				i++
			case 'f':
				out.WriteByte('\f')
				i++
			case 'n':
				out.WriteByte('\n')
				i++
			case 'r':
				out.WriteByte('\r')
				i++
			case 't':
				out.WriteByte('\t')
				i++
			case 'u':
				if i+1 < n && s[i+1] == '{' {
					j := strings.IndexByte(s[i+2:], '}')
					h := ""
					if j >= 0 {
						h = s[i+2 : i+2+j]
					}
					if len(h) < 1 || len(h) > 6 || !isHex(h) {
						p.failAt("E_INVALID_ESCAPE", "malformed \\u{...} escape", i-1)
					}
					cp, _ := strconv.ParseInt(h, 16, 64)
					if cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF) {
						p.failAt("E_INVALID_ESCAPE", "not a Unicode scalar value", i-1)
					}
					out.WriteRune(rune(cp))
					i = i + 2 + j + 1
				} else {
					cp := p.hex4(i + 1)
					i += 5
					if cp >= 0xD800 && cp <= 0xDBFF {
						if i+2 > n || s[i:i+2] != "\\u" {
							p.failAt("E_INVALID_ESCAPE", "lone high surrogate", i-6)
						}
						lo := p.hex4(i + 2)
						if !(lo >= 0xDC00 && lo <= 0xDFFF) {
							p.failAt("E_INVALID_ESCAPE", "high surrogate not followed by low surrogate", i-6)
						}
						cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00)
						i += 6
					} else if cp >= 0xDC00 && cp <= 0xDFFF {
						p.failAt("E_INVALID_ESCAPE", "lone low surrogate", i-6)
					}
					out.WriteRune(rune(cp))
				}
			default:
				p.failAt("E_INVALID_ESCAPE", "unknown escape", i-1)
			}
			start = i
			continue
		}
		if c < 0x20 {
			p.failAt("E_SYNTAX", "unescaped control character in string", i)
		}
		i++
	}
}

func (p *parser) obj(depth int) *Node {
	if depth >= MaxDepth {
		p.fail("E_DEPTH_LIMIT", "nesting too deep")
	}
	node := &Node{Kind: "object", Pos: p.i, Map: map[string]*Node{}}
	p.i++
	for {
		p.skip()
		if p.i >= p.n {
			p.fail("E_UNEXPECTED_EOF", "unterminated object")
		}
		c := p.s[p.i]
		if c == '}' {
			p.i++
			break
		}
		if c == '<' {
			node.Labels = append(node.Labels, p.label())
			continue
		}
		kpos := p.i
		var key string
		if c == '"' {
			key = p.str()
		} else if isIDStart(p.cp(p.i)) {
			key = p.ident()
		} else {
			p.fail("E_SYNTAX", "expected an object key")
		}
		p.skip()
		if p.i >= p.n || p.s[p.i] != ':' {
			p.fail("E_SYNTAX", "expected ':' after object key")
		}
		p.i++
		v := p.value(depth + 1)
		if _, dup := node.Map[key]; dup {
			p.failAt("E_DUPLICATE_KEY", "duplicate key", kpos)
		}
		v.Parent, v.Key = node, key
		node.Map[key] = v
		node.Keys = append(node.Keys, key)
		node.Items = append(node.Items, v)
		p.skip()
		if p.i >= p.n {
			p.fail("E_UNEXPECTED_EOF", "unterminated object")
		}
		d := p.s[p.i]
		if d == ',' {
			p.i++
		} else if d == '}' {
			p.i++
			break
		} else {
			p.fail("E_SYNTAX", "expected ',' or '}' after value")
		}
	}
	return node
}

func (p *parser) arr(depth int) *Node {
	if depth >= MaxDepth {
		p.fail("E_DEPTH_LIMIT", "nesting too deep")
	}
	node := &Node{Kind: "array", Pos: p.i}
	p.i++
	for {
		p.skip()
		if p.i >= p.n {
			p.fail("E_UNEXPECTED_EOF", "unterminated array")
		}
		if p.s[p.i] == ']' {
			p.i++
			break
		}
		v := p.value(depth + 1)
		v.Parent, v.Index = node, len(node.Items)
		node.Items = append(node.Items, v)
		p.skip()
		if p.i >= p.n {
			p.fail("E_UNEXPECTED_EOF", "unterminated array")
		}
		d := p.s[p.i]
		if d == ',' {
			p.i++
		} else if d == ']' {
			p.i++
			break
		} else {
			p.fail("E_SYNTAX", "expected ',' or ']' after element")
		}
	}
	return node
}

func (p *parser) label() *label {
	pos := p.i
	p.i++
	p.skip()
	npos := p.i
	v := false
	if p.i < p.n && p.s[p.i] == '$' {
		v = true
		p.i++
	}
	name := p.asciiIdent(p.i)
	if name == "" {
		p.failAt("E_INVALID_LABEL_NAME", "expected a label name after '<'", npos)
	}
	p.i += len(name)
	if p.i < p.n && isIDChar(p.cp(p.i)) {
		p.failAt("E_INVALID_LABEL_NAME", "label names are ASCII", npos)
	}
	var kind string
	if v {
		kind = "var"
		name = "$" + name
		if name == "$prev" || name == "$next" {
			p.failAt("E_RESERVED_LABEL", "'"+name+"' is a built-in contextual variable", pos)
		}
	} else if name[0] == '_' {
		p.failAt("E_INVALID_LABEL_NAME", "label names must start with a letter", npos)
	} else if name[0] >= 'A' && name[0] <= 'Z' {
		kind = "global"
	} else {
		kind = "local"
	}
	p.skip()
	var ex *expr
	if p.i < p.n && p.s[p.i] == '=' {
		p.i++
		p.skip()
		at := p.i
		if p.i < p.n && p.s[p.i] == '@' {
			p.i++
		}
		ex = p.path(at)
		p.skip()
	}
	if p.i >= p.n || p.s[p.i] != '>' {
		p.fail("E_SYNTAX", "expected '>' to close the label declaration")
	}
	p.i++
	return &label{name: name, kind: kind, expr: ex, pos: pos}
}

func (p *parser) segStart() bool {
	if p.i >= p.n {
		return false
	}
	c := p.s[p.i]
	return c == '!' || c == '"' || c == '$' || isIDChar(p.cp(p.i))
}
func (p *parser) segment() seg {
	s, i := p.s, p.i
	var c byte
	if i < p.n {
		c = s[i]
	}
	switch {
	case c == '!':
		p.i++
		return seg{kind: "parent"}
	case c == '"':
		return seg{kind: "key", s: p.str()}
	case c == '$':
		nm := p.asciiIdent(i + 1)
		if nm == "" {
			p.fail("E_INVALID_POINTER", "expected a name after '$'")
		}
		p.i = i + 1 + len(nm)
		if nm == "prev" {
			return seg{kind: "prev"}
		}
		if nm == "next" {
			return seg{kind: "next"}
		}
		return seg{kind: "desc", s: nm}
	case isDigit(c):
		j := i
		for j < p.n && isDigit(s[j]) {
			j++
		}
		if j < p.n && isIDStart(p.cp(j)) {
			p.fail("E_INVALID_POINTER", "malformed array index")
		}
		t := s[i:j]
		if len(t) > 1 && t[0] == '0' {
			p.fail("E_INVALID_POINTER", "array indexes have no leading zeros")
		}
		v, err := strconv.Atoi(t)
		if err != nil {
			v = math.MaxInt32
		}
		p.i = j
		return seg{kind: "index", i: v}
	}
	if i < p.n && isIDStart(p.cp(i)) {
		return seg{kind: "key", s: p.ident()}
	}
	p.fail("E_INVALID_POINTER", "expected a field name, index, '$name' or '!' in pointer")
	return seg{}
}

func (p *parser) path(at int) *expr {
	s := p.s
	var c byte
	if p.i < p.n {
		c = s[p.i]
	}
	var segs []seg
	var start, name string
	switch {
	case c == '!':
		p.i++
		if !(p.i < p.n && (isIDStart(p.cp(p.i)) || s[p.i] == '"')) {
			p.fail("E_INVALID_POINTER", "'@!' must be followed by a field name")
		}
		start = "cur"
		segs = append(segs, p.segment())
	case c == '.':
		if p.i+1 < p.n && s[p.i+1] == '.' {
			p.i += 2
			nm := p.asciiIdent(p.i)
			if nm == "" {
				p.fail("E_INVALID_POINTER", "'@..' must be followed by a global label name")
			}
			p.i += len(nm)
			start, name = "global", nm
		} else {
			p.i++
			start = "cur"
			if p.segStart() {
				segs = append(segs, p.segment())
			}
		}
	case c == '$':
		sg := p.segment()
		if sg.kind == "prev" || sg.kind == "next" {
			start, name = "ctx", sg.kind
		} else {
			start, name = "var", "$"+sg.s
		}
	case c != 0 && isIDStart(p.cp(p.i)):
		start, name = "name", p.ident()
	default:
		p.fail("E_INVALID_POINTER", "expected a label, field name, '.', '!' or '$' after '@'")
	}
	for p.i < p.n && s[p.i] == '.' {
		p.i++
		segs = append(segs, p.segment())
	}
	return &expr{start: start, name: name, segs: segs, text: s[at:p.i], pos: at}
}

func (p *parser) ref() *Node {
	at := p.i
	p.i++
	n := &Node{Kind: "ref", Pos: at}
	n.expr = p.path(at)
	return n
}

// ---------------------------------------------------------------- resolver
type resolver struct {
	root    *Node
	src     string
	globals map[string]*label
	byName  map[string][]*label
	refs    []*Node
	labels  []*label
	active  map[interface{}]bool
}

func (r *resolver) fail(code, msg string, pos int) { failAt(code, msg, pos) }
func (r *resolver) collect(n *Node) {
	switch n.Kind {
	case "object":
		seen := map[string]*label{}
		for _, lb := range n.Labels {
			lb.owner = n
			if lb.kind == "global" {
				if _, ok := r.globals[lb.name]; ok {
					r.fail("E_DUPLICATE_LABEL", "global label '"+lb.name+"' is already defined", lb.pos)
				}
				r.globals[lb.name] = lb
			} else {
				if _, ok := seen[lb.name]; ok {
					r.fail("E_DUPLICATE_LABEL", "label '"+lb.name+"' is declared twice in the same object", lb.pos)
				}
				seen[lb.name] = lb
			}
			r.byName[lb.name] = append(r.byName[lb.name], lb)
			r.labels = append(r.labels, lb)
		}
		n.lmap = seen
		for _, c := range n.Items {
			r.collect(c)
		}
	case "array":
		for _, c := range n.Items {
			r.collect(c)
		}
	case "ref":
		r.refs = append(r.refs, n)
	}
}
func (r *resolver) run() {
	r.collect(r.root)
	for _, l := range r.labels {
		r.labelTarget(l)
	}
	for _, n := range r.refs {
		r.target(n)
	}
}
func (r *resolver) target(n *Node) *Node {
	if n.Target != nil {
		return n.Target
	}
	if r.active[n] {
		r.fail("E_CYCLIC_REFERENCE", "reference depends on itself", n.Pos)
	}
	r.active[n] = true
	t := r.eval(n.expr, n, false, n.Pos)
	for t.Kind == "ref" {
		t = r.target(t)
	}
	delete(r.active, n)
	n.Target = t
	return t
}
func (r *resolver) labelTarget(lb *label) *Node {
	if lb.target != nil {
		return lb.target
	}
	if lb.expr == nil {
		lb.target = lb.owner
		return lb.owner
	}
	if r.active[lb] {
		r.fail("E_CYCLIC_REFERENCE", "label '"+lb.name+"' is defined in terms of itself", lb.pos)
	}
	r.active[lb] = true
	t := r.eval(lb.expr, lb.owner, true, lb.pos)
	for t.Kind == "ref" {
		t = r.target(t)
	}
	delete(r.active, lb)
	lb.target = t
	return t
}
func (r *resolver) deref(n *Node) *Node {
	for n.Kind == "ref" {
		n = r.target(n)
	}
	return n
}
func (r *resolver) elem(loc *Node) *Node {
	for n := loc; n != nil; n = n.Parent {
		if n.Parent != nil && n.Parent.Kind == "array" {
			return n
		}
	}
	return nil
}
func (r *resolver) findLabel(name string, loc *Node) *label {
	for n := loc; n != nil; n = n.Parent {
		if n.Kind == "object" && n.lmap != nil {
			if l, ok := n.lmap[name]; ok {
				return l
			}
		}
	}
	return r.globals[name]
}
func (r *resolver) describe(n *Node) string {
	if p := n.Path(); p != "" {
		return "'" + p + "'"
	}
	return "the root"
}
func (r *resolver) search(n *Node, name string, out *[]*label) {
	switch n.Kind {
	case "object":
		for _, lb := range n.Labels {
			if lb.name == name || lb.name == "$"+name {
				*out = append(*out, lb)
			}
		}
		for _, c := range n.Items {
			r.search(c, name, out)
		}
	case "array":
		for _, c := range n.Items {
			r.search(c, name, out)
		}
	}
}
func (r *resolver) eval(ex *expr, loc *Node, isLab bool, epos int) *Node {
	e := func(code, msg string) { r.fail(code, msg, epos) }
	segs := append([]seg(nil), ex.segs...)
	var cur *Node
	switch ex.start {
	case "cur":
		if isLab {
			cur = loc
		} else {
			cur = loc.Parent
			for cur != nil && cur.Kind != "object" {
				cur = cur.Parent
			}
		}
		if cur == nil {
			e("E_INVALID_CURRENT", "there is no enclosing object")
		}
	case "global":
		lb := r.globals[ex.name]
		if lb == nil {
			e("E_UNDEFINED_LABEL", "global label '"+ex.name+"' is not defined")
		}
		cur = r.labelTarget(lb)
	case "ctx":
		cur = r.elem(loc)
		if cur == nil {
			c := "E_INVALID_PREV"
			if ex.name == "next" {
				c = "E_INVALID_NEXT"
			}
			e(c, "'$"+ex.name+"' used where there is no enclosing array element")
		}
		segs = append([]seg{{kind: ex.name}}, segs...)
	default:
		nm := ex.name
		lb := r.findLabel(nm, loc)
		rk := false
		if ex.start == "name" && r.root.Kind == "object" {
			_, rk = r.root.Map[nm]
		}
		if lb != nil && rk {
			e("E_AMBIGUOUS_NAME", "'"+nm+"' names both a label and a field of the root object")
		}
		if lb != nil {
			cur = r.labelTarget(lb)
		} else if rk {
			cur = r.root.Map[nm]
		} else {
			if o := r.byName[nm]; len(o) > 0 {
				e("E_LABEL_OUT_OF_SCOPE", "label '"+nm+"' is not visible from here")
			}
			e("E_UNDEFINED_LABEL", "no label or root field named '"+nm+"'")
		}
	}
	for _, sg := range segs {
		switch sg.kind {
		case "key", "index":
			cur = r.deref(cur)
			if sg.kind == "key" {
				if cur.Kind != "object" {
					e("E_POINTER_TYPE", "cannot select field '"+sg.s+"' of a "+cur.Kind)
				}
				c, ok := cur.Map[sg.s]
				if !ok {
					e("E_POINTER_NO_FIELD", r.describe(cur)+" has no field '"+sg.s+"'")
				}
				cur = c
			} else {
				if cur.Kind != "array" {
					e("E_POINTER_TYPE", "cannot index a "+cur.Kind)
				}
				if sg.i >= len(cur.Items) {
					e("E_INVALID_INDEX", fmt.Sprintf("index %d is out of range (length %d)", sg.i, len(cur.Items)))
				}
				cur = cur.Items[sg.i]
			}
		case "parent":
			if cur.Parent == nil {
				e("E_INVALID_PARENT", r.describe(cur)+" has no parent")
			}
			cur = cur.Parent
		case "prev", "next":
			p := cur.Parent
			code := "E_INVALID_PREV"
			if sg.kind == "next" {
				code = "E_INVALID_NEXT"
			}
			if p == nil || p.Kind != "array" {
				e(code, "'$"+sg.kind+"' needs an array element")
			}
			i := cur.Index - 1
			if sg.kind == "next" {
				i = cur.Index + 1
			}
			if i < 0 {
				e(code, "'$prev' used on the first element: there is no previous element")
			}
			if i >= len(p.Items) {
				e(code, "'$next' used on the last element: there is no next element")
			}
			cur = p.Items[i]
		default:
			var found []*label
			r.search(cur, sg.s, &found)
			if len(found) == 0 {
				e("E_UNDEFINED_LABEL", "no label '"+sg.s+"' is declared inside "+r.describe(cur))
			}
			if len(found) > 1 {
				e("E_AMBIGUOUS_LABEL", "label '"+sg.s+"' is declared more than once inside "+r.describe(cur))
			}
			cur = r.labelTarget(found[0])
		}
	}
	return cur
}

// Parse parses JSONX text, validates it and resolves every label and pointer.
func Parse(text string) (doc *Document, err error) {
	if !utf8.ValidString(text) {
		return nil, &Error{Code: "E_INVALID_UTF8", Message: "input is not valid UTF-8", Pos: -1}
	}
	defer func() {
		if r := recover(); r != nil {
			if e, ok := r.(*Error); ok {
				if e.Line == 0 && e.Pos >= 0 {
					e.Line, e.Col = lineCol(text, e.Pos)
				}
				doc, err = nil, e
				return
			}
			panic(r)
		}
	}()
	p := &parser{s: text, n: len(text)}
	root := p.document()
	rs := &resolver{root: root, src: text, globals: map[string]*label{}, byName: map[string][]*label{}, active: map[interface{}]bool{}}
	rs.run()
	return &Document{Root: root, Src: text}, nil
}

// ---------------------------------------------------------------- canonical dump
func jstr(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, r := range s {
		switch {
		case r == '"':
			b.WriteString("\\\"")
		case r == '\\':
			b.WriteString("\\\\")
		case r < 0x20 || r == 0x7f:
			fmt.Fprintf(&b, "\\u%04x", r)
		default:
			b.WriteRune(r)
		}
	}
	b.WriteByte('"')
	return b.String()
}

// Dump renders the canonical dump format shared by all implementations.
func Dump(n *Node) string {
	var b strings.Builder
	dump(n, &b)
	return b.String()
}
func dump(n *Node, b *strings.Builder) {
	switch n.Kind {
	case "null":
		b.WriteString("null")
	case "bool":
		if n.Bool {
			b.WriteString("true")
		} else {
			b.WriteString("false")
		}
	case "int":
		b.WriteString(n.Int.String())
	case "float":
		fmt.Fprintf(b, "f64(0x%016x)", math.Float64bits(n.Float))
	case "f32":
		fmt.Fprintf(b, "f32(0x%08x)", math.Float32bits(float32(n.Float)))
	case "string":
		b.WriteString(jstr(n.Str))
	case "bytes":
		b.WriteString("bytes(\"" + base64.StdEncoding.EncodeToString(n.Bytes) + "\")")
	case "ref":
		b.WriteString("ref(" + jstr(n.Target.Path()) + ")")
	case "array":
		b.WriteByte('[')
		for i, c := range n.Items {
			if i > 0 {
				b.WriteByte(',')
			}
			dump(c, b)
		}
		b.WriteByte(']')
	default:
		b.WriteByte('{')
		for i, k := range n.Keys {
			if i > 0 {
				b.WriteByte(',')
			}
			b.WriteString(jstr(k) + ":")
			dump(n.Map[k], b)
		}
		b.WriteByte('}')
	}
}
