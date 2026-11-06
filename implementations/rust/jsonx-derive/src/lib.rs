//! `#[derive(Bjsonx)]` — typed, schema-checked BJSONX codecs and zero-copy views.
//!
//! ```ignore
//! use jsonx_derive::Bjsonx;
//!
//! #[derive(Bjsonx)]
//! #[bjsonx(schema = "schemas/shop.jxs", name = "Customer")]   // path relative to CARGO_MANIFEST_DIR; `name` defaults to the type name
//! struct Customer { id: u64, name: String, vip: bool, email: Option<String> }
//!
//! let blob = customer.to_blob()?;                 // BJSONX bytes (schema version of the .jxs; `to_blob_with` selects version/crc)
//! let back = Customer::from_blob(&blob)?;          // owned value
//! let open = Customer::open(&blob)?;               // validates the header, borrows `blob`
//! let view = Customer::view(&open)?;               // zero-copy typed view
//! let name: &str = view.name()?;                   // reads straight from the buffer
//! ```
//! The macro compiles the schema at build time (the build re-runs when the `.jxs` or its imports change), checks that the Rust type matches it
//! (field names, types, optionality; compile errors point at the offending field), embeds the canonical binary schema and generates:
//! `to_blob`, `to_blob_with`, `from_blob`, `open`, `view`, `schema` and a `<Name>View<'b,'a>` type with one accessor per field.
//!
//! Mapping: `bool`, `u8..u64`, `i8..i64`, `f32`, `f64`, `String` (string), `Vec<u8>` (bytes), `Vec<T>` (array), `Option<T>` (optional/nullable),
//! other derived structs / unit-variant enums (schema enums) / tuple-variant enums (schema unions) by type name.
//! `ref<T>` fields: decoded as a copy of the target (`T`) and readable through views; **encoding a struct that has ref fields is not supported**
//! (a Rust value tree cannot express sharing) — it returns `E_REF_TARGET`; build such documents from JSONX text. `any` fields are rejected at compile time.
//! Field/variant attribute: `#[bjsonx(rename = "wire_name")]`.
use jsonx::{Schema, Ty, TypeDef};
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as T2};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{parse_macro_input, Data, DeriveInput, Fields, LitStr, Type};

#[proc_macro_derive(Bjsonx, attributes(bjsonx))]
pub fn derive_bjsonx(input: TokenStream) -> TokenStream {
    match expand(parse_macro_input!(input as DeriveInput)) { Ok(t) => t.into(), Err(e) => e.to_compile_error().into() }
}

fn serr(span: Span, m: impl std::fmt::Display) -> syn::Error { syn::Error::new(span, m) }
fn norm(s: &str) -> String { s.chars().filter(|c| *c != '_').flat_map(|c| c.to_lowercase()).collect() }

struct Attr { schema: Option<LitStr>, name: Option<LitStr>, rename: Option<LitStr> }
fn attrs(a: &[syn::Attribute]) -> syn::Result<Attr> {
    let mut r = Attr { schema: None, name: None, rename: None };
    for at in a.iter().filter(|a| a.path().is_ident("bjsonx")) {
        at.parse_nested_meta(|m| {
            if m.path.is_ident("schema") { r.schema = Some(m.value()?.parse()?); } else if m.path.is_ident("name") { r.name = Some(m.value()?.parse()?); }
            else if m.path.is_ident("rename") { r.rename = Some(m.value()?.parse()?); } else { return Err(m.error("unknown bjsonx attribute (expected schema, name or rename)")); }
            Ok(())
        })?;
    }
    Ok(r)
}

fn rust_prim(t: &Ty) -> Option<&'static str> {
    Some(match t { Ty::Bool => "bool", Ty::U8 => "u8", Ty::U16 => "u16", Ty::U32 => "u32", Ty::U64 => "u64", Ty::I8 => "i8", Ty::I16 => "i16", Ty::I32 => "i32", Ty::I64 => "i64", Ty::F32 => "f32", Ty::F64 => "f64", _ => return None })
}
fn last_ident(t: &Type) -> Option<(String, &syn::PathSegment)> { if let Type::Path(p) = t { p.path.segments.last().map(|s| (s.ident.to_string(), s)) } else { None } }
fn generic_arg(seg: &syn::PathSegment) -> Option<&Type> {
    if let syn::PathArguments::AngleBracketed(a) = &seg.arguments { if let Some(syn::GenericArgument::Type(t)) = a.args.first() { return Some(t); } }
    None
}
/// Check that Rust type `t` is the canonical mapping of schema type `ty`.
fn check(sc: &Schema, ty: &Ty, t: &Type) -> Result<(), String> {
    let want = |w: &str| -> Result<(), String> { match last_ident(t) { Some((n, _)) if n == w => Ok(()), _ => Err(format!("expected `{}` for schema type {}", w, sc.type_name(ty))) } };
    if let Some(p) = rust_prim(ty) { return want(p); }
    match ty {
        Ty::Str => want("String"),
        Ty::Bytes => match last_ident(t) { Some((n, s)) if n == "Vec" && matches!(generic_arg(s).and_then(last_ident), Some((u, _)) if u == "u8") => Ok(()), _ => Err("expected `Vec<u8>` for schema type bytes".into()) },
        Ty::Array(e) => match last_ident(t) { Some((n, s)) if n == "Vec" => check(sc, e, generic_arg(s).ok_or("Vec needs a type argument")?), _ => Err(format!("expected `Vec<..>` for schema type {}", sc.type_name(ty))) },
        Ty::Ref(e) => check(sc, e, t),
        Ty::Struct(i) | Ty::Enum(i) | Ty::Union(i) => want(sc.types[*i].name()),
        Ty::Any => Err("`any` fields are not supported by #[derive(Bjsonx)] (use the Doc API)".into()),
        _ => unreachable!(),
    }
}
fn contains_any(t: &Ty) -> bool { match t { Ty::Any => true, Ty::Array(e) | Ty::Ref(e) => contains_any(e), _ => false } }
fn has_ref(t: &Ty) -> bool { match t { Ty::Ref(_) => true, Ty::Array(e) => has_ref(e), _ => false } }
fn tyid(n: &str) -> T2 { let i = format_ident!("{}", n); quote!(#i) }

/// Expression of type `Res<RustType>` reading schema type `ty` from `&Val` expression `v`.
fn from_expr(sc: &Schema, ty: &Ty, v: &T2) -> T2 {
    if let Some(p) = rust_prim(ty) {
        let t = tyid(p);
        return match ty { Ty::Bool => quote!(::jsonx::rt::to_bool(#v)), Ty::F32 => quote!(::jsonx::rt::to_f32(#v)), Ty::F64 => quote!(::jsonx::rt::to_f64(#v)), _ => quote!(::jsonx::rt::to_int::<#t>(#v, #p)) };
    }
    match ty {
        Ty::Str => quote!(match #v { ::jsonx::Val::Str(s) => Ok(s.to_string()), ::jsonx::Val::SchemaStr(s) => Ok(s.to_string()), o => Err(::jsonx::rt::mismatch("string", o)) }),
        Ty::Bytes => quote!(match #v { ::jsonx::Val::Bytes(b) => Ok(b.to_vec()), o => Err(::jsonx::rt::mismatch("bytes", o)) }),
        Ty::Enum(i) | Ty::Union(i) => { let n = tyid(sc.types[*i].name()); quote!(#n::__jx_from_val(#v)) }
        Ty::Struct(i) => { let n = tyid(sc.types[*i].name()); quote!(#n::__jx_from_val(#v)) }
        Ty::Ref(e) => from_expr(sc, e, v),
        Ty::Array(e) => {
            let inner = from_expr(sc, e, &quote!(&__e));
            quote!((|| -> ::jsonx::Res<_> { match #v { ::jsonx::Val::Array(__a) => { let mut __out = Vec::with_capacity(__a.len()); for __i in 0..__a.len() { let __e = __a.get(__i)?; __out.push(#inner?); } Ok(__out) }
                ::jsonx::Val::EmptyArray => Ok(Vec::new()), o => Err(::jsonx::rt::mismatch("array", o)) } })())
        }
        _ => quote!(compile_error!("unsupported")),
    }
}
/// Expression of type `Res<Id>` building schema type `ty` from reference expression `x` into builder `__b`.
fn to_expr(sc: &Schema, ty: &Ty, x: &T2) -> T2 {
    if let Some(p) = rust_prim(ty) {
        return match ty { Ty::Bool => quote!(Ok(__b.bool(*#x))), Ty::F32 => quote!(Ok(__b.f32(*#x))), Ty::F64 => quote!(Ok(__b.float(*#x))), _ => { let _ = p; quote!(Ok(__b.int(*#x as i128))) } };
    }
    match ty {
        Ty::Str => quote!(Ok(__b.string(#x.as_str()))),
        Ty::Bytes => quote!(Ok(__b.bytes(&#x[..]))),
        Ty::Enum(i) | Ty::Union(i) | Ty::Struct(i) => { let n = tyid(sc.types[*i].name()); quote!(#n::__jx_build(#x, __b, __ver)) }
        Ty::Ref(_) => quote!(Err(::jsonx::rt::unsupported("encoding a struct with ref<T> fields is not supported by #[derive(Bjsonx)]; build the document from JSONX text or a Doc"))),
        Ty::Array(e) => { let inner = to_expr(sc, e, &quote!(__e)); quote!((|| -> ::jsonx::Res<::jsonx::Id> { let __a = __b.array(); for __e in #x.iter() { let __c = #inner?; __b.push(__a, __c); } Ok(__a) })()) }
        _ => quote!(compile_error!("unsupported")),
    }
}
/// (return type, body expression of type Res<ret>) for a zero-copy view accessor; `v` is an owned `Val<'b,'a>` expression.
fn view_conv(sc: &Schema, ty: &Ty, v: &T2) -> (T2, T2) {
    if let Some(p) = rust_prim(ty) {
        let t = tyid(p);
        let body = match ty { Ty::Bool => quote!(::jsonx::rt::to_bool(&#v)), Ty::F32 => quote!(::jsonx::rt::to_f32(&#v)), Ty::F64 => quote!(::jsonx::rt::to_f64(&#v)), _ => quote!(::jsonx::rt::to_int::<#t>(&#v, #p)) };
        return (quote!(#t), body);
    }
    match ty {
        Ty::Str => (quote!(&'b str), quote!(match #v { ::jsonx::Val::Str(s) => Ok(s), ::jsonx::Val::SchemaStr(s) => Ok(s), o => Err(::jsonx::rt::mismatch("string", &o)) })),
        Ty::Bytes => (quote!(&'b [u8]), quote!(match #v { ::jsonx::Val::Bytes(b) => Ok(b), o => Err(::jsonx::rt::mismatch("bytes", &o)) })),
        Ty::Struct(i) => { let n = tyid(sc.types[*i].name()); let vn = format_ident!("{}View", sc.types[*i].name()); (quote!(#vn<'b, 'a>), quote!(match #v { ::jsonx::Val::Struct(s) => #n::__jx_view(s), o => Err(::jsonx::rt::mismatch("struct", &o)) })) }
        Ty::Enum(i) | Ty::Union(i) => { let n = tyid(sc.types[*i].name()); (quote!(#n), quote!(#n::__jx_from_val(&#v))) }
        Ty::Ref(e) => view_conv(sc, e, v),
        Ty::Array(_) => (quote!(::jsonx::rt::ViewArray<'b, 'a>), quote!(::jsonx::rt::to_array(#v))),
        _ => (quote!(()), quote!(compile_error!("unsupported"))),
    }
}

fn load_schema(lit: &LitStr) -> syn::Result<(Schema, Vec<std::path::PathBuf>)> {
    let dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| serr(lit.span(), "CARGO_MANIFEST_DIR is not set"))?;
    let path = std::path::Path::new(&dir).join(lit.value());
    let data = std::fs::read(&path).map_err(|e| serr(lit.span(), format!("cannot read schema {}: {}", path.display(), e)))?;
    if data.starts_with(b"BJXS") { return Schema::from_bytes(&data).map(|s| (s, vec![path])).map_err(|e| serr(lit.span(), e)); }
    jsonx::compile_file_deps(&path).map_err(|e| serr(lit.span(), format!("{}", e)))
}

fn expand(input: DeriveInput) -> syn::Result<T2> {
    let a = attrs(&input.attrs)?;
    let lit = a.schema.clone().ok_or_else(|| serr(input.ident.span(), "missing #[bjsonx(schema = \"path/to/schema.jxs\")]"))?;
    let (sc, deps) = load_schema(&lit)?;
    let ident = &input.ident; let tname = a.name.as_ref().map(|n| n.value()).unwrap_or_else(|| ident.to_string());
    let idx = sc.types.iter().position(|t| t.name() == tname).ok_or_else(|| serr(ident.span(), format!("schema has no type named `{}` (use #[bjsonx(name = \"..\")])", tname)))?;
    if tname != ident.to_string() { return Err(serr(ident.span(), format!("the Rust type must be named like the schema type (`{}`); nested references are resolved by name", tname))); }
    let dep_paths: Vec<String> = deps.iter().filter_map(|p| p.canonicalize().ok()).map(|p| p.to_string_lossy().to_string()).collect();
    let dep_tokens = quote!(#( const _: &[u8] = include_bytes!(#dep_paths); )*);
    match (&sc.types[idx], &input.data) {
        (TypeDef::Struct { fields, .. }, Data::Struct(ds)) => expand_struct(&sc, idx, fields, ds, ident, &tname, dep_tokens),
        (TypeDef::Enum { values, .. }, Data::Enum(de)) => expand_enum(values, de, ident, dep_tokens),
        (TypeDef::Union { variants, .. }, Data::Enum(de)) => expand_union(&sc, variants, de, ident, dep_tokens),
        (TypeDef::Struct { .. }, _) => Err(serr(ident.span(), format!("`{}` is a schema (struct) in the .jxs: derive on a struct with named fields", tname))),
        (TypeDef::Enum { .. }, _) => Err(serr(ident.span(), format!("`{}` is an enum in the .jxs: derive on an enum with unit variants", tname))),
        (TypeDef::Union { .. }, _) => Err(serr(ident.span(), format!("`{}` is a union in the .jxs: derive on an enum with one-field tuple variants", tname))),
    }
}

fn expand_struct(sc: &Schema, idx: usize, fields: &[jsonx::Field], ds: &syn::DataStruct, ident: &syn::Ident, tname: &str, deps: T2) -> syn::Result<T2> {
    let named = match &ds.fields { Fields::Named(n) => &n.named, _ => return Err(serr(ident.span(), "#[derive(Bjsonx)] needs a struct with named fields")) };
    // match Rust fields to schema fields by (renamed) name
    let mut rust: Vec<(&syn::Field, String)> = vec![];
    for f in named { let r = attrs(&f.attrs)?; let n = r.rename.map(|l| l.value()).unwrap_or_else(|| f.ident.as_ref().unwrap().to_string()); rust.push((f, n)); }
    for (f, n) in &rust { if !fields.iter().any(|sf| sf.name == *n) { return Err(serr(f.span(), format!("schema `{}` has no field `{}`", tname, n))); } }
    let mut b_stmts = vec![]; let mut from_inits = vec![]; let mut view_fns = vec![]; let mut has_ref_field = false;
    for sf in fields {
        let (rf, _) = rust.iter().find(|(_, n)| *n == sf.name).ok_or_else(|| serr(ident.span(), format!("struct `{}` is missing the schema field `{}` (type {}{})", ident, sf.name, sc.type_name(&sf.ty), if sf.optional { ", optional" } else { "" })))?;
        let fid = rf.ident.as_ref().unwrap(); let wire = &sf.name; let since = sf.since;
        if contains_any(&sf.ty) { return Err(serr(rf.ty.span(), "`any` fields are not supported by #[derive(Bjsonx)] (use the Doc API)")); }
        has_ref_field |= has_ref(&sf.ty);
        let opt = sf.optional || sf.nullable;
        let inner_ty: &Type = if opt { match last_ident(&rf.ty) { Some((n, s)) if n == "Option" => generic_arg(s).ok_or_else(|| serr(rf.ty.span(), "Option needs a type argument"))?, _ => return Err(serr(rf.ty.span(), format!("field `{}` is {} in the schema: its Rust type must be `Option<..>`", wire, if sf.optional { "optional" } else { "nullable" }))) } } else {
            if matches!(last_ident(&rf.ty), Some((n, _)) if n == "Option") { return Err(serr(rf.ty.span(), format!("field `{}` is required in the schema; use a non-Option type (or mark it optional<> there)", wire))); } &rf.ty };
        check(sc, &sf.ty, inner_ty).map_err(|m| serr(rf.ty.span(), format!("field `{}`: {}", wire, m)))?;
        // build
        let to_in = to_expr(sc, &sf.ty, &quote!(__x));
        b_stmts.push(if sf.optional { quote!(if __ver >= #since { if let Some(__x) = &self.#fid { let __c = #to_in?; __b.set(__o, #wire, __c); } }) }
            else if sf.nullable { quote!(if __ver >= #since { match &self.#fid { Some(__x) => { let __c = #to_in?; __b.set(__o, #wire, __c); } None => { let __c = __b.null(); __b.set(__o, #wire, __c); } } }) }
            else { quote!(if __ver >= #since { let __x = &self.#fid; let __c = #to_in?; __b.set(__o, #wire, __c); }) });
        // decode
        let fe = from_expr(sc, &sf.ty, &quote!(&__fv));
        from_inits.push(if opt { quote!(#fid: { let __fv = __sv.get(#wire)?; match &__fv { ::jsonx::Val::Null => None, _ => Some(#fe?) } }) } else { quote!(#fid: { let __fv = __sv.get(#wire)?; #fe? }) });
        // view accessor
        let (rt, body) = view_conv(sc, &sf.ty, &quote!(__fv));
        view_fns.push(if opt { quote!(pub fn #fid(&self) -> ::jsonx::Res<Option<#rt>> { let __fv = self.0.get(#wire)?; if let ::jsonx::Val::Null = __fv { return Ok(None); } let __r: ::jsonx::Res<#rt> = #body; __r.map(Some) } ) }
            else { quote!(pub fn #fid(&self) -> ::jsonx::Res<#rt> { let __fv = self.0.get(#wire)?; #body }) });
    }
    let view = format_ident!("{}View", ident); let bytes = sc.to_bytes(); let blit = proc_macro2::Literal::byte_string(&bytes);
    let _ = idx; let doc_enc = format!("Encode at the schema's current version.{}", if has_ref_field { " This type has ref<T> fields: returns `E_REF_TARGET` (decode and views work)." } else { "" });
    Ok(quote! {
        #deps
        impl #ident {
            /// The compiled schema (loaded once, shared by all calls).
            pub fn schema() -> &'static ::jsonx::Schema { static S: ::std::sync::OnceLock<::jsonx::Schema> = ::std::sync::OnceLock::new(); S.get_or_init(|| ::jsonx::Schema::from_bytes(#blit).expect("embedded schema is valid")) }
            #[doc(hidden)] pub fn __jx_build(&self, __b: &mut ::jsonx::rt::DocBuilder, __ver: usize) -> ::jsonx::Res<::jsonx::Id> { let __o = __b.object(); #(#b_stmts)* Ok(__o) }
            #[doc(hidden)] pub fn __jx_from_val(__v: &::jsonx::Val<'_, '_>) -> ::jsonx::Res<Self> {
                let __sv = match __v { ::jsonx::Val::Struct(s) => s, o => return Err(::jsonx::rt::mismatch(#tname, o)) };
                if __sv.type_name() != #tname { return Err(::jsonx::rt::unknown("schema", __sv.type_name())); }
                Ok(Self { #(#from_inits),* })
            }
            #[doc(hidden)] pub fn __jx_view<'b, 'a>(__s: ::jsonx::StructView<'b, 'a>) -> ::jsonx::Res<#view<'b, 'a>> { if __s.type_name() != #tname { return Err(::jsonx::rt::unknown("schema", __s.type_name())); } Ok(#view(__s)) }
            #[doc = #doc_enc]
            pub fn to_blob(&self) -> ::jsonx::Res<Vec<u8>> { self.to_blob_with(None, false) }
            /// Encode at an older layout `version` (for old readers) and/or with a CRC-32 trailer.
            pub fn to_blob_with(&self, version: Option<usize>, checksum: bool) -> ::jsonx::Res<Vec<u8>> {
                let __ver = version.unwrap_or(Self::schema().version); let mut __b = ::jsonx::rt::DocBuilder::new(); let __root = self.__jx_build(&mut __b, __ver)?;
                ::jsonx::encode(&__b.finish(__root), Some(Self::schema()), &::jsonx::EncodeOptions { root: Some(#tname.to_string()), version: Some(__ver), checksum })
            }
            /// Decode into an owned value (reads and converts every field; unknown newer fields are ignored, missing newer fields take their defaults).
            pub fn from_blob(bytes: &[u8]) -> ::jsonx::Res<Self> { let __blob = Self::open(bytes)?; let __v = __blob.root()?; Self::__jx_from_val(&__v) }
            /// Validate the header/schema and open the blob for zero-copy access.
            pub fn open<'a>(bytes: &'a [u8]) -> ::jsonx::Res<::jsonx::Blob<'a>> { ::jsonx::Blob::open(bytes, Some(Self::schema()), &::jsonx::ReadOptions::default()) }
            /// Typed zero-copy view of the root value of an opened blob.
            pub fn view<'b, 'a>(blob: &'b ::jsonx::Blob<'a>) -> ::jsonx::Res<#view<'b, 'a>> { match blob.root()? { ::jsonx::Val::Struct(s) => Self::__jx_view(s), o => Err(::jsonx::rt::mismatch(#tname, &o)) } }
        }
        /// Zero-copy typed view; every accessor reads from the buffer on demand.
        #[derive(Debug, Clone)] pub struct #view<'b, 'a>(pub ::jsonx::StructView<'b, 'a>);
        impl<'b, 'a> #view<'b, 'a> { #(#view_fns)* }
    })
}

fn variant_wire<'a>(names: impl Iterator<Item = &'a str>, rust: &str, span: Span) -> syn::Result<String> {
    let nn = norm(rust); let mut hit = names.filter(|n| norm(n) == nn);
    hit.next().map(|s| s.to_string()).ok_or_else(|| serr(span, format!("no schema member/variant matches `{}`", rust)))
}
fn expand_enum(values: &[(String, u32)], de: &syn::DataEnum, ident: &syn::Ident, deps: T2) -> syn::Result<T2> {
    let mut arms_b = vec![]; let mut arms_f = vec![]; let mut seen = vec![];
    for v in &de.variants {
        if !matches!(v.fields, Fields::Unit) { return Err(serr(v.span(), "schema enums map to enums with unit variants only")); }
        let r = attrs(&v.attrs)?; let rn = v.ident.to_string(); let wire = match r.rename { Some(l) => l.value(), None => variant_wire(values.iter().map(|(n, _)| n.as_str()), &rn, v.span())? };
        if !values.iter().any(|(n, _)| *n == wire) { return Err(serr(v.span(), format!("enum has no member `{}`", wire))); }
        let vi = &v.ident; arms_b.push(quote!(Self::#vi => __b.string(#wire))); arms_f.push(quote!(#wire => Ok(Self::#vi))); seen.push(wire);
    }
    for (n, _) in values { if !seen.contains(n) { return Err(serr(ident.span(), format!("missing a variant for enum member `{}`", n))); } }
    Ok(quote! { #deps impl #ident {
        #[doc(hidden)] pub fn __jx_build(&self, __b: &mut ::jsonx::rt::DocBuilder, _ver: usize) -> ::jsonx::Res<::jsonx::Id> { Ok(match self { #(#arms_b),* }) }
        #[doc(hidden)] pub fn __jx_from_val(__v: &::jsonx::Val<'_, '_>) -> ::jsonx::Res<Self> { match __v { ::jsonx::Val::Enum(n) => match *n { #(#arms_f,)* o => Err(::jsonx::rt::unknown("enum member", o)) },
            ::jsonx::Val::EnumRaw(x) => Err(::jsonx::rt::unknown("enum value", &x.to_string())), o => Err(::jsonx::rt::mismatch("enum", o)) } }
    } })
}
fn expand_union(sc: &Schema, variants: &[(String, u8, Ty)], de: &syn::DataEnum, ident: &syn::Ident, deps: T2) -> syn::Result<T2> {
    let mut arms_b = vec![]; let mut arms_f = vec![]; let mut seen = vec![];
    for v in &de.variants {
        let ty = match &v.fields { Fields::Unnamed(u) if u.unnamed.len() == 1 => &u.unnamed[0].ty, _ => return Err(serr(v.span(), "schema unions map to enums whose variants have exactly one unnamed field")) };
        let r = attrs(&v.attrs)?; let rn = v.ident.to_string(); let wire = match r.rename { Some(l) => l.value(), None => variant_wire(variants.iter().map(|(n, _, _)| n.as_str()), &rn, v.span())? };
        let (_, _, vt) = variants.iter().find(|(n, _, _)| *n == wire).ok_or_else(|| serr(v.span(), format!("union has no variant `{}`", wire)))?;
        check(sc, vt, ty).map_err(|m| serr(ty.span(), format!("variant `{}`: {}", wire, m)))?;
        let vi = &v.ident; let to = to_expr(sc, vt, &quote!(__x)); let from = from_expr(sc, vt, &quote!(&__pv));
        arms_b.push(quote!(Self::#vi(__x) => { let __c = #to?; let __o = __b.object(); __b.set(__o, #wire, __c); Ok(__o) }));
        arms_f.push(quote!(Some(#wire) => { let __pv = __u.value()?; Ok(Self::#vi(#from?)) })); seen.push(wire);
    }
    for (n, _, _) in variants { if !seen.contains(n) { return Err(serr(ident.span(), format!("missing a variant for union variant `{}`", n))); } }
    Ok(quote! { #deps impl #ident {
        #[doc(hidden)] pub fn __jx_build(&self, __b: &mut ::jsonx::rt::DocBuilder, __ver: usize) -> ::jsonx::Res<::jsonx::Id> { let _ = __ver; match self { #(#arms_b),* } }
        #[doc(hidden)] pub fn __jx_from_val(__v: &::jsonx::Val<'_, '_>) -> ::jsonx::Res<Self> { match __v { ::jsonx::Val::Union(__u) => match __u.name() { #(#arms_f,)* o => Err(::jsonx::rt::unknown("union variant", o.unwrap_or("?"))) }, o => Err(::jsonx::rt::mismatch("union", o)) } }
    } })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn err(src: &str) -> String { match expand(syn::parse_str::<DeriveInput>(src).unwrap()) { Ok(_) => "OK".into(), Err(e) => e.to_string() } }
    const A: &str = "#[bjsonx(schema = \"../../../examples/schemas/shop.jxs\")]";
    #[test] fn valid() { assert_eq!(err(&format!("{} struct Card {{ last4: String, brand: String }}", A)), "OK"); }
    #[test] fn missing_attribute() { assert!(err("struct Card { last4: String }").contains("missing #[bjsonx(schema")); }
    #[test] fn missing_schema_file() { assert!(err("#[bjsonx(schema = \"nope.jxs\")] struct Card { a: u8 }").contains("cannot read schema")); }
    #[test] fn unknown_type() { assert!(err(&format!("{} struct Nope {{ a: u8 }}", A)).contains("no type named `Nope`")); }
    #[test] fn extra_field() { assert!(err(&format!("{} struct Card {{ last4: String, brand: String, extra: u8 }}", A)).contains("has no field `extra`")); }
    #[test] fn missing_field() { let e = err(&format!("{} struct Card {{ last4: String }}", A)); assert!(e.contains("missing the schema field `brand`"), "{}", e); }
    #[test] fn wrong_type() { let e = err(&format!("{} struct Card {{ last4: u32, brand: String }}", A)); assert!(e.contains("field `last4`") && e.contains("expected `String`"), "{}", e); }
    #[test] fn optional_needs_option() { let e = err(&format!("{} struct Item {{ sku: String, qty: u32, price: f64, tags: Vec<String>, weight: f32 }}", A)); assert!(e.contains("must be `Option<..>`"), "{}", e); }
    #[test] fn required_must_not_be_option() { let e = err(&format!("{} struct Card {{ last4: Option<String>, brand: String }}", A)); assert!(e.contains("is required in the schema"), "{}", e); }
    #[test] fn bytes_needs_vec_u8() { let e = err(&format!("{} struct Receipt {{ id: u64, customers: Vec<Customer>, items: Vec<Item>, status: Status, payment: Option<Payment>, note: Option<String>, raw: Vec<u16>, grid: Vec<Vec<i16>>, nick: String }}", A)); assert!(e.contains("Vec<u8>"), "{}", e); }
    #[test] fn enum_member_missing() { let e = err(&format!("{} enum Status {{ Draft, Paid }}", A)); assert!(e.contains("missing a variant for enum member `shipped`"), "{}", e); }
    #[test] fn enum_unknown_variant() { let e = err(&format!("{} enum Status {{ Draft, Paid, Shipped, Lost }}", A)); assert!(e.contains("no schema member/variant matches `Lost`"), "{}", e); }
    #[test] fn union_shape() { let e = err(&format!("{} enum Payment {{ Card, Iban(String), Cash(u32) }}", A)); assert!(e.contains("exactly one unnamed field"), "{}", e); }
    #[test] fn kind_mismatch() { let e = err(&format!("{} struct Status {{ a: u8 }}", A)); assert!(e.contains("is an enum in the .jxs"), "{}", e); }
    #[test] fn rename() { assert_eq!(err(&format!("{} struct Card {{ #[bjsonx(rename = \"last4\")] tail: String, brand: String }}", A)), "OK"); }
}
