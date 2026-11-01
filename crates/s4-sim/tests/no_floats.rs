//! The simulation is integer-only: no `f32`/`f64` types or float literals in `src/` (code only,
//! comments are ignored). Complements the `clippy::float_arithmetic` deny in `lib.rs`.

#[test]
fn no_floating_point_in_sim_sources() {
    fn files(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                files(&p, out);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }
    let mut paths = Vec::new();
    files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut paths);
    let mut bad = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            // String literals are data (e.g. savegame text being parsed), not arithmetic.
            let code: String = code.split('"').step_by(2).collect::<Vec<_>>().join(" ");
            let words = code.as_str().split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'));
            for w in words {
                let is_float_type = w == "f32" || w == "f64" || w.ends_with("f32") || w.ends_with("f64");
                let b = w.as_bytes();
                let is_float_lit = b.first().is_some_and(u8::is_ascii_digit)
                    && b.windows(3).any(|p| p[0].is_ascii_digit() && p[1] == b'.' && p[2].is_ascii_digit());
                if is_float_type || is_float_lit {
                    bad.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(bad.is_empty(), "floating point in s4-sim:\n{}", bad.join("\n"));
}
