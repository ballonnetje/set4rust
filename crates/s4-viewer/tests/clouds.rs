//! The cloud shadows on the GPU: they shade the frame both ways, drift with time, stay on the
//! world and work in the camera window's viewport; skipped when the game data is not present.

use s4_viewer::clouds::{GROUND_Y, WIND, seconds};
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::scene::Scene;

const SIZE: (u32, u32) = (640, 480);

/// How many pixels of `a` are brighter and how many darker than in `b`.
fn compare(a: &[u8], b: &[u8]) -> (usize, usize) {
    let lum = |p: &[u8]| p[..3].iter().map(|&c| c as i32).sum::<i32>();
    a.chunks(4).zip(b.chunks(4)).fold((0, 0), |(up, down), (p, q)| {
        let d = lum(p) - lum(q);
        (up + (d > 6) as usize, down + (d < -6) as usize)
    })
}

/// The most any channel differs between `a`'s rectangle at `at` and `b`'s at `(0, 0)` of size `size`.
fn worst(a: &[u8], b: &[u8], stride: usize, at: (usize, usize), size: (usize, usize)) -> u8 {
    let mut worst = 0;
    for y in 0..size.1 {
        for x in 0..size.0 * 4 {
            let (p, q) = (a[(y + at.1) * stride + at.0 * 4 + x], b[y * stride + x]);
            worst = worst.max(p.abs_diff(q));
        }
    }
    worst
}

#[test]
fn clouds_shade_the_world() {
    if !s4_formats::game_dir().join("Map").exists() {
        return;
    }
    let mut scene = Scene::load(Some("Campaign/roman02"), true, false).unwrap();
    let mut gpu = Gpu::headless(SIZE.0, SIZE.1).unwrap();
    scene.upload(&mut gpu);
    let c = scene.centre();
    let view = View { x: c[0].round(), y: c[1].round(), zoom: 1.0 };

    let plain = gpu.capture(view).unwrap();
    gpu.effects.clouds = true;
    // A moment with both sun and clouds in this view is found by stepping the clock.
    let (mut lit, mut dark) = (0, 0);
    for step in 0..40 {
        gpu.cloud_time = seconds(step as f64 * 1500.0);
        let (l, d) = compare(&gpu.capture(view).unwrap(), &plain);
        lit = lit.max(l);
        dark = dark.max(d);
    }
    assert!(lit > 1000 && dark > 1000, "sun on {lit} px, shade on {dark} px");

    // Time moves the pattern; the same time shows the same frame (captures repeat).
    gpu.cloud_time = seconds(5000.0);
    let shot = gpu.capture(view).unwrap();
    assert_eq!(shot, gpu.capture(view).unwrap());
    gpu.cloud_time = seconds(5400.0);
    assert_ne!(shot, gpu.capture(view).unwrap());

    // Scrolling leaves the shading on the world: the frame only moves.
    gpu.cloud_time = seconds(5000.0);
    let (dx, dy) = (96usize, 60usize);
    let scrolled = gpu.capture(View { x: view.x + dx as f32, y: view.y + dy as f32, ..view }).unwrap();
    let stride = SIZE.0 as usize * 4;
    let rest = (SIZE.0 as usize - dx, SIZE.1 as usize - dy);
    let moved = worst(&shot, &scrolled, stride, (dx, dy), rest);
    assert!(moved <= 3, "scrolled frame differs by {moved}");

    // The camera window is its own viewport on the same world: its pixels are the main view's.
    let (rx, ry, rw, rh) = (200u32, 120u32, 240u32, 180u32);
    gpu.camera_window = Some((View { x: view.x + rx as f32, y: view.y + ry as f32, ..view }, (rx, ry, rw, rh)));
    let windowed = gpu.capture(view).unwrap();
    gpu.camera_window = None;
    let at = (rx as usize, ry as usize);
    let off = stride * at.1 + at.0 * 4;
    let inside = worst(&windowed[off..], &shot[off..], stride, (0, 0), (rw as usize, rh as usize));
    assert!(inside <= 3, "camera window differs by {inside}");
}

/// Build marks are interface: where one is drawn, the frame is the same with and without clouds.
#[test]
fn clouds_leave_the_build_marks_alone() {
    if !s4_formats::game_dir().join("Map").exists() {
        return;
    }
    let mut scene = Scene::load(Some("Campaign/roman02"), true, false).unwrap();
    let mut gpu = Gpu::headless(SIZE.0, SIZE.1).unwrap();
    scene.upload(&mut gpu);
    let c = scene.centre();
    let view = View { x: c[0].round(), y: c[1].round(), zoom: 1.0 };
    let bare = gpu.capture(view).unwrap();
    // A grid of build marks over the view (image 10 of the icons).
    let (nx, ny) = (scene.mesh.width as u16 / 2, scene.mesh.height as u16 / 2);
    let marks: Vec<(u16, u16, u8)> = (0..24).flat_map(|y| (0..24).map(move |x| (nx + x * 2 + y, ny + y * 2, 10))).collect();
    scene.set_markers(&mut gpu, &marks, None, &[]).unwrap();
    let marked = gpu.capture(view).unwrap();
    let mark_px: Vec<usize> = (0..bare.len() / 4).filter(|&i| bare[i * 4..i * 4 + 3] != marked[i * 4..i * 4 + 3]).collect();
    assert!(mark_px.len() > 500, "{} mark px", mark_px.len());

    gpu.effects.clouds = true;
    // The moment that shades the frame most.
    let shaded = (0..40)
        .map(|step| {
            gpu.cloud_time = seconds(step as f64 * 1500.0);
            gpu.capture(view).unwrap()
        })
        .max_by_key(|f| compare(f, &marked).1)
        .unwrap();
    assert!(compare(&shaded, &marked).1 > 10_000, "no moment with shade in this view");
    let moved = mark_px.iter().filter(|&&i| shaded[i * 4..i * 4 + 3] != marked[i * 4..i * 4 + 3]).count();
    assert_eq!(moved, 0, "{moved} of {} mark px changed", mark_px.len());
}

/// Water takes less of the shading than land, on a view that has both.
#[test]
fn the_sea_is_shaded_less_than_the_land() {
    if !s4_formats::game_dir().join("Map").exists() {
        return;
    }
    let mut scene = Scene::load(Some("Campaign/roman02"), true, false).unwrap();
    let mut gpu = Gpu::headless(SIZE.0, SIZE.1).unwrap();
    scene.upload(&mut gpu);
    let (w, h) = (scene.mesh.width, scene.mesh.height);
    let water = |x: usize, y: usize| (scene.mesh.nodes[y * w + x] >> 8) & 255 <= 8;
    // A coast: 60 nodes of water and 60 of land side by side on a row.
    let open = |x: usize, y: usize, wet: bool| (0..60).all(|i| x + i < w && water(x + i, y) == wet);
    let (x, y) = (0..h)
        .step_by(8)
        .find_map(|y| (0..w.saturating_sub(120)).find(|&x| open(x, y, true) && open(x + 60, y, false) || open(x, y, false) && open(x + 60, y, true)).map(|x| (x, y)))
        .expect("a coast");
    let p = scene.node_world(x + 60, y);
    let view = View { x: (p[0] - SIZE.0 as f32 / 2.0).round(), y: (p[1] - SIZE.1 as f32 / 2.0).round(), zoom: 1.0 };

    let plain = gpu.capture(view).unwrap();
    gpu.effects.clouds = true;
    // The darkest each pixel gets over a long drift, on the pixels on a node well inside one side.
    let side = |px: usize| {
        let (sx, sy) = ((px % SIZE.0 as usize) as f32 + view.x, (px / SIZE.0 as usize) as f32 + view.y);
        let row = sy / 12.0;
        let (nx, ny) = (((sx + 12.0 * row) / 24.0) as i64, row as i64);
        let ground = |dx: i64, dy: i64| {
            let (ax, ay) = (nx + dx, ny + dy);
            (ax >= 0 && ay >= 0 && (ax as usize) < w && (ay as usize) < h).then(|| water(ax as usize, ay as usize))
        };
        let all: Vec<_> = (-6..=6).flat_map(|d| (-6..=6).map(move |e| (d, e))).map(|(d, e)| ground(d, e)).collect();
        if all.iter().all(|g| *g == Some(true)) {
            Some(true)
        } else if all.iter().all(|g| *g == Some(false)) {
            Some(false)
        } else {
            None
        }
    };
    let n = plain.len() / 4;
    let sides: Vec<Option<bool>> = (0..n).map(side).collect();
    let mut darkest = vec![f32::MAX; n];
    for step in 0..60 {
        gpu.cloud_time = seconds(step as f64 * 2000.0);
        let f = gpu.capture(view).unwrap();
        for i in 0..n {
            let lum = |b: &[u8]| b[i * 4..i * 4 + 3].iter().map(|&c| c as f32).sum::<f32>();
            if lum(&plain) > 120.0 {
                darkest[i] = darkest[i].min(lum(&f) / lum(&plain));
            }
        }
    }
    let low = |wet: bool| {
        let mut v: Vec<f32> = (0..n).filter(|&i| sides[i] == Some(wet) && darkest[i] < f32::MAX).map(|i| darkest[i]).collect();
        v.sort_by(f32::total_cmp);
        assert!(v.len() > 2000, "{} px of {wet}", v.len());
        v[v.len() / 2]
    };
    let (sea, land) = (low(true), low(false));
    assert!(land < 0.80 && sea > land + 0.04, "darkest: sea {sea}, land {land}");
}

/// Clouds travel as bodies: followed at the wind's speed, the shading keeps its shape. Standing
/// still instead, it changes (the check can tell the two apart).
#[test]
fn clouds_hold_their_shape_as_they_travel() {
    if !s4_formats::game_dir().join("Map").exists() {
        return;
    }
    let mut scene = Scene::load(Some("Campaign/roman02"), true, false).unwrap();
    let mut gpu = Gpu::headless(SIZE.0, SIZE.1).unwrap();
    scene.upload(&mut gpu);
    let c = scene.centre();
    let view = View { x: c[0].round(), y: c[1].round(), zoom: 1.0 };
    // The ground under a pixel changes the shading too (sea, height): flat land for the clouds, so
    // only they differ between two views.
    let real = scene.mesh.nodes.clone();
    scene.mesh.nodes.fill(16 << 8);
    gpu.set_land(&scene.mesh);
    scene.mesh.nodes = real;
    // The clouds' brightness factor over the world under `view`.
    let mut shading = |view: View, time: f64| -> Vec<f32> {
        gpu.effects.clouds = false;
        let plain = gpu.capture(view).unwrap();
        gpu.effects.clouds = true;
        gpu.cloud_time = time;
        let on = gpu.capture(view).unwrap();
        let lum = |f: &[u8], i: usize| f[i * 4..i * 4 + 3].iter().map(|&c| c as f32).sum::<f32>();
        (0..plain.len() / 4).map(|i| if lum(&plain, i) > 120.0 { lum(&on, i) / lum(&plain, i) } else { -1.0 }).collect()
    };
    let difference = |a: &[f32], b: &[f32]| {
        let both: Vec<f32> = a.iter().zip(b).filter(|(p, q)| **p >= 0.0 && **q >= 0.0).map(|(p, q)| (p - q).abs()).collect();
        both.iter().sum::<f32>() / both.len() as f32
    };
    let (t0, dt) = (seconds(4000.0), 8.0);
    let (dx, dy) = ((WIND.0 * dt).round() as f32, (WIND.1 * dt / GROUND_Y).round() as f32);
    // The camera moves with the clouds: the same part of the clouds is in view, at the same place.
    let start = shading(view, t0);
    let followed = shading(View { x: view.x + dx, y: view.y + dy, ..view }, t0 + dt);
    let stood = shading(view, t0 + dt);
    let (kept, moved) = (difference(&start, &followed), difference(&start, &stood));
    println!("followed {kept:.4}, stood still {moved:.4}");
    assert!(kept < moved / 3.0, "the shape changed by {kept} following it, {moved} standing still");
}

/// A shadow lies on the ground: land raised by `h` shows at the pixel `h` px higher up what flat
/// land shows at its own.
#[test]
fn clouds_lie_on_the_height_of_the_land() {
    if !s4_formats::game_dir().join("Map").exists() {
        return;
    }
    let mut scene = Scene::load(Some("Campaign/roman02"), true, false).unwrap();
    let mut gpu = Gpu::headless(SIZE.0, SIZE.1).unwrap();
    scene.upload(&mut gpu);
    let c = scene.centre();
    let view = View { x: c[0].round(), y: c[1].round(), zoom: 1.0 };
    let plain = gpu.capture(view).unwrap();
    gpu.effects.clouds = true;
    let real = scene.mesh.nodes.clone();
    let mut shade_at = |height: u32| {
        scene.mesh.nodes.fill(height | 16 << 8);
        gpu.set_land(&scene.mesh);
        gpu.cloud_time = seconds(4000.0);
        let f = gpu.capture(view).unwrap();
        let lum = |f: &[u8], i: usize| f[i * 4..i * 4 + 3].iter().map(|&c| c as f32).sum::<f32>();
        (0..plain.len() / 4).map(|i| if lum(&plain, i) > 120.0 { lum(&f, i) / lum(&plain, i) } else { -1.0 }).collect::<Vec<f32>>()
    };
    // 80 units = 99 px. The raised ground's pixel 99 rows up shows the flat ground's pixel.
    let (flat, raised) = (shade_at(0), shade_at(80));
    let (w, rows) = (SIZE.0 as usize, 99usize);
    let (mut same, mut there) = ((0.0, 0), (0.0, 0));
    for i in 0..(SIZE.1 as usize - rows) * w {
        let (up, down) = (raised[i], flat[i + rows * w]);
        let (at, unshifted) = (flat[i], raised[i]);
        if up >= 0.0 && down >= 0.0 && at >= 0.0 {
            same = (same.0 + (up - down).abs(), same.1 + 1);
            there = (there.0 + (at - unshifted).abs(), there.1 + 1);
        }
    }
    let (shifted, not) = (same.0 / same.1 as f32, there.0 / there.1 as f32);
    println!("shifted {shifted:.4}, not shifted {not:.4}");
    assert!(shifted < not / 3.0, "{shifted} vs {not}");
    scene.mesh.nodes = real;
}

/// The start screens have no world, so nothing is drawn.
#[test]
fn no_clouds_without_a_world() {
    let mut gpu = Gpu::headless(64, 48).unwrap();
    let view = View { x: 0.0, y: 0.0, zoom: 1.0 };
    let plain = gpu.capture(view).unwrap();
    gpu.effects.clouds = true;
    gpu.cloud_time = 100.0;
    assert_eq!(plain, gpu.capture(view).unwrap());
}
