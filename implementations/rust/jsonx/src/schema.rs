use crate::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty { Bool, U8, U16, U32, U64, I8, I16, I32, I64, F32, F64, Str, Bytes, Any, Struct(usize), Enum(usize), Union(usize), Array(Box<Ty>), Ref(Box<Ty>) }
#[derive(Debug, Clone)]
pub enum DefaultVal { Null, Bool(bool), Int(i128), Float(f64), Str(String), Arr }
#[derive(Debug, Clone)]
pub struct Field { pub name: String, pub ty: Ty, pub optional: bool, pub nullable: bool, pub deprecated: bool, pub since: usize, pub default: Option<DefaultVal>, pub attrs: Vec<(String, String)>, pub cons: Vec<(String, Option<f64>, Option<f64>)> }
#[derive(Debug, Clone)]
pub enum TypeDef {
    Struct { name: String, fields: Vec<Field> },
    Enum { name: String, base: usize, values: Vec<(String, u32)> },
    Union { name: String, variants: Vec<(String, u8, Ty)> },
}
impl TypeDef { pub fn name(&self) -> &str { match self { TypeDef::Struct { name, .. } | TypeDef::Enum { name, .. } | TypeDef::Union { name, .. } => name } } }
#[derive(Debug, Clone)]
pub struct FieldLayout { pub fidx: usize, pub off: usize, pub pbyte: Option<usize>, pub bit: u8 }
#[derive(Debug, Clone, Default)]
pub struct Layout { pub lay: Vec<FieldLayout>, pub size: usize }
/// A compiled schema loaded from the canonical binary form (BJXS) produced by `jsonxc`.
#[derive(Debug, Clone)]
pub struct Schema { pub namespace: String, pub version: usize, pub root: Option<usize>, pub types: Vec<TypeDef>, layouts: Vec<Vec<Layout>> }

pub fn fnv1a64(b: &[u8]) -> u64 { let mut h = 0xcbf29ce484222325u64; for &x in b { h = (h ^ x as u64).wrapping_mul(0x100000001b3); } h }

impl Schema {
    pub fn id_for(&self, name: &str) -> u64 { fnv1a64(format!("{}\0{}", self.namespace, name).as_bytes()) }
    pub fn id(&self) -> u64 { match self.root { Some(r) => self.id_for(self.types[r].name()), None => 0 } }
    pub fn struct_index(&self, name: &str) -> Res<usize> {
        self.types.iter().position(|t| matches!(t, TypeDef::Struct { name: n, .. } if n == name)).ok_or_else(|| Error { code: "E_SCHEMA_UNKNOWN_TYPE".into(), message: format!("no schema named '{}'", name), pos: None, line: None, col: None })
    }
    pub fn fields(&self, idx: usize) -> &[Field] { match &self.types[idx] { TypeDef::Struct { fields, .. } => fields, _ => &[] } }
    pub fn type_name(&self, t: &Ty) -> String {
        match t { Ty::Struct(i) | Ty::Enum(i) | Ty::Union(i) => self.types[*i].name().to_string(), Ty::Array(e) => format!("array<{}>", self.type_name(e)), Ty::Ref(e) => format!("ref<{}>", self.type_name(e)), o => format!("{:?}", o).to_lowercase() }
    }
    pub fn slot_size(&self, t: &Ty) -> usize {
        match t { Ty::Bool | Ty::U8 | Ty::I8 => 1, Ty::U16 | Ty::I16 => 2, Ty::U32 | Ty::I32 | Ty::F32 => 4, Ty::U64 | Ty::I64 | Ty::F64 => 8,
            Ty::Enum(i) => match &self.types[*i] { TypeDef::Enum { base, .. } => *base, _ => 1 },
            Ty::Str | Ty::Bytes | Ty::Array(_) | Ty::Struct(_) | Ty::Ref(_) => 4, Ty::Union(_) | Ty::Any => 5 }
    }
    /// Layout of struct `idx` containing the fields with `since <= ver` (versions beyond the schema's use the schema's own).
    pub fn layout(&self, idx: usize, ver: usize) -> &Layout { &self.layouts[idx][ver.clamp(1, self.version) - 1] }
    fn compute(&self, idx: usize, ver: usize) -> Layout {
        let mut l = Layout::default(); let (mut off, mut nopt) = (0usize, 0usize); let mut pres: Vec<usize> = vec![];
        for (fi, f) in self.fields(idx).iter().enumerate() {
            if f.since > ver { continue; }
            let (mut pb, mut bit) = (None, 0u8);
            if f.optional || f.nullable { if nopt % 8 == 0 { pres.push(off); off += 1; } pb = Some(pres[nopt / 8]); bit = (nopt % 8) as u8; nopt += 1; }
            l.lay.push(FieldLayout { fidx: fi, off, pbyte: pb, bit }); off += self.slot_size(&f.ty);
        }
        l.size = off; l
    }
    pub fn from_bytes(d: &[u8]) -> Res<Schema> {
        struct C<'x> { d: &'x [u8], p: usize }
        impl<'x> C<'x> {
            fn bad<T>(&self, m: &str) -> Res<T> { err("E_SCHEMA_BINARY", m) }
            fn u8(&mut self) -> Res<u8> { if self.p >= self.d.len() { return self.bad("truncated schema"); } self.p += 1; Ok(self.d[self.p - 1]) }
            fn vi(&mut self) -> Res<u64> { let (mut v, mut sh) = (0u64, 0u32); for _ in 0..10 { let b = self.u8()?; v |= ((b & 0x7f) as u64) << sh; sh += 7; if b & 0x80 == 0 { return Ok(v); } } self.bad("varint too long") }
            fn vn(&mut self) -> Res<usize> { Ok(self.vi()? as usize) }
            fn st(&mut self) -> Res<String> { let n = self.vn()?; if self.p.checked_add(n).map_or(true, |e| e > self.d.len()) { return self.bad("truncated string"); } let s = String::from_utf8_lossy(&self.d[self.p..self.p + n]).into_owned(); self.p += n; Ok(s) }
            fn td(&mut self, dp: usize) -> Res<Ty> {
                if dp > 32 { return self.bad("type nesting too deep"); }
                Ok(match self.u8()? { 1 => Ty::Bool, 2 => Ty::U8, 3 => Ty::U16, 4 => Ty::U32, 5 => Ty::U64, 6 => Ty::I8, 7 => Ty::I16, 8 => Ty::I32, 9 => Ty::I64, 10 => Ty::F32, 11 => Ty::F64, 12 => Ty::Str, 13 => Ty::Bytes, 14 => Ty::Any,
                    20 => Ty::Struct(self.vn()?), 21 => Ty::Enum(self.vn()?), 22 => Ty::Union(self.vn()?), 23 => Ty::Array(Box::new(self.td(dp + 1)?)), 24 => Ty::Ref(Box::new(self.td(dp + 1)?)), _ => return self.bad("unknown type code") })
            }
        }
        let mut c = C { d, p: 5 };
        if d.len() < 5 || &d[..4] != b"BJXS" || d[4] != 1 { return c.bad("not a BJXS v1 schema"); }
        let namespace = c.st()?; let version = c.vn()?; let r = c.vn()?; let root = if r == 0 { None } else { Some(r - 1) };
        let n = c.vn()?; if n > 100000 { return c.bad("too many types"); }
        let mut types = vec![];
        for _ in 0..n {
            let k = c.u8()?; let name = c.st()?;
            match k {
                1 => {
                    let nf = c.vn()?; let mut fields = vec![];
                    for _ in 0..nf {
                        let fname = c.st()?; let ty = c.td(0)?; let fl = c.u8()?; let since = c.vn()?; let mut default = None;
                        if fl & 8 != 0 {
                            default = Some(match c.u8()? { 0 => DefaultVal::Null, 1 => DefaultVal::Bool(false), 2 => DefaultVal::Bool(true),
                                3 => { let z = c.vi()?; DefaultVal::Int(((z >> 1) as i128) ^ -((z & 1) as i128)) } 4 => DefaultVal::Int(c.vi()? as i128),
                                5 => { if c.p + 8 > d.len() { return c.bad("truncated default"); } let mut b = [0u8; 8]; b.copy_from_slice(&d[c.p..c.p + 8]); c.p += 8; DefaultVal::Float(f64::from_le_bytes(b)) }
                                6 => DefaultVal::Str(c.st()?), 7 => DefaultVal::Arr, _ => return c.bad("bad default tag") });
                        }
                        let na = c.vn()?; let mut cons = vec![]; let mut attrs = vec![];
                        for _ in 0..na {
                            let an = c.st()?; let aa = c.st()?; attrs.push((an.clone(), aa.clone()));
                            if an == "range" || an == "len" {
                                let q: Vec<&str> = aa.split(',').map(|x| x.trim()).collect();
                                if q.len() != 2 { return c.bad("bad constraint"); }
                                let pv = |x: &str| -> Res<Option<f64>> { if x == "_" { Ok(None) } else { x.parse::<f64>().map(Some).or_else(|_| c.bad("bad constraint number")) } };
                                let (lo, hi) = (pv(q[0])?, pv(q[1])?); cons.push((an, lo, hi));
                            }
                        }
                        fields.push(Field { name: fname, ty, optional: fl & 1 != 0, nullable: fl & 2 != 0, deprecated: fl & 4 != 0, since, default, attrs, cons });
                    }
                    types.push(TypeDef::Struct { name, fields });
                }
                2 => { let b = c.u8()?; if ![1, 2, 4].contains(&b) { return c.bad("bad enum base"); } let nv = c.vn()?; let mut values = vec![]; for _ in 0..nv { let s = c.st()?; values.push((s, c.vn()? as u32)); } types.push(TypeDef::Enum { name, base: b as usize, values }); }
                3 => { let nv = c.vn()?; let mut variants = vec![]; for _ in 0..nv { let s = c.st()?; let tag = c.u8()?; variants.push((s, tag, c.td(0)?)); } types.push(TypeDef::Union { name, variants }); }
                _ => return c.bad("bad type kind"),
            }
        }
        if c.p != d.len() { return c.bad("trailing bytes in schema"); }
        Ok(Schema::new(namespace, version, root, types))
    }
    /// Build a schema from its parts (computes all layouts).
    pub fn new(namespace: String, version: usize, root: Option<usize>, types: Vec<TypeDef>) -> Schema {
        let mut s = Schema { namespace, version, root, types, layouts: vec![] };
        s.layouts = (0..s.types.len()).map(|i| if matches!(s.types[i], TypeDef::Struct { .. }) { (1..=s.version.max(1)).map(|v| s.compute(i, v)).collect() } else { vec![] }).collect();
        s
    }
    /// Canonical binary schema (BJXS), byte-identical to the Python reference compiler.
    pub fn to_bytes(&self) -> Vec<u8> {
        fn vi(o: &mut Vec<u8>, mut v: u64) { loop { let b = (v & 0x7f) as u8; v >>= 7; if v != 0 { o.push(b | 0x80); } else { o.push(b); return; } } }
        fn st(o: &mut Vec<u8>, s: &str) { vi(o, s.len() as u64); o.extend_from_slice(s.as_bytes()); }
        fn td(o: &mut Vec<u8>, t: &Ty) {
            match t { Ty::Bool => o.push(1), Ty::U8 => o.push(2), Ty::U16 => o.push(3), Ty::U32 => o.push(4), Ty::U64 => o.push(5), Ty::I8 => o.push(6), Ty::I16 => o.push(7), Ty::I32 => o.push(8), Ty::I64 => o.push(9),
                Ty::F32 => o.push(10), Ty::F64 => o.push(11), Ty::Str => o.push(12), Ty::Bytes => o.push(13), Ty::Any => o.push(14),
                Ty::Struct(i) => { o.push(20); vi(o, *i as u64); } Ty::Enum(i) => { o.push(21); vi(o, *i as u64); } Ty::Union(i) => { o.push(22); vi(o, *i as u64); }
                Ty::Array(e) => { o.push(23); td(o, e); } Ty::Ref(e) => { o.push(24); td(o, e); } }
        }
        let mut o = b"BJXS\x01".to_vec();
        st(&mut o, &self.namespace); vi(&mut o, self.version as u64); vi(&mut o, self.root.map_or(0, |r| r as u64 + 1)); vi(&mut o, self.types.len() as u64);
        for t in &self.types {
            match t {
                TypeDef::Struct { name, fields } => {
                    o.push(1); st(&mut o, name); vi(&mut o, fields.len() as u64);
                    for f in fields {
                        st(&mut o, &f.name); td(&mut o, &f.ty);
                        o.push(f.optional as u8 | (f.nullable as u8) << 1 | (f.deprecated as u8) << 2 | (f.default.is_some() as u8) << 3); vi(&mut o, f.since as u64);
                        if let Some(d) = &f.default { match d {
                            DefaultVal::Null => o.push(0), DefaultVal::Bool(b) => o.push(if *b { 2 } else { 1 }),
                            DefaultVal::Int(v) => { if *v >= 1i128 << 63 { o.push(4); vi(&mut o, *v as u64); } else { o.push(3); let v = *v as i64; vi(&mut o, ((v << 1) ^ (v >> 63)) as u64); } }
                            DefaultVal::Float(x) => { o.push(5); o.extend_from_slice(&x.to_le_bytes()); } DefaultVal::Str(s) => { o.push(6); st(&mut o, s); } DefaultVal::Arr => o.push(7) } }
                        vi(&mut o, f.attrs.len() as u64); for (n, a) in &f.attrs { st(&mut o, n); st(&mut o, a); }
                    }
                }
                TypeDef::Enum { name, base, values } => { o.push(2); st(&mut o, name); o.push(*base as u8); vi(&mut o, values.len() as u64); for (n, v) in values { st(&mut o, n); vi(&mut o, *v as u64); } }
                TypeDef::Union { name, variants } => { o.push(3); st(&mut o, name); vi(&mut o, variants.len() as u64); for (n, tag, t) in variants { st(&mut o, n); o.push(*tag); td(&mut o, t); } }
            }
        }
        o
    }
    /// FNV-1a-64 of the canonical bytes (exact structural fingerprint).
    pub fn fingerprint(&self) -> u64 { fnv1a64(&self.to_bytes()) }
}
