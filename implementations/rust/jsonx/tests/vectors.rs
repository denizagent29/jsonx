use jsonx::*;
use std::fs;
const V: &str = "../../../tests/vectors/";
fn rd(p: &str) -> String { fs::read_to_string(format!("{}{}", V, p)).unwrap() }
fn rb(p: &str) -> Vec<u8> { fs::read(format!("{}{}", V, p)).unwrap() }
fn names(dir: &str, ext: &str) -> Vec<String> { let mut v: Vec<String> = fs::read_dir(format!("{}{}", V, dir)).unwrap().filter_map(|e| { let n = e.unwrap().file_name().into_string().unwrap(); n.strip_suffix(ext).map(|s| s.to_string()) }).collect(); v.sort(); v }
fn code<T>(r: &Res<T>) -> String { match r { Ok(_) => "OK".into(), Err(e) => match e.line { Some(l) => format!("{} {}:{}", e.code, l, e.col.unwrap()), None => e.code.clone() } } }
struct Counter(usize, Vec<String>);
impl Counter { fn chk(&mut self, c: bool, m: &str) { self.0 += 1; if !c { self.1.push(m.to_string()); } } }
#[test]
fn vectors() {
    let mut c = Counter(0, vec![]); let o = EncodeOptions::default(); let ro = ReadOptions::default();
    for nm in names("text/valid", ".jsonx") {
        let d = parse(&rd(&format!("text/valid/{}.jsonx", nm))).unwrap_or_else(|e| panic!("{}: {}", nm, e));
        c.chk(d.dump() == rd(&format!("text/valid/{}.dump", nm)), &format!("{} dump", nm));
        let b = encode(&d, None, &o).unwrap(); c.chk(b == rb(&format!("text/valid/{}.bjsonx", nm)), &format!("{} golden bytes", nm));
        c.chk(decode(&b, None, &ro).map(|x| x.dump() == d.dump()).unwrap_or(false), &format!("{} roundtrip", nm));
        c.chk(encode(&d, None, &EncodeOptions { checksum: true, ..Default::default() }).unwrap() == rb(&format!("text/valid/{}.crc.bjsonx", nm)), &format!("{} crc", nm));
    }
    for nm in names("text/errors", ".jsonx") { let r = parse(&rd(&format!("text/errors/{}.jsonx", nm))); c.chk(code(&r) == rd(&format!("text/errors/{}.err", nm)).trim(), &format!("error {} got {}", nm, code(&r))); }
    let sc = Schema::from_bytes(&rb("schema/lesson.jxsb")).unwrap(); let sc1 = Schema::from_bytes(&rb("schema/lesson_v1.jxsb")).unwrap();
    c.chk(rd("schema/ids.txt").contains(&format!("lesson_v2_id={:016x}", sc.id())), "schema id");
    for nm in rd("typed/manifest.txt").split_whitespace() {
        let d = parse(&rd(&format!("typed/{}.jsonx", nm))).unwrap(); let b = encode(&d, Some(&sc), &o).unwrap();
        c.chk(b == rb(&format!("typed/{}.bjsonx", nm)), &format!("{} typed golden", nm));
        let d2 = decode(&b, Some(&sc), &ro).unwrap(); c.chk(d2.dump() == rd(&format!("typed/{}.dump", nm)), &format!("{} typed dump", nm));
        c.chk(encode(&d, Some(&sc), &EncodeOptions { checksum: true, ..Default::default() }).unwrap() == rb(&format!("typed/{}.crc.bjsonx", nm)), &format!("{} typed crc", nm));
        c.chk(encode(&d2, Some(&sc), &o).map(|x| x == b).unwrap_or(false), &format!("{} re-encode identical", nm));
    }
    for nm in names("typed/errors", ".jsonx") { let r = parse(&rd(&format!("typed/errors/{}.jsonx", nm))).and_then(|d| encode(&d, Some(&sc), &o)); c.chk(code(&r) == rd(&format!("typed/errors/{}.err", nm)).trim(), &format!("typed error {} got {}", nm, code(&r))); }
    c.chk(decode(&rb("compat/v1_data.bjsonx"), Some(&sc), &ro).unwrap().dump() == rd("compat/v1_data_read_by_v2.dump"), "v1 data / v2 reader");
    c.chk(decode(&rb("compat/v2_data.bjsonx"), Some(&sc1), &ro).unwrap().dump() == rd("compat/v2_data_read_by_v1.dump"), "v2 data / v1 reader");
    for nm in names("binary/malformed", ".bin") {
        let s = if nm.starts_with("typed") { Some(&sc) } else { None };
        let r = decode(&rb(&format!("binary/malformed/{}.bin", nm)), s, &ro);
        c.chk(code(&r) == rd(&format!("binary/malformed/{}.err", nm)).trim(), &format!("malformed {} got {}", nm, code(&r)));
    }
    { // zero-copy view through a ref and alias identity
        let raw = rb("typed/lesson.bjsonx"); let blob = Blob::open(&raw, Some(&sc), &ro).unwrap();
        if let Val::Struct(root) = blob.root().unwrap() {
            if let Val::Array(att) = root.get("attendees").unwrap() { if let Val::Struct(a0) = att.get(0).unwrap() { if let Val::Struct(u) = a0.get("user").unwrap() { c.chk(matches!(u.get("name").unwrap(), Val::Str("Adil")), "view through ref"); } } }
            if let Val::Union(u) = root.get("shape").unwrap() { c.chk(u.name() == Some("circle"), "union view"); }
        }
        let d = decode(&raw, Some(&sc), &ro).unwrap();
        let a = d.at(d.get(d.root, "attendees").unwrap(), 0).and_then(|x| d.get(x, "user")); let u1 = d.get(d.root, "users").and_then(|u| d.at(u, 1));
        c.chk(a.is_some() && a == u1, "alias identity");
    }
    let mut seed = 99u64; let mut rnd = |m: usize| { seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); ((seed >> 33) as usize) % m };
    let seeds: Vec<(Vec<u8>, bool)> = vec![(rb("typed/lesson.bjsonx"), true), (rb("typed/lesson_many_users.bjsonx"), true), (rb("text/valid/many_refs.bjsonx"), false), (rb("text/valid/scalars.bjsonx"), false)];
    for _ in 0..5000 {
        let (b0, typed) = &seeds[rnd(4)]; let mut m = b0.clone();
        for _ in 0..=rnd(4) { let i = rnd(m.len()); m[i] = rnd(256) as u8; }
        if rnd(5) == 0 { let n = rnd(m.len()); m.truncate(n); }
        let _ = decode(&m, if *typed { Some(&sc) } else { None }, &ro); c.0 += 1;
    }
    assert!(c.1.is_empty(), "FAILURES: {:#?}", c.1);
    println!("rust: {} checks", c.0);
}
