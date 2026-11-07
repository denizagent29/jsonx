//! Runtime support for `#[derive(Bjsonx)]` (jsonx-derive). Public so generated code can call it; also usable directly.
use crate::*;

/// Convenience builder for [`Doc`] values (no source positions, no labels or references).
pub struct DocBuilder { pub doc: Doc }
impl Default for DocBuilder { fn default() -> Self { DocBuilder { doc: Doc::empty() } } }
impl DocBuilder {
    pub fn new() -> Self { Self::default() }
    fn mk(&mut self, k: Kind) -> Id { self.doc.add(k, NO_POS) }
    pub fn null(&mut self) -> Id { self.mk(Kind::Null) }
    pub fn bool(&mut self, v: bool) -> Id { self.mk(Kind::Bool(v)) }
    pub fn int(&mut self, v: i128) -> Id { self.mk(Kind::Int(v)) }
    pub fn float(&mut self, v: f64) -> Id { self.mk(Kind::Float(v)) }
    pub fn f32(&mut self, v: f32) -> Id { self.mk(Kind::F32(v)) }
    pub fn string(&mut self, v: &str) -> Id { self.mk(Kind::Str(v.to_string())) }
    pub fn bytes(&mut self, v: &[u8]) -> Id { self.mk(Kind::Bytes(v.to_vec())) }
    pub fn array(&mut self) -> Id { self.mk(Kind::Arr(vec![])) }
    pub fn object(&mut self) -> Id { self.mk(Kind::Obj(vec![])) }
    pub fn push(&mut self, arr: Id, child: Id) {
        let i = match &mut self.doc.nodes[arr].kind { Kind::Arr(a) => { a.push(child); a.len() - 1 } _ => panic!("push on a non-array") };
        self.doc.nodes[child].parent = Some(arr); self.doc.nodes[child].key = Key::Index(i);
    }
    pub fn set(&mut self, obj: Id, key: &str, child: Id) {
        match &mut self.doc.nodes[obj].kind { Kind::Obj(m) => m.push((key.to_string(), child)), _ => panic!("set on a non-object") }
        self.doc.nodes[child].parent = Some(obj); self.doc.nodes[child].key = Key::Name(key.to_string());
    }
    pub fn finish(mut self, root: Id) -> Doc { self.doc.root = root; self.doc }
}

fn vname(v: &Val) -> &'static str {
    match v { Val::Null => "null", Val::EmptyArray | Val::Array(_) | Val::AnyArray(_) => "array", Val::Bool(_) => "bool", Val::Int(_) => "integer", Val::F32(_) | Val::F64(_) => "float", Val::Str(_) | Val::SchemaStr(_) => "string",
        Val::Bytes(_) => "bytes", Val::Enum(_) | Val::EnumRaw(_) => "enum", Val::Struct(_) | Val::AnyObject(_) => "struct", Val::Union(_) => "union" }
}
pub fn mismatch(expected: &str, got: &Val) -> Error { Error { code: "E_TYPE_MISMATCH".into(), message: format!("expected {}, got {}", expected, vname(got)), pos: None, line: None, col: None } }
pub fn range(ty: &str, v: i128) -> Error { Error { code: "E_RANGE".into(), message: format!("{} does not fit in {}", v, ty), pos: None, line: None, col: None } }
pub fn unsupported(msg: &str) -> Error { Error { code: "E_REF_TARGET".into(), message: msg.into(), pos: None, line: None, col: None } }
pub fn unknown(what: &str, name: &str) -> Error { Error { code: "E_TYPE_MISMATCH".into(), message: format!("unknown {} '{}'", what, name), pos: None, line: None, col: None } }
pub fn to_int<T: TryFrom<i128>>(v: &Val, ty: &str) -> Res<T> { match v { Val::Int(i) => T::try_from(*i).map_err(|_| range(ty, *i)), o => Err(mismatch(ty, o)) } }
pub fn to_f32(v: &Val) -> Res<f32> { match v { Val::F32(x) => Ok(*x), Val::F64(x) => Ok(*x as f32), o => Err(mismatch("f32", o)) } }
pub fn to_f64(v: &Val) -> Res<f64> { match v { Val::F64(x) => Ok(*x), Val::F32(x) => Ok(*x as f64), o => Err(mismatch("f64", o)) } }
pub fn to_bool(v: &Val) -> Res<bool> { match v { Val::Bool(x) => Ok(*x), o => Err(mismatch("bool", o)) } }

/// An array read through a view; also covers the empty array supplied by a schema default for fields newer than the blob.
#[derive(Debug, Clone)]
pub enum ViewArray<'b, 'a> { Empty, Arr(ArrayView<'b, 'a>) }
impl<'b, 'a> ViewArray<'b, 'a> {
    pub fn len(&self) -> usize { match self { ViewArray::Empty => 0, ViewArray::Arr(a) => a.len() } }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub fn get(&self, i: usize) -> Res<Val<'b, 'a>> { match self { ViewArray::Empty => err("E_BIN_BOUNDS", "index out of range"), ViewArray::Arr(a) => a.get(i) } }
}
pub fn to_array<'b, 'a>(v: Val<'b, 'a>) -> Res<ViewArray<'b, 'a>> { match v { Val::Array(a) => Ok(ViewArray::Arr(a)), Val::EmptyArray => Ok(ViewArray::Empty), o => Err(mismatch("array", &o)) } }

// ======================================================================= direct encoder (no intermediate Doc)
/// Low-level buffer writer following the canonical BJSONX writing order; used by `#[derive(Bjsonx)]`.
pub struct Writer<'s> { pub sc: &'s Schema, pub ver: usize, pub buf: Vec<u8> }
impl<'s> Writer<'s> {
    pub fn new(sc: &'s Schema, ver: usize) -> Self { Writer { sc, ver, buf: vec![0; HDR] } }
    pub fn alloc(&mut self, n: usize) -> Res<usize> { let p = self.buf.len(); if p + n >= 0xFFFFFFFF { return err("E_BIN_LIMIT", "document exceeds the 4 GiB offset limit"); } self.buf.resize(p + n, 0); Ok(p) }
    pub fn varint(&mut self, mut p: usize, mut v: usize) -> usize { while v >= 0x80 { self.buf[p] = (v & 0x7f) as u8 | 0x80; v >>= 7; p += 1; } self.buf[p] = v as u8; p + 1 }
    pub fn set_u32(&mut self, p: usize, v: usize) { self.buf[p..p + 4].copy_from_slice(&(v as u32).to_le_bytes()); }
    pub fn put_str(&mut self, b: &[u8]) -> Res<usize> { let vl = { let mut n = 1; let mut v = b.len(); while v >= 0x80 { v >>= 7; n += 1; } n }; let p = self.alloc(vl + b.len())?; let q = self.varint(p, b.len()); self.buf[q..q + b.len()].copy_from_slice(b); Ok(p) }
    fn vlen(n: usize) -> usize { let mut l = 1; let mut v = n; while v >= 0x80 { v >>= 7; l += 1; } l }
    pub fn write_int(&mut self, t: &Ty, v: i128, pos: usize) -> Res<()> {
        let (lo, hi, n): (i128, i128, usize) = match t { Ty::U8 => (0, 255, 1), Ty::U16 => (0, 65535, 2), Ty::U32 => (0, u32::MAX as i128, 4), Ty::U64 => (0, u64::MAX as i128, 8),
            Ty::I8 => (-128, 127, 1), Ty::I16 => (-32768, 32767, 2), Ty::I32 => (i32::MIN as i128, i32::MAX as i128, 4), Ty::I64 => (i64::MIN as i128, i64::MAX as i128, 8), _ => return err("E_TYPE_MISMATCH", "integer written to a non-integer schema type") };
        if v < lo || v > hi { return Err(range(&self.sc.type_name(t), v)); }
        self.buf[pos..pos + n].copy_from_slice(&(v as u64).to_le_bytes()[..n]); Ok(())
    }
    pub fn write_enum(&mut self, t: &Ty, name: &str, pos: usize) -> Res<()> {
        if let Ty::Enum(ei) = t { if let TypeDef::Enum { base, values, .. } = &self.sc.types[*ei] { if let Some((_, v)) = values.iter().find(|(n, _)| n == name) { self.buf[pos..pos + base].copy_from_slice(&v.to_le_bytes()[..*base]); return Ok(()); } } }
        Err(unknown("enum member", name))
    }
    /// Check `@range` / `@len` constraints of a field (same rules and error code as the text encoder).
    pub fn constrain(&self, schema_name: &str, f: &Field, num: Option<f64>, len: Option<f64>) -> Res<()> {
        for (name, lo, hi) in &f.cons {
            let v = if name == "range" { num } else { len };
            if let Some(v) = v { if lo.map_or(false, |l| v < l) || hi.map_or(false, |h| v > h) { return err("E_CONSTRAINT", format!("{}.{} violates @{}", schema_name, f.name, name)); } }
        }
        Ok(())
    }
    /// Finish: header (+ optional CRC trailer) and return the blob.
    pub fn finish(mut self, root_off: usize, sid: u64, checksum: bool) -> Res<Vec<u8>> {
        if checksum { self.alloc(4)?; }
        finish_header(&mut self.buf, sid, self.ver, root_off, checksum); Ok(self.buf)
    }
}

/// Direct (schema-driven) encoding of Rust values. Implemented for primitives, `String`, `Vec<T>` and by `#[derive(Bjsonx)]`.
pub trait BjEncode {
    /// Write the *inline* form (scalars, enums, unions) at `pos`.
    fn put_inline(&self, _w: &mut Writer, t: &Ty, _pos: usize) -> Res<()> { err("E_TYPE_MISMATCH", format!("value cannot be written inline as {:?}", t)) }
    /// Allocate and write the by-offset object (string, bytes, array, struct instance, or a scalar payload); returns its offset.
    fn put_heap(&self, _w: &mut Writer, t: &Ty) -> Res<usize> { err("E_TYPE_MISMATCH", format!("value cannot be stored by offset as {:?}", t)) }
    /// Write a struct instance whose block starts at `base` (derived structs only).
    fn put_block(&self, _w: &mut Writer, _base: usize) -> Res<()> { err("E_TYPE_MISMATCH", "not a schema struct") }
    fn as_u8(&self) -> Option<u8> { None }
    fn con_num(&self) -> Option<f64> { None }
    fn con_len(&self) -> Option<f64> { None }
    /// Write the slot form at `pos` (provided).
    fn put(&self, w: &mut Writer, t: &Ty, pos: usize) -> Res<()> {
        match t { Ty::Str | Ty::Bytes | Ty::Array(_) | Ty::Struct(_) => { let off = self.put_heap(w, t)?; w.set_u32(pos, off); Ok(()) } Ty::Ref(_) => Err(unsupported("encoding ref<T> fields is not supported by #[derive(Bjsonx)]")), _ => self.put_inline(w, t, pos) }
    }
}
macro_rules! enc_int { ($($t:ty),*) => {$(
    impl BjEncode for $t {
        fn put_inline(&self, w: &mut Writer, t: &Ty, pos: usize) -> Res<()> { w.write_int(t, *self as i128, pos) }
        fn put_heap(&self, w: &mut Writer, t: &Ty) -> Res<usize> { let p = w.alloc(w.sc.slot_size(t))?; w.write_int(t, *self as i128, p)?; Ok(p) }
        fn con_num(&self) -> Option<f64> { Some(*self as f64) }
        fn as_u8(&self) -> Option<u8> { u8::try_from(*self as i128).ok() }
    })*} }
enc_int!(u8, u16, u32, u64, i8, i16, i32, i64);
impl BjEncode for bool {
    fn put_inline(&self, w: &mut Writer, _t: &Ty, pos: usize) -> Res<()> { w.buf[pos] = *self as u8; Ok(()) }
    fn put_heap(&self, w: &mut Writer, _t: &Ty) -> Res<usize> { let p = w.alloc(1)?; w.buf[p] = *self as u8; Ok(p) }
}
impl BjEncode for f32 {
    fn put_inline(&self, w: &mut Writer, _t: &Ty, pos: usize) -> Res<()> { w.buf[pos..pos + 4].copy_from_slice(&self.to_le_bytes()); Ok(()) }
    fn put_heap(&self, w: &mut Writer, t: &Ty) -> Res<usize> { let p = w.alloc(4)?; self.put_inline(w, t, p)?; Ok(p) }
    fn con_num(&self) -> Option<f64> { Some(*self as f64) }
}
impl BjEncode for f64 {
    fn put_inline(&self, w: &mut Writer, _t: &Ty, pos: usize) -> Res<()> { w.buf[pos..pos + 8].copy_from_slice(&self.to_le_bytes()); Ok(()) }
    fn put_heap(&self, w: &mut Writer, t: &Ty) -> Res<usize> { let p = w.alloc(8)?; self.put_inline(w, t, p)?; Ok(p) }
    fn con_num(&self) -> Option<f64> { Some(*self) }
}
impl BjEncode for String {
    fn put_heap(&self, w: &mut Writer, _t: &Ty) -> Res<usize> { w.put_str(self.as_bytes()) }
    fn con_len(&self) -> Option<f64> { Some(self.chars().count() as f64) }
}
impl<T: BjEncode> BjEncode for Vec<T> {
    fn put_heap(&self, w: &mut Writer, t: &Ty) -> Res<usize> {
        match t {
            Ty::Bytes => { let mut b = Vec::with_capacity(self.len()); for x in self { b.push(x.as_u8().ok_or_else(|| mismatch_ty("bytes"))?); } w.put_str(&b) }
            Ty::Array(elem) => {
                let n = self.len(); let sc = w.sc;
                if let Ty::Struct(si) = &**elem {
                    let size = sc.layout(*si, w.ver).size; let hl = Writer::vlen(n) + Writer::vlen(size); let blk = w.alloc(hl + n * size)?; let p = w.varint(blk, n); w.varint(p, size);
                    for (i, x) in self.iter().enumerate() { x.put_block(w, blk + hl + i * size)?; }
                    Ok(blk)
                } else {
                    let es = sc.slot_size(elem); let hl = Writer::vlen(n); let blk = w.alloc(hl + n * es)?; w.varint(blk, n);
                    for (i, x) in self.iter().enumerate() { x.put(w, elem, blk + hl + i * es)?; }
                    Ok(blk)
                }
            }
            _ => err("E_TYPE_MISMATCH", "Vec written to a non-array schema type"),
        }
    }
    fn con_len(&self) -> Option<f64> { Some(self.len() as f64) }
}
fn mismatch_ty(what: &str) -> Error { Error { code: "E_TYPE_MISMATCH".into(), message: format!("element does not fit in {}", what), pos: None, line: None, col: None } }
