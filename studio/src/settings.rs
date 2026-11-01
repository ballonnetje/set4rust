//! What the app remembers between runs: `settings.json` in the data folder.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::layers::LayerState;
use crate::pack::write_atomic;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub dark: bool,
    pub layers: Vec<LayerState>,
    pub window: Option<[f32; 2]>,
    pub sidebar: f32,
    pub inspector: f32,
    /// `"group/slot"` last open.
    pub last_slot: Option<String>,
    pub zoom: u32,
    pub team: usize,
    pub volume: f32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { dark: true, layers: Vec::new(), window: None, sidebar: 320.0, inspector: 320.0, last_slot: None, zoom: 2, team: 1, volume: 1.0 }
    }
}

impl Settings {
    pub fn load(data: &Path) -> Settings {
        std::fs::read(data.join("settings.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Best effort: losing a window size is no reason to bother anyone.
    pub fn save(&self, data: &Path) {
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = write_atomic(&data.join("settings.json"), &b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_tolerate_old_files() {
        let d = crate::pack::tests::temp("settings");
        assert_eq!(Settings::load(&d), Settings::default());
        let s = Settings { dark: false, zoom: 4, ..Settings::default() };
        s.save(&d);
        assert_eq!(Settings::load(&d), s);
        std::fs::write(d.join("settings.json"), r#"{"zoom": 3}"#).unwrap();
        assert_eq!(Settings::load(&d).zoom, 3);
        std::fs::write(d.join("settings.json"), "nope").unwrap();
        assert_eq!(Settings::load(&d), Settings::default());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
