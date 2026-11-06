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
