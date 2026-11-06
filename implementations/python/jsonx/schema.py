"""JSONX schema language: parser, model, canonical binary schema (BJXS) and struct layout."""
from __future__ import annotations
import re, struct, base64
from typing import Callable, Dict, List, Optional, Tuple
from .errors import JsonxError, line_col

INTS = {"u8": (0, 8), "u16": (0, 16), "u32": (0, 32), "u64": (0, 64),
        "i8": (1, 8), "i16": (1, 16), "i32": (1, 32), "i64": (1, 64)}
FLOATS = ("f32", "f64")
SCALARS = set(INTS) | set(FLOATS) | {"bool"}
PRIMS = SCALARS | {"string", "bytes", "any"}
SCALAR_SIZE = {"bool": 1, "u8": 1, "i8": 1, "u16": 2, "i16": 2, "u32": 4, "i32": 4, "f32": 4,
               "u64": 8, "i64": 8, "f64": 8}
TCODE = {"bool": 1, "u8": 2, "u16": 3, "u32": 4, "u64": 5, "i8": 6, "i16": 7, "i32": 8, "i64": 9,
         "f32": 10, "f64": 11, "string": 12, "bytes": 13, "any": 14}
CODE_T = {v: k for k, v in TCODE.items()}
T_STRUCT, T_ENUM, T_UNION, T_ARRAY, T_REF = 20, 21, 22, 23, 24
MASK64 = (1 << 64) - 1

def fnv1a64(b: bytes) -> int:
    h = 0xCBF29CE484222325
    for x in b:
        h = ((h ^ x) * 0x100000001B3) & MASK64
    return h

class T:
    """A type descriptor."""
    __slots__ = ("kind", "idx", "elem")
    def __init__(self, kind, idx=None, elem=None):
        self.kind, self.idx, self.elem = kind, idx, elem
    def key(self) -> str:
        k = self.kind
        if k in ("struct", "enum", "union"): return f"{k}:{self.idx}"
        if k in ("array", "ref"): return f"{k}<{self.elem.key()}>"
        return k
    def __eq__(self, o): return isinstance(o, T) and self.key() == o.key()
    def __hash__(self): return hash(self.key())
    def __repr__(self): return self.key()

class Field:
    __slots__ = ("name", "type", "optional", "nullable", "since", "deprecated", "default", "attrs", "cons")
    def __init__(self, name, type_):
        self.name, self.type = name, type_
        self.optional = self.nullable = self.deprecated = False
        self.since = 1; self.default = None; self.attrs: List[Tuple[str, str]] = []; self.cons: dict = {}

class Struct:
    kind = "struct"
    def __init__(self, name): self.name = name; self.fields: List[Field] = []; self.by_name: dict = {}
class Enum:
    kind = "enum"
    def __init__(self, name, base): self.name, self.base = name, base; self.values: List[Tuple[str, int]] = []
class Union:
    kind = "union"
    def __init__(self, name): self.name = name; self.variants: List[Tuple[str, int, T]] = []

class FieldLayout:
    __slots__ = ("field", "off", "pbyte", "bit")
    def __init__(self, field, off, pbyte, bit): self.field, self.off, self.pbyte, self.bit = field, off, pbyte, bit

ENUM_SIZE = {"u8": 1, "u16": 2, "u32": 4}

class Schema:
    def __init__(self):
        self.namespace = ""; self.version = 1
        self.types: list = []; self.by_name: Dict[str, int] = {}
        self.root: Optional[int] = None
        self._lay: dict = {}

    # ---- helpers
    def slot_size(self, t: T) -> int:
        k = t.kind
        if k in SCALAR_SIZE: return SCALAR_SIZE[k]
        if k == "enum": return ENUM_SIZE[self.types[t.idx].base]
        if k in ("string", "bytes", "array", "struct", "ref"): return 4
        return 5  # union, any

    def layout(self, idx: int, version: int):
        key = (idx, version)
        if key in self._lay: return self._lay[key]
        out, off, nopt, pres = [], 0, 0, []
        for f in self.types[idx].fields:
            if f.since > version: continue
            pb = bit = None
            if f.optional or f.nullable:
                if nopt % 8 == 0: pres.append(off); off += 1
                pb, bit = pres[nopt // 8], nopt % 8; nopt += 1
            out.append(FieldLayout(f, off, pb, bit)); off += self.slot_size(f.type)
        self._lay[key] = (out, off)
        return out, off

    @property
    def id(self) -> int:
        if self.root is None: return 0
        return fnv1a64((self.namespace + "\0" + self.types[self.root].name).encode())

    def fingerprint(self) -> int:
        return fnv1a64(self.to_bytes())

    def type_name(self, t: T) -> str:
        k = t.kind
        if k in ("struct", "enum", "union"): return self.types[t.idx].name
        if k in ("array", "ref"): return f"{k}<{self.type_name(t.elem)}>"
        return k

    def struct_index(self, name: str) -> int:
        i = self.by_name.get(name)
        if i is None or self.types[i].kind != "struct":
            raise JsonxError("E_SCHEMA_UNKNOWN_TYPE", f"no schema named '{name}'")
        return i

    # ---- derived data
    def finish(self):
        for t in self.types:
            if t.kind != "struct": continue
            t.by_name = {f.name: f for f in t.fields}
            for f in t.fields:
                f.cons = {}
                for n, a in f.attrs:
                    if n in ("range", "len"):
                        parts = [x.strip() for x in a.split(",")]
                        if len(parts) != 2: raise JsonxError("E_SCHEMA_INVALID", f"@{n} takes two arguments (use _ for unbounded)")
                        def conv(x):
                            if x == "_": return None
                            try: return int(x)
                            except ValueError:
                                try: return float(x)
                                except ValueError: raise JsonxError("E_SCHEMA_INVALID", f"@{n}: bad number '{x}'") from None
                        f.cons[n] = (conv(parts[0]), conv(parts[1]))
        self._lay = {}

    # ---- canonical binary form
    def to_bytes(self) -> bytes:
        o = bytearray(b"BJXS\x01")
        def vi(v):
            while True:
                b = v & 0x7F; v >>= 7
                if v: o.append(b | 0x80)
                else: o.append(b); return
        def st(s): b = s.encode(); vi(len(b)); o.extend(b)
        def td(t):
            k = t.kind
            if k in TCODE: o.append(TCODE[k])
            elif k == "struct": o.append(T_STRUCT); vi(t.idx)
            elif k == "enum": o.append(T_ENUM); vi(t.idx)
            elif k == "union": o.append(T_UNION); vi(t.idx)
            elif k == "array": o.append(T_ARRAY); td(t.elem)
            else: o.append(T_REF); td(t.elem)
        st(self.namespace); vi(self.version); vi(0 if self.root is None else self.root + 1); vi(len(self.types))
        for t in self.types:
            o.append({"struct": 1, "enum": 2, "union": 3}[t.kind]); st(t.name)
            if t.kind == "struct":
                vi(len(t.fields))
                for f in t.fields:
                    st(f.name); td(f.type)
                    o.append((1 if f.optional else 0) | (2 if f.nullable else 0) | (4 if f.deprecated else 0) | (8 if f.default is not None else 0))
                    vi(f.since)
                    if f.default is not None:
                        k, v = f.default
                        if k == "null": o.append(0)
                        elif k == "bool": o.append(2 if v else 1)
                        elif k == "int":
                            if v >= (1 << 63): o.append(4); vi(v)
                            else: o.append(3); vi(((v << 1) ^ (v >> 63)) & MASK64)
                        elif k == "float": o.append(5); o.extend(struct.pack("<d", v))
                        elif k == "string": o.append(6); st(v)
                        else: o.append(7)
                    vi(len(f.attrs))
                    for n, a in f.attrs: st(n); st(a)
            elif t.kind == "enum":
                o.append({"u8": 1, "u16": 2, "u32": 4}[t.base]); vi(len(t.values))
                for n, v in t.values: st(n); vi(v)
            else:
                vi(len(t.variants))
                for n, tag, ty in t.variants: st(n); o.append(tag); td(ty)
        return bytes(o)

    @staticmethod
    def from_bytes(data: bytes) -> "Schema":
        d = memoryview(bytes(data)); p = [0]
        def bad(m): raise JsonxError("E_SCHEMA_BINARY", m)
        def u8():
            if p[0] >= len(d): bad("truncated schema")
            v = d[p[0]]; p[0] += 1; return v
        def vi():
            v = 0; sh = 0
            for _ in range(10):
                b = u8(); v |= (b & 0x7F) << sh; sh += 7
                if not b & 0x80: return v
            bad("varint too long")
        def st():
            n = vi()
            if p[0] + n > len(d): bad("truncated string")
            s = bytes(d[p[0]:p[0] + n]).decode("utf-8", "replace"); p[0] += n; return s
        def td(depth=0):
            if depth > 32: bad("type nesting too deep")
            c = u8()
            if c in CODE_T: return T(CODE_T[c])
            if c == T_STRUCT: return T("struct", vi())
            if c == T_ENUM: return T("enum", vi())
            if c == T_UNION: return T("union", vi())
            if c == T_ARRAY: return T("array", elem=td(depth + 1))
            if c == T_REF: return T("ref", elem=td(depth + 1))
            bad(f"unknown type code {c}")
        if bytes(d[:4]) != b"BJXS" or len(d) < 5 or d[4] != 1: bad("not a BJXS v1 schema")
        p[0] = 5
        s = Schema(); s.namespace = st(); s.version = vi(); r = vi(); s.root = None if r == 0 else r - 1
        n = vi()
        if n > 100000: bad("too many types")
        for _ in range(n):
            k = u8(); name = st()
            if k == 1:
                t = Struct(name)
                for _ in range(vi()):
                    f = Field(st(), td()); fl = u8()
                    f.optional, f.nullable, f.deprecated = bool(fl & 1), bool(fl & 2), bool(fl & 4)
                    f.since = vi()
                    if fl & 8:
                        c = u8()
                        if c == 0: f.default = ("null", None)
                        elif c in (1, 2): f.default = ("bool", c == 2)
                        elif c == 3:
                            z = vi(); f.default = ("int", (z >> 1) ^ -(z & 1))
                        elif c == 4: f.default = ("int", vi())
                        elif c == 5:
                            f.default = ("float", struct.unpack("<d", bytes(d[p[0]:p[0] + 8]))[0]); p[0] += 8
                        elif c == 6: f.default = ("string", st())
                        elif c == 7: f.default = ("array", [])
                        else: bad("bad default tag")
                    for _ in range(vi()): f.attrs.append((st(), st()))
                    t.fields.append(f)
            elif k == 2:
                b = u8()
                if b not in (1, 2, 4): bad("bad enum base")
                t = Enum(name, {1: "u8", 2: "u16", 4: "u32"}[b])
                for _ in range(vi()): t.values.append((st(), vi()))
            elif k == 3:
                t = Union(name)
                for _ in range(vi()):
                    vn = st(); tag = u8(); t.variants.append((vn, tag, td()))
            else: bad("bad type kind")
            s.by_name[name] = len(s.types); s.types.append(t)
        if p[0] != len(d): bad("trailing bytes in schema")
        s.finish()
        return s

# ===================================================================== parser
_TOK = re.compile(r'''(?P<ws>\s+|//[^\n]*|/\*.*?\*/)|(?P<num>-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(?P<id>[A-Za-z_][A-Za-z0-9_]*)|(?P<str>"(?:[^"\\\n]|\\.)*")|(?P<p>[{}<>():,;=@.\[\]])''', re.S)

class _Tok:
    __slots__ = ("kind", "val", "pos")
    def __init__(self, k, v, p): self.kind, self.val, self.pos = k, v, p

def _lex(src, fname):
    out, i = [], 0
    while i < len(src):
        m = _TOK.match(src, i)
        if not m:
            l, c = line_col(src, i)
            raise JsonxError("E_SCHEMA_SYNTAX", f"{fname}: unexpected character {src[i]!r}", pos=i, line=l, col=c)
        if m.lastgroup != "ws": out.append(_Tok(m.lastgroup, m.group(), i))
        i = m.end()
    out.append(_Tok("eof", "", len(src)))
    return out

class _RawDecl:
    pass

class _SP:
    def __init__(self, src, fname):
        self.src, self.fname, self.t, self.i = src, fname, _lex(src, fname), 0
    def err(self, msg, tok=None, code="E_SCHEMA_SYNTAX"):
        tok = tok or self.t[self.i]
        l, c = line_col(self.src, tok.pos)
        raise JsonxError(code, f"{self.fname}: {msg}", pos=tok.pos, line=l, col=c)
    def peek(self): return self.t[self.i]
    def nxt(self): t = self.t[self.i]; self.i += 1; return t
    def is_p(self, v): t = self.t[self.i]; return t.kind == "p" and t.val == v
    def eat_p(self, v):
        if self.is_p(v): self.i += 1; return True
        return False
    def expect_p(self, v):
        if not self.eat_p(v): self.err(f"expected '{v}', found '{self.peek().val}'")
    def ident(self):
        t = self.nxt()
        if t.kind != "id": self.i -= 1; self.err(f"expected a name, found '{t.val}'")
        return t.val
    def kw(self, v): t = self.peek(); return t.kind == "id" and t.val == v

    def type_(self):
        t0 = self.peek(); name = self.ident()
        if name in ("array", "ref", "optional", "nullable"):
            self.expect_p("<"); inner = self.type_(); self.expect_p(">")
            return (name, inner, t0)
        return ("name", name, t0)

    def literal(self):
        t = self.nxt()
        if t.kind == "num":
            return ("int", int(t.val)) if re.fullmatch(r"-?\d+", t.val) else ("float", float(t.val))
        if t.kind == "str":
            return ("string", _unq(t.val))
        if t.kind == "id" and t.val in ("true", "false"): return ("bool", t.val == "true")
        if t.kind == "id" and t.val == "null": return ("null", None)
        if t.kind == "p" and t.val == "[":
            self.expect_p("]"); return ("array", [])
        self.i -= 1; self.err("expected a literal default value")

    def attrs(self):
        out = []
        while self.is_p("@"):
            self.nxt(); name = self.ident(); args = ""
            if self.is_p("("):
                a = self.nxt(); depth = 1
                while depth:
                    t = self.nxt()
                    if t.kind == "eof": self.err("unterminated attribute arguments", a)
                    if t.kind == "p" and t.val == "(": depth += 1
                    elif t.kind == "p" and t.val == ")": depth -= 1
                args = self.src[a.pos + 1:t.pos].strip()
            out.append((name, args))
        return out

def _unq(s):
    return bytes(s[1:-1], "utf-8").decode("unicode_escape").encode("latin-1").decode("utf-8") if "\\" in s else s[1:-1]

def _range_ok(tn, v): 
    sg, bits = INTS[tn]
    return (-(1 << (bits - 1)) <= v < (1 << (bits - 1))) if sg else (0 <= v < (1 << bits))

def compile_schema(text: str, filename: str = "<schema>", loader: Optional[Callable[[str, str], Tuple[str, str]]] = None,
                   _seen=None) -> Schema:
    """Compile schema source text. `loader(path, from_file) -> (text, canonical_name)` resolves imports."""
    decls, meta = _parse_file(text, filename, loader, _seen or [filename])
    return _build(decls, meta, filename)

def _parse_file(text, fname, loader, stack):
    p = _SP(text, fname); decls = []; meta = {"namespace": "", "version": 1, "root": None, "root_tok": None, "ver_tok": None}
    while p.peek().kind != "eof":
        t = p.peek()
        if t.kind != "id": p.err(f"unexpected '{t.val}' at top level")
        w = t.val
        if w == "namespace":
            p.nxt(); parts = [p.ident()]
            while p.eat_p("."): parts.append(p.ident())
            meta["namespace"] = ".".join(parts)
        elif w == "version":
            p.nxt(); n = p.nxt()
            if n.kind != "num" or not n.val.isdigit() or int(n.val) < 1: p.err("version must be a positive integer", n)
            meta["version"] = int(n.val); meta["ver_tok"] = n
        elif w == "root":
            p.nxt(); meta["root_tok"] = p.peek(); meta["root"] = p.ident()
        elif w == "import":
            p.nxt(); s = p.nxt()
            if s.kind != "str": p.err("import expects a quoted path", s)
            if loader is None: p.err("imports need a loader (use the file-based compiler)", s, "E_SCHEMA_IMPORT")
            path = _unq(s.val)
            try: itext, iname = loader(path, fname)
            except OSError as ex: p.err(f"cannot import '{path}': {ex.strerror}", s, "E_SCHEMA_IMPORT")
            if iname in stack: p.err(f"import cycle through '{iname}'", s, "E_SCHEMA_IMPORT")
            sub, smeta = _parse_file(itext, iname, loader, stack + [iname])
            decls = sub + decls if False else decls + sub
        elif w in ("schema", "struct"):
            p.nxt(); d = _RawDecl(); d.kind = "struct"; d.tok = p.peek(); d.name = p.ident(); d.fname = fname; d.src = text; d.fields = []
            p.expect_p("{")
            while not p.eat_p("}"):
                ft = p.peek(); fn = p.ident(); p.expect_p(":"); ty = p.type_(); dflt = None
                if p.eat_p("="): dflt = p.literal()
                attrs = p.attrs()
                p.eat_p(",") or p.eat_p(";")
                d.fields.append((fn, ty, dflt, attrs, ft))
            decls.append(d)
        elif w == "enum":
            p.nxt(); d = _RawDecl(); d.kind = "enum"; d.tok = p.peek(); d.name = p.ident(); d.fname = fname; d.src = text; d.base = None; d.values = []
            if p.eat_p(":"):
                d.base = p.ident()
                if d.base not in ("u8", "u16", "u32"): p.err("enum base must be u8, u16 or u32")
            p.expect_p("{"); nxt_val = 0
            while not p.eat_p("}"):
                vt = p.peek(); vn = p.ident()
                if p.eat_p("="):
                    n = p.nxt()
                    if n.kind != "num" or not re.fullmatch(r"\d+", n.val): p.err("enum values are non-negative integers", n)
                    nxt_val = int(n.val)
                d.values.append((vn, nxt_val, vt)); nxt_val += 1
                p.eat_p(",") or p.eat_p(";")
            decls.append(d)
        elif w == "union":
            p.nxt(); d = _RawDecl(); d.kind = "union"; d.tok = p.peek(); d.name = p.ident(); d.fname = fname; d.src = text; d.variants = []
            p.expect_p("{")
            while not p.eat_p("}"):
                vt = p.peek(); vn = p.ident(); p.expect_p("=")
                n = p.nxt()
                if n.kind != "num" or not re.fullmatch(r"\d+", n.val) or not 1 <= int(n.val) <= 255: p.err("union tags are integers 1..255", n)
                p.expect_p(":"); ty = p.type_(); d.variants.append((vn, int(n.val), ty, vt))
                p.eat_p(",") or p.eat_p(";")
            decls.append(d)
        else: p.err(f"unknown declaration '{w}'")
        p.eat_p(";")
    meta["_p"] = p
    return decls, meta

def _build(decls, meta, fname) -> Schema:
    s = Schema(); s.namespace = meta["namespace"]; s.version = meta["version"]
    def err(d_or_tok, msg, code="E_SCHEMA_INVALID", src=None, fn=None):
        tok = d_or_tok.tok if hasattr(d_or_tok, "tok") else d_or_tok
        src = src if src is not None else meta["_p"].src
        l, c = line_col(src, tok.pos)
        raise JsonxError(code, f"{fn or fname}: {msg}", pos=tok.pos, line=l, col=c)
    for d in decls:
        if d.name in s.by_name: err(d, f"duplicate definition of '{d.name}'", "E_SCHEMA_DUPLICATE", d.src, d.fname)
        if d.name in PRIMS or d.name in ("array", "ref", "optional", "nullable"): err(d, f"'{d.name}' is a reserved type name", src=d.src, fn=d.fname)
        s.by_name[d.name] = len(s.types)
        s.types.append(Struct(d.name) if d.kind == "struct" else Enum(d.name, d.base) if d.kind == "enum" else Union(d.name))
    def res(raw, d, top=False, allow=True):
        k, v, tok = raw
        if k == "name":
            if v in PRIMS: return T(v), False, False
            if v not in s.by_name: err(tok, f"unknown type '{v}'", "E_SCHEMA_UNKNOWN_TYPE", d.src, d.fname)
            i = s.by_name[v]; return T(s.types[i].kind, i), False, False
        if k in ("optional", "nullable"):
            if not top: err(tok, f"{k}<> is only allowed as a field's outermost type", src=d.src, fn=d.fname)
            t, o, n = res(v, d, False)
            return t, k == "optional", k == "nullable"
        if k == "array":
            t, _, _ = res(v, d, False); return T("array", elem=t), False, False
        t, _, _ = res(v, d, False)    # ref
        return T("ref", elem=t), False, False
    for di in sorted(range(len(decls)), key=lambda i: decls[i].kind == "struct"):   # enums/unions first, so struct defaults can name any enum
        d, ty = decls[di], s.types[di]
        if d.kind == "enum":
            names = set(); vals = set()
            for vn, vv, vt in d.values:
                if vn in names or vv in vals: err(vt, f"duplicate enum member '{vn}' or value {vv}", "E_SCHEMA_DUPLICATE", d.src, d.fname)
                names.add(vn); vals.add(vv); ty.values.append((vn, vv))
            if not ty.values: err(d, "enum has no members", src=d.src, fn=d.fname)
            if ty.base is None:
                m = max(v for _, v in ty.values); ty.base = "u8" if m < 256 else "u16" if m < 65536 else "u32"
            lim = {"u8": 256, "u16": 65536, "u32": 1 << 32}[ty.base]
            if max(v for _, v in ty.values) >= lim: err(d, f"enum value does not fit in {ty.base}", src=d.src, fn=d.fname)
        elif d.kind == "union":
            names, tags = set(), set()
            for vn, tag, raw, vt in d.variants:
                t, _, _ = res(raw, d, False)
                if vn in names or tag in tags: err(vt, f"duplicate union variant '{vn}' or tag {tag}", "E_SCHEMA_DUPLICATE", d.src, d.fname)
                if t.kind in ("union", "any", "ref"): err(vt, f"union variants cannot have type {t.kind}", src=d.src, fn=d.fname)
                names.add(vn); tags.add(tag); ty.variants.append((vn, tag, t))
            if not ty.variants: err(d, "union has no variants", src=d.src, fn=d.fname)
        else:
            last_since = 1
            for fn, raw, dflt, attrs, ft in d.fields:
                if fn in ty.by_name: err(ft, f"duplicate field '{fn}'", "E_SCHEMA_DUPLICATE", d.src, d.fname)
                t, opt, nul = res(raw, d, True)
                f = Field(fn, t); f.optional, f.nullable = opt, nul
                for an, aa in attrs:
                    if an == "since":
                        if not aa.isdigit() or int(aa) < 1: err(ft, "@since needs a positive integer", src=d.src, fn=d.fname)
                        f.since = int(aa)
                    elif an == "deprecated": f.deprecated = True; f.attrs.append((an, aa))
                    else: f.attrs.append((an, aa))
                for an, aa in f.attrs:
                    if an in ("range", "len"):
                        parts = [x.strip() for x in aa.split(",")]
                        if len(parts) != 2: err(ft, f"@{an} takes two arguments (use _ for unbounded)", src=d.src, fn=d.fname)
                        for x in parts:
                            if x != "_":
                                try: float(x)
                                except ValueError: err(ft, f"@{an}: bad number '{x}'", src=d.src, fn=d.fname)
                        num = t.kind in INTS or t.kind in FLOATS
                        if an == "range" and not num: err(ft, f"{d.name}.{fn}: @range needs a numeric type", src=d.src, fn=d.fname)
                        if an == "len" and t.kind not in ("string", "bytes", "array"): err(ft, f"{d.name}.{fn}: @len needs string, bytes or array", src=d.src, fn=d.fname)
                if t.kind == "ref" and t.elem.kind in ("ref",): err(ft, "ref<ref<..>> is not allowed", src=d.src, fn=d.fname)
                if dflt is not None:
                    if opt: err(ft, "optional fields cannot have defaults", src=d.src, fn=d.fname)
                    f.default = _check_default(t, dflt, s, lambda m: err(ft, m, src=d.src, fn=d.fname), nul)
                ty.fields.append(f); ty.by_name[fn] = f
            for f in ty.fields:
                pass
            # evolution rules
            for fn_, f in zip([x[0] for x in d.fields], ty.fields):
                if f.since < last_since: err(d, f"field '{f.name}': @since values must not decrease along the declaration order (fields are append-only)", src=d.src, fn=d.fname)
                last_since = f.since
                if f.since > s.version: err(d, f"field '{f.name}' has @since({f.since}) but the schema version is {s.version}", src=d.src, fn=d.fname)
                if f.since > 1 and not (f.optional or f.nullable or f.default is not None):
                    err(d, f"field '{f.name}' added in version {f.since} must be optional, nullable or have a default (old data lacks it)", src=d.src, fn=d.fname)
            if not ty.fields or ty.fields[0].since != 1: err(d, f"schema '{d.name}' needs at least one field present since version 1", src=d.src, fn=d.fname)
    if meta["root"] is not None:
        if meta["root"] not in s.by_name or s.types[s.by_name[meta["root"]]].kind != "struct":
            err(meta["root_tok"], f"root '{meta['root']}' is not a schema", "E_SCHEMA_UNKNOWN_TYPE")
        s.root = s.by_name[meta["root"]]
    s.finish()
    return s

def _check_default(t, lit, s, bad, nullable):
    k, v = lit
    tk = t.kind
    if k == "null":
        if not nullable and tk != "any": bad("null default needs a nullable<> field")
        return lit
    if tk in INTS:
        if k != "int" or not _range_ok(tk, v): bad(f"default {v!r} is not a valid {tk}")
        return lit
    if tk in FLOATS:
        if k not in ("int", "float"): bad("default must be a number")
        return ("float", float(v))
    if tk == "bool":
        if k != "bool": bad("default must be true or false")
        return lit
    if tk == "string":
        if k != "string": bad("default must be a string")
        return lit
    if tk == "enum":
        if k != "string" or v not in [n for n, _ in s.types[t.idx].values]: bad("default must be one of the enum's member names (as a string)")
        return lit
    if tk == "array":
        if k != "array": bad("only [] is allowed as an array default")
        return lit
    if tk == "any": return lit
    bad(f"fields of type {tk} cannot have defaults")

def compile_file(path: str) -> Schema:
    import os
    def loader(p, frm):
        full = os.path.normpath(os.path.join(os.path.dirname(frm) or ".", p))
        with open(full, encoding="utf-8") as fh: return fh.read(), full
    with open(path, encoding="utf-8") as fh: text = fh.read()
    return compile_schema(text, os.path.normpath(path), loader, [os.path.normpath(path)])

def load_schema(data: bytes) -> Schema:
    return Schema.from_bytes(data)
