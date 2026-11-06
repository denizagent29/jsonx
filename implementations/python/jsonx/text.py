"""JSONX text format: parser, label/pointer resolver, canonical dump, writer."""
from __future__ import annotations
import base64, re, struct
from typing import Any, Dict, List, Optional
from .errors import JsonxError, line_col

MAX_DEPTH = 256
_NUM = re.compile(r"-?(?:0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?")
_ASCII_ID_START = re.compile(r"[A-Za-z_]")
_ASCII_ID = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

def _nonascii_ok(o: int) -> bool:
    """Non-ASCII code points allowed in identifiers (table-free rule, see text-format.md §4)."""
    return not (0x80 <= o <= 0xA0 or o == 0x1680 or 0x2000 <= o <= 0x200F or 0x2028 <= o <= 0x202F
                or 0x205F <= o <= 0x206F or o == 0x3000 or o == 0xFEFF or o in (0xFFFE, 0xFFFF))

def is_id_start(c: str) -> bool:
    o = ord(c)
    if o < 0x80:
        return c == "_" or ("a" <= c <= "z") or ("A" <= c <= "Z")
    return _nonascii_ok(o)

def is_id_char(c: str) -> bool:
    return is_id_start(c) or ("0" <= c <= "9")

class Label:
    __slots__ = ("name", "kind", "expr", "pos", "owner", "_target")
    def __init__(self, name, kind, expr, pos):
        self.name, self.kind, self.expr, self.pos = name, kind, expr, pos
        self.owner = None; self._target = None

class RefExpr:
    __slots__ = ("start", "name", "segs", "text", "pos")
    def __init__(self, start, name, segs, text, pos):
        self.start, self.name, self.segs, self.text, self.pos = start, name, segs, text, pos

class Node:
    """A JSONX value. kind: null bool int float f32 string bytes array object ref."""
    __slots__ = ("kind", "value", "pos", "parent", "key", "items", "labels", "lmap", "expr", "target")
    def __init__(self, kind, value=None, pos=-1):
        self.kind, self.value, self.pos = kind, value, pos
        self.parent = None; self.key = None; self.items = None
        self.labels = None; self.lmap = None; self.expr = None; self.target = None

    def deref(self) -> "Node":
        return self.target if self.kind == "ref" else self

    def __getitem__(self, k):
        n = self.deref()
        if n.kind == "object":
            return n.items[k].deref()
        if n.kind == "array":
            return n.items[k].deref()
        raise TypeError(f"cannot index {n.kind}")

    def __len__(self):
        return len(self.deref().items)

    def __iter__(self):
        n = self.deref()
        return iter(n.items.values() if n.kind == "object" else n.items)

    def path(self) -> str:
        segs = []
        n = self
        while n.parent is not None:
            k = n.key
            segs.append(str(k).replace("~", "~0").replace("/", "~1"))
            n = n.parent
        return "".join("/" + s for s in reversed(segs))

    def to_python(self, scalar_refs: str = "value"):
        return _to_python(self, {})

class Document:
    def __init__(self, root: Node, src: str = "", globals_=None):
        self.root, self.src, self.globals = root, src, globals_ or {}

    def to_python(self):
        return _to_python(self.root, {})

    def dump(self) -> str:
        return dump(self.root)

    def __getitem__(self, k):
        return self.root[k]

def _to_python(n: Node, memo):
    k = n.kind
    if k == "ref":
        return _to_python(n.target, memo)
    if k == "object":
        if n in memo: return memo[n]
        d = {}; memo[n] = d
        for kk, v in n.items.items(): d[kk] = _to_python(v, memo)
        return d
    if k == "array":
        if n in memo: return memo[n]
        l: list = []; memo[n] = l
        for v in n.items: l.append(_to_python(v, memo))
        return l
    return n.value

# ------------------------------------------------------------------ parser
class _Parser:
    def __init__(self, src: str):
        self.s, self.n, self.i = src, len(src), 0

    def err(self, code, msg, pos=None, **kw):
        raise JsonxError(code, msg, pos=self.i if pos is None else pos, **kw)

    def skip(self):
        s, i, n = self.s, self.i, self.n
        while i < n:
            c = s[i]
            if c in " \t\r\n": i += 1
            elif c == "/" and i + 1 < n and s[i + 1] == "/":
                i += 2
                while i < n and s[i] not in "\r\n": i += 1
            elif c == "/" and i + 1 < n and s[i + 1] == "*":
                j = s.find("*/", i + 2)
                if j < 0: self.err("E_SYNTAX", "unterminated block comment", i)
                i = j + 2
            else: break
        self.i = i

    def ident(self) -> str:
        s, i, n = self.s, self.i, self.n
        j = i
        while j < n and is_id_char(s[j]): j += 1
        self.i = j
        return s[i:j]

    def document(self) -> Node:
        if self.s.startswith("\ufeff"): self.i = 1
        self.skip()
        if self.i >= self.n: self.err("E_UNEXPECTED_EOF", "empty document: expected a value")
        v = self.value(0)
        self.skip()
        if self.i < self.n: self.err("E_SYNTAX", "unexpected content after the root value")
        return v

    def value(self, depth) -> Node:
        self.skip()
        s, i = self.s, self.i
        if i >= self.n: self.err("E_UNEXPECTED_EOF", "unexpected end of input: expected a value")
        c = s[i]
        if c == "{": return self.obj(depth)
        if c == "[": return self.arr(depth)
        if c == '"': return Node("string", self.string(), i)
        if c == "@": return self.ref()
        if c == "-" or "0" <= c <= "9": return self.number()
        if c in "+.": self.err("E_INVALID_NUMBER", f"numbers cannot start with '{c}'")
        if is_id_start(c):
            w = self.ident()
            if w == "true": return Node("bool", True, i)
            if w == "false": return Node("bool", False, i)
            if w == "null": return Node("null", None, i)
            self.err("E_SYNTAX", f"unexpected identifier '{w}': expected a value", i,
                     hint="strings must be quoted; references start with '@'")
        if c == "$":
            self.err("E_SYNTAX", "unexpected '$': contextual variables are only valid inside references",
                     hint="write @$prev / @$next")
        self.err("E_SYNTAX", f"unexpected character {c!r}")

    def number(self) -> Node:
        s, i = self.s, self.i
        m = _NUM.match(s, i)
        if not m: self.err("E_INVALID_NUMBER", "'-' must be followed by a digit")
        j = m.end()
        if j < self.n and (s[j] == "." or s[j].isdigit() and s[j].isascii() or is_id_char(s[j])):
            self.err("E_INVALID_NUMBER", f"malformed number: unexpected {s[j]!r} after '{s[i:j]}'", i)
        self.i = j
        txt = m.group(0)
        if m.group(1) is None and m.group(2) is None:
            v = int(txt)
            if v < -(1 << 63) or v >= (1 << 64):
                self.err("E_NUMBER_OVERFLOW", f"integer {txt} does not fit in [-2^63, 2^64-1]", i)
            return Node("int", v, i)
        v = float(txt)
        if v in (float("inf"), float("-inf")):
            self.err("E_NUMBER_OVERFLOW", f"float {txt} overflows IEEE-754 binary64", i)
        return Node("float", v, i)

    def string(self) -> str:
        s, n = self.s, self.n
        i = self.i + 1
        out = []
        start = i
        while True:
            if i >= n: self.err("E_UNEXPECTED_EOF", "unterminated string", self.i)
            c = s[i]
            if c == '"': out.append(s[start:i]); self.i = i + 1; return "".join(out)
            if c == "\\":
                out.append(s[start:i])
                i += 1
                if i >= n: self.err("E_UNEXPECTED_EOF", "unterminated escape", i)
                e = s[i]
                m = {'"': '"', "\\": "\\", "/": "/", "b": "\b", "f": "\f", "n": "\n", "r": "\r", "t": "\t"}
                if e in m: out.append(m[e]); i += 1
                elif e == "u":
                    if i + 1 < n and s[i + 1] == "{":
                        j = s.find("}", i + 2)
                        h = s[i + 2:j] if j > 0 else ""
                        if not (1 <= len(h) <= 6) or not all(x in "0123456789abcdefABCDEF" for x in h):
                            self.err("E_INVALID_ESCAPE", "malformed \\u{...} escape", i - 1)
                        cp = int(h, 16)
                        if cp > 0x10FFFF or 0xD800 <= cp <= 0xDFFF:
                            self.err("E_INVALID_ESCAPE", f"\\u{{{h}}} is not a Unicode scalar value", i - 1)
                        out.append(chr(cp)); i = j + 1
                    else:
                        cp = self.hex4(i + 1)
                        i += 5
                        if 0xD800 <= cp <= 0xDBFF:
                            if s[i:i + 2] != "\\u": self.err("E_INVALID_ESCAPE", "lone high surrogate", i - 6)
                            lo = self.hex4(i + 2)
                            if not 0xDC00 <= lo <= 0xDFFF: self.err("E_INVALID_ESCAPE", "high surrogate not followed by low surrogate", i - 6)
                            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00); i += 6
                        elif 0xDC00 <= cp <= 0xDFFF:
                            self.err("E_INVALID_ESCAPE", "lone low surrogate", i - 6)
                        out.append(chr(cp))
                else: self.err("E_INVALID_ESCAPE", f"unknown escape '\\{e}'", i - 1)
                start = i; continue
            if ord(c) < 0x20: self.err("E_SYNTAX", "unescaped control character in string", i)
            i += 1

    def hex4(self, i) -> int:
        h = self.s[i:i + 4]
        if len(h) != 4 or not all(x in "0123456789abcdefABCDEF" for x in h):
            self.err("E_INVALID_ESCAPE", "\\u needs exactly 4 hex digits", i - 2)
        return int(h, 16)

    def obj(self, depth) -> Node:
        if depth >= MAX_DEPTH: self.err("E_DEPTH_LIMIT", f"nesting deeper than {MAX_DEPTH}")
        node = Node("object", None, self.i); node.items = {}; node.labels = []
        self.i += 1
        s = self.s
        while True:
            self.skip()
            if self.i >= self.n: self.err("E_UNEXPECTED_EOF", "unterminated object")
            c = s[self.i]
            if c == "}": self.i += 1; break
            if c == "<": node.labels.append(self.label()); continue
            kpos = self.i
            if c == '"': key = self.string()
            elif is_id_start(c): key = self.ident()
            else: self.err("E_SYNTAX", f"expected an object key, found {c!r}")
            self.skip()
            if self.i >= self.n or s[self.i] != ":": self.err("E_SYNTAX", "expected ':' after object key")
            self.i += 1
            v = self.value(depth + 1)
            if key in node.items:
                self.err("E_DUPLICATE_KEY", f"duplicate key {key!r}", kpos)
            node.items[key] = v; v.parent = node; v.key = key
            self.skip()
            if self.i >= self.n: self.err("E_UNEXPECTED_EOF", "unterminated object")
            c = s[self.i]
            if c == ",": self.i += 1
            elif c == "}": self.i += 1; break
            else: self.err("E_SYNTAX", f"expected ',' or '}}' after value, found {c!r}")
        return node

    def arr(self, depth) -> Node:
        if depth >= MAX_DEPTH: self.err("E_DEPTH_LIMIT", f"nesting deeper than {MAX_DEPTH}")
        node = Node("array", None, self.i); node.items = []
        self.i += 1
        s = self.s
        while True:
            self.skip()
            if self.i >= self.n: self.err("E_UNEXPECTED_EOF", "unterminated array")
            if s[self.i] == "]": self.i += 1; break
            v = self.value(depth + 1)
            v.parent = node; v.key = len(node.items); node.items.append(v)
            self.skip()
            if self.i >= self.n: self.err("E_UNEXPECTED_EOF", "unterminated array")
            c = s[self.i]
            if c == ",": self.i += 1
            elif c == "]": self.i += 1; break
            else: self.err("E_SYNTAX", f"expected ',' or ']' after element, found {c!r}")
        return node

    # --- labels and pointers
    def label(self) -> Label:
        pos = self.i; s = self.s
        self.i += 1; self.skip()
        npos = self.i
        var = False
        if self.i < self.n and s[self.i] == "$": var = True; self.i += 1
        m = _ASCII_ID.match(s, self.i)
        if not m: self.err("E_INVALID_LABEL_NAME", "expected a label name after '<'", npos)
        name = m.group(0); self.i = m.end()
        if self.i < self.n and is_id_char(s[self.i]):
            self.err("E_INVALID_LABEL_NAME", "label names are ASCII [A-Za-z][A-Za-z0-9_]*", npos)
        if var:
            kind = "var"; name = "$" + name
            if name in ("$prev", "$next"): self.err("E_RESERVED_LABEL", f"'{name}' is a built-in contextual variable", pos)
        elif name[0] == "_" : self.err("E_INVALID_LABEL_NAME", "label names must start with a letter", npos)
        else: kind = "global" if name[0].isupper() else "local"
        self.skip()
        expr = None
        if self.i < self.n and s[self.i] == "=":
            self.i += 1; self.skip()
            at = self.i
            if self.i < self.n and s[self.i] == "@": self.i += 1
            expr = self.path(at)
            self.skip()
        if self.i >= self.n or s[self.i] != ">": self.err("E_SYNTAX", "expected '>' to close the label declaration")
        self.i += 1
        return Label(name, kind, expr, pos)

    def seg_start(self) -> bool:
        if self.i >= self.n: return False
        c = self.s[self.i]
        return c in '!"$' or is_id_char(c)

    def segment(self):
        s, i = self.s, self.i
        c = s[i] if i < self.n else ""
        if c == "!": self.i += 1; return ("parent", None)
        if c == '"': return ("key", self.string())
        if c == "$":
            m = _ASCII_ID.match(s, i + 1)
            if not m: self.err("E_INVALID_POINTER", "expected a name after '$'")
            self.i = m.end(); nm = m.group(0)
            return ("prev", None) if nm == "prev" else ("next", None) if nm == "next" else ("desc", nm)
        if "0" <= c <= "9" and c.isascii():
            j = i
            while j < self.n and "0" <= s[j] <= "9": j += 1
            if j < self.n and is_id_start(s[j]):
                self.err("E_INVALID_POINTER", "malformed array index")
            txt = s[i:j]
            if len(txt) > 1 and txt[0] == "0": self.err("E_INVALID_POINTER", "array indexes have no leading zeros")
            self.i = j; return ("index", int(txt))
        if c and is_id_start(c): return ("key", self.ident())
        self.err("E_INVALID_POINTER", "expected a field name, index, '$name' or '!' in pointer")

    def path(self, at) -> RefExpr:
        s = self.s
        c = s[self.i] if self.i < self.n else ""
        segs = []
        if c == "!":
            self.i += 1
            if not (self.i < self.n and (is_id_start(s[self.i]) or s[self.i] == '"')):
                self.err("E_INVALID_POINTER", "'@!' must be followed by a field name")
            start, name = "cur", None; segs.append(self.segment())
        elif c == ".":
            if s[self.i + 1:self.i + 2] == ".":
                self.i += 2
                m = _ASCII_ID.match(s, self.i)
                if not m: self.err("E_INVALID_POINTER", "'@..' must be followed by a global label name")
                self.i = m.end(); start, name = "global", m.group(0)
            else:
                self.i += 1; start, name = "cur", None
                if self.seg_start(): segs.append(self.segment())
        elif c == "$":
            seg = self.segment()
            if seg[0] in ("prev", "next"): start, name = "ctx", seg[0]
            else:
                start, name = "var", "$" + seg[1]
        elif c and is_id_start(c):
            start, name = "name", self.ident()
        else:
            self.err("E_INVALID_POINTER", "expected a label, field name, '.', '!' or '$' after '@'")
        while self.i < self.n and s[self.i] == ".":
            self.i += 1
            segs.append(self.segment())
        return RefExpr(start, name, segs, s[at:self.i], at)

    def ref(self) -> Node:
        at = self.i; self.i += 1
        n = Node("ref", None, at); n.expr = self.path(at)
        return n

def parse_tree(src: str) -> Node:
    return _Parser(src).document()

# ------------------------------------------------------------------ resolver
_KIND = {"int": "integer", "float": "float", "string": "string", "bool": "boolean", "null": "null",
         "array": "array", "object": "object", "ref": "reference"}

class _Resolver:
    def __init__(self, root: Node, src: str):
        self.root, self.src = root, src
        self.globals: Dict[str, Label] = {}
        self.by_name: Dict[str, List[Label]] = {}
        self.refs: List[Node] = []; self.labels: List[Label] = []
        self.active: set = set()

    def err(self, code, msg, pos, **kw):
        line, col = line_col(self.src, pos)
        raise JsonxError(code, msg, pos=pos, line=line, col=col, **kw)

    def where(self, pos) -> str:
        l, c = line_col(self.src, pos); return f"{l}:{c}"

    def collect(self, n: Node):
        k = n.kind
        if k == "object":
            seen = {}
            for lb in n.labels:
                lb.owner = n
                if lb.kind == "global":
                    if lb.name in self.globals:
                        self.err("E_DUPLICATE_LABEL", f"global label '{lb.name}' is already defined at {self.where(self.globals[lb.name].pos)}", lb.pos)
                    self.globals[lb.name] = lb
                else:
                    if lb.name in seen:
                        self.err("E_DUPLICATE_LABEL", f"label '{lb.name}' is declared twice in the same object (first at {self.where(seen[lb.name].pos)})", lb.pos)
                    seen[lb.name] = lb
                self.by_name.setdefault(lb.name, []).append(lb)
                self.labels.append(lb)
            n.lmap = seen
            for c in n.items.values(): self.collect(c)
        elif k == "array":
            for c in n.items: self.collect(c)
        elif k == "ref":
            self.refs.append(n)

    def run(self):
        self.collect(self.root)
        for lb in self.labels: self.label_target(lb)
        for r in self.refs: self.target(r)

    # -- evaluation
    def target(self, r: Node) -> Node:
        if r.target is not None: return r.target
        if r in self.active:
            self.err("E_CYCLIC_REFERENCE", f"reference '@{r.expr.text.lstrip('@')}' depends on itself", r.pos)
        self.active.add(r)
        t = self.eval(r.expr, r, False, r.pos)
        while t.kind == "ref": t = self.target(t)
        self.active.discard(r)
        r.target = t
        return t

    def label_target(self, lb: Label) -> Node:
        if lb._target is not None: return lb._target
        if lb.expr is None:
            lb._target = lb.owner; return lb.owner
        if lb in self.active:
            self.err("E_CYCLIC_REFERENCE", f"label '{lb.name}' is defined in terms of itself", lb.pos)
        self.active.add(lb)
        t = self.eval(lb.expr, lb.owner, True, lb.pos)
        while t.kind == "ref": t = self.target(t)
        self.active.discard(lb)
        lb._target = t
        return t

    def deref(self, n: Node) -> Node:
        while n.kind == "ref": n = self.target(n)
        return n

    def cur_obj(self, loc, is_lab):
        if is_lab: return loc
        n = loc.parent
        while n is not None and n.kind != "object": n = n.parent
        return n

    def elem(self, loc):
        n = loc
        while n is not None:
            if n.parent is not None and n.parent.kind == "array": return n
            n = n.parent
        return None

    def find_label(self, name, loc):
        n = loc
        while n is not None:
            if n.kind == "object" and n.lmap and name in n.lmap: return n.lmap[name]
            n = n.parent
        return self.globals.get(name)

    def describe(self, n: Node) -> str:
        p = n.path()
        return f"'{p}'" if p else "the root"

    def eval(self, ex: RefExpr, loc: Node, is_lab: bool, epos: int) -> Node:
        e = lambda code, msg, **kw: self.err(code, msg, epos, **kw)
        txt = "@" + ex.text.lstrip("@")
        st, segs = ex.start, list(ex.segs)
        if st == "cur":
            cur = self.cur_obj(loc, is_lab)
            if cur is None: e("E_INVALID_CURRENT", f"'{txt}': there is no enclosing object")
        elif st == "global":
            lb = self.globals.get(ex.name)
            if lb is None: e("E_UNDEFINED_LABEL", f"global label '{ex.name}' is not defined", hint="global labels start with an uppercase letter")
            cur = self.label_target(lb)
        elif st == "ctx":
            cur = self.elem(loc)
            if cur is None:
                code = "E_INVALID_PREV" if ex.name == "prev" else "E_INVALID_NEXT"
                e(code, f"'${ex.name}' used where there is no enclosing array element (the location is not inside any array)")
            segs.insert(0, (ex.name, None))
        else:  # name / var
            nm = ex.name
            lb = self.find_label(nm, loc)
            rk = st == "name" and self.root.kind == "object" and nm in self.root.items
            if lb is not None and rk:
                e("E_AMBIGUOUS_NAME", f"'{nm}' names both a label (declared at {self.where(lb.pos)}) and a field of the root object",
                  hint="rename one of them")
            if lb is not None: cur = self.label_target(lb)
            elif rk: cur = self.root.items[nm]
            else:
                others = [x for x in self.by_name.get(nm, [])]
                if others:
                    o = others[0]; op = o.owner.path()
                    e("E_LABEL_OUT_OF_SCOPE", f"label '{nm}' (declared at {self.where(o.pos)}) is not visible from here: local labels are only visible inside the object that declares them",
                      hint=f"reach it explicitly with a path such as @{op.strip('/').replace('/', '.') + '.' if op else ''}${nm.lstrip('$')}, or make it global with an uppercase name")
                e("E_UNDEFINED_LABEL", f"no label or root field named '{nm}'")
        for kind, val in segs:
            if kind in ("key", "index"):
                cur = self.deref(cur)
                if kind == "key":
                    if cur.kind != "object": e("E_POINTER_TYPE", f"'{txt}': cannot select field '{val}' of a {_KIND[cur.kind]} (at {self.describe(cur)})", expected="object", actual=cur.kind)
                    if val not in cur.items: e("E_POINTER_NO_FIELD", f"'{txt}': object {self.describe(cur)} has no field '{val}'")
                    cur = cur.items[val]
                else:
                    if cur.kind != "array": e("E_POINTER_TYPE", f"'{txt}': cannot index a {_KIND[cur.kind]} with [{val}] (at {self.describe(cur)})", expected="array", actual=cur.kind)
                    if val >= len(cur.items): e("E_INVALID_INDEX", f"'{txt}': index {val} is out of range for array {self.describe(cur)} of length {len(cur.items)}")
                    cur = cur.items[val]
            elif kind == "parent":
                if cur.parent is None: e("E_INVALID_PARENT", f"'{txt}': {self.describe(cur)} has no parent")
                cur = cur.parent
            elif kind in ("prev", "next"):
                p = cur.parent
                code = "E_INVALID_PREV" if kind == "prev" else "E_INVALID_NEXT"
                if p is None or p.kind != "array":
                    what = "the root" if p is None else f"a member of an object (key '{cur.key}')"
                    e(code, f"'${kind}' needs an array element, but {self.describe(cur)} is {what}")
                i = cur.key + (-1 if kind == "prev" else 1)
                if i < 0: e(code, f"'$prev' used on the first element of array {self.describe(p)}: there is no previous element")
                if i >= len(p.items): e(code, f"'$next' used on the last element of array {self.describe(p)}: there is no next element")
                cur = p.items[i]
            else:  # desc: explicit descent to a local label inside the subtree
                found = []
                self.search(cur, val, found)
                if not found: e("E_UNDEFINED_LABEL", f"no label '{val}' is declared inside {self.describe(cur)}")
                if len(found) > 1: e("E_AMBIGUOUS_LABEL", f"label '{val}' is declared {len(found)} times inside {self.describe(cur)} (at " + ", ".join(self.where(x.pos) for x in found) + ")")
                cur = self.label_target(found[0])
        return cur

    def search(self, n: Node, name: str, out: list):
        if n.kind == "object":
            for lb in n.labels:
                if lb.name == name or lb.name == "$" + name: out.append(lb)
            for c in n.items.values(): self.search(c, name, out)
        elif n.kind == "array":
            for c in n.items: self.search(c, name, out)

def parse(text, *, resolve: bool = True) -> Document:
    """Parse JSONX text, validate and resolve all labels/pointers. Raises JsonxError."""
    if isinstance(text, (bytes, bytearray, memoryview)):
        try: text = bytes(text).decode("utf-8")
        except UnicodeDecodeError as ex:
            raise JsonxError("E_INVALID_UTF8", f"input is not valid UTF-8: {ex.reason}") from None
    try:
        root = _Parser(text).document()
        r = _Resolver(root, text)
        if resolve: r.run()
    except JsonxError as ex:
        if ex.line is None and ex.pos is not None:
            ex.line, ex.col = line_col(text, ex.pos)
        raise
    return Document(root, text, r.globals)

def loads(text) -> Any:
    """Parse and return plain Python objects. References to objects/arrays preserve aliasing."""
    return parse(text).to_python()

# ------------------------------------------------------------------ canonical dump
def _jstr(s: str) -> str:
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == '"': out.append('\\"')
        elif ch == "\\": out.append("\\\\")
        elif o < 0x20 or o == 0x7F: out.append("\\u%04x" % o)
        else: out.append(ch)
    out.append('"'); return "".join(out)

def dump(n: Node) -> str:
    out: List[str] = []
    _dump(n, out); return "".join(out)

def _dump(n: Node, out):
    k = n.kind
    if k == "null": out.append("null")
    elif k == "bool": out.append("true" if n.value else "false")
    elif k == "int": out.append(str(n.value))
    elif k == "float": out.append("f64(0x%016x)" % struct.unpack("<Q", struct.pack("<d", n.value)))
    elif k == "f32": out.append("f32(0x%08x)" % struct.unpack("<I", struct.pack("<f", n.value)))
    elif k == "string": out.append(_jstr(n.value))
    elif k == "bytes": out.append('bytes("%s")' % base64.b64encode(n.value).decode())
    elif k == "ref": out.append("ref(" + _jstr(n.target.path()) + ")")
    elif k == "array":
        out.append("[")
        for i, c in enumerate(n.items):
            if i: out.append(",")
            _dump(c, out)
        out.append("]")
    else:
        out.append("{")
        for i, (kk, c) in enumerate(n.items.items()):
            if i: out.append(",")
            out.append(_jstr(kk)); out.append(":"); _dump(c, out)
        out.append("}")

# ------------------------------------------------------------------ writer (plain data -> text)
_IDENT_OK = re.compile(r"[A-Za-z_][A-Za-z0-9_]*\Z")

def dumps(obj: Any, indent: Optional[int] = None) -> str:
    """Serialize plain Python data (dict/list/str/int/float/bool/None) as JSONX text."""
    out: List[str] = []
    def esc(s):
        r = ['"']
        for ch in s:
            o = ord(ch)
            if ch == '"': r.append('\\"')
            elif ch == "\\": r.append("\\\\")
            elif ch == "\n": r.append("\\n")
            elif ch == "\r": r.append("\\r")
            elif ch == "\t": r.append("\\t")
            elif o < 0x20 or o == 0x7F: r.append("\\u%04x" % o)
            else: r.append(ch)
        r.append('"'); return "".join(r)
    def w(o, lvl):
        nl = "" if indent is None else "\n" + " " * (indent * (lvl + 1))
        nle = "" if indent is None else "\n" + " " * (indent * lvl)
        if o is None: out.append("null")
        elif o is True: out.append("true")
        elif o is False: out.append("false")
        elif isinstance(o, int):
            if not (-(1 << 63) <= o < (1 << 64)): raise JsonxError("E_NUMBER_OVERFLOW", f"integer {o} out of range")
            out.append(str(o))
        elif isinstance(o, float):
            if o != o or o in (float("inf"), float("-inf")): raise JsonxError("E_INVALID_NUMBER", "NaN/Infinity are not representable in JSONX text")
            r = repr(o)
            out.append(r if ("." in r or "e" in r) else r + ".0")
        elif isinstance(o, str): out.append(esc(o))
        elif isinstance(o, (list, tuple)):
            if not o: out.append("[]"); return
            out.append("[")
            for i, v in enumerate(o):
                out.append(("," if i else "") + nl); w(v, lvl + 1)
            out.append(nle + "]")
        elif isinstance(o, dict):
            if not o: out.append("{}"); return
            out.append("{")
            for i, (k, v) in enumerate(o.items()):
                out.append(("," if i else "") + nl)
                out.append(k if _IDENT_OK.match(k) else esc(k)); out.append(": " if indent is not None else ":"); w(v, lvl + 1)
            out.append(nle + "}")
        else: raise TypeError(f"cannot serialize {type(o).__name__}")
    w(obj, 0)
    return "".join(out)
