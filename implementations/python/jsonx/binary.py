"""BJSONX binary format: encoder, validating decoder and zero-copy views. See specification/binary-format.md."""
from __future__ import annotations
import base64, binascii, struct, zlib
from typing import Optional
from .errors import JsonxError, line_col
from .schema import Schema, T, INTS, FLOATS, SCALARS, SCALAR_SIZE
from .text import Node, Document

HDR = 28
MAGIC = b"BJX"
FORMAT_VERSION = 1
F_CRC = 1
MAX_DEPTH = 256
ANY = T("any")
TAG_NULL, TAG_FALSE, TAG_TRUE, TAG_I32, TAG_I64, TAG_U64, TAG_F64, TAG_STR, TAG_ARR, TAG_OBJ, TAG_REF, TAG_BYTES = range(12)
_FMT = {"u8": "<B", "u16": "<H", "u32": "<I", "u64": "<Q", "i8": "<b", "i16": "<h", "i32": "<i", "i64": "<q", "f32": "<f", "f64": "<d"}
F32_MAX = 3.4028234663852886e38

def vlen(n: int) -> int:
    return 1 if n < 0x80 else 2 if n < 0x4000 else 3 if n < 0x200000 else 4 if n < 0x10000000 else 5

def _kd(n: Node) -> str:
    return {"int": "integer", "f32": "float", "float": "float"}.get(n.kind, n.kind)

# ===================================================================== encoder
class _Enc:
    def __init__(self, schema, version, src):
        self.sc, self.ver, self.src = schema, version, src
        self.buf = bytearray(HDR); self.refs = []; self.locs = {}; self.keys = {}

    def err(self, code, msg, node=None, **kw):
        line = col = None
        if node is not None and node.pos >= 0 and self.src:
            line, col = line_col(self.src, node.pos)
        raise JsonxError(code, msg, pos=getattr(node, "pos", None), line=line, col=col, **kw)

    def mismatch(self, t, node):
        self.err("E_TYPE_MISMATCH", f"expected {self.sc.type_name(t) if self.sc else t.kind}, got {_kd(node)}", node,
                 expected=self.sc.type_name(t) if self.sc else t.kind, actual=_kd(node))

    def alloc(self, n):
        p = len(self.buf)
        if p + n >= 0xFFFFFFFF: raise JsonxError("E_BIN_LIMIT", "document exceeds the 4 GiB offset limit")
        self.buf.extend(bytes(n)); return p

    def varint(self, p, v):
        b = self.buf
        while v >= 0x80: b[p] = (v & 0x7F) | 0x80; v >>= 7; p += 1
        b[p] = v; return p + 1

    def u32(self, p, v): struct.pack_into("<I", self.buf, p, v)

    def reg(self, node, loc, t): self.locs[node] = (loc, t)

    def put_str(self, b: bytes) -> int:
        p = self.alloc(vlen(len(b)) + len(b)); q = self.varint(p, len(b)); self.buf[q:q + len(b)] = b; return p

    # ---- schema-less (any)
    def any_slot(self, node, pos, depth=0):
        k = node.kind; b = self.buf
        if depth > MAX_DEPTH: raise JsonxError("E_DEPTH_LIMIT", "nesting too deep")
        self.reg(node, pos, ANY)
        if k == "null": return
        if k == "bool": b[pos] = TAG_TRUE if node.value else TAG_FALSE
        elif k == "int":
            v = node.value
            if -(1 << 31) <= v < (1 << 31): b[pos] = TAG_I32; struct.pack_into("<i", b, pos + 1, v)
            elif -(1 << 63) <= v < (1 << 63):
                p = self.alloc(8); struct.pack_into("<q", b, p, v); b[pos] = TAG_I64; self.u32(pos + 1, p)
            else:
                p = self.alloc(8); struct.pack_into("<Q", b, p, v); b[pos] = TAG_U64; self.u32(pos + 1, p)
        elif k in ("float", "f32"):
            p = self.alloc(8); struct.pack_into("<d", b, p, node.value); b[pos] = TAG_F64; self.u32(pos + 1, p)
        elif k == "string":
            b[pos] = TAG_STR; self.u32(pos + 1, self.put_str(node.value.encode("utf-8")))
        elif k == "bytes":
            b[pos] = TAG_BYTES; self.u32(pos + 1, self.put_str(node.value))
        elif k == "array":
            n = len(node.items); hl = vlen(n); blk = self.alloc(hl + 5 * n)
            self.varint(blk, n); b[pos] = TAG_ARR; self.u32(pos + 1, blk)
            for i, c in enumerate(node.items): self.any_slot(c, blk + hl + 5 * i, depth + 1)
        elif k == "object":
            n = len(node.items); hl = vlen(n); blk = self.alloc(hl + 9 * n)
            self.varint(blk, n); b[pos] = TAG_OBJ; self.u32(pos + 1, blk)
            for i, (key, c) in enumerate(node.items.items()):
                e = blk + hl + 9 * i
                ko = self.keys.get(key)
                if ko is None: ko = self.keys[key] = self.put_str(key.encode("utf-8"))
                self.u32(e, ko); self.any_slot(c, e + 4, depth + 1)
        elif k == "ref":
            b[pos] = TAG_REF; self.refs.append((pos + 1, node, ANY))
        else: raise JsonxError("E_TYPE_MISMATCH", f"cannot encode {k}")

    # ---- typed
    def scalar(self, t, node, pos):
        k = t.kind; b = self.buf
        if k == "bool":
            if node.kind != "bool": self.mismatch(t, node)
            b[pos] = 1 if node.value else 0
        elif k in INTS:
            if node.kind != "int": self.mismatch(t, node)
            v = node.value; sg, bits = INTS[k]
            lo, hi = (-(1 << (bits - 1)), (1 << (bits - 1)) - 1) if sg else (0, (1 << bits) - 1)
            if not lo <= v <= hi: self.err("E_RANGE", f"{v} does not fit in {k} ({lo}..{hi})", node)
            struct.pack_into(_FMT[k], b, pos, v)
        elif k in FLOATS:
            if node.kind not in ("int", "float", "f32"): self.mismatch(t, node)
            v = float(node.value)
            if k == "f32":
                if abs(v) > F32_MAX and v == v and abs(v) != float("inf"):
                    r = struct.unpack("<f", struct.pack("<f", 0))[0]
                    # round-to-nearest: values above F32_MAX that still round to max are ok, others overflow
                    if abs(v) >= 3.4028235677973366e38: self.err("E_RANGE", f"{v} overflows f32", node)
                struct.pack_into("<f", b, pos, v)
            else: struct.pack_into("<d", b, pos, v)
        elif k == "enum":
            en = self.sc.types[t.idx]; size = {"u8": 1, "u16": 2, "u32": 4}[en.base]
            if node.kind == "string":
                m = [v for n, v in en.values if n == node.value]
                if not m: self.err("E_TYPE_MISMATCH", f"'{node.value}' is not a member of enum {en.name}", node, expected="one of " + ", ".join(n for n, _ in en.values), actual=node.value)
                v = m[0]
            elif node.kind == "int" and 0 <= node.value < (1 << (8 * size)): v = node.value
            else: self.mismatch(t, node)
            b[pos:pos + size] = v.to_bytes(size, "little")

    def heap_put(self, t, node) -> int:
        k = t.kind
        if k == "string":
            if node.kind != "string": self.mismatch(t, node)
            return self.put_str(node.value.encode("utf-8"))
        if k == "bytes":
            return self.put_str(self.bytes_of(node))
        if k == "array": return self.array_block(t, node)
        if k == "struct":
            if node.kind != "object": self.mismatch(t, node)
            _, size = self.sc.layout(t.idx, self.ver); p = self.alloc(size)
            self.struct_block(t.idx, node, p); return p
        p = self.alloc(self.sc.slot_size(t)); self.scalar(t, node, p); return p

    def bytes_of(self, node) -> bytes:
        if node.kind == "bytes": return node.value
        if node.kind != "string": self.mismatch(T("bytes"), node)
        try: b = base64.b64decode(node.value, validate=True)
        except (binascii.Error, ValueError): b = None
        if b is None or base64.b64encode(b).decode() != node.value: self.err("E_TYPE_MISMATCH", "bytes fields take a canonical base64 string", node)
        return b

    def slot(self, t, node, pos):
        k = t.kind
        if k in SCALARS or k == "enum": self.scalar(t, node, pos); self.reg(node, pos, t)
        elif k in ("string", "bytes", "array", "struct"):
            off = self.heap_put(t, node); self.u32(pos, off)
            if k != "struct": self.reg(node, pos, t)
        elif k == "union":
            if node.kind != "object" or len(node.items) != 1:
                self.err("E_TYPE_MISMATCH", f"{self.sc.type_name(t)} value must be an object with exactly one key naming a variant", node)
            un = self.sc.types[t.idx]; (name, child), = node.items.items()
            v = [x for x in un.variants if x[0] == name]
            if not v: self.err("E_UNKNOWN_FIELD", f"'{name}' is not a variant of union {un.name}", child, expected=", ".join(x[0] for x in un.variants), actual=name)
            _, tag, vt = v[0]
            off = self.heap_put(vt, child); self.buf[pos] = tag; self.u32(pos + 1, off); self.reg(node, pos, t)
        elif k == "any": self.any_slot(node, pos)
        elif k == "ref":
            if node.kind != "ref": self.err("E_TYPE_MISMATCH", f"expected {self.sc.type_name(t)}, got {_kd(node)} (inline values must be stored in a non-ref field)", node)
            self.refs.append((pos, node, t.elem))

    def array_block(self, t, node):
        if node.kind != "array": self.mismatch(t, node)
        elem = t.elem; items = node.items; n = len(items)
        if elem.kind == "struct":
            _, size = self.sc.layout(elem.idx, self.ver); hl = vlen(n) + vlen(size)
            blk = self.alloc(hl + n * size); p = self.varint(blk, n); self.varint(p, size)
            for i, c in enumerate(items):
                if c.kind != "object": self.mismatch(elem, c)
                self.struct_block(elem.idx, c, blk + hl + i * size)
        else:
            es = self.sc.slot_size(elem); hl = vlen(n); blk = self.alloc(hl + n * es); self.varint(blk, n)
            for i, c in enumerate(items): self.slot(elem, c, blk + hl + i * es)
        return blk

    def struct_block(self, idx, node, base):
        sc = self.sc; st = sc.types[idx]
        self.reg(node, base, T("struct", idx))
        lay, _ = sc.layout(idx, self.ver)
        known = {fl.field.name for fl in lay}
        for k, c in node.items.items():
            if k not in known: self.err("E_UNKNOWN_FIELD", f"schema {st.name} has no field '{k}'" + (f" in version {self.ver}" if k in st.by_name else ""), c)
        for fl in lay:
            f = fl.field; child = node.items.get(f.name); opt = f.optional or f.nullable
            if child is None:
                if f.default is not None: child = _mk_default(f.default, f.type)
                elif f.optional: continue
                elif f.nullable: self.err("E_MISSING_FIELD", f"{st.name}.{f.name} is nullable but not optional: write null explicitly", node)
                else: self.err("E_MISSING_FIELD", f"{st.name}.{f.name} is required", node, expected=f.name)
            if opt:
                if child.kind == "null": continue
                self.buf[base + fl.pbyte] |= 1 << fl.bit
            elif child.kind == "null" and f.type.kind != "any":
                self.err("E_TYPE_MISMATCH", f"{st.name}.{f.name} is not nullable", child)
            self.slot(f.type, child, base + fl.off)
            if f.cons: self.constrain(st, f, child)

    def constrain(self, st, f, node):
        for name, (lo, hi) in f.cons.items():
            if name == "range":
                v = node.value
                if node.kind in ("int", "float", "f32") and ((lo is not None and v < lo) or (hi is not None and v > hi)):
                    self.err("E_CONSTRAINT", f"{st.name}.{f.name} = {v} violates @range({lo if lo is not None else '_'}, {hi if hi is not None else '_'})", node)
            else:
                n = len(node.value) if node.kind in ("string",) else len(node.value) if node.kind == "bytes" else len(node.items) if node.kind == "array" else None
                if node.kind == "string": n = len(node.value)  # code points
                if node.kind == "string" and f.type.kind == "bytes": n = len(self.bytes_of(node))
                if n is not None and ((lo is not None and n < lo) or (hi is not None and n > hi)):
                    self.err("E_CONSTRAINT", f"{st.name}.{f.name} has length {n}, violating @len({lo if lo is not None else '_'}, {hi if hi is not None else '_'})", node)

def _mk_default(d, t) -> Node:
    k, v = d
    n = Node(k if k != "array" else "array", v)
    if k == "array": n.items = []
    return n

def encode(doc, schema: Optional[Schema] = None, *, root: Optional[str] = None, version: Optional[int] = None,
           checksum: bool = False) -> bytes:
    """Encode a parsed Document (or Node) to BJSONX. Without `schema` the document is stored in dynamic (any) mode."""
    node = doc.root if isinstance(doc, Document) else doc
    src = doc.src if isinstance(doc, Document) else ""
    if schema is None:
        e = _Enc(None, 0, src); p = e.alloc(5); e.any_slot(node, p); root_off = p; sid = 0; ver = 0
    else:
        ver = schema.version if version is None else version
        if ver < 1 or ver > schema.version: raise JsonxError("E_SCHEMA_INVALID", f"cannot encode version {ver} with a schema of version {schema.version}")
        ridx = schema.root if root is None else schema.struct_index(root)
        if ridx is None: raise JsonxError("E_SCHEMA_INVALID", "schema declares no root; pass root=<schema name>")
        e = _Enc(schema, ver, src)
        if node.kind != "object": e.mismatch(T("struct", ridx), node)
        _, size = schema.layout(ridx, ver); root_off = e.alloc(size)
        e.struct_block(ridx, node, root_off)
        sid = schema.id if root is None else __import__("jsonx.schema", fromlist=["x"]).fnv1a64((schema.namespace + "\0" + root).encode())
    for pos, rnode, exp in e.refs:
        tgt = e.locs.get(rnode.target)
        if tgt is None: e.err("E_REF_TARGET", "reference target is not addressable in the binary layout (e.g. a union payload that is not a schema)", rnode)
        loc, tt = tgt
        if tt != exp: e.err("E_REF_TARGET", f"reference of type ref<{exp.key() if schema is None else schema.type_name(exp)}> points at a value of type {tt.key() if schema is None else schema.type_name(tt)}", rnode,
                            expected=exp.key(), actual=tt.key())
        e.u32(pos, loc)
    buf = e.buf
    flags = F_CRC if checksum else 0
    if checksum: buf.extend(b"\0\0\0\0")
    struct.pack_into("<3sBHHQIII", buf, 0, MAGIC, FORMAT_VERSION, flags, 0, sid, ver, root_off, len(buf))
    if checksum: struct.pack_into("<I", buf, len(buf) - 4, zlib.crc32(bytes(buf[:-4])))
    return bytes(buf)

# ===================================================================== reader
class Reader:
    def __init__(self, data, schema: Optional[Schema] = None, verify_crc: bool = True, strict_version: bool = False):
        mv = memoryview(data)
        if mv.ndim != 1 or mv.itemsize != 1: mv = mv.cast("B")
        self.d = mv; n = self.n = len(mv)
        def bad(c, m): raise JsonxError(c, m)
        if n < HDR: bad("E_BIN_TRUNCATED", f"blob is {n} bytes, shorter than the {HDR}-byte header")
        if bytes(mv[:3]) != MAGIC: bad("E_BIN_MAGIC", "not a BJSONX blob (bad magic)")
        if mv[3] != FORMAT_VERSION: bad("E_BIN_VERSION", f"unsupported BJSONX format version {mv[3]}")
        _, _, flags, res, sid, sver, root, length = struct.unpack_from("<3sBHHQIII", mv, 0)
        if flags & ~F_CRC or res: bad("E_BIN_INVALID", "reserved header bits are set")
        if length > n: bad("E_BIN_TRUNCATED", f"header says {length} bytes but only {n} are present")
        if length < n: bad("E_BIN_INVALID", f"{n - length} trailing bytes after the blob (use read_frame for streams)")
        if flags & F_CRC:
            if n < HDR + 4: bad("E_BIN_TRUNCATED", "missing checksum")
            if verify_crc and zlib.crc32(mv[:n - 4]) != struct.unpack_from("<I", mv, n - 4)[0]: bad("E_BIN_CHECKSUM", "CRC-32 mismatch")
            self.n = n - 4
        self.sid, self.sver, self.root, self.sc = sid, sver, root, schema
        self.dynamic = sid == 0 and sver == 0
        if not self.dynamic:
            if schema is None: bad("E_BIN_SCHEMA_MISMATCH", f"blob requires schema {sid:016x} version {sver}; none supplied")
            if sver < 1: bad("E_BIN_INVALID", "typed blob with schema version 0")
            self.ridx = None
            for i, t in enumerate(schema.types):
                if t.kind == "struct" and __import__("jsonx.schema", fromlist=["x"]).fnv1a64((schema.namespace + "\0" + t.name).encode()) == sid:
                    self.ridx = i; break
            if self.ridx is None: bad("E_BIN_SCHEMA_MISMATCH", f"schema id {sid:016x} does not match any schema in the supplied definition")
            if strict_version and sver > schema.version: bad("E_BIN_SCHEMA_MISMATCH", f"blob version {sver} is newer than the reader's schema version {schema.version}")
        self.w = sver
        self.root_end = None
        if root < HDR or root >= self.n: bad("E_BIN_BOUNDS", f"root offset {root} is outside the payload")
        self._f = {}

    # -- primitives (all bounds-checked)
    def need(self, p, k):
        if p < HDR or p + k > self.n: raise JsonxError("E_BIN_BOUNDS", f"access of {k} bytes at offset {p} is outside the payload [{HDR}, {self.n})")
    def u8(self, p): self.need(p, 1); return self.d[p]
    def u32(self, p): self.need(p, 4); return struct.unpack_from("<I", self.d, p)[0]
    def varint(self, p):
        v = 0; sh = 0
        for i in range(5):
            self.need(p + i, 1); b = self.d[p + i]; v |= (b & 0x7F) << sh; sh += 7
            if not b & 0x80:
                if i and b == 0: raise JsonxError("E_BIN_INVALID", f"non-minimal varint at offset {p}")
                if v > 0xFFFFFFFF: raise JsonxError("E_BIN_INVALID", "varint exceeds u32")
                return v, p + i + 1
        raise JsonxError("E_BIN_INVALID", f"varint longer than 5 bytes at offset {p}")
    def fwd(self, off, ms, what):
        if off <= ms: raise JsonxError("E_BIN_INVALID", f"{what} offset {off} does not point forward of its container at {ms}")
        return off
    def hbytes(self, off):
        n, p = self.varint(off); self.need(p, n); return self.d[p:p + n]
    def hstr(self, off):
        b = self.hbytes(off)
        try: return str(b, "utf-8")
        except UnicodeDecodeError: raise JsonxError("E_BIN_UTF8", f"invalid UTF-8 in string at offset {off}") from None
    def scalar(self, k, p):
        if k == "bool":
            v = self.u8(p)
            if v > 1: raise JsonxError("E_BIN_INVALID", f"bool byte {v} at offset {p}")
            return v == 1
        self.need(p, SCALAR_SIZE[k]); return struct.unpack_from(_FMT[k], self.d, p)[0]
    def enum_raw(self, t, p):
        en = self.sc.types[t.idx]; sz = {"u8": 1, "u16": 2, "u32": 4}[en.base]; self.need(p, sz)
        return int.from_bytes(self.d[p:p + sz], "little"), en
    def fields(self, idx):
        f = self._f.get(idx)
        if f is None:
            lay, size = self.sc.layout(idx, self.w); f = self._f[idx] = ({fl.field.name: fl for fl in lay}, lay, size)
        return f

    # -- typed reads returning python scalars / views
    def slot_get(self, t, pos, ms):
        k = t.kind
        if k in SCALARS: return self.scalar(k, pos)
        if k == "enum":
            v, en = self.enum_raw(t, pos)
            for n, x in en.values:
                if x == v: return n
            return v
        if k == "any": return self.any_get(pos, ms)
        if k == "union":
            return UnionView(self, t.idx, pos, ms)
        if k == "ref":
            loc = self.u32(pos); return self.value_at(t.elem, loc)
        off = self.fwd(self.u32(pos), ms, k)
        return self.heap_get(t, off, ms)

    def heap_get(self, t, off, ms):
        k = t.kind
        if k == "string": return self.hstr(off)
        if k == "bytes": return self.hbytes(off)
        if k == "array": return self.array_view(t, off)
        if k == "struct": return StructView(self, t.idx, off)
        if k in SCALARS: return self.scalar(k, off)
        if k == "enum":
            v, en = self.enum_raw(t, off)
            for n, x in en.values:
                if x == v: return n
            return v
        raise JsonxError("E_BIN_INVALID", f"type {k} cannot be stored by offset")

    def value_at(self, t, loc):
        if t.kind == "struct": return StructView(self, t.idx, loc)
        self.need(loc, self.sc.slot_size(t))
        return self.slot_get(t, loc, 0)

    def array_view(self, t, off):
        n, p = self.varint(off); elem = t.elem
        if elem.kind == "struct":
            stride, p = self.varint(p); _, _, ksz = self.fields(elem.idx)
            if stride < ksz: raise JsonxError("E_BIN_INVALID", f"array stride {stride} smaller than struct size {ksz}")
            self.need(p, n * stride) if n else None
            return ArrayView(self, elem, off, n, p, stride)
        es = self.sc.slot_size(elem); self.need(p, n * es) if n else None
        return ArrayView(self, elem, off, n, p, es)

    def any_get(self, pos, ms):
        tag = self.u8(pos); pl = self.u32(pos + 1)
        if tag in (TAG_NULL, TAG_FALSE, TAG_TRUE):
            if pl: raise JsonxError("E_BIN_INVALID", f"nonzero payload on constant tag at {pos}")
            return None if tag == 0 else tag == TAG_TRUE
        if tag == TAG_I32: return struct.unpack_from("<i", self.d, pos + 1)[0]
        if tag == TAG_REF:
            self.need(pl, 5)
            if self.d[pl] == TAG_REF: raise JsonxError("E_BIN_INVALID_REF", f"reference at {pos} targets another reference")
            return self.any_get(pl, 0)
        self.fwd(pl, ms, "value")
        if tag == TAG_I64: return self.scalar("i64", pl)
        if tag == TAG_U64: return self.scalar("u64", pl)
        if tag == TAG_F64: return self.scalar("f64", pl)
        if tag == TAG_STR: return self.hstr(pl)
        if tag == TAG_BYTES: return self.hbytes(pl)
        if tag == TAG_ARR:
            n, p = self.varint(pl); self.need(p, 5 * n) if n else None; return AnyArray(self, pl, n, p)
        if tag == TAG_OBJ:
            n, p = self.varint(pl); self.need(p, 9 * n) if n else None; return AnyObject(self, pl, n, p)
        raise JsonxError("E_BIN_INVALID", f"unknown any-tag {tag} at offset {pos}")

class StructView:
    """Zero-copy view of a struct instance; fields are decoded on access."""
    __slots__ = ("r", "idx", "loc")
    def __init__(self, r, idx, loc):
        self.r, self.idx, self.loc = r, idx, loc
        r.need(loc, r.fields(idx)[2])
    def _find(self, name):
        by, _, _ = self.r.fields(self.idx)
        fl = by.get(name)
        return fl
    def keys(self): return [fl.field.name for fl in self.r.fields(self.idx)[1]]
    def __contains__(self, name): return self[name] is not None
    def __getitem__(self, name):
        fl = self._find(name); r = self.r
        if fl is None:
            f = r.sc.types[self.idx].by_name.get(name)
            if f is None: raise KeyError(name)
            if f.default is not None: return f.default[1] if f.default[0] != "array" else []
            return None
        if fl.pbyte is not None and not (r.u8(self.loc + fl.pbyte) >> fl.bit) & 1: return None
        return r.slot_get(fl.field.type, self.loc + fl.off, self.loc)
    def get(self, name, default=None):
        try: v = self[name]
        except KeyError: return default
        return default if v is None else v
    def items(self): return [(k, self[k]) for k in self.keys()]
    def __iter__(self): return iter(self.keys())

class ArrayView:
    __slots__ = ("r", "elem", "blk", "n", "start", "stride")
    def __init__(self, r, elem, blk, n, start, stride): self.r, self.elem, self.blk, self.n, self.start, self.stride = r, elem, blk, n, start, stride
    def __len__(self): return self.n
    def __getitem__(self, i):
        if i < 0: i += self.n
        if not 0 <= i < self.n: raise IndexError(i)
        p = self.start + i * self.stride
        if self.elem.kind == "struct": return StructView(self.r, self.elem.idx, p)
        return self.r.slot_get(self.elem, p, self.blk)
    def __iter__(self):
        for i in range(self.n): yield self[i]

class UnionView:
    __slots__ = ("r", "idx", "pos", "ms", "tag")
    def __init__(self, r, idx, pos, ms): self.r, self.idx, self.pos, self.ms = r, idx, pos, ms; self.tag = r.u8(pos); r.need(pos, 5)
    @property
    def name(self):
        for n, tag, _ in self.r.sc.types[self.idx].variants:
            if tag == self.tag: return n
        return None
    @property
    def value(self):
        for n, tag, t in self.r.sc.types[self.idx].variants:
            if tag == self.tag:
                off = self.r.fwd(self.r.u32(self.pos + 1), self.ms, "union payload"); return self.r.heap_get(t, off, self.ms)
        return None

class AnyArray:
    __slots__ = ("r", "blk", "n", "start")
    def __init__(self, r, blk, n, start): self.r, self.blk, self.n, self.start = r, blk, n, start
    def __len__(self): return self.n
    def __getitem__(self, i):
        if i < 0: i += self.n
        if not 0 <= i < self.n: raise IndexError(i)
        return self.r.any_get(self.start + 5 * i, self.blk)
    def __iter__(self):
        for i in range(self.n): yield self[i]

class AnyObject:
    __slots__ = ("r", "blk", "n", "start")
    def __init__(self, r, blk, n, start): self.r, self.blk, self.n, self.start = r, blk, n, start
    def __len__(self): return self.n
    def key_at(self, i): return self.r.hstr(self.r.u32(self.start + 9 * i))
    def keys(self): return [self.key_at(i) for i in range(self.n)]
    def __getitem__(self, key):
        for i in range(self.n):
            if self.key_at(i) == key: return self.r.any_get(self.start + 9 * i + 4, self.blk)
        raise KeyError(key)
    def get(self, key, default=None):
        try: return self[key]
        except KeyError: return default
    def items(self): return [(self.key_at(i), self.r.any_get(self.start + 9 * i + 4, self.blk)) for i in range(self.n)]
    def __iter__(self): return iter(self.keys())

def view(data, schema: Optional[Schema] = None, **kw):
    """Open a blob without copying. Returns an AnyView-like object (dynamic) or StructView (typed)."""
    r = Reader(data, schema, **kw)
    if r.dynamic: return r.any_get(r.root, r.root)
    return StructView(r, r.ridx, r.root)

# ===================================================================== owned decoder
class _Dec:
    def __init__(self, r: Reader):
        self.r = r; self.memo = {}; self.pending = []; self.visited = 0
        self.budget = max(1 << 20, 256 * r.n); self.used = 0

    def node(self, kind, value, parent, key, loc, t, depth):
        if depth > MAX_DEPTH: raise JsonxError("E_BIN_LIMIT", f"nesting deeper than {MAX_DEPTH}")
        self.visited += 1
        if self.visited > self.r.n: raise JsonxError("E_BIN_LIMIT", "decoded node count exceeds the blob size (malformed or hostile input)")
        n = Node(kind, value); n.parent = parent; n.key = key
        if loc is not None:
            k = (loc, t.key())
            if k in self.memo: raise JsonxError("E_BIN_INVALID", f"location {loc} is used by two values (shared or overlapping data)")
            self.memo[k] = n
        return n

    def charge(self, k):
        self.used += k
        if self.used > self.budget: raise JsonxError("E_BIN_LIMIT", "decoded string/bytes volume exceeds the expansion limit")

    def run(self) -> Node:
        r = self.r
        if r.dynamic: root = self.any_node(r.root, r.root, None, None, 0)
        else: root = self.struct_node(r.ridx, r.root, None, None, 0)
        for n, loc, t in self.pending:
            tgt = self.memo.get((loc, t.key()))
            if tgt is None: raise JsonxError("E_BIN_INVALID_REF", f"reference to offset {loc} does not hit a value of type {t.key() if r.sc is None else (r.sc.type_name(t))}")
            n.target = tgt
        return root

    def any_node(self, pos, ms, parent, key, depth):
        r = self.r; tag = r.u8(pos); pl = r.u32(pos + 1)
        T_ = ANY
        if tag in (TAG_NULL, TAG_FALSE, TAG_TRUE):
            if pl: raise JsonxError("E_BIN_INVALID", f"nonzero payload on constant tag at {pos}")
            return self.node("null" if tag == 0 else "bool", None if tag == 0 else tag == TAG_TRUE, parent, key, pos, T_, depth)
        if tag == TAG_I32: return self.node("int", struct.unpack_from("<i", r.d, pos + 1)[0], parent, key, pos, T_, depth)
        if tag == TAG_REF:
            n = self.node("ref", None, parent, key, None, T_, depth); self.pending.append((n, pl, T_))
            r.need(pl, 5); return n
        r.fwd(pl, ms, "value")
        if tag == TAG_I64: return self.node("int", r.scalar("i64", pl), parent, key, pos, T_, depth)
        if tag == TAG_U64: return self.node("int", r.scalar("u64", pl), parent, key, pos, T_, depth)
        if tag == TAG_F64: return self.node("float", r.scalar("f64", pl), parent, key, pos, T_, depth)
        if tag == TAG_STR:
            s = r.hstr(pl); self.charge(len(s)); return self.node("string", s, parent, key, pos, T_, depth)
        if tag == TAG_BYTES:
            b = bytes(r.hbytes(pl)); self.charge(len(b)); return self.node("bytes", b, parent, key, pos, T_, depth)
        if tag == TAG_ARR:
            n, p = r.varint(pl); r.need(p, 5 * n) if n else None
            node = self.node("array", None, parent, key, pos, T_, depth); node.items = []
            for i in range(n): node.items.append(self.any_node(p + 5 * i, pl, node, i, depth + 1))
            return node
        if tag == TAG_OBJ:
            n, p = r.varint(pl); r.need(p, 9 * n) if n else None
            node = self.node("object", None, parent, key, pos, T_, depth); node.items = {}; node.labels = []
            for i in range(n):
                e = p + 9 * i; k = r.hstr(r.u32(e)); self.charge(len(k))
                if k in node.items: raise JsonxError("E_BIN_INVALID", f"duplicate object key {k!r}")
                node.items[k] = self.any_node(e + 4, pl, node, k, depth + 1)
            return node
        raise JsonxError("E_BIN_INVALID", f"unknown any-tag {tag} at offset {pos}")

    def struct_node(self, idx, base, parent, key, depth):
        r = self.r; by, lay, size = r.fields(idx); sc = r.sc
        r.need(base, size)
        node = self.node("object", None, parent, key, base, T("struct", idx), depth); node.items = {}; node.labels = []
        have = set()
        for fl in lay:
            f = fl.field; have.add(f.name)
            if fl.pbyte is not None and not (r.u8(base + fl.pbyte) >> fl.bit) & 1:
                if f.nullable:
                    c = Node("null"); c.parent = node; c.key = f.name; node.items[f.name] = c
                continue
            node.items[f.name] = self.slot_node(f.type, base + fl.off, base, node, f.name, depth + 1)
        for f in sc.types[idx].fields:   # fields newer than the blob: defaults
            if f.name not in have and f.default is not None:
                c = _mk_default(f.default, f.type); c.parent = node; c.key = f.name; node.items[f.name] = c
        return node

    def slot_node(self, t, pos, ms, parent, key, depth):
        r = self.r; k = t.kind
        if k == "bool": return self.node("bool", r.scalar("bool", pos), parent, key, pos, t, depth)
        if k in INTS: return self.node("int", r.scalar(k, pos), parent, key, pos, t, depth)
        if k == "f64": return self.node("float", r.scalar("f64", pos), parent, key, pos, t, depth)
        if k == "f32": return self.node("f32", r.scalar("f32", pos), parent, key, pos, t, depth)
        if k == "enum": return self.enum_node(t, pos, parent, key, pos, depth)
        if k == "any": return self.any_node(pos, ms, parent, key, depth)
        if k == "ref":
            n = self.node("ref", None, parent, key, None, t, depth); loc = r.u32(pos); r.need(loc, 1); self.pending.append((n, loc, t.elem)); return n
        if k == "union":
            un = r.sc.types[t.idx]; tag = r.u8(pos); off = r.fwd(r.u32(pos + 1), ms, "union payload")
            v = [x for x in un.variants if x[1] == tag]
            if not v: raise JsonxError("E_BIN_INVALID", f"unknown union tag {tag} in {un.name}")
            node = self.node("object", None, parent, key, pos, t, depth); node.items = {}; node.labels = []
            node.items[v[0][0]] = self.heap_node(v[0][2], off, ms, node, v[0][0], depth + 1)
            return node
        off = r.fwd(r.u32(pos), ms, k)
        n = self.heap_node(t, off, ms, parent, key, depth, slotpos=pos)
        return n

    def enum_node(self, t, pos, parent, key, loc, depth):
        v, en = self.r.enum_raw(t, pos)
        for n, x in en.values:
            if x == v: return self.node("string", n, parent, key, loc, t, depth)
        return self.node("int", v, parent, key, loc, t, depth)

    def heap_node(self, t, off, ms, parent, key, depth, slotpos=None):
        r = self.r; k = t.kind
        loc = slotpos if slotpos is not None else None
        if k == "string":
            s = r.hstr(off); self.charge(len(s)); return self.node("string", s, parent, key, loc, t, depth)
        if k == "bytes":
            b = bytes(r.hbytes(off)); self.charge(len(b)); return self.node("bytes", b, parent, key, loc, t, depth)
        if k == "struct": return self.struct_node(t.idx, off, parent, key, depth)
        if k == "array":
            av = r.array_view(t, off)
            node = self.node("array", None, parent, key, loc, t, depth); node.items = []
            for i in range(av.n):
                p = av.start + i * av.stride
                if t.elem.kind == "struct": node.items.append(self.struct_node(t.elem.idx, p, node, i, depth + 1))
                else: node.items.append(self.slot_node(t.elem, p, off, node, i, depth + 1))
            return node
        if k in SCALARS or k == "enum":   # union payload scalar
            tmp = self.slot_node(t, off, ms, parent, key, depth)
            return tmp
        raise JsonxError("E_BIN_INVALID", f"type {k} cannot be stored by offset")

def decode(data, schema: Optional[Schema] = None, **kw) -> Document:
    """Fully validate and decode a blob into an owned Document (aliasing preserved)."""
    r = Reader(data, schema, **kw)
    return Document(_Dec(r).run())

def blob_length(header: bytes) -> int:
    """For stream framing: given at least 28 header bytes, return the total blob length."""
    if len(header) < HDR: raise JsonxError("E_BIN_TRUNCATED", "need 28 header bytes")
    if header[:3] != MAGIC: raise JsonxError("E_BIN_MAGIC", "bad magic")
    return struct.unpack_from("<I", header, 24)[0]
