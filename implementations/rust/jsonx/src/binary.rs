use crate::*;
use std::collections::HashMap;

pub const HDR: usize = 28;
const MAXD: usize = 256;
const T_NULL: u8 = 0;
const T_FALSE: u8 = 1;
const T_TRUE: u8 = 2;
const T_I32: u8 = 3;
const T_I64: u8 = 4;
const T_U64: u8 = 5;
const T_F64: u8 = 6;
const T_STR: u8 = 7;
const T_ARR: u8 = 8;
const T_OBJ: u8 = 9;
const T_REF: u8 = 10;
const T_BYTES: u8 = 11;

fn vlen(n: usize) -> usize { if n < 0x80 { 1 } else if n < 0x4000 { 2 } else if n < 0x200000 { 3 } else if n < 0x10000000 { 4 } else { 5 } }
pub fn crc32(b: &[u8]) -> u32 {
    let mut t = [0u32; 256];
    for i in 0..256u32 { let mut c = i; for _ in 0..8 { c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 }; } t[i as usize] = c; }
    let mut c = 0xffffffffu32; for &x in b { c = t[((c ^ x as u32) & 255) as usize] ^ (c >> 8); } !c
}
fn int_range(t: &Ty) -> Option<(i128, i128)> {
    Some(match t { Ty::U8 => (0, 255), Ty::U16 => (0, 65535), Ty::U32 => (0, u32::MAX as i128), Ty::U64 => (0, u64::MAX as i128),
        Ty::I8 => (-128, 127), Ty::I16 => (-32768, 32767), Ty::I32 => (i32::MIN as i128, i32::MAX as i128), Ty::I64 => (i64::MIN as i128, i64::MAX as i128), _ => return None })
}
fn is_scalar(t: &Ty) -> bool { matches!(t, Ty::Bool | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::F32 | Ty::F64) }
fn kd(k: &Kind) -> &'static str { match k { Kind::Null => "null", Kind::Bool(_) => "boolean", Kind::Int(_) => "integer", Kind::Float(_) | Kind::F32(_) => "float", Kind::Str(_) => "string", Kind::Bytes(_) => "bytes", Kind::Arr(_) => "array", Kind::Obj(_) => "object", Kind::Ref => "reference" } }

// ===================================================================== encoder
struct Enc<'a> { doc: &'a Doc, sc: Option<&'a Schema>, ver: usize, buf: Vec<u8>, refs: Vec<(usize, Id, Ty)>, locs: HashMap<Id, (usize, Ty)>, keys: HashMap<String, usize>, defaults: HashMap<(usize, usize), Id> }
impl<'a> Enc<'a> {
    fn fail<T>(&self, code: &str, msg: String, n: Option<Id>) -> Res<T> {
        let mut e = Error { code: code.into(), message: msg, pos: None, line: None, col: None };
        if let Some(id) = n { let p = self.doc.nodes[id].pos; if p != NO_POS && !self.doc.src.is_empty() { let (l, c) = line_col(&self.doc.src, p); e.pos = Some(p); e.line = Some(l); e.col = Some(c); } }
        Err(e)
    }
    fn tname(&self, t: &Ty) -> String { match self.sc { Some(s) => s.type_name(t), None => format!("{:?}", t).to_lowercase() } }
    fn mismatch<T>(&self, t: &Ty, id: Id) -> Res<T> { self.fail("E_TYPE_MISMATCH", format!("expected {}, got {}", self.tname(t), kd(&self.doc.nodes[id].kind)), Some(id)) }
    fn alloc(&mut self, n: usize) -> Res<usize> { let p = self.buf.len(); if p + n >= 0xFFFFFFFF { return err("E_BIN_LIMIT", "document exceeds the 4 GiB offset limit"); } self.buf.resize(p + n, 0); Ok(p) }
    fn varint(&mut self, mut p: usize, mut v: usize) -> usize { while v >= 0x80 { self.buf[p] = (v & 0x7f) as u8 | 0x80; v >>= 7; p += 1; } self.buf[p] = v as u8; p + 1 }
    fn u32(&mut self, p: usize, v: usize) { self.buf[p..p + 4].copy_from_slice(&(v as u32).to_le_bytes()); }
    fn put_str(&mut self, b: &[u8]) -> Res<usize> { let p = self.alloc(vlen(b.len()) + b.len())?; let q = self.varint(p, b.len()); self.buf[q..q + b.len()].copy_from_slice(b); Ok(p) }

    fn any_slot(&mut self, id: Id, pos: usize, depth: usize) -> Res<()> {
        if depth > MAXD { return err("E_DEPTH_LIMIT", "nesting too deep"); }
        self.locs.insert(id, (pos, Ty::Any));
        let doc = self.doc;
        match &doc.nodes[id].kind {
            Kind::Null => {}
            Kind::Bool(b) => self.buf[pos] = if *b { T_TRUE } else { T_FALSE },
            Kind::Int(v) => {
                if *v >= i32::MIN as i128 && *v <= i32::MAX as i128 { self.buf[pos] = T_I32; self.buf[pos + 1..pos + 5].copy_from_slice(&(*v as i32).to_le_bytes()); }
                else if *v >= i64::MIN as i128 && *v <= i64::MAX as i128 { let p = self.alloc(8)?; self.buf[p..p + 8].copy_from_slice(&(*v as i64).to_le_bytes()); self.buf[pos] = T_I64; self.u32(pos + 1, p); }
                else { let p = self.alloc(8)?; self.buf[p..p + 8].copy_from_slice(&(*v as u64).to_le_bytes()); self.buf[pos] = T_U64; self.u32(pos + 1, p); }
            }
            Kind::Float(f) => { let p = self.alloc(8)?; self.buf[p..p + 8].copy_from_slice(&f.to_le_bytes()); self.buf[pos] = T_F64; self.u32(pos + 1, p); }
            Kind::F32(f) => { let p = self.alloc(8)?; self.buf[p..p + 8].copy_from_slice(&(*f as f64).to_le_bytes()); self.buf[pos] = T_F64; self.u32(pos + 1, p); }
            Kind::Str(s) => { self.buf[pos] = T_STR; let p = self.put_str(s.as_bytes())?; self.u32(pos + 1, p); }
            Kind::Bytes(b) => { self.buf[pos] = T_BYTES; let p = self.put_str(b)?; self.u32(pos + 1, p); }
            Kind::Arr(items) => {
                let n = items.len(); let hl = vlen(n); let blk = self.alloc(hl + 5 * n)?; self.varint(blk, n); self.buf[pos] = T_ARR; self.u32(pos + 1, blk);
                for (i, c) in items.iter().enumerate() { self.any_slot(*c, blk + hl + 5 * i, depth + 1)?; }
            }
            Kind::Obj(m) => {
                let n = m.len(); let hl = vlen(n); let blk = self.alloc(hl + 9 * n)?; self.varint(blk, n); self.buf[pos] = T_OBJ; self.u32(pos + 1, blk);
                for (i, (k, c)) in m.iter().enumerate() {
                    let e = blk + hl + 9 * i;
                    let ko = match self.keys.get(k) { Some(o) => *o, None => { let o = self.put_str(k.as_bytes())?; self.keys.insert(k.clone(), o); o } };
                    self.u32(e, ko); self.any_slot(*c, e + 4, depth + 1)?;
                }
            }
            Kind::Ref => { self.buf[pos] = T_REF; self.refs.push((pos + 1, id, Ty::Any)); }
        }
        Ok(())
    }

    fn scalar(&mut self, t: &Ty, id: Id, pos: usize) -> Res<()> {
        let doc = self.doc; let k = &doc.nodes[id].kind;
        match t {
            Ty::Bool => match k { Kind::Bool(b) => self.buf[pos] = *b as u8, _ => return self.mismatch(t, id) },
            Ty::F32 | Ty::F64 => {
                let v = match k { Kind::Int(i) => *i as f64, Kind::Float(f) => *f, Kind::F32(f) => *f as f64, _ => return self.mismatch(t, id) };
                if *t == Ty::F32 { let f = v as f32; if f.is_infinite() && v.is_finite() { return self.fail("E_RANGE", format!("{} overflows f32", v), Some(id)); } self.buf[pos..pos + 4].copy_from_slice(&f.to_le_bytes()); }
                else { self.buf[pos..pos + 8].copy_from_slice(&v.to_le_bytes()); }
            }
            Ty::Enum(ei) => {
                let (name, base, values) = match &self.sc.unwrap().types[*ei] { TypeDef::Enum { name, base, values } => (name, *base, values), _ => unreachable!() };
                let v: u32 = match k {
                    Kind::Str(s) => match values.iter().find(|(n, _)| n == s) { Some((_, v)) => *v, None => return self.fail("E_TYPE_MISMATCH", format!("'{}' is not a member of enum {}", s, name), Some(id)) },
                    Kind::Int(i) if *i >= 0 && *i < (1i128 << (8 * base)) => *i as u32,
                    _ => return self.mismatch(t, id),
                };
                self.buf[pos..pos + base].copy_from_slice(&v.to_le_bytes()[..base]);
            }
            _ => {
                let (lo, hi) = int_range(t).unwrap();
                let v = match k { Kind::Int(i) => *i, _ => return self.mismatch(t, id) };
                if v < lo || v > hi { return self.fail("E_RANGE", format!("{} does not fit in {}", v, self.tname(t)), Some(id)); }
                let n = self.sc.map(|s| s.slot_size(t)).unwrap_or(8); self.buf[pos..pos + n].copy_from_slice(&(v as u64).to_le_bytes()[..n]);
            }
        }
        Ok(())
    }
    fn bytes_of(&self, id: Id) -> Res<Vec<u8>> {
        match &self.doc.nodes[id].kind {
            Kind::Bytes(b) => Ok(b.clone()),
            Kind::Str(s) => match unb64(s) { Some(b) if b64(&b) == *s => Ok(b), _ => self.fail("E_TYPE_MISMATCH", "bytes fields take a base64 string".into(), Some(id)) },
            _ => self.mismatch(&Ty::Bytes, id),
        }
    }
    fn heap_put(&mut self, t: &Ty, id: Id) -> Res<usize> {
        let doc = self.doc; let sc = self.sc.unwrap();
        match t {
            Ty::Str => match &doc.nodes[id].kind { Kind::Str(s) => self.put_str(s.as_bytes()), _ => self.mismatch(t, id) },
            Ty::Bytes => { let b = self.bytes_of(id)?; self.put_str(&b) }
            Ty::Array(_) => self.array_block(t, id),
            Ty::Struct(i) => { if !matches!(doc.nodes[id].kind, Kind::Obj(_)) { return self.mismatch(t, id); } let p = self.alloc(sc.layout(*i, self.ver).size)?; self.struct_block(*i, id, p)?; Ok(p) }
            _ => { let p = self.alloc(sc.slot_size(t))?; self.scalar(t, id, p)?; Ok(p) }
        }
    }
    fn slot(&mut self, t: &Ty, id: Id, pos: usize) -> Res<()> {
        let doc = self.doc; let sc = self.sc.unwrap();
        match t {
            Ty::Str | Ty::Bytes | Ty::Array(_) | Ty::Struct(_) => { let off = self.heap_put(t, id)?; self.u32(pos, off); if !matches!(t, Ty::Struct(_)) { self.locs.insert(id, (pos, t.clone())); } }
            Ty::Union(ui) => {
                let m = match &doc.nodes[id].kind { Kind::Obj(m) if m.len() == 1 => m, _ => return self.fail("E_TYPE_MISMATCH", "union value must be an object with exactly one key naming a variant".into(), Some(id)) };
                let (uname, variants) = match &sc.types[*ui] { TypeDef::Union { name, variants } => (name, variants), _ => unreachable!() };
                let (name, child) = &m[0];
                let v = match variants.iter().find(|v| v.0 == *name) { Some(v) => v, None => return self.fail("E_UNKNOWN_FIELD", format!("'{}' is not a variant of union {}", name, uname), Some(*child)) };
                let off = self.heap_put(&v.2, *child)?; self.buf[pos] = v.1; self.u32(pos + 1, off); self.locs.insert(id, (pos, t.clone()));
            }
            Ty::Any => self.any_slot(id, pos, 0)?,
            Ty::Ref(e) => { if doc.nodes[id].kind != Kind::Ref { return self.fail("E_TYPE_MISMATCH", format!("expected {}, got {}", self.tname(t), kd(&doc.nodes[id].kind)), Some(id)); } self.refs.push((pos, id, (**e).clone())); }
            _ => { self.scalar(t, id, pos)?; self.locs.insert(id, (pos, t.clone())); }
        }
        Ok(())
    }
    fn array_block(&mut self, t: &Ty, id: Id) -> Res<usize> {
        let doc = self.doc; let sc = self.sc.unwrap();
        let items = match &doc.nodes[id].kind { Kind::Arr(a) => a, _ => return self.mismatch(t, id) };
        let elem = match t { Ty::Array(e) => &**e, _ => unreachable!() }; let n = items.len();
        let blk;
        if let Ty::Struct(si) = elem {
            let size = sc.layout(*si, self.ver).size; let hl = vlen(n) + vlen(size); blk = self.alloc(hl + n * size)?; let p = self.varint(blk, n); self.varint(p, size);
            for (i, c) in items.iter().enumerate() { if !matches!(doc.nodes[*c].kind, Kind::Obj(_)) { return self.mismatch(elem, *c); } self.struct_block(*si, *c, blk + hl + i * size)?; }
        } else {
            let es = sc.slot_size(elem); let hl = vlen(n); blk = self.alloc(hl + n * es)?; self.varint(blk, n);
            for (i, c) in items.iter().enumerate() { self.slot(elem, *c, blk + hl + i * es)?; }
        }
        Ok(blk)
    }
    fn struct_block(&mut self, idx: usize, id: Id, base: usize) -> Res<()> {
        let doc = self.doc; let sc = self.sc.unwrap();
        self.locs.insert(id, (base, Ty::Struct(idx)));
        let fields = sc.fields(idx); let sname = sc.types[idx].name().to_string();
        let lay = sc.layout(idx, self.ver);
        let m = match &doc.nodes[id].kind { Kind::Obj(m) => m, _ => return self.mismatch(&Ty::Struct(idx), id) };
        for (k, c) in m { if !lay.lay.iter().any(|fl| fields[fl.fidx].name == *k) { return self.fail("E_UNKNOWN_FIELD", format!("schema {} has no field '{}'", sname, k), Some(*c)); } }
        for fl in &lay.lay {
            let f = &fields[fl.fidx]; let opt = f.optional || f.nullable;
            let child = match m.iter().find(|(k, _)| *k == f.name) {
                Some((_, c)) => *c,
                None => if let Some(d) = self.defaults.get(&(idx, fl.fidx)) { *d } else if f.optional { continue; }
                    else if f.nullable { return self.fail("E_MISSING_FIELD", format!("{}.{} is nullable but not optional: write null explicitly", sname, f.name), Some(id)); }
                    else { return self.fail("E_MISSING_FIELD", format!("{}.{} is required", sname, f.name), Some(id)); },
            };
            let is_null = doc.nodes[child].kind == Kind::Null;
            if opt { if is_null { continue; } self.buf[base + fl.pbyte.unwrap()] |= 1 << fl.bit; }
            else if is_null && f.ty != Ty::Any { return self.fail("E_TYPE_MISMATCH", format!("{}.{} is not nullable", sname, f.name), Some(child)); }
            self.slot(&f.ty, child, base + fl.off)?;
            if !f.cons.is_empty() { self.constrain(&sname, f, child)?; }
        }
        Ok(())
    }
    fn constrain(&self, sname: &str, f: &Field, id: Id) -> Res<()> {
        for (name, lo, hi) in &f.cons {
            let k = &self.doc.nodes[id].kind;
            let v: Option<f64> = if name == "range" { match k { Kind::Int(i) => Some(*i as f64), Kind::Float(x) => Some(*x), Kind::F32(x) => Some(*x as f64), _ => None } }
                else { match k { Kind::Str(_) if f.ty == Ty::Bytes => Some(self.bytes_of(id)?.len() as f64), Kind::Str(s) => Some(s.chars().count() as f64), Kind::Bytes(b) => Some(b.len() as f64), Kind::Arr(a) => Some(a.len() as f64), _ => None } };
            if let Some(v) = v { if lo.map_or(false, |l| v < l) || hi.map_or(false, |h| v > h) { return self.fail("E_CONSTRAINT", format!("{}.{} violates @{}", sname, f.name, name), Some(id)); } }
        }
        Ok(())
    }
}

/// Write the 28-byte header (and the CRC-32 trailer if `checksum`; the trailer's 4 bytes must already be allocated) into a finished buffer.
pub fn finish_header(b: &mut Vec<u8>, sid: u64, ver: usize, root_off: usize, checksum: bool) {
    let n = b.len();
    b[0..3].copy_from_slice(b"BJX"); b[3] = 1; b[4..6].copy_from_slice(&(checksum as u16).to_le_bytes()); b[6..8].copy_from_slice(&[0, 0]);
    b[8..16].copy_from_slice(&sid.to_le_bytes()); b[16..20].copy_from_slice(&(ver as u32).to_le_bytes()); b[20..24].copy_from_slice(&(root_off as u32).to_le_bytes()); b[24..28].copy_from_slice(&(n as u32).to_le_bytes());
    if checksum { let c = crc32(&b[..n - 4]); b[n - 4..].copy_from_slice(&c.to_le_bytes()); }
}

#[derive(Default, Clone)]
pub struct EncodeOptions { pub root: Option<String>, pub version: Option<usize>, pub checksum: bool }

/// Encode a document. `schema = None` selects the dynamic (any) layout.
pub fn encode(doc: &Doc, schema: Option<&Schema>, o: &EncodeOptions) -> Res<Vec<u8>> {
    let mut d2 = doc.clone(); let mut defaults = HashMap::new();
    if let Some(sc) = schema {
        for (idx, t) in sc.types.iter().enumerate() { if let TypeDef::Struct { fields, .. } = t { for (fi, f) in fields.iter().enumerate() { if let Some(d) = &f.default {
            let k = match d { DefaultVal::Null => Kind::Null, DefaultVal::Bool(b) => Kind::Bool(*b), DefaultVal::Int(i) => Kind::Int(*i), DefaultVal::Float(x) => Kind::Float(*x), DefaultVal::Str(s) => Kind::Str(s.clone()), DefaultVal::Arr => Kind::Arr(vec![]) };
            let id = d2.add(k, NO_POS); defaults.insert((idx, fi), id); } } } }
    }
    let mut e = Enc { doc: &d2, sc: schema, ver: 0, buf: vec![0; HDR], refs: vec![], locs: HashMap::new(), keys: HashMap::new(), defaults };
    let (root_off, sid, ver);
    match schema {
        None => { let p = e.alloc(5)?; e.any_slot(d2.root, p, 0)?; root_off = p; sid = 0u64; ver = 0usize; }
        Some(sc) => {
            ver = o.version.unwrap_or(sc.version);
            if ver < 1 || ver > sc.version { return err("E_SCHEMA_INVALID", "cannot encode that version with this schema"); }
            let (ridx, id) = match &o.root { Some(n) => (sc.struct_index(n)?, sc.id_for(n)), None => (sc.root.ok_or_else(|| Error { code: "E_SCHEMA_INVALID".into(), message: "schema declares no root".into(), pos: None, line: None, col: None })?, sc.id()) };
            e.ver = ver; sid = id;
            if !matches!(d2.nodes[d2.root].kind, Kind::Obj(_)) { return e.mismatch(&Ty::Struct(ridx), d2.root); }
            let p = e.alloc(sc.layout(ridx, ver).size)?; e.struct_block(ridx, d2.root, p)?; root_off = p;
        }
    }
    let refs = std::mem::take(&mut e.refs);
    for (pos, rid, exp) in refs {
        let tgt = d2.nodes[rid].target.unwrap();
        let (loc, tt) = match e.locs.get(&tgt) { Some(x) => x.clone(), None => return e.fail("E_REF_TARGET", "reference target is not addressable in the binary layout".into(), Some(rid)) };
        if tt != exp { return e.fail("E_REF_TARGET", format!("reference of type ref<{}> points at a value of type {}", e.tname(&exp), e.tname(&tt)), Some(rid)); }
        e.u32(pos, loc);
    }
    if o.checksum { e.alloc(4)?; }
    let mut b = e.buf; finish_header(&mut b, sid, ver, root_off, o.checksum);
    Ok(b)
}

// ===================================================================== reader / zero-copy views
#[derive(Default, Clone)]
pub struct ReadOptions { pub skip_checksum: bool, pub strict_version: bool }
/// An opened blob. All views borrow from it and from the underlying byte slice; nothing is copied.
pub struct Blob<'a> { d: &'a [u8], n: usize, sc: Option<&'a Schema>, root: usize, dynamic: bool, ridx: usize, w: usize }
enum Sc { B(bool), I(i128), F32(f32), F64(f64) }
/// A value read from a view. Strings and bytes borrow the blob.
#[derive(Debug, Clone)]
pub enum Val<'b, 'a> { Null, EmptyArray, SchemaStr(&'b str), Bool(bool), Int(i128), F32(f32), F64(f64), Str(&'a str), Bytes(&'a [u8]), Enum(&'b str), EnumRaw(u32), Struct(StructView<'b, 'a>), Array(ArrayView<'b, 'a>), Union(UnionView<'b, 'a>), AnyArray(AnyArray<'b, 'a>), AnyObject(AnyObject<'b, 'a>) }
#[derive(Debug, Clone)] pub struct StructView<'b, 'a> { r: &'b Blob<'a>, idx: usize, loc: usize }
#[derive(Debug, Clone)] pub struct ArrayView<'b, 'a> { r: &'b Blob<'a>, elem: &'b Ty, blk: usize, n: usize, start: usize, stride: usize }
#[derive(Debug, Clone)] pub struct UnionView<'b, 'a> { r: &'b Blob<'a>, idx: usize, pos: usize, ms: usize, pub tag: u8 }
#[derive(Debug, Clone)] pub struct AnyArray<'b, 'a> { r: &'b Blob<'a>, blk: usize, n: usize, start: usize }
#[derive(Debug, Clone)] pub struct AnyObject<'b, 'a> { r: &'b Blob<'a>, blk: usize, n: usize, start: usize }
impl<'a> std::fmt::Debug for Blob<'a> { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "Blob({} bytes)", self.n) } }

impl<'a> Blob<'a> {
    pub fn open(data: &'a [u8], sc: Option<&'a Schema>, o: &ReadOptions) -> Res<Blob<'a>> {
        let n = data.len();
        if n < HDR { return err("E_BIN_TRUNCATED", "blob is shorter than the 28-byte header"); }
        if &data[..3] != b"BJX" { return err("E_BIN_MAGIC", "not a BJSONX blob (bad magic)"); }
        if data[3] != 1 { return err("E_BIN_VERSION", "unsupported BJSONX format version"); }
        let u16_ = |p: usize| u16::from_le_bytes([data[p], data[p + 1]]); let u32_ = |p: usize| u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as usize;
        let flags = u16_(4); let res = u16_(6); let mut sid = [0u8; 8]; sid.copy_from_slice(&data[8..16]); let sid = u64::from_le_bytes(sid);
        let (sver, root, length) = (u32_(16), u32_(20), u32_(24));
        if flags & !1 != 0 || res != 0 { return err("E_BIN_INVALID", "reserved header bits are set"); }
        if length > n { return err("E_BIN_TRUNCATED", "header length exceeds the data"); }
        if length < n { return err("E_BIN_INVALID", "trailing bytes after the blob"); }
        let mut end = n;
        if flags & 1 != 0 {
            if n < HDR + 4 { return err("E_BIN_TRUNCATED", "missing checksum"); }
            if !o.skip_checksum && crc32(&data[..n - 4]) != u32_(n - 4) as u32 { return err("E_BIN_CHECKSUM", "CRC-32 mismatch"); }
            end = n - 4;
        }
        let dynamic = sid == 0 && sver == 0; let mut ridx = 0;
        if !dynamic {
            let sc = match sc { Some(s) => s, None => return err("E_BIN_SCHEMA_MISMATCH", "blob requires a schema; none supplied") };
            if sver < 1 { return err("E_BIN_INVALID", "typed blob with schema version 0"); }
            ridx = match sc.types.iter().position(|t| matches!(t, TypeDef::Struct { name, .. } if sc.id_for(name) == sid)) { Some(i) => i, None => return err("E_BIN_SCHEMA_MISMATCH", "schema id does not match any schema in the supplied definition") };
            if o.strict_version && sver > sc.version { return err("E_BIN_SCHEMA_MISMATCH", "blob is newer than the reader's schema"); }
        }
        if root < HDR || root >= end { return err("E_BIN_BOUNDS", "root offset is outside the payload"); }
        Ok(Blob { d: data, n: end, sc, root, dynamic, ridx, w: sver })
    }
    /// The root value: a `StructView` for typed blobs, an any-value for dynamic ones.
    pub fn root<'b>(&'b self) -> Res<Val<'b, 'a>> { if self.dynamic { self.any_get(self.root, self.root) } else { Ok(Val::Struct(self.struct_view(self.ridx, self.root)?)) } }
    pub fn is_dynamic(&self) -> bool { self.dynamic }

    fn need(&self, p: usize, k: usize) -> Res<()> { if p < HDR || p.checked_add(k).map_or(true, |e| e > self.n) { return err("E_BIN_BOUNDS", format!("access of {} bytes at offset {} is outside the payload", k, p)); } Ok(()) }
    fn need_mul(&self, p: usize, cnt: usize, sz: usize) -> Res<()> { if cnt == 0 { return Ok(()); } match cnt.checked_mul(sz) { Some(t) => self.need(p, t), None => err("E_BIN_BOUNDS", "array larger than the payload") } }
    fn u8(&self, p: usize) -> Res<u8> { self.need(p, 1)?; Ok(self.d[p]) }
    fn u32(&self, p: usize) -> Res<usize> { self.need(p, 4)?; Ok(u32::from_le_bytes([self.d[p], self.d[p + 1], self.d[p + 2], self.d[p + 3]]) as usize) }
    fn varint(&self, p: usize) -> Res<(usize, usize)> {
        let mut v = 0usize;
        for i in 0..5 { self.need(p + i, 1)?; let b = self.d[p + i]; v |= ((b & 0x7f) as usize) << (7 * i);
            if b & 0x80 == 0 { if i > 0 && b == 0 { return err("E_BIN_INVALID", "non-minimal varint"); } if v > 0xFFFFFFFF { return err("E_BIN_INVALID", "varint exceeds u32"); } return Ok((v, p + i + 1)); } }
        err("E_BIN_INVALID", "varint longer than 5 bytes")
    }
    fn fwd(&self, off: usize, ms: usize, what: &str) -> Res<usize> { if off <= ms { return err("E_BIN_INVALID", format!("{} offset does not point forward of its container", what)); } Ok(off) }
    fn hbytes(&self, off: usize) -> Res<&'a [u8]> { let (n, p) = self.varint(off)?; self.need(p, n)?; Ok(&self.d[p..p + n]) }
    fn hstr(&self, off: usize) -> Res<&'a str> { let b = self.hbytes(off)?; std::str::from_utf8(b).or_else(|_| err("E_BIN_UTF8", "invalid UTF-8 in string")) }
    fn scalar(&self, t: &Ty, p: usize) -> Res<Sc> {
        let n = self.sc.map(|s| s.slot_size(t)).unwrap_or(match t { Ty::Bool | Ty::U8 | Ty::I8 => 1, Ty::U16 | Ty::I16 => 2, Ty::U32 | Ty::I32 | Ty::F32 => 4, _ => 8 });
        self.need(p, n)?; let mut b8 = [0u8; 8]; b8[..n].copy_from_slice(&self.d[p..p + n]); let u = u64::from_le_bytes(b8);
        Ok(match t { Ty::Bool => { if u > 1 { return err("E_BIN_INVALID", "invalid bool byte"); } Sc::B(u == 1) }
            Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 => Sc::I(u as i128), Ty::I8 => Sc::I(u as u8 as i8 as i128), Ty::I16 => Sc::I(u as u16 as i16 as i128), Ty::I32 => Sc::I(u as u32 as i32 as i128), Ty::I64 => Sc::I(u as i64 as i128),
            Ty::F32 => Sc::F32(f32::from_bits(u as u32)), _ => Sc::F64(f64::from_bits(u)) })
    }
    fn enum_raw(&self, ei: usize, p: usize) -> Res<(u32, &'a TypeDef)> {
        let sc = self.sc.unwrap(); let td = &sc.types[ei]; let base = match td { TypeDef::Enum { base, .. } => *base, _ => 1 };
        self.need(p, base)?; let mut v = 0u32; for i in (0..base).rev() { v = (v << 8) | self.d[p + i] as u32; } Ok((v, td))
    }
    fn slot_size(&self, t: &Ty) -> usize { self.sc.map(|s| s.slot_size(t)).unwrap_or(5) }
    fn sc_val<'b>(s: Sc) -> Val<'b, 'a> { match s { Sc::B(b) => Val::Bool(b), Sc::I(i) => Val::Int(i), Sc::F32(f) => Val::F32(f), Sc::F64(f) => Val::F64(f) } }
    fn struct_view<'b>(&'b self, idx: usize, loc: usize) -> Res<StructView<'b, 'a>> { self.need(loc, self.sc.unwrap().layout(idx, self.w).size)?; Ok(StructView { r: self, idx, loc }) }
    fn enum_val<'b>(&'b self, ei: usize, p: usize) -> Res<Val<'b, 'a>> {
        let (v, td) = self.enum_raw(ei, p)?; if let TypeDef::Enum { values, .. } = td { if let Some((n, _)) = values.iter().find(|(_, x)| *x == v) { return Ok(Val::Enum(n)); } } Ok(Val::EnumRaw(v))
    }
    fn slot_get<'b>(&'b self, t: &'b Ty, pos: usize, ms: usize) -> Res<Val<'b, 'a>> {
        if is_scalar(t) { return Ok(Self::sc_val(self.scalar(t, pos)?)); }
        match t {
            Ty::Enum(ei) => self.enum_val(*ei, pos),
            Ty::Any => self.any_get(pos, ms),
            Ty::Union(ui) => { self.need(pos, 5)?; Ok(Val::Union(UnionView { r: self, idx: *ui, pos, ms, tag: self.d[pos] })) }
            Ty::Ref(e) => { let loc = self.u32(pos)?; self.value_at(e, loc) }
            _ => { let off = self.fwd(self.u32(pos)?, ms, "value")?; self.heap_get(t, off, ms) }
        }
    }
    fn heap_get<'b>(&'b self, t: &'b Ty, off: usize, ms: usize) -> Res<Val<'b, 'a>> {
        match t {
            Ty::Str => Ok(Val::Str(self.hstr(off)?)), Ty::Bytes => Ok(Val::Bytes(self.hbytes(off)?)),
            Ty::Array(e) => Ok(Val::Array(self.array_view(e, off)?)), Ty::Struct(i) => Ok(Val::Struct(self.struct_view(*i, off)?)),
            Ty::Enum(ei) => self.enum_val(*ei, off),
            t if is_scalar(t) => Ok(Self::sc_val(self.scalar(t, off)?)),
            _ => { let _ = ms; err("E_BIN_INVALID", "type cannot be stored by offset") }
        }
    }
    fn value_at<'b>(&'b self, t: &'b Ty, loc: usize) -> Res<Val<'b, 'a>> {
        if let Ty::Struct(i) = t { return Ok(Val::Struct(self.struct_view(*i, loc)?)); }
        self.need(loc, self.slot_size(t))?; self.slot_get(t, loc, 0)
    }
    fn array_view<'b>(&'b self, elem: &'b Ty, off: usize) -> Res<ArrayView<'b, 'a>> {
        let (n, mut p) = self.varint(off)?;
        let stride;
        if let Ty::Struct(si) = elem {
            let (s, q) = self.varint(p)?; p = q; stride = s;
            if stride < self.sc.unwrap().layout(*si, self.w).size { return err("E_BIN_INVALID", "array stride smaller than struct size"); }
        } else { stride = self.slot_size(elem); }
        self.need_mul(p, n, stride)?;
        Ok(ArrayView { r: self, elem, blk: off, n, start: p, stride })
    }
    fn any_get<'b>(&'b self, pos: usize, ms: usize) -> Res<Val<'b, 'a>> {
        let tag = self.u8(pos)?; let pl = self.u32(pos + 1)?;
        if tag <= T_TRUE { if pl != 0 { return err("E_BIN_INVALID", "nonzero payload on constant tag"); } return Ok(if tag == T_NULL { Val::Null } else { Val::Bool(tag == T_TRUE) }); }
        if tag == T_I32 { return Ok(Val::Int(i32::from_le_bytes([self.d[pos + 1], self.d[pos + 2], self.d[pos + 3], self.d[pos + 4]]) as i128)); }
        if tag == T_REF { self.need(pl, 5)?; if self.d[pl] == T_REF { return err("E_BIN_INVALID_REF", "reference targets another reference"); } return self.any_get(pl, 0); }
        self.fwd(pl, ms, "value")?;
        match tag {
            T_I64 => Ok(Self::sc_val(self.scalar(&Ty::I64, pl)?)), T_U64 => Ok(Self::sc_val(self.scalar(&Ty::U64, pl)?)), T_F64 => Ok(Self::sc_val(self.scalar(&Ty::F64, pl)?)),
            T_STR => Ok(Val::Str(self.hstr(pl)?)), T_BYTES => Ok(Val::Bytes(self.hbytes(pl)?)),
            T_ARR => { let (n, p) = self.varint(pl)?; self.need_mul(p, n, 5)?; Ok(Val::AnyArray(AnyArray { r: self, blk: pl, n, start: p })) }
            T_OBJ => { let (n, p) = self.varint(pl)?; self.need_mul(p, n, 9)?; Ok(Val::AnyObject(AnyObject { r: self, blk: pl, n, start: p })) }
            _ => err("E_BIN_INVALID", "unknown any-tag"),
        }
    }
}
impl<'b, 'a> StructView<'b, 'a> {
    /// Name of the schema (struct type) this view reads.
    pub fn type_name(&self) -> &'b str { self.r.sc.unwrap().types[self.idx].name() }
    pub fn keys(&self) -> Vec<&'b str> { let sc = self.r.sc.unwrap(); sc.layout(self.idx, self.r.w).lay.iter().map(|fl| sc.fields(self.idx)[fl.fidx].name.as_str()).collect() }
    /// Read a field. Absent optional/null values are `Val::Null`.
    pub fn get(&self, name: &str) -> Res<Val<'b, 'a>> {
        let r = self.r; let sc = r.sc.unwrap(); let fields = sc.fields(self.idx);
        for fl in &sc.layout(self.idx, r.w).lay {
            let f = &fields[fl.fidx]; if f.name != name { continue; }
            if let Some(pb) = fl.pbyte { if (r.u8(self.loc + pb)? >> fl.bit) & 1 == 0 { return Ok(Val::Null); } }
            return r.slot_get(&f.ty, self.loc + fl.off, self.loc);
        }
        match fields.iter().find(|f| f.name == name) {
            None => err("E_UNKNOWN_FIELD", format!("no field {}", name)),
            Some(f) => Ok(match &f.default {
                Some(DefaultVal::Bool(b)) => Val::Bool(*b), Some(DefaultVal::Int(i)) => Val::Int(*i),
                Some(DefaultVal::Float(x)) => if f.ty == Ty::F32 { Val::F32(*x as f32) } else { Val::F64(*x) },
                Some(DefaultVal::Str(s)) => if matches!(f.ty, Ty::Enum(_)) { Val::Enum(s.as_str()) } else { Val::SchemaStr(s.as_str()) },
                Some(DefaultVal::Arr) => Val::EmptyArray, _ => Val::Null }),
        }
    }
}
impl<'b, 'a> ArrayView<'b, 'a> {
    pub fn len(&self) -> usize { self.n }
    pub fn is_empty(&self) -> bool { self.n == 0 }
    pub fn get(&self, i: usize) -> Res<Val<'b, 'a>> {
        if i >= self.n { return err("E_BIN_BOUNDS", "index out of range"); }
        let p = self.start + i * self.stride;
        if let Ty::Struct(si) = self.elem { return Ok(Val::Struct(self.r.struct_view(*si, p)?)); }
        self.r.slot_get(self.elem, p, self.blk)
    }
}
impl<'b, 'a> UnionView<'b, 'a> {
    pub fn name(&self) -> Option<&'b str> { match &self.r.sc.unwrap().types[self.idx] { TypeDef::Union { variants, .. } => variants.iter().find(|v| v.1 == self.tag).map(|v| v.0.as_str()), _ => None } }
    pub fn value(&self) -> Res<Val<'b, 'a>> {
        if let TypeDef::Union { variants, .. } = &self.r.sc.unwrap().types[self.idx] { if let Some(v) = variants.iter().find(|v| v.1 == self.tag) { let off = self.r.fwd(self.r.u32(self.pos + 1)?, self.ms, "union payload")?; return self.r.heap_get(&v.2, off, self.ms); } }
        Ok(Val::Null)
    }
}
impl<'b, 'a> AnyArray<'b, 'a> {
    pub fn len(&self) -> usize { self.n }
    pub fn get(&self, i: usize) -> Res<Val<'b, 'a>> { if i >= self.n { return err("E_BIN_BOUNDS", "index out of range"); } self.r.any_get(self.start + 5 * i, self.blk) }
}
impl<'b, 'a> AnyObject<'b, 'a> {
    pub fn len(&self) -> usize { self.n }
    pub fn key_at(&self, i: usize) -> Res<&'a str> { self.r.hstr(self.r.u32(self.start + 9 * i)?) }
    pub fn get(&self, key: &str) -> Res<Option<Val<'b, 'a>>> { for i in 0..self.n { if self.key_at(i)? == key { return Ok(Some(self.r.any_get(self.start + 9 * i + 4, self.blk)?)); } } Ok(None) }
}

// ===================================================================== owned decoder
struct Dec<'b, 'a> { r: &'b Blob<'a>, doc: Doc, memo: HashMap<(usize, Ty), Id>, pending: Vec<(Id, usize, Ty)>, visited: usize, budget: usize, used: usize }
impl<'b, 'a> Dec<'b, 'a> {
    fn mk(&mut self, kind: Kind, parent: Option<Id>, key: Key, loc: Option<usize>, t: &Ty, depth: usize) -> Res<Id> {
        if depth > MAXD { return err("E_BIN_LIMIT", "nesting too deep"); }
        self.visited += 1; if self.visited > self.r.n { return err("E_BIN_LIMIT", "decoded node count exceeds the blob size"); }
        let id = self.doc.add(kind, NO_POS); self.doc.nodes[id].parent = parent; self.doc.nodes[id].key = key;
        if let Some(l) = loc { if self.memo.insert((l, t.clone()), id).is_some() { return err("E_BIN_INVALID", "location is used by two values"); } }
        Ok(id)
    }
    fn charge(&mut self, k: usize) -> Res<()> { self.used += k; if self.used > self.budget { return err("E_BIN_LIMIT", "decoded string/bytes volume exceeds the expansion limit"); } Ok(()) }
    fn run(&mut self) -> Res<Id> {
        let r = self.r;
        let root = if r.dynamic { self.any_node(r.root, r.root, None, Key::None, 0)? } else { self.struct_node(r.ridx, r.root, None, Key::None, 0)? };
        for (n, loc, t) in std::mem::take(&mut self.pending) { match self.memo.get(&(loc, t)) { Some(tg) => self.doc.nodes[n].target = Some(*tg), None => return err("E_BIN_INVALID_REF", "reference does not hit a value of the expected type") } }
        Ok(root)
    }
    fn any_node(&mut self, pos: usize, ms: usize, parent: Option<Id>, key: Key, depth: usize) -> Res<Id> {
        let r = self.r; let tag = r.u8(pos)?; let pl = r.u32(pos + 1)?; let t = Ty::Any;
        if tag <= T_TRUE { if pl != 0 { return err("E_BIN_INVALID", "nonzero payload on constant tag"); } return self.mk(if tag == T_NULL { Kind::Null } else { Kind::Bool(tag == T_TRUE) }, parent, key, Some(pos), &t, depth); }
        if tag == T_I32 { let v = i32::from_le_bytes([r.d[pos + 1], r.d[pos + 2], r.d[pos + 3], r.d[pos + 4]]); return self.mk(Kind::Int(v as i128), parent, key, Some(pos), &t, depth); }
        if tag == T_REF { let n = self.mk(Kind::Ref, parent, key, None, &t, depth)?; self.pending.push((n, pl, Ty::Any)); r.need(pl, 5)?; return Ok(n); }
        r.fwd(pl, ms, "value")?;
        match tag {
            T_I64 => { let v = match r.scalar(&Ty::I64, pl)? { Sc::I(i) => i, _ => 0 }; self.mk(Kind::Int(v), parent, key, Some(pos), &t, depth) }
            T_U64 => { let v = match r.scalar(&Ty::U64, pl)? { Sc::I(i) => i, _ => 0 }; self.mk(Kind::Int(v), parent, key, Some(pos), &t, depth) }
            T_F64 => { let v = match r.scalar(&Ty::F64, pl)? { Sc::F64(f) => f, _ => 0.0 }; self.mk(Kind::Float(v), parent, key, Some(pos), &t, depth) }
            T_STR => { let s = r.hstr(pl)?; self.charge(s.len())?; self.mk(Kind::Str(s.to_string()), parent, key, Some(pos), &t, depth) }
            T_BYTES => { let b = r.hbytes(pl)?; self.charge(b.len())?; self.mk(Kind::Bytes(b.to_vec()), parent, key, Some(pos), &t, depth) }
            T_ARR => {
                let (n, p) = r.varint(pl)?; r.need_mul(p, n, 5)?; let id = self.mk(Kind::Arr(vec![]), parent, key, Some(pos), &t, depth)?; let mut items = vec![];
                for i in 0..n { items.push(self.any_node(p + 5 * i, pl, Some(id), Key::Index(i), depth + 1)?); }
                self.doc.nodes[id].kind = Kind::Arr(items); Ok(id)
            }
            T_OBJ => {
                let (n, p) = r.varint(pl)?; r.need_mul(p, n, 9)?; let id = self.mk(Kind::Obj(vec![]), parent, key, Some(pos), &t, depth)?; let mut m: Vec<(String, Id)> = vec![];
                for i in 0..n {
                    let e = p + 9 * i; let k = r.hstr(r.u32(e)?)?.to_string(); self.charge(k.len())?;
                    if m.iter().any(|(x, _)| *x == k) { return err("E_BIN_INVALID", "duplicate object key"); }
                    let c = self.any_node(e + 4, pl, Some(id), Key::Name(k.clone()), depth + 1)?; m.push((k, c));
                }
                self.doc.nodes[id].kind = Kind::Obj(m); Ok(id)
            }
            _ => err("E_BIN_INVALID", "unknown any-tag"),
        }
    }
    fn struct_node(&mut self, idx: usize, base: usize, parent: Option<Id>, key: Key, depth: usize) -> Res<Id> {
        let r = self.r; let sc = r.sc.unwrap(); let lay = sc.layout(idx, r.w); let fields = sc.fields(idx);
        r.need(base, lay.size)?;
        let id = self.mk(Kind::Obj(vec![]), parent, key, Some(base), &Ty::Struct(idx), depth)?; let mut m: Vec<(String, Id)> = vec![]; let mut have = vec![false; fields.len()];
        for fl in &lay.lay {
            let f = &fields[fl.fidx]; have[fl.fidx] = true;
            if let Some(pb) = fl.pbyte { if (r.u8(base + pb)? >> fl.bit) & 1 == 0 { if f.nullable { let n = self.doc.add(Kind::Null, NO_POS); self.doc.nodes[n].parent = Some(id); self.doc.nodes[n].key = Key::Name(f.name.clone()); m.push((f.name.clone(), n)); } continue; } }
            let c = self.slot_node(&f.ty, base + fl.off, base, Some(id), Key::Name(f.name.clone()), depth + 1)?; m.push((f.name.clone(), c));
        }
        for (fi, f) in fields.iter().enumerate() { if !have[fi] { if let Some(d) = &f.default {
            let k = match d { DefaultVal::Null => Kind::Null, DefaultVal::Bool(b) => Kind::Bool(*b), DefaultVal::Int(i) => Kind::Int(*i), DefaultVal::Float(x) => Kind::Float(*x), DefaultVal::Str(s) => Kind::Str(s.clone()), DefaultVal::Arr => Kind::Arr(vec![]) };
            let n = self.doc.add(k, NO_POS); self.doc.nodes[n].parent = Some(id); self.doc.nodes[n].key = Key::Name(f.name.clone()); m.push((f.name.clone(), n)); } } }
        self.doc.nodes[id].kind = Kind::Obj(m); Ok(id)
    }
    fn slot_node(&mut self, t: &Ty, pos: usize, ms: usize, parent: Option<Id>, key: Key, depth: usize) -> Res<Id> {
        let r = self.r;
        if is_scalar(t) {
            let kind = match r.scalar(t, pos)? { Sc::B(b) => Kind::Bool(b), Sc::I(i) => Kind::Int(i), Sc::F32(f) => Kind::F32(f), Sc::F64(f) => Kind::Float(f) };
            return self.mk(kind, parent, key, Some(pos), t, depth);
        }
        match t {
            Ty::Enum(ei) => { let (v, td) = r.enum_raw(*ei, pos)?; let name = if let TypeDef::Enum { values, .. } = td { values.iter().find(|(_, x)| *x == v).map(|(n, _)| n.clone()) } else { None };
                self.mk(match name { Some(n) => Kind::Str(n), None => Kind::Int(v as i128) }, parent, key, Some(pos), t, depth) }
            Ty::Any => self.any_node(pos, ms, parent, key, depth),
            Ty::Ref(e) => { let n = self.mk(Kind::Ref, parent, key, None, t, depth)?; let loc = r.u32(pos)?; r.need(loc, 1)?; self.pending.push((n, loc, (**e).clone())); Ok(n) }
            Ty::Union(ui) => {
                let (tag, off) = (r.u8(pos)?, r.fwd(r.u32(pos + 1)?, ms, "union payload")?);
                let (name, vt) = match &r.sc.unwrap().types[*ui] { TypeDef::Union { variants, .. } => match variants.iter().find(|v| v.1 == tag) { Some(v) => (v.0.clone(), v.2.clone()), None => return err("E_BIN_INVALID", "unknown union tag") }, _ => unreachable!() };
                let id = self.mk(Kind::Obj(vec![]), parent, key, Some(pos), t, depth)?;
                let c = self.heap_node(&vt, off, ms, Some(id), Key::Name(name.clone()), depth + 1, None)?; self.doc.nodes[id].kind = Kind::Obj(vec![(name, c)]); Ok(id)
            }
            _ => { let off = r.fwd(r.u32(pos)?, ms, "value")?; self.heap_node(t, off, ms, parent, key, depth, Some(pos)) }
        }
    }
    fn heap_node(&mut self, t: &Ty, off: usize, ms: usize, parent: Option<Id>, key: Key, depth: usize, slotpos: Option<usize>) -> Res<Id> {
        let r = self.r;
        match t {
            Ty::Str => { let s = r.hstr(off)?; self.charge(s.len())?; self.mk(Kind::Str(s.to_string()), parent, key, slotpos, t, depth) }
            Ty::Bytes => { let b = r.hbytes(off)?; self.charge(b.len())?; self.mk(Kind::Bytes(b.to_vec()), parent, key, slotpos, t, depth) }
            Ty::Struct(i) => self.struct_node(*i, off, parent, key, depth),
            Ty::Array(e) => {
                let av = r.array_view(e, off)?; let id = self.mk(Kind::Arr(vec![]), parent, key, slotpos, t, depth)?; let mut items = vec![];
                for i in 0..av.n { let p = av.start + i * av.stride; items.push(match &**e { Ty::Struct(si) => self.struct_node(*si, p, Some(id), Key::Index(i), depth + 1)?, el => self.slot_node(el, p, off, Some(id), Key::Index(i), depth + 1)? }); }
                self.doc.nodes[id].kind = Kind::Arr(items); Ok(id)
            }
            t if is_scalar(t) || matches!(t, Ty::Enum(_)) => self.slot_node(t, off, ms, parent, key, depth),
            _ => err("E_BIN_INVALID", "type cannot be stored by offset"),
        }
    }
}

/// Fully validate a blob and build an owned [`Doc`] (aliasing preserved through reference targets).
pub fn decode(data: &[u8], schema: Option<&Schema>, o: &ReadOptions) -> Res<Doc> {
    let b = Blob::open(data, schema, o)?;
    let mut d = Dec { r: &b, doc: Doc { nodes: vec![], root: 0, src: vec![], labels: vec![] }, memo: HashMap::new(), pending: vec![], visited: 0, budget: (256 * b.n).max(1 << 20), used: 0 };
    let root = d.run()?; d.doc.root = root; Ok(d.doc)
}
/// For stream framing: total blob length from the first 28 header bytes.
pub fn blob_length(h: &[u8]) -> Res<usize> { if h.len() < HDR { return err("E_BIN_TRUNCATED", "need 28 header bytes"); } if &h[..3] != b"BJX" { return err("E_BIN_MAGIC", "bad magic"); } Ok(u32::from_le_bytes([h[24], h[25], h[26], h[27]]) as usize) }
