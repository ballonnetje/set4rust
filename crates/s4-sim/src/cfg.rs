//! The game's `Config\*.cfg` text format (`CConfigManager`), as far as the simulation reads it:
//! `[SECTION]` blocks of `KEY = value` lines, `;` comments, case-insensitive names; array
//! entries `KEY[n] = v0, v1, …` (`TStaticConfigIntArray<n>`). Same reading as `s4-audio`'s
//! parser (TODO: move one shared parser into `s4-formats`).

use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Cfg {
    /// Upper-cased section → (upper-cased key without `[n]`, raw value).
    sections: HashMap<String, Vec<(String, String)>>,
}

impl Cfg {
    pub fn parse(text: &str) -> Cfg {
        let mut cfg = Cfg::default();
        let mut current: Option<String> = None;
        for line in text.lines() {
            let mut quoted = false;
            let cut = line.char_indices().find(|&(_, c)| {
                if c == '"' {
                    quoted = !quoted;
                }
                c == ';' && !quoted
            });
            let line = cut.map_or(line, |(i, _)| &line[..i]).trim();
            if line.is_empty() || line == "{" || line == "}" {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                let name = name.trim().to_ascii_uppercase();
                cfg.sections.entry(name.clone()).or_default();
                current = Some(name);
                continue;
            }
            let (Some(sec), Some((k, v))) = (&current, line.split_once('=')) else { continue };
            let key = k.trim();
            let key = key.split_once('[').map_or(key, |(k, _)| k).trim().to_ascii_uppercase();
            let entries = cfg.sections.get_mut(sec).expect("section inserted above");
            match entries.iter_mut().find(|(ek, _)| *ek == key) {
                Some(e) => e.1 = v.trim().to_string(),
                None => entries.push((key, v.trim().to_string())),
            }
        }
        cfg
    }

    /// `Config\<name>` of the install, empty when missing.
    pub fn load_default(name: &str) -> Cfg {
        let p = s4_formats::path::resolve(s4_formats::game_dir().join("Config").join(name));
        std::fs::read(p).map(|b| Cfg::parse(&String::from_utf8_lossy(&b))).unwrap_or_default()
    }

    /// `CConfigManager` vtbl+0x2c: whether the file has the section.
    pub fn has_section(&self, section: &str) -> bool {
        self.sections.contains_key(&section.to_ascii_uppercase())
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let entries = self.sections.get(&section.to_ascii_uppercase())?;
        let key = key.to_ascii_uppercase();
        entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }

    /// `TStaticConfigIntArray<n>`: the comma-separated values (missing or non-numeric → 0).
    pub fn int_array<const N: usize>(&self, section: &str, key: &str) -> [i32; N] {
        let mut out = [0; N];
        if let Some(v) = self.get(section, key) {
            for (o, x) in out.iter_mut().zip(v.split(',')) {
                *o = x.trim().parse().unwrap_or(0);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrays() {
        let c = Cfg::parse("[S]\n{\n ; c\n\tA_B[6]= 25,\t\t25, 20, 0 ,\n}\n");
        assert_eq!(c.int_array::<6>("s", "a_b"), [25, 25, 20, 0, 0, 0]);
        assert_eq!(c.int_array::<2>("s", "x"), [0, 0]);
    }
}
