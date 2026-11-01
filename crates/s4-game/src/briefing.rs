//! The mission goal the extras' briefing page shows (the std::string 0x149be6c; its siblings are
//! the title 0x149be3c, the story 0x149be54 and the tips 0x149be84). Decompiles in
//! `tools/re/gui-2026-10-23/brief*.c`, `maptext.c`.
//!
//! * A campaign briefing writes the mission's goal string first: the main campaigns 0x511070
//!   (types 1..=4, `frontend::briefing_strings`), the Add-On 0x50e7c0, the Mission CD 0x524b80,
//!   MCD2 0x523c80, XMD3 0x529330 (`frontend::addon::briefing`'s goal). Loading the map
//!   (0x5069b0) leaves it alone, except for a random map, which clears all four.
//! * A game started from a lobby (`CStateLobbyGameSettings::SetLobbyState` 0x51d6f1, and the
//!   XMD3 briefing for its other maps, 0x528c74) copies the map's goal, `GlobalGameState
//!   +0x3a0`, which 0x5069b0 parses out of the map file ([`map_texts`]).
//! * In the game: the extras submenu's init (`ExtrasMenuCallback` 0x47b4e0 code 0) enables its
//!   929 only with a goal or in game type 3, the key 'M' (`IngameKeybindings` 0x4890b0) presses
//!   it under the same condition, and page 35's init 0x46cce0 shows string 880 (369) and the goal
//!   (370). The savegame keeps the string (`CGameChunkGeneral` 0x4ffee0); here it is derived again
//!   from the campaign and the map on load. [inf] the same text unless the language changed.

use s4_formats::map::{MapFile, MapText};
use std::path::Path;

/// The four texts of a map file (`GlobalGameState` +0x388 title, +0x3a0 goal, +0x358 story,
/// +0x370 tips).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapTexts {
    pub title: String,
    pub goal: String,
    pub story: String,
    pub tips: String,
}

/// The map's text chunks as `0x68c000(i)` reads them: 0 description (chunk 11), 1 tips (12),
/// 2 English description (14), 3 English tips (15) (`MA_ReadFile` 0x68b390's switch).
pub fn chunks(map: &MapFile) -> [String; 4] {
    [MapText::Description, MapText::Tips, MapText::DescriptionEnglish, MapText::TipsEnglish].map(|w| map.text(w).ok().flatten().unwrap_or_default())
}

/// A "$KEY$" text (0x506f7a..0x50700b): trimmed of characters up to the space, 2..0x80 long,
/// '$' at both ends, `A-Z 0-9 _` between.
fn key_of(t: &str) -> Option<&str> {
    let t = t.trim_matches(|c: char| c as u32 <= 0x20);
    let n = t.chars().count();
    if !(2..0x80).contains(&n) || !t.starts_with('$') || !t.ends_with('$') {
        return None;
    }
    let inner = &t[1..t.len() - 1];
    inner.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_').then_some(inner)
}

/// 0x5069b0 (0x506ecb..0x5072f4) for a map that is not random: the four strings cleared; a
/// description chunk "$KEY$" names the texts `KEY_DESC` / `KEY_TIPS` (a key ending in `_DESC`
/// that names a text loses it first; the name search 0x50bec0, `text` = the string table);
/// otherwise the description and tips of the language (`0x11b59e4` = 1 German: chunks 0 / 1,
/// else 2 / 3). The description splits at '@' into title, goal and story; without a second
/// '@' the rest is the story, without any the whole.
pub fn map_texts(chunks: &[String; 4], language: u32, text: &dyn Fn(usize) -> Option<String>, names: &[String]) -> MapTexts {
    let (desc, tips) = match key_of(&chunks[0]) {
        Some(k) => {
            let mut key = k.to_string();
            if key.len() > 5 && key.ends_with("_DESC") && s4_formats::text::string_id(names, &key).is_some() {
                key.truncate(key.len() - 5);
            }
            let named = |suffix: &str| s4_formats::text::string_id(names, &format!("{key}{suffix}")).and_then(text).unwrap_or_default();
            (named("_DESC"), named("_TIPS"))
        }
        None => {
            let i = if language == 1 { 0 } else { 2 };
            (chunks[i].clone(), chunks[i + 1].clone())
        }
    };
    let mut m = MapTexts { tips, ..Default::default() };
    if !desc.is_empty() {
        match desc.split_once('@') {
            None => m.story = desc,
            Some((title, rest)) => {
                m.title = title.to_string();
                match rest.split_once('@') {
                    None => m.story = rest.to_string(),
                    Some((goal, story)) => {
                        m.goal = goal.to_string();
                        m.story = story.to_string();
                    }
                }
            }
        }
    }
    m
}

/// The goal string id a campaign briefing writes for mission `level` of `campaign`.
fn campaign_goal(campaign: u8, level: u32) -> Option<usize> {
    match campaign {
        1..=4 => Some(crate::frontend::briefing_strings(campaign)[0] + level as usize),
        _ => crate::frontend::addon::briefing(campaign, level).map(|b| b[0]),
    }
}

/// The installed release's string names, read once.
fn names() -> &'static [String] {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| s4_formats::text::installed_string_names().inspect_err(|e| tracing::warn!("string names: {e}")).unwrap_or_default())
}

/// 0x149be6c for a game of `map`: the campaign mission's goal, else the map's.
pub fn goal(campaign: Option<(u8, u32)>, map: &Path, language: u32, text: &dyn Fn(usize) -> Option<String>) -> String {
    if let Some(id) = campaign.and_then(|(c, l)| campaign_goal(c, l)) {
        return text(id).unwrap_or_default();
    }
    match MapFile::open(map) {
        Ok(m) => map_texts(&chunks(&m), language, text, names()).goal,
        Err(e) => {
            tracing::warn!("briefing: {}: {e}", map.display());
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        t.to_string()
    }

    #[test]
    fn description_splits_at_the_at_signs() {
        let none = |_: usize| None;
        let c = [s("Fear Fortress@Defeat the Viking!@Two years"), s("tip"), s("T@G@S@more"), s("en tip")];
        assert_eq!(map_texts(&c, 1, &none, &[]), MapTexts { title: s("Fear Fortress"), goal: s("Defeat the Viking!"), story: s("Two years"), tips: s("tip") });
        assert_eq!(map_texts(&c, 0, &none, &[]), MapTexts { title: s("T"), goal: s("G"), story: s("S@more"), tips: s("en tip") });
        let c = [s(""), s(""), s("only a story"), s("")];
        assert_eq!(map_texts(&c, 0, &none, &[]).story, "only a story");
        let c = [s(""), s(""), s("Title@story"), s("")];
        let m = map_texts(&c, 0, &none, &[]);
        assert_eq!((m.title.as_str(), m.goal.as_str(), m.story.as_str()), ("Title", "", "story"));
    }

    #[test]
    fn keyed_descriptions() {
        let names = [s("A_DESC"), s("A_TIPS"), s("B_DESC_DESC"), s("X")];
        let text = |id: usize| Some(["t@g@s", "tips", "bb@goal b@", ""][id].to_string());
        let c = [s("  $A_DESC$ "), s(""), s("ignored@x@y"), s("")];
        assert_eq!(map_texts(&c, 0, &text, &names), MapTexts { title: s("t"), goal: s("g"), story: s("s"), tips: s("tips") });
        // The bare key; "B_DESC" names no text, so its suffix stays ("B_DESC_DESC").
        let c = [s("$A$"), s(""), s("ignored@x@y"), s("")];
        assert_eq!(map_texts(&c, 0, &text, &names).goal, "g");
        let c = [s("$B_DESC$"), s(""), s(""), s("")];
        assert_eq!(map_texts(&c, 0, &text, &names).goal, "goal b");
        // A key without texts: nothing, the language's chunks are not read (+0x8299 stays set).
        let c = [s("$Q$"), s(""), s("x@from the map@y"), s("")];
        assert_eq!(map_texts(&c, 0, &text, &names), MapTexts::default());
        // Not a key (lower case): the language's chunks.
        let c = [s("$a$"), s(""), s("x@from the map@y"), s("")];
        assert_eq!(map_texts(&c, 0, &text, &names).goal, "from the map");
    }

    #[test]
    fn shipped_goals() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let t = s4_formats::text::TextTable::open(s4_formats::game_dir(), 0).unwrap();
        let text = |id: usize| t.get(id).map(str::to_string);
        let map = |m: &str| s4_formats::game_dir().join("Map").join(m);
        // A keyed single player map (texts 479 / 480) and a plain one.
        assert_eq!(goal(None, &map("Singleplayer/Aeneas.map"), 0, &text), "Found a new settlement with Lavinia and ensure the safety of your subjects!");
        assert_eq!(goal(None, &map("Singleplayer/FearFortress.map"), 0, &text), "Defeat the Viking!");
        // Campaigns: the briefing's goal (Roman 1: 3363; Add-On Roman 1: 410).
        assert_eq!(goal(Some((1, 0)), &map("Campaign/roman01.map"), 0, &text), t.get(3363).unwrap());
        assert_eq!(goal(Some((11, 0)), &map("Campaign/AO_roman1.map"), 0, &text), t.get(410).unwrap());
        // roman01 without its campaign: the description has no '@'.
        assert_eq!(goal(None, &map("Campaign/roman01.map"), 0, &text), "");
    }
}
