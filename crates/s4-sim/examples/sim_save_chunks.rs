//! Lists the chunks of a savegame (id, subtype, param, size, first bytes); with `--dump DIR`
//! writes each decompressed chunk to `DIR/<id>_<sub>.bin`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("path to .sav");
    let dump = args.iter().position(|a| a == "--dump").and_then(|i| args.get(i + 1));
    let m = s4_formats::map::MapFile::open(path).expect("open");
    println!("version {}", m.version());
    for h in &m.container.chunks {
        match m.container.load(h) {
            Ok(d) => {
                println!("{:4} {:3} param {:10} {:8} {}", h.kind, h.subtype, h.param, d.len(), hex(&d));
                if let Some(dir) = dump {
                    std::fs::create_dir_all(dir).unwrap();
                    std::fs::write(format!("{dir}/{}_{}.bin", h.kind, h.subtype), &d).unwrap();
                }
            }
            Err(e) => println!("{:4} {:3} ERR {e}", h.kind, h.subtype),
        }
    }
}
fn hex(d: &[u8]) -> String {
    d.iter().take(24).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}
