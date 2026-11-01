//! `GameData\objectInfo.xml` — landscape (deco) object properties.
//!
//! Reader: `CDecoObjMgr::ReadObjectInfos` (MapObjects/DecoObj/CDecoObjMgr.cpp). `object@id` is an
//! OBJECT_* define (`CConfigManager::GetDefineValue`); only ids 1..=290 are stored, unknown ids are
//! skipped with a trace ("Unknown object"). The game reads `blocking`, `building`, `repellent` and
//! `pingPong`; `animType`, `layer` and `version` are editor data it never reads.
//!
//! What it derives (8 bytes per object at `CDecoObjMgr + 38444 + 8*id`):
//! * the effective "repellent" flag is **hard-coded**: true only for object ids 162, 209, 210, 211
//!   and 255 (the XML value is just asserted equal);
//! * object 216 always gets `building = 1`;
//! * if `blocking <= 0 && repellent`: `[1, 0, 1, 1, building+1]` (a repelling, non-blocking ring),
//!   else `[0, blocking, 0, blocking, building+blocking]` — i.e. `blocking` = rings of blocked cells
//!   around the object, `building` = additional rings where no building may be placed;
//! * byte 7 = `pingPong != 0` (animation plays back and forth).
//!
//! Trees (ids 1..=22) and dark trees (23..=40) must have blocking 1 and building 1.

use super::defines::Object;
use super::xml::{atoi, child, child_int, elements, id, text};
use super::Ctx;
use crate::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectInfo {
    pub object: Object,
    /// Rings of blocked cells.
    pub blocking: i32,
    /// Additional rings in which nothing may be built.
    pub building: i32,
    /// XML `repellent` (the game uses [`ObjectInfo::game_repellent`] instead).
    pub repellent: i32,
    pub ping_pong: bool,
    // Not read by the game:
    pub anim_type: Option<i32>,
    pub layer: Option<i32>,
    pub version: i32,
}

impl ObjectInfo {
    /// The repellent flag as the game actually sets it (hard-coded object list).
    pub fn game_repellent(&self) -> bool {
        matches!(self.object as u16, 162 | 209 | 210 | 211 | 255)
    }

    /// Building rings as the game stores them (object 216 forced to 1).
    pub fn game_building(&self) -> i32 {
        if self.object as u16 == 216 { 1 } else { self.building }
    }
}

/// Returns `(ObjectXMLVersion, objects)`.
pub(crate) fn parse_objects(text_: &str, ctx: &mut Ctx) -> Result<(u32, Vec<ObjectInfo>)> {
    let doc = roxmltree::Document::parse(text_).map_err(|e| crate::Error::Format(format!("objectInfo.xml: {e}")))?;
    let mut version = 0;
    let mut out = Vec::new();
    for o in elements(doc.root_element()) {
        if o.has_tag_name("ObjectXMLVersion") {
            version = atoi(text(o)) as u32;
            continue;
        }
        let Some(object) = ctx.enum_by_define(id(o), Object::from_u16, "object") else { continue };
        let opt = |f: &str| child(o, f, 0).map(|c| atoi(text(c)));
        out.push(ObjectInfo {
            object,
            blocking: child_int(o, "blocking"),
            building: child_int(o, "building"),
            repellent: child_int(o, "repellent"),
            ping_pong: child_int(o, "pingPong") != 0,
            anim_type: opt("animType"),
            layer: opt("layer"),
            version: child_int(o, "version"),
        });
    }
    Ok((version, out))
}
