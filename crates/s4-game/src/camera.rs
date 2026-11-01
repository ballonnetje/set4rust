//! The camera controller. **Deliberately not the original's** (user request 2026-10-23): the
//! release moves the camera in steps from `EventManagement` (0x5172f0), the border scroll
//! 0x519bc0 once per 71 ms game-loop iteration and a right drag that hides the cursor and keeps
//! putting it back at the screen centre (SetCursorPos); with late-landing warps that drag
//! jumped. Here one [`Camera`] owns the view (position and zoom), every input feeds it, and it
//! integrates per real frame ([`Camera::update`] with the frame's `dt`), so the motion is the
//! same at any frame rate:
//!
//! * **Right drag = grab and pan.** The world point under the cursor at the press stays under
//!   the cursor, 1:1, computed from the press (not summed from events), so uneven frames, stalls
//!   and coalesced mouse moves cannot make it jump. No cursor warping or hiding. A move of more
//!   than [`DRAG_PX`] in x or y from the press starts the drag; a release before that is a
//!   right click (the selection's order, as before).
//! * **Arrows / W A S D and the border:** a velocity ramping linearly from 0 to its speed in
//!   [`KEY_RAMP`] s, integrated exactly over real time; the keys at [`KEY_SPEED`] (1.5 × the
//!   border scroll's old 72 px per 71 ms), the border at the same speed scaled by
//!   `ScrollStepValue` / 72. Two keys move diagonally at the same speed. The border is the
//!   window's outermost pixel (fullscreen and window), only while no button is held.
//! * **Zoom** (wheel, PageUp / PageDown, the GUI slider, both buttons): the wanted zoom moves by
//!   the original's units (20000 of +0x4c per unit, 5 per wheel notch) inside its limits, and the
//!   view eases to it (time constant [`ZOOM_TAU`]) around a screen point (the cursor for the
//!   wheel and both buttons, the centre for keys and slider) that keeps its world point.
//! * **Jumps** (minimap, Home / H, group centring, go-to buttons, messages, the tutorial's
//!   camera calls, a loaded game's view): [`Camera::jump`], at once.
//! * **Clamp:** the original's bounds (0x519a70: camera y in `0..=size · z / 2`, x in
//!   `−y..=size · z − y`, `z` px per node column) in floats after every change, so there is no
//!   rounding at the edges and pushing against one does not jitter.

use s4_viewer::client::{ZOOM_MAX, ZOOM_MIN, ZOOM_STEP, camera_anchor};
use s4_viewer::gpu::View;

/// The window the camera draws in and the map it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    /// Window px.
    pub size: (u32, u32),
    /// Screen px per world px at `View::zoom` 1 (the HiDPI factor, else 1).
    pub hidpi: f32,
    /// Map size in nodes.
    pub map: usize,
}

impl Screen {
    /// Screen px per world px of `v`.
    fn ez(&self, v: View) -> f32 {
        v.zoom * self.hidpi
    }

    fn centre(&self) -> (f64, f64) {
        (self.size.0 as f64 / 2.0, self.size.1 as f64 / 2.0)
    }
}

/// Key scrolling speed in screen px per second: 1.5 × the border scroll's old 72 px per 71 ms.
pub const KEY_SPEED: f32 = 1.5 * 72.0 / 0.071;
/// Seconds from standstill to full speed.
pub const KEY_RAMP: f32 = 0.15;
/// A longer frame counts as this long (a stall must not throw the view across the map).
pub const MAX_DT: f32 = 0.25;
/// The zoom's time constant: the rest to go shrinks by e in this many seconds.
pub const ZOOM_TAU: f32 = 0.06;
/// The drag threshold: more than this many px in x or y from the press.
pub const DRAG_PX: f64 = 4.0;

/// Wheel units (EventManagement case 0xe): `(delta / 120) · 5` with the wheel delta in 1/120
/// notches (C division); up zooms in.
pub fn wheel_units(delta120: i32) -> i32 {
    (delta120 / 120) * 5
}

/// A scroll key (arrows and W / A / S / D).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollKey {
    Left,
    Right,
    Up,
    Down,
    A,
    D,
    W,
    S,
}

/// The original's bounds (0x519a70) for `v`, in floats; an axis that needs nothing keeps its
/// value exactly.
pub fn clamp(v: View, s: &Screen) -> View {
    let ez = s.ez(v);
    if ez.is_nan() || ez <= 0.0 {
        return v;
    }
    let (a, b) = camera_anchor(s.size);
    let (a, b) = (a as f32, b as f32);
    let (cx, cy) = (v.x * ez + a, v.y * ez + b);
    let (m, zc) = (s.map as f32, ez * 24.0);
    let ny = cy.clamp(0.0, m * zc / 2.0);
    let nx = cx.clamp(-ny, (m * zc - ny).max(-ny));
    View { x: if nx != cx { (nx - a) / ez } else { v.x }, y: if ny != cy { (ny - b) / ez } else { v.y }, zoom: v.zoom }
}

/// The world point under screen point `p`.
fn world_at(v: View, s: &Screen, p: (f64, f64)) -> (f32, f32) {
    let ez = s.ez(v);
    (v.x + p.0 as f32 / ez, v.y + p.1 as f32 / ez)
}

/// The view at zoom `zoom` that puts world point `w` under screen point `p`.
fn put(w: (f32, f32), p: (f64, f64), zoom: f32, s: &Screen) -> View {
    let ez = zoom * s.hidpi;
    View { x: w.0 - p.0 as f32 / ez, y: w.1 - p.1 as f32 / ez, zoom }
}

/// +0x4c (16.16 px per node column) of a view zoom.
fn units_of(zoom: f32, s: &Screen) -> i32 {
    (zoom * s.hidpi * 24.0 * 65536.0).round() as i32
}

/// A right press and, past the threshold, the grab.
#[derive(Clone, Copy, Debug)]
struct Grab {
    press: (f64, f64),
    /// The world point that stays under the cursor.
    world: (f32, f32),
    dragging: bool,
}

/// Both buttons held: vertical movement zooms around the press point.
#[derive(Clone, Copy, Debug)]
struct Both {
    press: (f64, f64),
    last_y: f64,
    /// Vertical px not yet turned into zoom units (4 px per unit).
    acc: f64,
}

/// The camera: the view and everything that moves it.
#[derive(Clone, Debug)]
pub struct Camera {
    view: View,
    /// The zoom the view eases to and the screen point it zooms around.
    zoom_to: f32,
    zoom_at: (f64, f64),
    /// Held keys: bit 0 left, 1 right, 2 up, 3 down; the arrows in bits 0..4, W A S D in 4..8.
    held: u8,
    /// Seconds since scrolling (keys or border) started.
    ramp: f32,
    /// The border flags: −1 left / up, 1 right / down, 0 none.
    edge: (i8, i8),
    /// `BorderScroll` and `ScrollStepValue` (the border speed is `KEY_SPEED · step / 72`).
    pub border: bool,
    pub step: i32,
    grab: Option<Grab>,
    both: Option<Both>,
    /// The cursor's last position (window px).
    cursor: (f64, f64),
}

impl Camera {
    pub fn new(view: View, border: bool, step: i32) -> Camera {
        Camera { view, zoom_to: view.zoom, zoom_at: (0.0, 0.0), held: 0, ramp: 0.0, edge: (0, 0), border, step, grab: None, both: None, cursor: (0.0, 0.0) }
    }

    pub fn view(&self) -> View {
        self.view
    }

    /// The zoom the view is easing to.
    pub fn zoom_target(&self) -> f32 {
        self.zoom_to
    }

    /// Put the view at `v` at once (a jump: minimap, Home, a message, the tutorial, a load);
    /// any zoom under way ends there. A grab goes on from the point now under the cursor.
    pub fn jump(&mut self, v: View, s: &Screen) {
        self.view = clamp(v, s);
        self.zoom_to = self.view.zoom;
        self.regrab(s);
    }

    /// The grabbed point becomes the one under the cursor now (after a jump or a clamp).
    fn regrab(&mut self, s: &Screen) {
        let (v, c) = (self.view, self.cursor);
        if let Some(g) = &mut self.grab {
            g.world = world_at(v, s, c);
        }
    }

    // --- keys ---

    fn bit(k: ScrollKey) -> u8 {
        1 << k as u8
    }

    /// A scroll key went down or up.
    pub fn key(&mut self, k: ScrollKey, down: bool) {
        if down {
            self.held |= Self::bit(k);
        } else {
            self.held &= !Self::bit(k);
        }
    }

    /// Every key up (focus lost, a text field took the keyboard).
    pub fn release_keys(&mut self) {
        self.held = 0;
    }

    /// The held keys' direction, −1 / 0 / 1 per axis (opposite keys cancel).
    fn key_dir(&self) -> (f32, f32) {
        let h = self.held | self.held >> 4;
        let axis = |neg: u8, pos: u8| (h & pos != 0) as i32 as f32 - (h & neg != 0) as i32 as f32;
        (axis(1, 2), axis(4, 8))
    }

    // --- mouse ---

    /// The cursor moved to `p` in the window. `buttons`: a mouse button is held.
    pub fn moved(&mut self, p: (f64, f64), s: &Screen, buttons: bool) {
        self.cursor = p;
        let (w, h) = (s.size.0 as f64, s.size.1 as f64);
        if !buttons && self.grab.is_none() {
            let (x, y) = (p.0.floor(), p.1.floor());
            self.edge.0 = if x <= 0.0 { -1 } else if x >= w - 1.0 { 1 } else { 0 };
            self.edge.1 = if y <= 0.0 { -1 } else if y >= h - 1.0 { 1 } else { 0 };
        }
        if let Some(b) = &mut self.both {
            b.acc += p.1 - b.last_y;
            b.last_y = p.1;
            // Down zooms out, up in: one unit per 4 px.
            let n = (b.acc / 4.0).trunc();
            if n != 0.0 {
                b.acc -= n * 4.0;
                let at = b.press;
                self.zoom_by(-(n as i32), at, s);
            }
            return;
        }
        if let Some(g) = &mut self.grab {
            if !g.dragging && ((p.0 - g.press.0).abs() > DRAG_PX || (p.1 - g.press.1).abs() > DRAG_PX) {
                g.dragging = true;
            }
            if g.dragging {
                self.place_grab(s);
            }
        }
    }

    /// The grabbed point under the cursor, then the bounds.
    fn place_grab(&mut self, s: &Screen) {
        let Some(g) = self.grab.filter(|g| g.dragging) else { return };
        let v = put(g.world, self.cursor, self.view.zoom, s);
        let c = clamp(v, s);
        self.view = c;
        if c != v {
            self.regrab(s);
        }
    }

    /// The cursor left the window: no edge.
    pub fn left_window(&mut self) {
        if self.grab.is_none() {
            self.edge = (0, 0);
        }
    }

    /// Right button down on the map at `p`.
    pub fn right_down(&mut self, p: (f64, f64), s: &Screen) {
        self.cursor = p;
        self.grab = Some(Grab { press: p, world: world_at(self.view, s, p), dragging: false });
    }

    /// Right button up: the press point when it was a click (no drag).
    pub fn right_up(&mut self) -> Option<(f64, f64)> {
        let g = self.grab.take()?;
        (!g.dragging).then_some(g.press)
    }

    /// A right drag is under way.
    pub fn right_dragging(&self) -> bool {
        self.grab.is_some_and(|g| g.dragging)
    }

    /// The right button is held (a click or a drag, not decided yet or dragging).
    pub fn right_held(&self) -> bool {
        self.grab.is_some()
    }

    /// The second button went down at `p` while the other is held: vertical movement zooms.
    pub fn both_down(&mut self, p: (f64, f64)) {
        self.both = Some(Both { press: p, last_y: p.1, acc: 0.0 });
        self.grab = None;
    }

    pub fn both_held(&self) -> bool {
        self.both.is_some()
    }

    /// A button went up while both were held: the zoom ends (no click).
    pub fn both_up(&mut self) {
        self.both = None;
    }

    // --- zoom ---

    /// Zoom by `units` (+ in, − out) of the original's steps around screen point `at`, within
    /// its limits. Returns the wanted +0x4c.
    pub fn zoom_by(&mut self, units: i32, at: (f64, f64), s: &Screen) -> i32 {
        let old = units_of(self.zoom_to, s);
        self.zoom_to_units(old + units * ZOOM_STEP, at, s)
    }

    /// Zoom to +0x4c `z` (kept in the limits) around screen point `at`. Returns the wanted +0x4c.
    pub fn zoom_to_units(&mut self, z: i32, at: (f64, f64), s: &Screen) -> i32 {
        let z = z.clamp(ZOOM_MIN, ZOOM_MAX);
        self.zoom_to = z as f32 / (24.0 * 65536.0) / s.hidpi;
        self.zoom_at = at;
        z
    }

    /// The zoom around the screen centre (keys, the slider).
    pub fn zoom_by_centre(&mut self, units: i32, s: &Screen) -> i32 {
        self.zoom_by(units, s.centre(), s)
    }

    // --- per frame ---

    /// Advance by `dt` seconds of real time: the zoom eases, a grab holds its point, the keys or
    /// the border scroll, and the bounds apply.
    pub fn update(&mut self, dt: f32, s: &Screen) {
        let dt = if dt.is_finite() { dt.clamp(0.0, MAX_DT) } else { 0.0 };
        self.ease_zoom(1.0 - (-dt / ZOOM_TAU).exp(), s);
        if self.right_dragging() {
            self.place_grab(s);
            self.ramp = 0.0;
        } else {
            self.scroll(dt, s);
        }
        let c = clamp(self.view, s);
        if c != self.view {
            self.view = c;
            self.regrab(s);
        }
    }

    /// Finish the zoom at once (scripted runs without frames).
    pub fn settle(&mut self, s: &Screen) {
        self.ease_zoom(1.0, s);
        self.view = clamp(self.view, s);
    }

    /// Move fraction `k` of the way to the wanted zoom around its point.
    fn ease_zoom(&mut self, k: f32, s: &Screen) {
        if self.view.zoom == self.zoom_to {
            return;
        }
        let mut z = self.view.zoom + (self.zoom_to - self.view.zoom) * k;
        if (z - self.zoom_to).abs() <= self.zoom_to * 1e-4 {
            z = self.zoom_to;
        }
        let at = self.zoom_at;
        let w = world_at(self.view, s, at);
        self.view = put(w, at, z, s);
    }

    /// The scroll direction and speed: the keys, else the border.
    fn scroll_velocity(&self) -> ((f32, f32), f32) {
        let k = self.key_dir();
        if k != (0.0, 0.0) {
            return (k, KEY_SPEED);
        }
        if self.border && self.edge != (0, 0) && self.grab.is_none() && self.both.is_none() {
            return ((self.edge.0 as f32, self.edge.1 as f32), KEY_SPEED * self.step.max(0) as f32 / 72.0);
        }
        ((0.0, 0.0), 0.0)
    }

    /// Distance covered `t` s after starting at speed `v`: `v·t²/(2r)` on the ramp, then
    /// `v·(t − r/2)`.
    fn distance(v: f32, t: f32) -> f32 {
        if t < KEY_RAMP { v * t * t / (2.0 * KEY_RAMP) } else { v * (t - KEY_RAMP / 2.0) }
    }

    fn scroll(&mut self, dt: f32, s: &Screen) {
        let ((x, y), v) = self.scroll_velocity();
        if (x, y) == (0.0, 0.0) {
            self.ramp = 0.0;
            return;
        }
        let d = Self::distance(v, self.ramp + dt) - Self::distance(v, self.ramp);
        self.ramp += dt;
        let k = if x != 0.0 && y != 0.0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 };
        let ez = s.ez(self.view);
        self.view.x += x * d * k / ez;
        self.view.y += y * d * k / ez;
    }

    /// The move of holding `dir` (−1 / 0 / 1 per axis) for `secs` from standstill, in frames of
    /// at most [`MAX_DT`] (scripted input); the camera is left as it was otherwise.
    pub fn hold(&mut self, dir: (i32, i32), secs: f32, s: &Screen) {
        let keys = [(dir.0 < 0, ScrollKey::Left), (dir.0 > 0, ScrollKey::Right), (dir.1 < 0, ScrollKey::Up), (dir.1 > 0, ScrollKey::Down)];
        let before = self.held;
        self.held = 0;
        self.ramp = 0.0;
        for (on, key) in keys {
            if on {
                self.key(key, true);
            }
        }
        let mut left = secs;
        while left > 0.0 {
            self.update(left.min(MAX_DT), s);
            left -= MAX_DT;
        }
        self.held = before;
        self.ramp = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Screen = Screen { size: (1280, 1024), hidpi: 1.0, map: 512 };

    /// A view in the middle of the map.
    fn mid() -> View {
        View { x: 3000.0, y: 2500.0, zoom: 1.0 }
    }

    /// Frame lengths at `fps` with ±40 % jitter, `secs` in all (deterministic).
    fn frames(fps: f32, secs: f32, seed: u32) -> Vec<f32> {
        let mut r = seed;
        let mut out = Vec::new();
        let mut t = 0.0;
        while t < secs {
            r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let dt = ((1.0 + 0.8 * ((r >> 16) as f32 / 65536.0 - 0.5)) / fps).min(secs - t);
            out.push(dt);
            t += dt;
        }
        out
    }

    /// Keys, border and zoom at 30, 60 and 144 fps with jittered frames end at the same place;
    /// the key speed after the ramp is `KEY_SPEED` every frame; diagonals keep the speed; opposite
    /// keys cancel; a stall counts at most `MAX_DT`.
    #[test]
    fn frame_rate_independent() {
        let mut ends = Vec::new();
        for (i, fps) in [30.0f32, 60.0, 144.0].into_iter().enumerate() {
            let mut c = Camera::new(mid(), true, 72);
            c.key(ScrollKey::Right, true);
            let mut t = 0.0;
            for dt in frames(fps, 2.0, 7 + i as u32) {
                let x0 = c.view().x;
                c.update(dt, &S);
                let dx = c.view().x - x0;
                assert!(dx > 0.0 && dx <= KEY_SPEED * dt * 1.0001, "{fps} fps at {t}: {dx} in {dt}");
                if t > KEY_RAMP {
                    assert!((dx / dt - KEY_SPEED).abs() < 2.0, "{fps} fps at {t}: {}", dx / dt);
                }
                t += dt;
            }
            c.key(ScrollKey::Right, false);
            // The border (step 72: the key speed) for 1 s, then a zoom out by 2 wheel notches
            // around a point for 1 s.
            c.moved((0.0, 500.0), &S, false);
            for dt in frames(fps, 1.0, 11 + i as u32) {
                c.update(dt, &S);
            }
            c.moved((600.0, 500.0), &S, false);
            c.zoom_by(-10, (900.0, 300.0), &S);
            for dt in frames(fps, 1.0, 13 + i as u32) {
                c.update(dt, &S);
            }
            ends.push(c.view());
        }
        assert!(ends[0].zoom < 1.0 && ends[0].x != mid().x, "{ends:?}");
        for e in &ends[1..] {
            assert!((e.x - ends[0].x).abs() < 0.5 && (e.y - ends[0].y).abs() < 0.5 && e.zoom == ends[0].zoom, "{ends:?}");
        }
        // Diagonal: the same speed along the diagonal.
        let along = |dir: (i32, i32)| {
            let mut c = Camera::new(mid(), false, 72);
            c.hold(dir, 1.0, &S);
            ((c.view().x - mid().x) as f64, (c.view().y - mid().y) as f64)
        };
        let (dx, dy) = along((1, -1));
        assert!(((dx * dx + dy * dy).sqrt() - along((1, 0)).0).abs() < 0.01 && dy < 0.0);
        // Opposite keys cancel; releasing all stops.
        let mut c = Camera::new(mid(), false, 72);
        c.key(ScrollKey::W, true);
        c.key(ScrollKey::Down, true);
        c.update(0.5, &S);
        assert_eq!(c.view(), mid());
        c.release_keys();
        c.update(0.5, &S);
        assert_eq!(c.view(), mid());
        // A stall counts at most MAX_DT.
        let mut c = Camera::new(mid(), false, 72);
        c.key(ScrollKey::A, true);
        c.update(1.0, &S);
        let x = c.view().x;
        c.update(5.0, &S);
        assert!(x - c.view().x <= KEY_SPEED * MAX_DT + 0.01);
    }

    /// Right drag: the press's world point stays under the cursor across uneven frames, frame
    /// stalls and coalesced moves, at zoom 1 and with HiDPI; the release adds nothing.
    #[test]
    fn grab_keeps_the_point_under_the_cursor() {
        for s in [S, Screen { hidpi: 1.5, ..S }] {
            let mut c = Camera::new(View { x: 3000.25, y: 2500.5, zoom: 1.3 }, true, 72);
            let press = (640.0, 512.0);
            c.moved(press, &s, false);
            c.right_down(press, &s);
            let grabbed = world_at(c.view(), &s, press);
            let mut p = press;
            let moves: Vec<((f64, f64), f32)> = (0..40)
                .map(|i| ((7.0, if i % 2 == 0 { 3.0 } else { -2.0 }), 0.016))
                .chain([((180.0, 40.0), 0.2), ((-300.0, 25.0), 0.0), ((5.0, -90.0), 0.12), ((-9.0, 2.0), 0.007)])
                .chain((0..30).map(|_| ((-11.0, 4.0), 0.033)))
                .collect();
            for (i, (d, frame)) in moves.iter().enumerate() {
                p = (p.0 + d.0, p.1 + d.1);
                c.moved(p, &s, true);
                if *frame > 0.0 {
                    c.update(*frame, &s);
                }
                let w = world_at(c.view(), &s, p);
                assert!((w.0 - grabbed.0).abs() < 1e-2 && (w.1 - grabbed.1).abs() < 1e-2, "hidpi {} move {i}: {w:?} for {grabbed:?}", s.hidpi);
            }
            let v = c.view();
            assert_eq!(c.right_up(), None, "a drag, not a click");
            c.update(0.016, &s);
            assert_eq!(c.view(), v, "nothing on release");
        }
    }

    /// Up to 4 px from the press in x and y the release is a click and the view does not move;
    /// past it the drag catches up at once (the grabbed point under the cursor).
    #[test]
    fn click_versus_drag() {
        let mut c = Camera::new(mid(), true, 72);
        c.right_down((100.0, 100.0), &S);
        c.moved((104.0, 96.0), &S, true);
        c.update(0.016, &S);
        assert_eq!(c.view(), mid());
        assert_eq!(c.right_up(), Some((100.0, 100.0)));
        c.right_down((100.0, 100.0), &S);
        c.moved((105.0, 100.0), &S, true);
        assert!(c.right_dragging());
        assert_eq!(c.view().x, mid().x - 5.0);
        assert_eq!(c.right_up(), None);
    }

    /// Zoom around the cursor: the world point under it stays there throughout the easing, the
    /// wanted zoom moves in the original's units within its limits.
    #[test]
    fn zoom_around_the_cursor() {
        let mut c = Camera::new(mid(), true, 72);
        let at = (1000.0, 200.0);
        let w = world_at(c.view(), &S, at);
        let z = c.zoom_by(wheel_units(120), at, &S);
        assert_eq!(z, 24 * 65536 + 5 * ZOOM_STEP);
        for _ in 0..30 {
            c.update(0.016, &S);
            let now = world_at(c.view(), &S, at);
            assert!((now.0 - w.0).abs() < 1e-2 && (now.1 - w.1).abs() < 1e-2, "{now:?} {w:?}");
        }
        assert_eq!(c.view().zoom, c.zoom_target());
        // The limits.
        assert_eq!(c.zoom_by(1000, at, &S), ZOOM_MAX);
        assert_eq!(c.zoom_by(-1000, at, &S), ZOOM_MIN);
        c.settle(&S);
        assert!((c.view().zoom * 24.0 * 65536.0 - ZOOM_MIN as f32).abs() < 1.0);
        // Both buttons: 9 px down = 2 units out around the press, 5 px up = 1 in.
        let mut c = Camera::new(mid(), true, 72);
        c.right_down((300.0, 300.0), &S);
        c.both_down((300.0, 300.0));
        assert!(!c.right_held());
        let base = units_of(c.zoom_target(), &S);
        c.moved((300.0, 309.0), &S, true);
        assert_eq!(units_of(c.zoom_target(), &S), base - 2 * ZOOM_STEP);
        c.moved((300.0, 304.0), &S, true);
        assert_eq!(units_of(c.zoom_target(), &S), base - ZOOM_STEP);
        c.both_up();
        assert!(!c.both_held());
    }

    /// The bounds: pushing against an edge with the keys, the border or a drag stays put with no
    /// jitter; a drag back moves at once.
    #[test]
    fn clamp_without_jitter() {
        let mut c = Camera::new(mid(), true, 72);
        c.jump(View { x: -1e6, y: -1e6, zoom: 1.0 }, &S);
        let edge = c.view();
        assert_eq!(clamp(edge, &S), edge);
        c.key(ScrollKey::Left, true);
        c.key(ScrollKey::Up, true);
        for dt in frames(60.0, 1.0, 3) {
            c.update(dt, &S);
            assert_eq!(c.view(), edge);
        }
        c.release_keys();
        // A drag up-left past the edge, then back: the map follows the reversal at once.
        c.right_down((600.0, 600.0), &S);
        c.moved((700.0, 700.0), &S, true);
        assert_eq!(c.view(), edge);
        c.moved((690.0, 700.0), &S, true);
        assert!((c.view().x - (edge.x + 10.0)).abs() < 1e-3, "{:?} {edge:?}", c.view());
    }

    /// A jump puts the view at once, ends a zoom under way and keeps a grab going from the point
    /// now under the cursor.
    #[test]
    fn jumps_end_zooms_and_regrab() {
        let mut c = Camera::new(mid(), true, 72);
        c.zoom_by(10, (100.0, 100.0), &S);
        let to = View { x: 1000.0, y: 1200.0, zoom: 1.0 };
        c.jump(to, &S);
        c.update(0.1, &S);
        assert_eq!(c.view(), to);
        c.right_down((500.0, 500.0), &S);
        c.moved((520.0, 500.0), &S, true);
        c.jump(mid(), &S);
        c.moved((530.0, 500.0), &S, true);
        assert_eq!(c.view().x, mid().x - 10.0);
    }
}
