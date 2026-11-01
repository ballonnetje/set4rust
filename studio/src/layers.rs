//! The stack of layers the views draw: images, the map grid, building data.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerId {
    Anchor,
    Worker,
    Path,
    MiniFlagPx,
    SoldiersPx,
    Piles,
    Builders,
    Work,
    Flag,
    Door,
    Dig,
    Block,
    Footprint,
    Grid,
    Yours,
    Original,
}

/// How a layer's data is drawn and edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    Anchor,
    Worker,
    Path,
    Px1,
    PxN,
    Piles,
    CellN,
    Cell1,
    Area,
    Grid,
    Image,
}

pub struct LayerDef {
    pub id: LayerId,
    /// The key in the spec's `building`, for building data.
    pub key: &'static str,
    pub label: &'static str,
    pub color: [u8; 3],
    pub kind: LayerKind,
    /// Exists on buildings only.
    pub bld: bool,
    pub vis: bool,
}

const fn def(id: LayerId, key: &'static str, label: &'static str, color: [u8; 3], kind: LayerKind, bld: bool, vis: bool) -> LayerDef {
    LayerDef { id, key, label, color, kind, bld, vis }
}

/// Top first.
pub const LAYER_DEFS: [LayerDef; 16] = [
    def(LayerId::Anchor, "anchor", "Anchor (map node)", [0xff, 0x4d, 0x4d], LayerKind::Anchor, false, true),
    def(LayerId::Worker, "worker", "Worker (animated)", [0xff, 0xff, 0xff], LayerKind::Worker, true, true),
    def(LayerId::Path, "path", "Worker route", [0xff, 0xd1, 0x66], LayerKind::Path, true, true),
    def(LayerId::MiniFlagPx, "mini_flag_px", "Mini flag (px)", [0x9b, 0xe7, 0xff], LayerKind::Px1, true, true),
    def(LayerId::SoldiersPx, "soldiers_px", "Soldier slots (px)", [0xff, 0x7a, 0xa2], LayerKind::PxN, true, true),
    def(LayerId::Piles, "piles", "Piles", [0x2e, 0xc4, 0xb6], LayerKind::Piles, true, true),
    def(LayerId::Builders, "builders", "Builder spots", [0xf4, 0xf4, 0xf4], LayerKind::CellN, true, true),
    def(LayerId::Work, "work", "Work spot", [0xd6, 0x5b, 0xff], LayerKind::Cell1, true, true),
    def(LayerId::Flag, "flag", "Flag", [0x5b, 0x9c, 0xff], LayerKind::Cell1, true, true),
    def(LayerId::Door, "door", "Door", [0xf2, 0xd2, 0x2e], LayerKind::Cell1, true, true),
    def(LayerId::Dig, "dig", "Dig (levelled)", [0xc8, 0xa0, 0x60], LayerKind::Area, true, false),
    def(LayerId::Block, "block", "Blocked", [0xf0, 0x8c, 0x28], LayerKind::Area, true, true),
    def(LayerId::Footprint, "footprint", "Building area", [0x3c, 0xc8, 0x6e], LayerKind::Area, true, true),
    def(LayerId::Grid, "grid", "Map grid", [0xcf, 0xd6, 0xdd], LayerKind::Grid, false, true),
    def(LayerId::Yours, "yours", "Your image", [0x5b, 0x9c, 0xff], LayerKind::Image, false, true),
    def(LayerId::Original, "original", "Original image", [0x9a, 0xa3, 0xad], LayerKind::Image, false, true),
];

pub fn layer(id: LayerId) -> &'static LayerDef {
    LAYER_DEFS.iter().find(|d| d.id == id).expect("every layer has a definition")
}

/// The layer of a building data key.
pub fn layer_of_key(key: &str) -> Option<&'static LayerDef> {
    LAYER_DEFS.iter().find(|d| d.key == key)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerState {
    pub id: LayerId,
    #[serde(default = "yes")]
    pub vis: bool,
}

fn yes() -> bool {
    true
}

pub fn default_layers() -> Vec<LayerState> {
    LAYER_DEFS.iter().map(|d| LayerState { id: d.id, vis: d.vis }).collect()
}

/// The saved order, with layers added since it was saved at their default place.
pub fn restore_layers(saved: &[LayerState]) -> Vec<LayerState> {
    let mut out: Vec<LayerState> = Vec::new();
    for l in saved {
        if !out.iter().any(|o| o.id == l.id) {
            out.push(*l);
        }
    }
    for (i, d) in LAYER_DEFS.iter().enumerate() {
        if !out.iter().any(|o| o.id == d.id) {
            let at = i.min(out.len());
            out.insert(at, LayerState { id: d.id, vis: d.vis });
        }
    }
    out
}

/// `layers` with entry `from` moved to `to`; nothing when out of range.
pub fn move_layer(layers: &mut Vec<LayerState>, from: usize, to: usize) {
    if from >= layers.len() || to >= layers.len() || from == to {
        return;
    }
    let l = layers.remove(from);
    layers.insert(to, l);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_order_survives_and_new_layers_join() {
        let saved = [LayerState { id: LayerId::Original, vis: false }, LayerState { id: LayerId::Anchor, vis: true }, LayerState { id: LayerId::Original, vis: true }];
        let l = restore_layers(&saved);
        assert_eq!(l.len(), LAYER_DEFS.len());
        assert_eq!((l[0].id, l[0].vis), (LayerId::Original, false));
        assert_eq!(l.iter().filter(|x| x.id == LayerId::Original).count(), 1);
        assert_eq!(restore_layers(&default_layers()), default_layers());
        assert!(!default_layers().iter().find(|x| x.id == LayerId::Dig).unwrap().vis);
    }

    #[test]
    fn layers_move() {
        let mut l = default_layers();
        move_layer(&mut l, 0, 2);
        assert_eq!(l[2].id, LayerId::Anchor);
        let before = l.clone();
        move_layer(&mut l, 0, 99);
        assert_eq!(l, before);
    }

    #[test]
    fn saved_form_is_readable_json() {
        let s = serde_json::to_string(&default_layers()[3]).unwrap();
        assert_eq!(s, r#"{"id":"mini_flag_px","vis":true}"#);
        let back: LayerState = serde_json::from_str(r#"{"id":"grid"}"#).unwrap();
        assert!(back.vis);
    }
}
