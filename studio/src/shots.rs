//! The app driven headless: flows through the real UI code, and with `S4_SHOTS_DIR` set, PNGs of
//! every screen for looking at. With `S4_SHOT_SPEC` also set (an asset-spec folder) the shots use
//! the real spec, so they hold the game's art: keep them out of the repository. `export_fixture`
//! builds a varied pack on a real spec and writes exports for comparing with other tools.

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::app::App;
use crate::editor::Tool;
use crate::layers::LayerId;
use crate::model::{Spec, test_spec};
use crate::pack::tests::{png_bytes, temp};
use crate::pack::{Pack, bld_path, conf_path, frame_path, mask_path, sound_path};
use crate::pngio::encode_rgba;
use serde_json::Value;
use crate::ui::{Studio, Tab};

/// Paints the test spec's reference strips so views have something to show.
fn write_strips(spec: &Spec) {
    for g in spec.groups.iter().filter(|g| g.kind == crate::model::Kind::Gfx) {
        for q in g.slots.iter().flat_map(|s| &s.seqs) {
            let Some(rel) = &q.reference else { continue };
            let w = q.frames.iter().map(|f| f[6] + f[1]).max().unwrap_or(1) as u32;
            let h = q.frames.iter().map(|f| f[7] + f[2]).max().unwrap_or(1) as u32;
            let mut px = vec![0u8; (w * h * 4) as usize];
            for f in &q.frames {
                for y in 0..f[2] {
                    for x in 0..f[1] {
                        let o = (((f[7] + y) as u32 * w + (f[6] + x) as u32) * 4) as usize;
                        let edge = x == 0 || y == 0 || x == f[1] - 1 || y == f[2] - 1;
                        px[o..o + 4].copy_from_slice(if edge { &[240, 200, 120, 255] } else { &[120, 90, 60, 255] });
                    }
                }
            }
            let p = spec.ref_path(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, encode_rgba(w, h, &px).unwrap()).unwrap();
            if let Some(mask) = &q.mask {
                std::fs::write(spec.ref_path(mask), encode_rgba(w, h, &vec![0u8; (w * h * 4) as usize]).unwrap()).unwrap();
            }
        }
    }
}

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        Fixture { dir: temp(name) }
    }

    fn spec(&self) -> Spec {
        let s = test_spec(&self.dir.join("spec"));
        write_strips(&s);
        s
    }

    fn pack(&self) -> Pack {
        Pack::open(&self.dir.join("pack")).unwrap()
    }

    fn harness(&self, size: [f32; 2]) -> Harness<'static, App> {
        let dir = self.dir.clone();
        let (spec, pack) = (self.spec(), self.pack());
        Harness::builder().with_size(size).with_accessibility_check(false).build_eframe(move |cc| App::with_studio(&cc.egui_ctx, dir, spec, pack, true))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn steps(h: &mut Harness<'_, App>, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

fn studio<'a>(h: &'a mut Harness<'_, App>) -> &'a mut Studio {
    h.state_mut().studio()
}

fn go(h: &mut Harness<'_, App>, id: &str) {
    let s = studio(h);
    let r = s.spec.find(id).unwrap();
    s.go(Some(r));
    steps(h, 3);
}

#[test]
fn a_slot_opens_with_its_cards_and_editor() {
    let f = Fixture::new("flow0");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    go(&mut h, "buildings-roman/BUILDING_MILL");
    assert!(studio(&mut h).ed.is_some(), "a building has an editor");
    assert!(h.query_all_by_label("BUILDING_MILL").next().is_some());
    assert!(h.query_by_label_contains("0 construction").is_some());
    assert!(h.query_all_by_label_contains("Add frames").next().is_some());
    studio(&mut h).tab = Tab::Edit;
    steps(&mut h, 3);
    assert!(h.query_by_label_contains("Worker routine").is_some());
    assert!(h.query_by_label_contains("Inspector").is_some());
    assert!(h.query_by_label_contains("Gameplay data").is_some());
    go(&mut h, "gui-roman/0001");
    assert!(studio(&mut h).ed.is_none(), "GUI images have no editor");
    assert_eq!(studio(&mut h).tab, Tab::Compare);
    go(&mut h, "sounds/SND_STEP");
    assert!(h.query_by_label_contains("empty in the original").is_some());
    assert!(h.query_by_label_contains("Choose file").is_some());
}

#[test]
fn leaving_a_dirty_slot_asks_first() {
    let f = Fixture::new("flow1");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    go(&mut h, "buildings-roman/BUILDING_MILL");
    studio(&mut h).ed.as_mut().unwrap().edit("flag", None, |b| b.flag = [7, 7]);
    assert!(studio(&mut h).dirty());
    let target = studio(&mut h).spec.find("settlers-roman/JOB_X").unwrap();
    let before = studio(&mut h).sel;
    studio(&mut h).go(Some(target));
    steps(&mut h, 3);
    assert_eq!(studio(&mut h).sel, before, "still on the slot");
    assert!(studio(&mut h).modal_open());
    h.get_by_label_contains("unsaved changes. Discard them?");
    h.get_by_label("Discard").click();
    steps(&mut h, 3);
    assert_eq!(studio(&mut h).sel, Some(target));
    assert!(!studio(&mut h).dirty());
}

#[test]
fn the_keyboard_drives_the_editor() {
    let f = Fixture::new("flow2");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    go(&mut h, "buildings-roman/BUILDING_MILL");
    studio(&mut h).tab = Tab::Edit;
    steps(&mut h, 2);
    for (key, tool) in [(egui::Key::B, Tool::Paint), (egui::Key::E, Tool::Erase), (egui::Key::A, Tool::Add), (egui::Key::N, Tool::Anchor), (egui::Key::V, Tool::Select)] {
        h.key_press(key);
        steps(&mut h, 2);
        assert_eq!(studio(&mut h).ed.as_ref().unwrap().tool, tool);
    }
    // Nudge, undo, redo, save.
    studio(&mut h).ed.as_mut().unwrap().sel = Some(crate::editor::Sel { layer: LayerId::Door, index: -1 });
    h.key_press(egui::Key::ArrowRight);
    steps(&mut h, 2);
    assert_eq!(studio(&mut h).ed.as_ref().unwrap().building().unwrap().door, [1, 2]);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    steps(&mut h, 2);
    assert_eq!(studio(&mut h).ed.as_ref().unwrap().building().unwrap().door, [0, 2]);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Y);
    steps(&mut h, 2);
    assert_eq!(studio(&mut h).ed.as_ref().unwrap().building().unwrap().door, [1, 2]);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    steps(&mut h, 2);
    let s = studio(&mut h);
    let (g, slot) = s.selected().unwrap();
    assert_eq!(s.pack.file(&bld_path(g, slot)).unwrap().text.as_deref(), Some(r#"{"door":[1,2]}"#));
    assert!(!s.dirty());
    // Esc deselects, and the arrows then step frames.
    studio(&mut h).ed.as_mut().unwrap().sel = Some(crate::editor::Sel { layer: LayerId::Door, index: -1 });
    h.key_press(egui::Key::Escape);
    steps(&mut h, 2);
    assert_eq!(studio(&mut h).ed.as_ref().unwrap().sel, None);
    h.key_press(egui::Key::Space);
    steps(&mut h, 2);
    assert!(!studio(&mut h).play.playing);
    let p0 = studio(&mut h).play.pos;
    h.key_press(egui::Key::ArrowRight);
    steps(&mut h, 2);
    assert_ne!(studio(&mut h).play.pos, p0);
}

#[test]
fn j_and_k_walk_the_slots_and_slash_goes_to_search() {
    let f = Fixture::new("flow3");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    h.key_press(egui::Key::J);
    steps(&mut h, 3);
    assert!(studio(&mut h).sel.is_some());
    h.key_press(egui::Key::J);
    steps(&mut h, 3);
    let g0 = studio(&mut h).sel.unwrap().g;
    assert_eq!(g0, 0, "stays in the slot's group");
    h.key_press(egui::Key::Slash);
    steps(&mut h, 3);
    assert!(h.ctx.egui_wants_keyboard_input(), "the search box has focus");
}

#[test]
fn saving_frames_from_files_updates_the_pack_and_progress() {
    let f = Fixture::new("flow4");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    go(&mut h, "settlers-roman/JOB_X");
    let drop_dir = f.dir.join("drop");
    std::fs::create_dir_all(&drop_dir).unwrap();
    let mut files = Vec::new();
    // Eight frames on a sequence of four: the app offers 2×. Names sort numerically; masks go apart.
    for i in [10, 2, 1, 3, 4, 5, 6, 7] {
        let p = drop_dir.join(format!("run{i}.png"));
        std::fs::write(&p, png_bytes(16, 24)).unwrap();
        files.push(p);
    }
    let bmp = drop_dir.join("team_mask.bmp");
    let mut b = Vec::new();
    image::RgbaImage::from_pixel(16, 24, image::Rgba([255, 255, 255, 255])).write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Bmp).unwrap();
    std::fs::write(&bmp, b).unwrap();
    files.push(bmp);
    let s = studio(&mut h);
    let r = s.sel.unwrap();
    s.save_frames(r, 0, files);
    steps(&mut h, 3);
    assert!(studio(&mut h).modal_open(), "the count is a multiple: asks");
    h.get_by_label_contains("You dropped 8 frames: 2× the original's 4.");
    h.get_by_label("Set and save").click();
    for _ in 0..200 {
        steps(&mut h, 1);
        if studio(&mut h).pack.has("gfx/settlers-roman/JOB_X/0/007.png") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let s = studio(&mut h);
    let (g, slot) = s.selected().unwrap();
    let q = &slot.seqs[0];
    assert!(s.pack.has(&frame_path(g, slot, q, 7)));
    assert!(s.pack.file(&mask_path(g, slot, q, 0)).is_some_and(|m| m.w == 16), "a BMP mask became a PNG");
    assert_eq!(s.pack.conf(g, slot, q).multiplier(), Some(2));
    assert_eq!(s.status.slot(r).have, 8);
}

#[test]
fn a_single_image_while_paused_replaces_the_shown_frame() {
    let f = Fixture::new("flow5");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    go(&mut h, "settlers-roman/JOB_X");
    let p = f.dir.join("one.png");
    std::fs::write(&p, png_bytes(16, 24)).unwrap();
    let s = studio(&mut h);
    let r = s.sel.unwrap();
    let now = s.now();
    s.play.seek_tick(2, now);
    s.save_frames(r, 1, vec![p]);
    for _ in 0..200 {
        steps(&mut h, 1);
        if studio(&mut h).pack.has("gfx/settlers-roman/JOB_X/1/002.png") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(studio(&mut h).pack.has("gfx/settlers-roman/JOB_X/1/002.png"));
    assert!(!studio(&mut h).pack.has("gfx/settlers-roman/JOB_X/1/000.png"));
}

#[test]
fn audio_files_are_stored_as_wav_and_music_as_given() {
    let f = Fixture::new("flow6");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    go(&mut h, "music/track1");
    let track = f.dir.join("t.ogg");
    std::fs::write(&track, b"OggS..").unwrap();
    let s = studio(&mut h);
    let r = s.sel.unwrap();
    s.save_audio(r, 0, track);
    let txt = f.dir.join("t.txt");
    std::fs::write(&txt, b"x").unwrap();
    for _ in 0..200 {
        steps(&mut h, 1);
        if studio(&mut h).pack.has("music/track1.ogg") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(std::fs::read(f.dir.join("pack/music/track1.ogg")).unwrap(), b"OggS..");
    go(&mut h, "sounds/SND_STEP");
    let wav = f.dir.join("s.wav");
    std::fs::write(&wav, crate::audio::encode_wav(&[0.0; 100], 1, 22050)).unwrap();
    let s = studio(&mut h);
    let r = s.sel.unwrap();
    s.save_audio(r, 0, wav);
    for _ in 0..200 {
        steps(&mut h, 1);
        if studio(&mut h).pack.has("snd/SND_STEP/00.wav") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let s = studio(&mut h);
    let (_, slot) = s.selected().unwrap();
    assert!(s.pack.has(&sound_path(slot, &slot.seqs[0])));
}

#[test]
fn layer_order_and_theme_are_remembered() {
    let f = Fixture::new("flow7");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    let s = studio(&mut h);
    crate::layers::move_layer(&mut s.layers, 0, 3);
    s.layers[1].vis = false;
    s.store_settings();
    let want = s.layers.clone();
    drop(h);
    let saved = crate::settings::Settings::load(&f.dir);
    assert_eq!(crate::layers::restore_layers(&saved.layers), want);
}

#[test]
fn export_dialog_opens_on_the_slot() {
    let f = Fixture::new("flow8");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    go(&mut h, "buildings-roman/BUILDING_MILL");
    studio(&mut h).defer(Studio::open_export);
    steps(&mut h, 3);
    assert!(h.query_by_label_contains("This slot: BUILDING_MILL").is_some());
    assert!(h.query_by_label_contains("Sprite sheets per sequence").is_some());
    assert!(h.query_by_label_contains("1 finished (1 frame)").is_some());
}

fn to_screen(h: &mut Harness<'_, App>, w: [f32; 2]) -> egui::Pos2 {
    let [ox, oy, z] = studio(h).xf;
    egui::pos2(ox + w[0] * z, oy + w[1] * z)
}

/// Press at `from`, move to `to` in a few steps, release.
fn drag(h: &mut Harness<'_, App>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    steps(h, 2);
    h.drag_at(from);
    steps(h, 2);
    for k in 1..=3 {
        h.hover_at(from + (to - from) * (k as f32 / 3.0));
        steps(h, 2);
    }
    h.drop_at(to);
    steps(h, 3);
}

#[test]
fn the_mouse_works_the_large_view() {
    let f = Fixture::new("flow9");
    let mut h = f.harness([1500.0, 1000.0]);
    steps(&mut h, 2);
    go(&mut h, "buildings-roman/BUILDING_MILL");
    studio(&mut h).tab = Tab::Edit;
    let now = studio(&mut h).now();
    studio(&mut h).play.set_playing(false, now);
    steps(&mut h, 3);
    // Move the door (cell (0, 2), pixel (-24, 24)) to the node at pixel (-48, 48).
    let (from, to) = (to_screen(&mut h, [-24.0, 24.0]), to_screen(&mut h, [-48.0, 48.0]));
    drag(&mut h, from, to);
    assert_eq!(studio(&mut h).ed.as_ref().unwrap().building().unwrap().door, [0, 4]);
    assert_eq!(studio(&mut h).ed.as_ref().unwrap().sel, Some(crate::editor::Sel { layer: LayerId::Door, index: -1 }));
    // Paint a stroke of cells.
    studio(&mut h).ed.as_mut().unwrap().tool = Tool::Paint;
    let (from, to) = (to_screen(&mut h, [72.0, 24.0]), to_screen(&mut h, [120.0, 24.0]));
    drag(&mut h, from, to);
    let fp = studio(&mut h).ed.as_ref().unwrap().building().unwrap().footprint.clone();
    assert!(fp.contains(&[4, 2]) && fp.contains(&[6, 2]), "{fp:?}");
    assert!(studio(&mut h).dirty());
    // Add a builder spot with a click.
    {
        let ed = studio(&mut h).ed.as_mut().unwrap();
        ed.tool = Tool::Add;
        ed.add = LayerId::Builders;
    }
    let at = to_screen(&mut h, [-96.0, 0.0]);
    drag(&mut h, at, at);
    assert_eq!(studio(&mut h).ed.as_ref().unwrap().building().unwrap().builders.last().copied(), Some([-4, 0, 0]));
    // The wheel zooms around the pointer: the world point under it stays put.
    studio(&mut h).ed.as_mut().unwrap().tool = Tool::Select;
    let at = to_screen(&mut h, [40.0, 40.0]);
    let before = studio(&mut h).xf;
    h.hover_at(at);
    steps(&mut h, 2);
    h.event(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 120.0), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: egui::Modifiers::NONE });
    steps(&mut h, 12);
    let after = studio(&mut h).xf;
    assert!(after[2] > before[2], "{before:?} -> {after:?}");
    let moved = to_screen(&mut h, [40.0, 40.0]);
    assert!((moved - at).length() < 3.0, "{at:?} -> {moved:?}");
    // The middle button pans.
    let before = studio(&mut h).view.pan;
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Middle, pressed: true, modifiers: egui::Modifiers::NONE });
    steps(&mut h, 2);
    h.hover_at(at + egui::vec2(30.0, 10.0));
    steps(&mut h, 2);
    h.event(egui::Event::PointerButton { pos: at + egui::vec2(30.0, 10.0), button: egui::PointerButton::Middle, pressed: false, modifiers: egui::Modifiers::NONE });
    steps(&mut h, 2);
    assert_ne!(studio(&mut h).view.pan, before);
    // Set anchor needs a frame of yours.
    studio(&mut h).ed.as_mut().unwrap().tool = Tool::Anchor;
    h.hover_at(at);
    steps(&mut h, 2);
    h.drag_at(at);
    steps(&mut h, 2);
    h.drop_at(at);
    steps(&mut h, 2);
    h.get_by_label_contains("No frame of yours here");
}

#[test]
fn layers_reorder_by_dragging_the_grip() {
    let f = Fixture::new("flow10");
    let mut h = f.harness([1500.0, 1000.0]);
    steps(&mut h, 2);
    go(&mut h, "buildings-roman/BUILDING_MILL");
    studio(&mut h).tab = Tab::Edit;
    steps(&mut h, 3);
    let order = studio(&mut h).layers.clone();
    let rects: Vec<egui::Rect> = h.query_all_by_label(crate::theme::icon::DOTS_SIX_VERTICAL).map(|n| n.rect()).collect();
    assert_eq!(rects.len(), order.len());
    drag(&mut h, rects[0].center(), rects[3].center());
    let now = studio(&mut h).layers.clone();
    assert_eq!(now[3].id, order[0].id, "the top layer went to the fourth place");
    assert_eq!(now[0].id, order[1].id);
}

#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

#[test]
fn dropped_files_ask_for_the_sequence_then_save() {
    let f = Fixture::new("flow11");
    let mut h = f.harness([1500.0, 1000.0]);
    steps(&mut h, 2);
    go(&mut h, "settlers-roman/JOB_X");
    let mut dropped: Vec<egui::DroppedFileHandle> = Vec::new();
    for i in 0..4 {
        let p = f.dir.join(format!("d{i}.png"));
        std::fs::write(&p, png_bytes(16, 24)).unwrap();
        dropped.push(std::sync::Arc::new(Dropped(p)));
    }
    h.input_mut().dropped_files = dropped;
    steps(&mut h, 3);
    assert!(studio(&mut h).drop_choice.is_some());
    h.get_by_label_contains("direction 1 (4 frames)").click();
    steps(&mut h, 2);
    h.get_by_label("Add").click();
    for _ in 0..200 {
        steps(&mut h, 1);
        if studio(&mut h).pack.has("gfx/settlers-roman/JOB_X/1/003.png") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(studio(&mut h).pack.has("gfx/settlers-roman/JOB_X/1/003.png"));
    assert!(!studio(&mut h).pack.has("gfx/settlers-roman/JOB_X/0/000.png"));
}

#[test]
fn the_theme_button_changes_the_look_and_stays() {
    let f = Fixture::new("flow12");
    let mut h = f.harness([1500.0, 1000.0]);
    steps(&mut h, 2);
    assert!(crate::theme::palette(&h.ctx).dark);
    h.get_by_label(crate::theme::icon::SUN).click();
    steps(&mut h, 3);
    assert!(!crate::theme::palette(&h.ctx).dark);
    assert!(!studio(&mut h).settings.dark);
    studio(&mut h).store_settings();
    assert!(!crate::settings::Settings::load(&f.dir).dark);
}

/// On a machine with an audio device, with `S4_AUDIO_TEST` set to a sound or music file.
#[test]
fn real_audio_plays_and_seeks() {
    let Some(path) = std::env::var_os("S4_AUDIO_TEST").map(PathBuf::from) else { return };
    let mut a = crate::audio::Audio::default();
    a.toggle(&path, 0.05).unwrap();
    assert!(a.is_playing(&path));
    std::thread::sleep(std::time::Duration::from_millis(600));
    let (p0, total) = a.position(&path).unwrap();
    println!("position {p0:?} of {total:?}");
    assert!(p0 > std::time::Duration::from_millis(200), "{p0:?}");
    a.toggle(&path, 0.05).unwrap();
    assert!(!a.is_playing(&path) && a.is_current(&path), "paused");
    a.toggle(&path, 0.05).unwrap();
    assert!(a.is_playing(&path), "resumed");
    println!("seek: {:?}", a.seek(std::time::Duration::from_millis(100)));
    std::thread::sleep(std::time::Duration::from_millis(200));
    println!("after the seek {:?}", a.position(&path).unwrap().0);
    a.stop();
    assert!(!a.any_playing());
}

/// Builds a varied pack on the real spec and writes the native exports, for the comparison with
/// the web export (scratch `diff/run.mjs`). Needs `S4_EXPORT_DIR`, `S4_SHOT_SPEC` and `S4_DIR`.
#[test]
fn export_fixture() {
    use crate::export::{Job, Options, Progress, Scope, run, slot_list};
    use crate::model::{F_HALF, Kind};
    use std::sync::Arc;
    let (Some(out), Some(spec_dir)) = (std::env::var_os("S4_EXPORT_DIR").map(PathBuf::from), std::env::var_os("S4_SHOT_SPEC").map(PathBuf::from)) else { return };
    let game = PathBuf::from(std::env::var_os("S4_DIR").unwrap());
    let spec = Arc::new(Spec::load(&spec_dir, &game).unwrap());
    let _ = std::fs::remove_dir_all(&out);
    let mut pack = Pack::open(&out.join("pack")).unwrap();
    let frame = |w: i32, h: i32| encode_rgba(w.max(1) as u32, h.max(1) as u32, &vec![200u8; (w.max(1) * h.max(1) * 4) as usize]).unwrap();
    // A settler job: 2× with masks on some frames; the next direction with per-frame anchors.
    let (g, slot) = spec.groups.iter().find(|g| g.id == "settlers-roman").map(|g| (g, g.slots.iter().find(|s| s.sound.is_some() && s.seqs.len() == 6 && s.seqs[0].frames.len() > 3 && s.seqs[0].mask.is_some()).unwrap())).unwrap();
    let q = &slot.seqs[0];
    for j in 0..q.frames.len() * 2 {
        let fr = q.frames[j / 2];
        pack.put(&frame_path(g, slot, q, j), &frame(fr[1], fr[2])).unwrap();
        if j % 3 == 0 {
            pack.put(&mask_path(g, slot, q, j), &frame(fr[1], fr[2])).unwrap();
        }
    }
    pack.put(&conf_path(g, slot, q), br#"{"frame_multiplier":2,"note":"keep"}"#).unwrap();
    let q = &slot.seqs[1];
    let anchors: Vec<[f64; 2]> = (0..q.frames.len()).map(|i| [i as f64 + 0.5, 3.0]).collect();
    pack.put(&conf_path(g, slot, q), json!({"anchors": anchors}).to_string().as_bytes()).unwrap();
    for j in 0..q.frames.len() {
        pack.put(&frame_path(g, slot, q, j), &frame(40, 50)).unwrap();
    }
    // A building: a changed copy of the gameplay data, a common anchor, odd cases of each key.
    let (g, slot) = spec.groups.iter().find(|g| g.id == "buildings-roman").map(|g| (g, g.slots.iter().find(|s| s.building.as_ref().is_some_and(|b| !b.work_lists.is_empty() && !b.piles.is_empty())).unwrap())).unwrap();
    let mut b = (**slot.building.as_ref().unwrap()).clone();
    b.flag = [b.flag[0] + 1, b.flag[1] - 1];
    b.footprint.push([20, 20]);
    b.piles[0].good = None;
    b.mini_flag_px = None;
    b.work = Some([1, 2]);
    b.builders.push([1, 1, 3]);
    b.soldiers_px.push([1, 2, 3, 1]);
    pack.put_building(g, slot, &b).unwrap();
    for q in &slot.seqs {
        for (j, fr) in q.frames.iter().enumerate() {
            pack.put(&frame_path(g, slot, q, j), &frame(fr[1], fr[2])).unwrap();
        }
    }
    pack.put(&conf_path(g, slot, &slot.seqs[0]), br#"{"anchor":[12,34]}"#).unwrap();
    // A building with a sound on a patch.
    let (g, slot) = spec.groups.iter().filter(|g| g.id.starts_with("buildings-")).find_map(|g| g.slots.iter().find(|s| s.seqs.iter().any(|q| q.patch_sound().is_some())).map(|s| (g, s))).unwrap();
    let q = slot.seqs.iter().find(|q| q.patch_sound().is_some()).unwrap();
    for (j, fr) in q.frames.iter().enumerate() {
        pack.put(&frame_path(g, slot, q, j), &frame(fr[1], fr[2])).unwrap();
    }
    // Half-size frames: anchors of .5.
    let (g, slot, q) = spec.groups.iter().filter(|g| g.kind == Kind::Gfx).find_map(|g| g.slots.iter().find_map(|s| s.seqs.iter().find(|q| q.frames.iter().any(|f| f[5] & F_HALF != 0 && f[3] % 2 == 1)).map(|q| (g, s, q)))).unwrap();
    for (j, fr) in q.frames.iter().enumerate() {
        pack.put(&frame_path(g, slot, q, j), &frame(fr[1] / 2, fr[2] / 2)).unwrap();
    }
    // A GUI image, a sound, a track.
    let g = spec.group("gui-roman").unwrap();
    let slot = g.slots.iter().find(|s| !s.uses.is_empty()).unwrap();
    let fr = slot.seqs[0].frames[0];
    pack.put(&frame_path(g, slot, &slot.seqs[0], 0), &frame(fr[1], fr[2])).unwrap();
    let g = spec.group("sounds").unwrap();
    let slot = g.slots.iter().find(|s| s.seqs.len() > 1).unwrap();
    pack.put(&sound_path(slot, &slot.seqs[0]), &crate::audio::encode_wav(&[0.0; 50], 1, 22050)).unwrap();
    let g = spec.group("music").unwrap();
    pack.put(&format!("music/{}.ogg", g.slots[0].id), b"OggS").unwrap();

    // Things validation must complain about.
    let (g, slot) = spec.groups.iter().find(|g| g.id == "settlers-viking").map(|g| (g, g.slots.iter().find(|s| s.seqs.len() == 6 && s.seqs[0].frames.len() >= 8 && s.seqs[0].mask.is_some()).unwrap())).unwrap();
    let n = slot.seqs[0].frames.len();
    for j in [0, 1, 2, 3, n, n + 1, 2 * n - 1] {
        pack.put(&frame_path(g, slot, &slot.seqs[0], j), &png_bytes(5, 6)).unwrap();
    }
    pack.put(&mask_path(g, slot, &slot.seqs[0], 1), &png_bytes(7, 7)).unwrap();
    for j in [0, n + 3] {
        pack.put(&frame_path(g, slot, &slot.seqs[1], j), &png_bytes(5, 6)).unwrap();
    }
    pack.put(&conf_path(g, slot, &slot.seqs[2]), b"{nope").unwrap();
    pack.put(&frame_path(g, slot, &slot.seqs[2], 0), &png_bytes(5, 6)).unwrap();
    pack.put(&conf_path(g, slot, &slot.seqs[3]), br#"{"frame_multiplier":11}"#).unwrap();
    pack.put(&frame_path(g, slot, &slot.seqs[3], 0), &png_bytes(5, 6)).unwrap();
    pack.put(&conf_path(g, slot, &slot.seqs[4]), br#"{"frame_multiplier":3}"#).unwrap();
    pack.put(&frame_path(g, slot, &slot.seqs[4], 3 * n + 2), &png_bytes(5, 6)).unwrap();
    let g = spec.group("gui-viking").unwrap();
    let slot = g.slots.iter().find(|s| s.exact()).unwrap();
    pack.put(&frame_path(g, slot, &slot.seqs[0], 0), &png_bytes(5, 6)).unwrap();
    pack.put(&frame_path(g, slot, &slot.seqs[0], 1), &png_bytes(5, 6)).unwrap();
    let g = spec.group("objects").unwrap();
    let slot = g.slots.iter().find(|s| s.seqs[0].frames.len() > 3).unwrap();
    for j in 0..slot.seqs[0].frames.len() + 2 {
        pack.put(&frame_path(g, slot, &slot.seqs[0], j), &png_bytes(5, 6)).unwrap();
    }

    // The logic the web page also computes, for comparing.
    let logic = |pack: &Pack, name: &str| {
        let mut status = serde_json::Map::new();
        let (mut routine, mut boxes) = (serde_json::Map::new(), serde_json::Map::new());
        for g in &spec.groups {
            for slot in &g.slots {
                let id = format!("{}/{}", g.id, slot.id);
                let s = crate::status::slot_status(pack, g, slot);
                let state = match s.state {
                    crate::status::State::Done => "done",
                    crate::status::State::Partial => "partial",
                    crate::status::State::Problem => "problem",
                    crate::status::State::Missing => "missing",
                };
                status.insert(id.clone(), json!([s.need, s.have, state]));
                for q in &slot.seqs {
                    let st = crate::status::seq_status(pack, g, slot, q, pack.conf(g, slot, q));
                    let probs: Vec<_> = st.probs.iter().map(|p| json!([format!("{:?}", p.level).to_lowercase(), p.msg])).collect();
                    status.insert(format!("{id}#{}", q.key), json!([st.need, st.have, probs]));
                    if g.kind == Kind::Gfx {
                        let b = pack.saved_building(g, slot).map(|x| x.0);
                        let bx = crate::geometry::seq_box(q, b.as_ref());
                        boxes.insert(format!("{id}#{}", q.key), json!([bx.x0, bx.y0, bx.w, bx.h, crate::geometry::card_zoom(2.0, &bx)]));
                    }
                }
                if let Some(b) = &slot.building {
                    for wl in b.work_lists.iter() {
                        let rows: Vec<_> = (0..wl.ticks).map(|t| {
                            let s = crate::geometry::routine_at(wl, t as i64).unwrap();
                            json!([s.i, s.dir, s.frame, s.pos[0], s.pos[1], s.t])
                        }).collect();
                        routine.insert(format!("{id}/{}", wl.name), Value::Array(rows));
                    }
                }
            }
        }
        let snap: Vec<_> = (0..4000).map(|i| {
            let (x, y) = ((i * 7 % 613) as f64 * 0.37 - 110.0, (i * 11 % 509) as f64 * 0.29 - 70.0);
            let c = crate::geometry::snap_cell(x, y);
            json!([x, y, c[0], c[1]])
        }).collect();
        std::fs::write(out.join(name), Value::Object([("status".to_string(), Value::Object(status)), ("routine".to_string(), Value::Object(routine)), ("boxes".to_string(), Value::Object(boxes)), ("snap".to_string(), Value::Array(snap))].into_iter().collect()).to_string()).unwrap();
    };
    let export = |pack: &Pack, name: &str, scope: Scope, group: usize, reference: bool, keys: Option<&[&str]>| {
        let opt = Options { images: true, sheets: true, data: true, csv: true, sounds: true, empty: false, reference, team: 2, seq_keys: keys.map(|k| k.iter().map(|s| s.to_string()).collect()) };
        let list = slot_list(&spec, pack, scope, group, None, &opt);
        let job = Job { spec: spec.clone(), pack: Arc::new(pack.clone()), scope, list, opt, out: out.join(name) };
        let t = std::time::Instant::now();
        let s = run(&job, &Progress::default()).unwrap();
        println!("{name}: {} in {:.2} s", s.label(), t.elapsed().as_secs_f64());
    };
    logic(&pack, "logic-a.json");
    export(&pack, "a.zip", Scope::All, 0, false, None);
    export(&pack, "b.zip", Scope::Pack, 0, false, Some(&["0", "1"]));
    let group = spec.group_index("settlers-roman").unwrap();
    export(&pack, "c.zip", Scope::Group, group, true, None);
    // The same pack at 2× pixels and 2 frames by default.
    pack.set_meta("scale", 2.into()).unwrap();
    pack.set_meta("frame_multiplier", 2.into()).unwrap();
    logic(&pack, "logic-d.json");
    export(&pack, "d.zip", Scope::All, 0, false, None);
}

#[test]
fn a_folder_without_the_game_is_refused_on_the_first_run_screen() {
    let f = Fixture::new("flow13");
    let data = f.dir.clone();
    let mut h: Harness<'static, App> = Harness::builder().with_size([1280.0, 800.0]).with_accessibility_check(false).build_eframe(move |cc| App::first_run(&cc.egui_ctx, data, None));
    steps(&mut h, 3);
    h.get_by_label_contains("Choose the game folder");
    let (ctx, nope) = (h.ctx.clone(), f.dir.join("nope"));
    std::fs::create_dir_all(&nope).unwrap();
    h.state_mut().folder_chosen(&ctx, nope);
    steps(&mut h, 3);
    let text = h.state().problem().unwrap().to_string();
    assert!(text.contains("it lacks S4_Main.exe") && text.contains("Choose the folder"), "{text}");
    h.get_by_label_contains("it lacks S4_Main.exe");
    assert!(!f.dir.join("game_dir.txt").exists(), "nothing is remembered");
}

/// The dialog's Export button (the header has another, with an icon in its label).
fn press_export(h: &mut Harness<'_, App>) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Export").click();
    steps(h, 3);
}

/// A slot with a frame, and the export dialog open on it.
fn export_ready(f: &Fixture, h: &mut Harness<'_, App>) {
    steps(h, 2);
    go(h, "settlers-roman/JOB_X");
    let s = studio(h);
    let (g, slot) = s.selected().unwrap();
    let rel = frame_path(g, slot, &slot.seqs[0], 0);
    std::fs::create_dir_all(f.dir.join("pack").join("gfx/settlers-roman/JOB_X/0")).unwrap();
    std::fs::write(f.dir.join("pack").join(&rel), png_bytes(16, 24)).unwrap();
    s.defer(Studio::open_export);
    steps(h, 3);
}

fn busy(h: &mut Harness<'_, App>) -> bool {
    studio(h).export.as_ref().unwrap().busy()
}

/// Runs until the export is done; the status line.
fn export_status(h: &mut Harness<'_, App>) -> String {
    for _ in 0..400 {
        steps(h, 1);
        if !busy(h) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    studio(h).export.as_ref().unwrap().status().to_string()
}

#[test]
fn cancelling_the_save_dialog_leaves_export_usable() {
    let f = Fixture::new("flow14");
    let mut h = f.harness([1400.0, 900.0]);
    export_ready(&f, &mut h);
    assert!(!busy(&mut h));
    press_export(&mut h);
    assert!(busy(&mut h), "the save dialog is up");
    assert!(studio(&mut h).dialog_open());
    h.get_by_label_contains("Waiting for the save dialog");
    // Cancel: an empty answer.
    studio(&mut h).dialog_tx.take().unwrap().send(Vec::new()).unwrap();
    steps(&mut h, 3);
    assert!(!busy(&mut h), "cancelling the save dialog ends the wait");
    assert!(!studio(&mut h).dialog_open());
    // Export works again, to the end.
    press_export(&mut h);
    assert!(busy(&mut h) && studio(&mut h).dialog_open(), "Export reacts after a cancel");
    let out = f.dir.join("again.zip");
    studio(&mut h).dialog_tx.take().unwrap().send(vec![out.clone()]).unwrap();
    let status = export_status(&mut h);
    assert!(status.starts_with("Done: 1 slot"), "{status}");
    assert!(out.is_file());
}

#[test]
fn export_is_not_left_busy_when_another_dialog_is_up() {
    let f = Fixture::new("flow15");
    let mut h = f.harness([1400.0, 900.0]);
    export_ready(&f, &mut h);
    studio(&mut h).pick_files("Add frames", &crate::ui::IMAGE_EXT, true, |_, _| {});
    assert!(studio(&mut h).dialog_open());
    press_export(&mut h);
    assert!(!busy(&mut h), "no save dialog opened, so nothing is waited for");
    assert_eq!(studio(&mut h).toast_text(), Some("Close the open file dialog first."));
    // The other dialog goes: Export opens its own.
    studio(&mut h).dialog_tx.take().unwrap().send(Vec::new()).unwrap();
    steps(&mut h, 3);
    press_export(&mut h);
    assert!(busy(&mut h) && studio(&mut h).dialog_open());
}

#[test]
fn a_cancelled_file_picker_calls_nobody() {
    let f = Fixture::new("flow16");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    go(&mut h, "settlers-roman/JOB_X");
    let r = studio(&mut h).sel.unwrap();
    studio(&mut h).pick_files("Add frames", &crate::ui::IMAGE_EXT, true, move |st, files| st.save_frames(r, 0, files));
    studio(&mut h).dialog_tx.take().unwrap().send(Vec::new()).unwrap();
    steps(&mut h, 3);
    assert!(!studio(&mut h).dialog_open());
    assert_eq!(studio(&mut h).toast_text(), None, "a cancel is no \"No image files.\"");
}

#[test]
fn a_dialog_whose_thread_died_does_not_block_the_next() {
    let f = Fixture::new("flow17");
    let mut h = f.harness([1400.0, 900.0]);
    export_ready(&f, &mut h);
    press_export(&mut h);
    assert!(busy(&mut h));
    studio(&mut h).dialog_tx = None;
    steps(&mut h, 3);
    assert!(!busy(&mut h) && !studio(&mut h).dialog_open());
    press_export(&mut h);
    assert!(busy(&mut h) && studio(&mut h).dialog_open());
}

#[test]
fn an_export_whose_thread_died_can_be_closed() {
    let f = Fixture::new("flow18");
    let mut h = f.harness([1400.0, 900.0]);
    export_ready(&f, &mut h);
    studio(&mut h).export.as_mut().unwrap().fake_dead_export();
    steps(&mut h, 3);
    assert!(!busy(&mut h));
    assert!(studio(&mut h).export.as_ref().unwrap().status().starts_with("Export failed"));
    h.get_by_label("Close").click();
    steps(&mut h, 3);
    assert!(studio(&mut h).export.is_none());
}

#[test]
fn a_job_whose_thread_died_ends_with_a_message() {
    let f = Fixture::new("flow19");
    let mut h = f.harness([1400.0, 900.0]);
    steps(&mut h, 2);
    studio(&mut h).spawn("Saving", || panic!("expected by this test"));
    for _ in 0..200 {
        steps(&mut h, 1);
        if !studio(&mut h).job_running() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!studio(&mut h).job_running());
    assert!(studio(&mut h).toast_text().is_some_and(|t| t.contains("Saving")), "{:?}", studio(&mut h).toast_text());
}

#[test]
fn the_folder_picker_can_be_cancelled_and_can_die() {
    let f = Fixture::new("flow20");
    let data = f.dir.clone();
    let mut h: Harness<'static, App> = Harness::builder().with_size([1280.0, 800.0]).with_accessibility_check(false).build_eframe(move |cc| App::first_run(&cc.egui_ctx, data, None));
    steps(&mut h, 3);
    let tx = h.state_mut().fake_picking();
    assert!(h.state().picking());
    tx.send(None).unwrap();
    steps(&mut h, 3);
    assert!(!h.state().picking(), "cancelled");
    let tx = h.state_mut().fake_picking();
    drop(tx);
    steps(&mut h, 3);
    assert!(!h.state().picking(), "the thread died");
}

#[test]
fn a_spec_writer_that_died_ends_on_the_failure_screen() {
    let f = Fixture::new("flow21");
    let data = f.dir.clone();
    let mut h: Harness<'static, App> = Harness::builder().with_size([1280.0, 800.0]).with_accessibility_check(false).build_eframe(move |cc| App::first_run(&cc.egui_ctx, data, None));
    steps(&mut h, 3);
    h.state_mut().fake_dead_prepare();
    steps(&mut h, 3);
    assert!(h.state().failed().is_some_and(|m| m.contains("stopped")), "{:?}", h.state().failed());
}

/// Timings on the real spec with every frame of the game "replaced". Needs `S4_SHOT_SPEC`.
#[test]
fn perf() {
    use crate::pack::{PackFile, frame_path, seq_dir};
    use std::time::Instant;
    let Some(dir) = std::env::var_os("S4_SHOT_SPEC").map(PathBuf::from) else { return };
    let game = PathBuf::from(std::env::var_os("S4_DIR").unwrap_or_default());
    let t = Instant::now();
    let spec = Spec::load(&dir, &game).unwrap();
    println!("spec load: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let f = Fixture::new("perf");
    let mut pack = f.pack();
    let mut frames = 0;
    for g in spec.groups.iter().filter(|g| g.kind == crate::model::Kind::Gfx) {
        for slot in &g.slots {
            for q in &slot.seqs {
                let d = seq_dir(g, slot, q);
                let mut names = Vec::with_capacity(q.frames.len());
                for (j, fr) in q.frames.iter().enumerate() {
                    pack.files.insert(frame_path(g, slot, q, j), PackFile { stamp: 1, w: fr[1] as u32, h: fr[2] as u32, text: None });
                    names.push(format!("{j:03}.png"));
                    frames += 1;
                }
                pack.dirs.insert(d, names);
            }
        }
    }
    let t = Instant::now();
    let st = crate::status::Statuses::compute(&spec, &pack);
    println!("status of {frames} frames, {} have: {:.1} ms", st.have, t.elapsed().as_secs_f64() * 1000.0);
    let d = f.dir.clone();
    let mut h: Harness<'static, App> = Harness::builder().with_size([1600.0, 1000.0]).with_accessibility_check(false).build_eframe(move |cc| App::with_studio(&cc.egui_ctx, d, spec, pack, true));
    steps(&mut h, 3);
    let s = studio(&mut h);
    s.open.extend(s.spec.categories.iter().map(|c| c.id.clone()));
    s.open.extend(s.spec.groups.iter().map(|g| g.id.clone()));
    s.rows_dirty = true;
    steps(&mut h, 2);
    let rows = studio(&mut h).rows.len();
    let time = |h: &mut Harness<'_, App>, name: &str| {
        let t = Instant::now();
        steps(h, 30);
        println!("{name}: {:.2} ms per UI pass", t.elapsed().as_secs_f64() * 1000.0 / 30.0);
    };
    time(&mut h, &format!("overview, tree of {rows} rows"));
    let s = studio(&mut h);
    let big = (0..s.spec.groups.len()).flat_map(|g| (0..s.spec.groups[g].slots.len()).map(move |i| crate::model::SlotRef { g, s: i })).max_by_key(|r| s.spec.slot(*r).1.seqs.iter().map(|q| q.frames.len()).sum::<usize>()).unwrap();
    let name = s.spec.slot(big).1.name.clone();
    s.go(Some(big));
    steps(&mut h, 2);
    time(&mut h, &format!("slot {name}, compare"));
    studio(&mut h).tab = Tab::Edit;
    steps(&mut h, 2);
    time(&mut h, "slot, editor");
    let s = studio(&mut h);
    let t = Instant::now();
    s.search = "walk".to_string();
    s.rows_dirty = true;
    steps(&mut h, 1);
    println!("search pass: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    let t = Instant::now();
    studio(&mut h).pack_changed();
    println!("pack_changed: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
}

/// `dir/name.png` of the app as it stands.
fn save(h: &mut Harness<'_, App>, dir: &Path, name: &str) {
    let img = h.render().expect("render");
    let (w, hh) = (img.width(), img.height());
    std::fs::write(dir.join(format!("{name}.png")), encode_rgba(w, hh, img.as_raw()).unwrap()).unwrap();
}

/// Steps until the workers are done.
fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..600 {
        h.step();
        if studio(h).images_pending() == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    steps(h, 4);
}

fn shot_harness(dir: PathBuf, spec: Spec, pack: Pack, dark: bool, size: [f32; 2]) -> Harness<'static, App> {
    let theme = if dark { egui::Theme::Dark } else { egui::Theme::Light };
    Harness::builder().with_size(size).with_theme(theme).with_accessibility_check(false).wgpu().with_max_steps(100_000).build_eframe(move |cc| App::with_studio(&cc.egui_ctx, dir, spec, pack, dark))
}

/// Fills a scratch pack so the shots show replaced frames, masks, problems and building changes.
fn fill_pack(spec: &Spec, pack: &mut Pack, ids: (&str, &str)) {
    let (g, slot) = spec.slot(spec.find(ids.0).unwrap());
    if let Some(q) = slot.seqs.first() {
        for (i, fr) in q.frames.iter().enumerate() {
            let (w, h) = (fr[1].max(1) as u32, fr[2].max(1) as u32);
            let mut px = vec![0u8; (w * h * 4) as usize];
            for (o, c) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (x, y) = (o as u32 % w, o as u32 / w);
                let on = (x + y + i as u32 * 3) % 9 < 5;
                c.copy_from_slice(if on { &[110, 180, 240, 255] } else { &[60, 90, 130, 200] });
            }
            pack.put(&frame_path(g, slot, q, i), &encode_rgba(w, h, &px).unwrap()).unwrap();
        }
    }
    let (g, slot) = spec.slot(spec.find(ids.1).unwrap());
    if let Some(q) = slot.seqs.first() {
        pack.put(&frame_path(g, slot, q, 0), &png_bytes(3, 3)).unwrap();
        pack.put(&frame_path(g, slot, q, q.frames.len() + 2), &png_bytes(3, 3)).unwrap();
    }
    pack.put(&conf_path(g, slot, &slot.seqs[0]), json!({"anchor": [1, 1]}).to_string().as_bytes()).unwrap();
}

#[test]
fn shots() {
    let Some(out) = std::env::var_os("S4_SHOTS_DIR").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    let f = Fixture::new("shots");
    let real = std::env::var_os("S4_SHOT_SPEC").map(PathBuf::from);
    let game = std::env::var_os("S4_DIR").map(PathBuf::from).unwrap_or_default();
    let spec_of = || match &real {
        Some(d) => Spec::load(d, &game).unwrap(),
        None => f.spec(),
    };
    let (building, job, gui, sound, music) = if real.is_some() { ("buildings-roman/BUILDING_SAWMILL", "settlers-roman/JOB_SETTLER_CARRIER_WALK", "gui-roman/0100", "sounds/SND_GUI_CLICK", "music/track1") } else { ("buildings-roman/BUILDING_MILL", "settlers-roman/JOB_X", "gui-roman/0001", "sounds/SND_STEP", "music/track1") };
    let spec = spec_of();
    // Real ids differ per install of the spec: fall back to the first slot of the group.
    let pick = |id: &str| -> String {
        if spec.find(id).is_some() {
            return id.to_string();
        }
        let gid = id.split('/').next().unwrap();
        let g = spec.group(gid).expect("group");
        let s = g.slots.iter().find(|s| s.building.as_ref().is_some_and(|b| !b.work_lists.is_empty())).or(g.slots.iter().find(|s| s.seqs.len() > 1 && s.seqs.iter().any(|q| q.mask.is_some() && q.frames.len() > 4))).or(g.slots.first()).expect("slot");
        format!("{gid}/{}", s.id)
    };
    let (building, job, gui, sound, music) = (pick(building), pick(job), pick(gui), pick(sound), pick(music));
    let mut pack = f.pack();
    fill_pack(&spec, &mut pack, (&building, &job));
    let (g, slot) = spec.slot(spec.find(&building).unwrap());
    if let Some(b) = &slot.building {
        let mut nb = (**b).clone();
        nb.flag = [nb.flag[0] + 1, nb.flag[1]];
        nb.footprint.push([6, 6]);
        pack.put_building(g, slot, &nb).unwrap();
    }

    // First-run screens.
    {
        let dir = f.dir.join("first");
        std::fs::create_dir_all(&dir).unwrap();
        let mut h: Harness<'_, App> = Harness::builder().with_size([1280.0, 800.0]).with_accessibility_check(false).wgpu().build_eframe(move |cc| App::first_run(&cc.egui_ctx, dir, None));
        steps(&mut h, 3);
        let mut img = h.render().unwrap();
        std::fs::write(out.join("first-run.png"), encode_rgba(img.width(), img.height(), img.as_raw()).unwrap()).unwrap();
        let dir = f.dir.join("first2");
        std::fs::create_dir_all(&dir).unwrap();
        let mut h: Harness<'_, App> = Harness::builder().with_size([1280.0, 800.0]).with_accessibility_check(false).wgpu().build_eframe(move |cc| App::first_run(&cc.egui_ctx, dir, Some("D:\\Games\\nope\nis not a The Settlers IV History Edition folder: it lacks S4_Main.exe, Gfx, Snd.\nChoose the folder that holds S4_Main.exe.".to_string())));
        steps(&mut h, 3);
        img = h.render().unwrap();
        std::fs::write(out.join("first-run-error.png"), encode_rgba(img.width(), img.height(), img.as_raw()).unwrap()).unwrap();
    }

    let open_tree = |h: &mut Harness<'_, App>| {
        let s = studio(h);
        s.open.insert("settlers".to_string());
        s.open.insert("settlers-roman".to_string());
        s.rows_dirty = true;
    };
    for (dark, tag) in [(true, "dark"), (false, "light")] {
        let mut h = shot_harness(f.dir.clone(), spec_of(), pack.clone(), dark, [1600.0, 1000.0]);
        steps(&mut h, 3);
        open_tree(&mut h);
        settle(&mut h);
        save(&mut h, &out, &format!("{tag}-overview"));
        let now = studio(&mut h).now();
        studio(&mut h).play.set_playing(false, now);
        studio(&mut h).play.pos = 12.0;
        go(&mut h, &building);
        settle(&mut h);
        save(&mut h, &out, &format!("{tag}-slot"));
        studio(&mut h).tab = Tab::Edit;
        steps(&mut h, 2);
        settle(&mut h);
        save(&mut h, &out, &format!("{tag}-editor"));
        {
            let s = studio(&mut h);
            let ed = s.ed.as_mut().unwrap();
            ed.sel = Some(crate::editor::Sel { layer: LayerId::Piles, index: 0 });
            ed.edit("x", None, |b| b.door = [b.door[0] + 1, b.door[1]]);
        }
        steps(&mut h, 3);
        save(&mut h, &out, &format!("{tag}-editor-selected"));
        studio(&mut h).defer(Studio::open_export);
        steps(&mut h, 3);
        save(&mut h, &out, &format!("{tag}-export"));
        studio(&mut h).export = None;
        studio(&mut h).editor_discard();
        studio(&mut h).tab = Tab::Compare;
        studio(&mut h).team = 0;
        go(&mut h, &job);
        settle(&mut h);
        save(&mut h, &out, &format!("{tag}-settler"));
        go(&mut h, &gui);
        settle(&mut h);
        save(&mut h, &out, &format!("{tag}-gui"));
        go(&mut h, &sound);
        settle(&mut h);
        save(&mut h, &out, &format!("{tag}-sound"));
        go(&mut h, &music);
        settle(&mut h);
        save(&mut h, &out, &format!("{tag}-music"));
    }
}
