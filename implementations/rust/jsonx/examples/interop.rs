use jsonx::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sc = Schema::from_bytes(&std::fs::read(&a[1]).unwrap()).unwrap();
    let d = parse(&std::fs::read_to_string(&a[2]).unwrap()).unwrap();
    let (o, ro, t) = (EncodeOptions::default(), ReadOptions::default(), &a[3]);
    std::fs::write(format!("{}/rs.bjsonx", t), encode(&d, Some(&sc), &o).unwrap()).unwrap();
    std::fs::write(format!("{}/rs.dyn", t), encode(&d, None, &o).unwrap()).unwrap();
    let x = decode(&std::fs::read(format!("{}/py.bjsonx", t)).unwrap(), Some(&sc), &ro).unwrap();
    let y = decode(&std::fs::read(format!("{}/py.dyn", t)).unwrap(), None, &ro).unwrap();
    std::fs::write(format!("{}/rs_reads_py.txt", t), format!("{}\n{}", x.dump(), y.dump())).unwrap();
}
