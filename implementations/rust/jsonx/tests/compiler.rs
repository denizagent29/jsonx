use jsonx::*;
use std::fs;
const V: &str = "../../../tests/vectors/";
fn names(dir: &str, ext: &str) -> Vec<String> { let mut v: Vec<String> = fs::read_dir(format!("{}{}", V, dir)).unwrap().filter_map(|e| e.unwrap().file_name().into_string().unwrap().strip_suffix(ext).map(|s| s.to_string())).collect(); v.sort(); v }
fn code<T>(r: &Res<T>) -> String { match r { Ok(_) => "OK".into(), Err(e) => format!("{} {}:{}", e.code, e.line.unwrap_or(0), e.col.unwrap_or(0)) } }
fn loader(dir: &'static str) -> impl Fn(&str, &str) -> std::io::Result<(String, String)> { move |p: &str, _f: &str| Ok((fs::read_to_string(format!("{}{}/{}", V, dir, p))?, p.to_string())) }
#[test]
fn compiler_matches_reference() {
    let mut n = 0;
    for nm in names("schema/ok", ".jxs") {
        let src = fs::read_to_string(format!("{}schema/ok/{}.jxs", V, nm)).unwrap();
        let sc = compile_str(&src, &format!("{}.jxs", nm), None).unwrap_or_else(|e| panic!("{}: {}", nm, e));
        assert_eq!(sc.to_bytes(), fs::read(format!("{}schema/ok/{}.jxsb", V, nm)).unwrap(), "schema bytes differ for {}", nm); n += 1;
    }
    let sc = compile_file(std::path::Path::new(&format!("{}schema/lesson.jxs", V))).unwrap();
    assert_eq!(sc.to_bytes(), fs::read(format!("{}schema/lesson.jxsb", V)).unwrap(), "lesson.jxsb"); n += 1;
    let l = loader("ok");
    for nm in names("schema/errors", ".jxs") {
        let src = fs::read_to_string(format!("{}schema/errors/{}.jxs", V, nm)).unwrap();
        let r = compile_str(&src, &format!("{}.jxs", nm), Some(&l));
        assert_eq!(code(&r), fs::read_to_string(format!("{}schema/errors/{}.err", V, nm)).unwrap().trim(), "schema error {}", nm); n += 1;
    }
    println!("compiler: {} schema checks", n);
}
