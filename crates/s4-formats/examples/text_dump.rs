//! Decode every shipped `Txt\s4_texts.datN`, report counts, and dump languages to
//! `out/text/<N>_<name>.txt` as UTF-8 (`id<TAB>string`, newlines escaped).
//! Usage: `cargo run -p s4-formats --example text_dump [lang...]` (default: all shipped)

use s4_formats::text::{Language, RawTextFile, TextTable};

fn main() -> anyhow::Result<()> {
    let game = s4_formats::game_dir();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/text");
    std::fs::create_dir_all(&out)?;
    let args: Vec<u32> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let langs = if args.is_empty() {
        Language::SHIPPED.to_vec()
    } else {
        args
    };

    for n in langs {
        let raw = RawTextFile::open(&game.join(format!("Txt/s4_texts.dat{n}")))?;
        let empty = raw.strings.iter().filter(|s| s.is_none()).count();
        let t = TextTable::open(&game, n)?;
        let replaced: usize = (0..t.len())
            .filter_map(|i| t.get(i))
            .map(|s| s.matches('\u{FFFD}').count())
            .sum();
        let missing = (0..t.len()).filter(|&i| t.get(i).is_none()).count();
        let lang = t.language();
        println!(
            "dat{n:<2} {:10} {:?}: {} records ({empty} empty), after fallback {missing} missing, {replaced} undecodable chars | #1000 = {:?}",
            lang.name(),
            lang.code_page(),
            raw.strings.len(),
            t.get(1000).unwrap_or("")
        );
        let mut s = String::new();
        for i in 0..t.len() {
            let v = t
                .get(i)
                .unwrap_or("<none>")
                .replace('\r', "\\r")
                .replace('\n', "\\n");
            s.push_str(&format!("{i}\t{v}\n"));
        }
        std::fs::write(
            out.join(format!(
                "{n}_{}.txt",
                lang.name().split(' ').next().unwrap_or("x")
            )),
            s,
        )?;
    }
    Ok(())
}
