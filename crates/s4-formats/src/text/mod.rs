//! Game string tables: `Txt\s4_texts.dat<lang>`.
//!
//! Mirrors `CStringEngineEx` (decompilation addresses, debug build):
//! - `CStringEngineEx::CStringEngineEx` @0x14ce780 builds the path `Txt\S4_Texts.dat%d` from the
//!   language index and imports, in order: the language file, then `dat0` (English), then `dat1`
//!   (German). For German (1) only `dat1` is imported. Each import only fills slots that are still
//!   empty, so missing/empty strings fall back EN -> DE.
//! - `CStringEngineEx::ExtractStrings` @0x14cea80: `u32 version` (must be 0x15 = 21), then a flat
//!   sequence of `{ i32 len; u8 bytes[len] }` records until end of file; record *n* is string id *n*.
//!   A negative length is treated as 0. No terminator, no encryption, no compression.
//!   An empty record does not fill a slot (`_bFillEmptyStrings` = 1), so it falls back too.
//! - `CStringEngineEx::CreateTextForEmptyStrings` @0x14cef30: slots still empty afterwards get
//!   `"<STRING_NAME>"` (the define name). We return `None` instead.
//!
//! `STRINGID_MAX`: 3837 in the decompiled build, but **3838 (0xEFE)** in the shipped release
//! `S4_Main.exe` (`push 0xEFE` next to the "%i strings in file version" trace, file offset 0x128f33).
//! `dat0`..`dat2` contain 3838 records, every other language 3836.
//!
//! Strings are raw ANSI bytes in the language's Windows code page (the game renders them with
//! GDI fonts created with a per-language charset). The code page is not in the data; it follows
//! from the language index — see [`Language`]. We convert to UTF-8 on load. Known data quirk: the
//! credits strings (ids 717/720) contain German `ß`/`ü` bytes even in the Korean and Hebrew files,
//! which are unmappable there and become U+FFFD (the game shows whatever glyph the charset has).

use crate::reader::Reader;
use crate::{Result, bail};
use std::path::Path;

/// Number of string ids in the shipped release executable.
pub const STRINGID_MAX: usize = 3838;
/// Required file version (`ExtractStrings` compares the first dword against 0x15).
pub const TEXT_VERSION: u32 = 0x15;

/// Language index = the `N` in `s4_texts.datN` (`CGameSettings::m_iLanguage`).
///
/// Code evidence: 0 = English, 1 = German, 2 = French (`CStringEngineEx::GetString`,
/// `CGameSettings::GetManualURL` switch on 1 = DE, 2 = FR, else EN). The GDI font charset chosen
/// per language in `IGuiEngine::Init` @0x2f9fff0 fixes the code pages: 5/11/13 -> EASTEUROPE
/// (1250), 6 -> HANGUL (949), 7 -> CHINESEBIG5 (950), 12 -> HEBREW (1255, plus right-aligned
/// layout), 16 -> RUSSIAN (1251), 18 -> SHIFTJIS (932), everything else ANSI (1252).
/// 6 and 7 also get no-space line breaking in `CScrollMultiline` (generated line 96).
/// The individual language *names* (other than 0/1/2) are inferred from the text content, which
/// also agrees with every charset above. Shipped files: 0-6, 8-13, 18 (no 7, 14-17).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Language(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodePage {
    Cp1250,
    Cp1251,
    Cp1252,
    Cp1255,
    Cp932,
    Cp949,
}

impl Language {
    pub const ENGLISH: Language = Language(0);
    pub const GERMAN: Language = Language(1);
    pub const FRENCH: Language = Language(2);

    /// Language indices with a file in the History Edition install.
    pub const SHIPPED: [u32; 14] = [0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 18];

    pub fn name(self) -> &'static str {
        match self.0 {
            0 => "English",
            1 => "German",
            2 => "French",
            3 => "Spanish",
            4 => "Italian",
            5 => "Polish",
            6 => "Korean",
            7 => "Chinese (no file shipped; CJK per CScrollMultiline)",
            8 => "Swedish",
            9 => "Danish",
            10 => "Norwegian",
            11 => "Hungarian",
            12 => "Hebrew",
            13 => "Czech",
            16 => "Russian (no file shipped)",
            18 => "Japanese",
            _ => "unknown",
        }
    }

    /// Windows code page of the file's bytes (`IGuiEngine::Init` charset switch). Language 7
    /// (Big5) has no shipped file and no table here; it falls back to 1252.
    pub fn code_page(self) -> CodePage {
        match self.0 {
            5 | 11 | 13 => CodePage::Cp1250,
            16 => CodePage::Cp1251,
            6 => CodePage::Cp949,
            12 => CodePage::Cp1255,
            18 => CodePage::Cp932,
            _ => CodePage::Cp1252,
        }
    }
}

/// One `s4_texts.datN` file, undecoded: string id -> raw bytes (`None` = empty record / absent).
pub struct RawTextFile {
    pub strings: Vec<Option<Vec<u8>>>,
}

impl RawTextFile {
    /// `ExtractStrings` @0x14cea80. Unlike the game we reject truncated files instead of clamping.
    pub fn parse(data: &[u8]) -> Result<RawTextFile> {
        let mut r = Reader::new(data, "s4_texts");
        let version = r.u32()?;
        if version != TEXT_VERSION {
            bail!("s4_texts: version {version}, expected {TEXT_VERSION}");
        }
        let mut strings = Vec::new();
        while r.remaining() > 0 {
            let len = r.i32()?.max(0) as usize;
            if len > r.remaining() {
                bail!(
                    "s4_texts: string {} needs {len} bytes, {} left",
                    strings.len(),
                    r.remaining()
                );
            }
            let s = r.bytes(len)?;
            strings.push((len > 0).then(|| s.to_vec()));
        }
        Ok(RawTextFile { strings })
    }

    pub fn open(path: &Path) -> Result<RawTextFile> {
        Self::parse(&std::fs::read(crate::path::resolve(path))?)
    }
}

/// All game strings for one language, with the game's EN/DE fallback applied, as UTF-8.
pub struct TextTable {
    lang: Language,
    strings: Vec<Option<String>>,
    /// Number of records in the language's own file (before fallback).
    own_count: usize,
}

impl TextTable {
    /// `dir` is the game root (containing `Txt\`). Applies the same import chain as
    /// `CStringEngineEx::CStringEngineEx`: own file, then EN (0), then DE (1).
    /// Missing fallback files are tolerated; a missing own file is an error.
    pub fn open(dir: impl AsRef<Path>, lang: u32) -> Result<TextTable> {
        let dir = dir.as_ref();
        let path = |n: u32| dir.join("Txt").join(format!("s4_texts.dat{n}"));
        let lang = Language(lang);
        let own = RawTextFile::open(&path(lang.0))?;
        let own_count = own.strings.len();
        let mut chain = vec![(own, lang)];
        if lang != Language::GERMAN {
            for fb in [Language::ENGLISH, Language::GERMAN] {
                if fb != lang
                    && let Ok(f) = RawTextFile::open(&path(fb.0))
                {
                    chain.push((f, fb));
                }
            }
        }
        let mut strings: Vec<Option<String>> = vec![None; STRINGID_MAX];
        for (file, l) in &chain {
            for (id, s) in file.strings.iter().enumerate().take(STRINGID_MAX) {
                if strings[id].is_none()
                    && let Some(b) = s
                {
                    strings[id] = Some(decode(b, l.code_page()));
                }
            }
        }
        Ok(TextTable {
            lang,
            strings,
            own_count,
        })
    }

    pub fn language(&self) -> Language {
        self.lang
    }
    /// Always [`STRINGID_MAX`].
    pub fn len(&self) -> usize {
        self.strings.len()
    }
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
    /// Records present in the language's own file.
    pub fn own_count(&self) -> usize {
        self.own_count
    }
    /// String `id`, or `None` if no file in the chain has it (game shows `<NAME>` then).
    /// `CStringEngineEx::LoadString` (0x529eb0): ids below [`FILE_STRINGS`] from the files, the
    /// ones above from the exe's own tables by `Language` (1 German 0xff0a60, 2 French 0xfece50,
    /// else English 0xff4670; [`BUILTIN`]).
    pub fn get(&self, id: usize) -> Option<&str> {
        if id >= FILE_STRINGS {
            let k = match self.lang {
                Language::GERMAN => 1,
                Language::FRENCH => 2,
                _ => 0,
            };
            return BUILTIN.get(id - FILE_STRINGS).map(|s| s[k]);
        }
        self.strings.get(id)?.as_deref()
    }
}

/// The strings the text files hold (0xefe, `ExtractStrings` 0x529b20).
pub const FILE_STRINGS: usize = 0xefe;

/// The exe's strings from 0xefe on (English, German, French): random maps, quick match, invite,
/// Mission CD 2 (3841, the main menu's button), Mission CD 3 (3842), score.
pub const BUILTIN: [[&str; 3]; 6] = [
    ["Random Maps", "Zufallskarte", "Carte aléatoire"],
    ["Quick Match", "Schnelles Spiel", "Partie rapide"],
    ["Invite", "Einladen", "Inviter"],
    ["New World", "Neue Welt", "Nouveau Monde"],
    ["Great Crusades", "Große Feldzüge", "Grandes Croisades"],
    ["Score", "Punktestand", "Score"],
];

/// .rdata VA of the release's string-id names: `STRINGID_MAX` pairs `(u32 id, char *name)`,
/// sorted by id (`AO_BONUS_BRIEFING_01` = 0 …), that the name search 0x50bec0 walks.
pub const STRING_NAMES_VA: u32 = 0x100db80;

/// The string-id names (`STRINGID_MAX` of them, index = id) from the bytes of `S4_Main.exe`
/// ([`STRING_NAMES_VA`]).
pub fn string_names(exe: &[u8]) -> Result<Vec<String>> {
    let rd = |o: usize| exe.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let pe = rd(0x3c).ok_or_else(|| crate::Error::Format("names: short image".into()))? as usize;
    if exe.get(pe..pe + 4) != Some(b"PE\0\0") {
        bail!("names: not a PE image");
    }
    let sections = exe.get(pe + 6..pe + 8).map_or(0, |b| u16::from_le_bytes([b[0], b[1]])) as usize;
    let opt_size = exe.get(pe + 20..pe + 22).map_or(0, |b| u16::from_le_bytes([b[0], b[1]])) as usize;
    let base = rd(pe + 24 + 28).ok_or_else(|| crate::Error::Format("names: header".into()))?;
    let file_of = |va: u32| -> Option<usize> {
        let rva = va.checked_sub(base)?;
        (0..sections).find_map(|s| {
            let h = pe + 24 + opt_size + 40 * s;
            let (vsize, vaddr, raw) = (rd(h + 8)?, rd(h + 12)?, rd(h + 20)?);
            (vaddr..vaddr + vsize).contains(&rva).then(|| (raw + rva - vaddr) as usize)
        })
    };
    let table = file_of(STRING_NAMES_VA).ok_or_else(|| crate::Error::Format("names: table outside the image".into()))?;
    let mut names = vec![String::new(); STRINGID_MAX];
    for i in 0..STRINGID_MAX {
        let (id, ptr) = (rd(table + 8 * i), rd(table + 8 * i + 4));
        let (Some(id), Some(at)) = (id, ptr.and_then(file_of)) else { bail!("names: entry {i}") };
        let s = exe.get(at..).and_then(|t| t.split(|&b| b == 0).next()).unwrap_or_default();
        if let Some(n) = names.get_mut(id as usize) {
            *n = String::from_utf8_lossy(s).into_owned();
        }
    }
    Ok(names)
}

/// [`string_names`] of the installed game's `S4_Main.exe`.
pub fn installed_string_names() -> Result<Vec<String>> {
    string_names(&std::fs::read(crate::path::data("S4_Main.exe"))?)
}

/// 0x50bec0: the id named `name` (the first in id order), `None` for an empty name or none.
pub fn string_id(names: &[String], name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    names.iter().position(|n| n == name)
}

static CP1250: &[u8] = include_bytes!("cp1250.bin");
static CP1251: &[u8] = include_bytes!("cp1251.bin");
static CP1252: &[u8] = include_bytes!("cp1252.bin");
static CP1255: &[u8] = include_bytes!("cp1255.bin");
static CP932: &[u8] = include_bytes!("cp932.bin");
static CP949: &[u8] = include_bytes!("cp949.bin");

/// Table layout (generated from the Windows code page mappings): 128 × u16 LE for bytes
/// 0x80..=0xFF (0 = lead byte / undefined), then for DBCS pages 126 × 191 × u16 for
/// lead 0x81..=0xFE, trail 0x40..=0xFE (0 = undefined).
fn table_u16(t: &[u8], i: usize) -> u16 {
    t.get(2 * i..2 * i + 2)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

/// Decode ANSI bytes in `cp` to a `String`; unmappable bytes become U+FFFD.
pub fn decode(bytes: &[u8], cp: CodePage) -> String {
    let (t, dbcs) = match cp {
        CodePage::Cp1250 => (CP1250, false),
        CodePage::Cp1251 => (CP1251, false),
        CodePage::Cp1252 => (CP1252, false),
        CodePage::Cp1255 => (CP1255, false),
        CodePage::Cp932 => (CP932, true),
        CodePage::Cp949 => (CP949, true),
    };
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b < 0x80 {
            out.push(b as char);
            continue;
        }
        let single = table_u16(t, (b - 0x80) as usize);
        if single != 0 {
            out.push(char::from_u32(single as u32).unwrap_or('\u{FFFD}'));
            continue;
        }
        if dbcs
            && (0x81..=0xFE).contains(&b)
            && let Some(&tr) = bytes.get(i)
            && (0x40..=0xFE).contains(&tr)
        {
            let c = table_u16(t, 128 + (b as usize - 0x81) * 191 + (tr as usize - 0x40));
            if c != 0 {
                i += 1;
                out.push(char::from_u32(c as u32).unwrap_or('\u{FFFD}'));
                continue;
            }
        }
        out.push('\u{FFFD}');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let mut d = 0x15u32.to_le_bytes().to_vec();
        d.extend(3i32.to_le_bytes());
        d.extend(b"abc");
        d.extend(0i32.to_le_bytes());
        d.extend((-5i32).to_le_bytes());
        let f = RawTextFile::parse(&d).unwrap();
        assert_eq!(f.strings, vec![Some(b"abc".to_vec()), None, None]);
    }

    #[test]
    fn names_of_the_release() {
        let Ok(names) = installed_string_names() else { return };
        assert_eq!(names.len(), STRINGID_MAX);
        assert_eq!(names[0], "AO_BONUS_BRIEFING_01");
        assert_eq!(string_id(&names, "AO_SINGLE_MAP_AENEAS_DESC"), Some(479));
        assert_eq!(string_id(&names, "AO_SINGLE_MAP_AENEAS_TIPS"), Some(480));
        assert_eq!(string_id(&names, "AO_SINGLEMAP_AENEAS"), None);
        assert!(names.iter().all(|n| !n.is_empty()));
    }

    #[test]
    fn parse_rejects_bad() {
        assert!(RawTextFile::parse(&[1, 0, 0, 0]).is_err());
        let mut d = 0x15u32.to_le_bytes().to_vec();
        d.extend(9i32.to_le_bytes());
        d.extend(b"ab");
        assert!(RawTextFile::parse(&d).is_err());
    }

    #[test]
    fn decode_pages() {
        assert_eq!(decode(b"B\xe4nk", CodePage::Cp1252), "Bänk");
        assert_eq!(decode(b"lavi\xe8ka", CodePage::Cp1250), "lavička");
        assert_eq!(decode(b"\xf1\xf4\xf1\xec", CodePage::Cp1255), "ספסל");
        assert_eq!(
            decode(b"\x8a\xdb\x83x\x83\x93\x83`", CodePage::Cp932),
            "丸ベンチ"
        );
        assert_eq!(
            decode(b"\xbf\xf8\xc7\xfc \xba\xa5\xc4\xa1", CodePage::Cp949),
            "원형 벤치"
        );
        assert_eq!(decode(b"\x81", CodePage::Cp932), "\u{FFFD}");
        assert_eq!(decode(b"\xcc\xe8\xf0", CodePage::Cp1251), "Мир");
    }
}

#[cfg(test)]
mod builtin_tests {
    /// Ids from 0xefe come from the exe's tables (0x529eb0), by language.
    #[test]
    fn strings_past_the_files() {
        let dir = crate::game_dir();
        for (lang, want) in [(0, "New World"), (1, "Neue Welt"), (2, "Nouveau Monde")] {
            if let Ok(t) = super::TextTable::open(&dir, lang) {
                assert_eq!(t.get(3841), Some(want));
                assert_eq!(t.get(0xf04), None);
            }
        }
    }
}
