//! The game's `Config\*.cfg` text format, as `CConfigManager` reads it:
//!
//! ```text
//! ; comment
//! [SECTION]
//! {
//!     KEY = 123
//!     KEY2 = "quoted string"
//! }
//! ```
//!
//! Section and key lookups are case-insensitive. Only what `SoundFX.cfg` and `SoundTracks.cfg`
//! need is supported: integer and (optionally quoted) string values; the entry count of a section
//! (`GetSectionEntryCount`).

use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Cfg {
    /// upper-cased section -> entries in file order (upper-cased key, raw value)
    sections: HashMap<String, Vec<(String, String)>>,
}

impl Cfg {
    pub fn parse(text: &str) -> Cfg {
        let mut cfg = Cfg::default();
        let mut current: Option<String> = None;
        for line in text.lines() {
            // a ';' outside quotes starts a comment
            let mut quoted = false;
            let cut = line.char_indices().find(|&(_, c)| {
                if c == '"' {
                    quoted = !quoted;
                }
                c == ';' && !quoted
            });
            let line = cut.map_or(line, |(i, _)| &line[..i]);
            let line = line.trim();
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
            let v = v.trim();
            let v = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(v);
            let entries = cfg.sections.get_mut(sec).expect("section inserted above");
            let key = k.trim().to_ascii_uppercase();
            // a later duplicate overrides, like adding a second config file
            match entries.iter_mut().find(|(ek, _)| *ek == key) {
                Some(e) => e.1 = v.to_string(),
                None => entries.push((key, v.to_string())),
            }
        }
        cfg
    }

    /// Merge `other` over `self` (the user-directory copy of an optional config file).
    pub fn merge(&mut self, other: Cfg) {
        for (sec, entries) in other.sections {
            let mine = self.sections.entry(sec).or_default();
            for (k, v) in entries {
                match mine.iter_mut().find(|(ek, _)| *ek == k) {
                    Some(e) => e.1 = v,
                    None => mine.push((k, v)),
                }
            }
        }
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let entries = self.sections.get(&section.to_ascii_uppercase())?;
        let key = key.to_ascii_uppercase();
        entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }

    /// `GetIntValue(section, key, default)`: non-numeric values read as `default`.
    pub fn int(&self, section: &str, key: &str, default: i32) -> i32 {
        self.get(section, key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    /// `GetSectionEntryCount`.
    pub fn entry_count(&self, section: &str) -> usize {
        self.sections.get(&section.to_ascii_uppercase()).map_or(0, Vec::len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections() {
        let c = Cfg::parse("; x\n[A_B]\n{\n\tK0 = \"snd/a.mp3\" ; c\n\tk1 = 25\n}\n[C]\n{\n}\n");
        assert_eq!(c.get("a_b", "K0"), Some("snd/a.mp3"));
        assert_eq!(c.int("A_B", "K1", 0), 25);
        assert_eq!(c.int("A_B", "K2", 7), 7);
        assert_eq!(c.entry_count("A_B"), 2);
        assert_eq!(c.entry_count("C"), 0);
        assert_eq!(c.entry_count("D"), 0);
    }
}
