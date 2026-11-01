//! The look: fonts, colours, spacing. Two themes over one palette structure.

use std::sync::Arc;

use egui::epaint::Shadow;
use egui::style::{HandleShape, ScrollStyle, Selection, TextCursorStyle, WidgetVisuals, Widgets};
use egui::{Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke, TextStyle, vec2};

/// Phosphor icons (assets/fonts/Phosphor.ttf: only the private-use code points, see its README).
pub mod icon {
    pub const ARROWS_CLOCKWISE: &str = "\u{E094}";
    pub const ARROW_DOWN: &str = "\u{E03E}";
    pub const ARROW_UP: &str = "\u{E08E}";
    pub const CARET_DOWN: &str = "\u{E136}";
    pub const CARET_RIGHT: &str = "\u{E13A}";
    pub const CASTLE_TURRET: &str = "\u{E9D0}";
    pub const CHECK_CIRCLE: &str = "\u{E184}";
    pub const COLUMNS: &str = "\u{E546}";
    pub const CROSSHAIR: &str = "\u{E1D6}";
    pub const CURSOR: &str = "\u{E1DC}";
    pub const DOTS_SIX_VERTICAL: &str = "\u{EAE2}";
    pub const ERASER: &str = "\u{E21E}";
    pub const EXPORT: &str = "\u{EAF0}";
    pub const EYE: &str = "\u{E220}";
    pub const EYE_SLASH: &str = "\u{E224}";
    pub const FILE_PLUS: &str = "\u{E236}";
    pub const FLOPPY_DISK: &str = "\u{E248}";
    pub const FOLDER_OPEN: &str = "\u{E256}";
    pub const GAUGE: &str = "\u{E628}";
    pub const HOUSE: &str = "\u{E2C2}";
    pub const INFO: &str = "\u{E2CE}";
    pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
    pub const MOON: &str = "\u{E330}";
    pub const PACKAGE: &str = "\u{E390}";
    pub const PAINT_BRUSH: &str = "\u{E6F0}";
    pub const PAUSE: &str = "\u{E39E}";
    pub const PENCIL_SIMPLE: &str = "\u{E3B4}";
    pub const PLAY: &str = "\u{E3D0}";
    pub const PLUS_CIRCLE: &str = "\u{E3D6}";
    pub const SKIP_BACK: &str = "\u{E5A4}";
    pub const SKIP_FORWARD: &str = "\u{E5A6}";
    pub const SPEAKER_HIGH: &str = "\u{E44A}";
    pub const SUN: &str = "\u{E472}";
    pub const TRASH: &str = "\u{E4A6}";
    pub const UNDO: &str = "\u{E08A}";
    pub const REDO: &str = "\u{E08C}";
    pub const WARNING: &str = "\u{E4E0}";
    pub const WARNING_CIRCLE: &str = "\u{E4E2}";
    pub const X: &str = "\u{E4F6}";
}

const MEDIUM: &str = "medium";
const SEMIBOLD: &str = "semibold";

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub fn from_rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub panel2: Color32,
    pub raised: Color32,
    pub line: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    pub info: Color32,
}

pub const DARK: Palette = Palette {
    dark: true,
    bg: rgb(0x111318),
    panel: rgb(0x171a20),
    panel2: rgb(0x1f232b),
    raised: rgb(0x282d37),
    line: rgb(0x2b303a),
    text: rgb(0xe7eaf0),
    dim: rgb(0x98a1af),
    faint: rgb(0x687180),
    accent: rgb(0x6b9dff),
    accent_soft: rgb(0x24324d),
    on_accent: rgb(0x0d1420),
    ok: rgb(0x46c07a),
    warn: rgb(0xe6ae45),
    err: rgb(0xeb5f57),
    info: rgb(0x8496ab),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: rgb(0xf3f4f7),
    panel: rgb(0xffffff),
    panel2: rgb(0xeef0f4),
    raised: rgb(0xe3e6ec),
    line: rgb(0xdfe2e8),
    text: rgb(0x1b1f27),
    dim: rgb(0x5d6675),
    faint: rgb(0x8d95a3),
    accent: rgb(0x2f66e0),
    accent_soft: rgb(0xdfe8fb),
    on_accent: rgb(0xffffff),
    ok: rgb(0x1e9a55),
    warn: rgb(0xb97a0c),
    err: rgb(0xd03b33),
    info: rgb(0x64748b),
};

pub fn palette(ctx: &Context) -> Palette {
    if ctx.global_style().visuals.dark_mode { DARK } else { LIGHT }
}

pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_string(), Arc::new(FontData::from_static(bytes)));
    };
    add("inter", include_bytes!("../assets/fonts/Inter-Regular.ttf"));
    add("inter-medium", include_bytes!("../assets/fonts/Inter-Medium.ttf"));
    add("inter-semibold", include_bytes!("../assets/fonts/Inter-SemiBold.ttf"));
    add("phosphor", include_bytes!("../assets/fonts/Phosphor.ttf"));
    let fallback: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let stack = |first: &str| {
        let mut v = vec!["phosphor".to_string(), first.to_string()];
        v.extend(fallback.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, stack("inter"));
    fonts.families.insert(FontFamily::Name(MEDIUM.into()), stack("inter-medium"));
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), stack("inter-semibold"));
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.push("phosphor".to_string());
    }
    ctx.set_fonts(fonts);
}

fn visuals_for(p: &Palette) -> egui::Visuals {
    let base = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let radius = CornerRadius::same(6);
    let wv = |bg: Color32, weak: Color32, stroke: Stroke, fg: Color32| WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: weak,
        bg_stroke: stroke,
        corner_radius: radius,
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    };
    egui::Visuals {
        dark_mode: p.dark,
        override_text_color: None,
        widgets: Widgets {
            noninteractive: wv(p.panel, p.panel, Stroke::new(1.0, p.line), p.text),
            inactive: wv(p.panel2, p.panel2, Stroke::new(1.0, p.line), p.text),
            hovered: wv(p.raised, p.raised, Stroke::new(1.0, p.faint), p.text),
            active: wv(p.raised, p.raised, Stroke::new(1.0, p.accent), p.text),
            open: wv(p.raised, p.raised, Stroke::new(1.0, p.line), p.text),
        },
        selection: Selection { bg_fill: p.accent_soft, stroke: Stroke::new(1.0, p.accent) },
        hyperlink_color: p.accent,
        faint_bg_color: p.panel2,
        extreme_bg_color: if p.dark { rgb(0x0d0f13) } else { rgb(0xffffff) },
        code_bg_color: p.panel2,
        warn_fg_color: p.warn,
        error_fg_color: p.err,
        window_corner_radius: CornerRadius::same(12),
        window_shadow: Shadow { offset: [0, 10], blur: 34, spread: 0, color: Color32::from_black_alpha(if p.dark { 150 } else { 50 }) },
        window_fill: p.panel,
        window_stroke: Stroke::new(1.0, p.line),
        window_highlight_topmost: false,
        menu_corner_radius: CornerRadius::same(8),
        panel_fill: p.bg,
        popup_shadow: Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(if p.dark { 130 } else { 40 }) },
        text_cursor: TextCursorStyle { stroke: Stroke::new(1.5, p.accent), ..TextCursorStyle::default() },
        handle_shape: HandleShape::Circle,
        slider_trailing_fill: true,
        indent_has_left_vline: false,
        collapsing_header_frame: false,
        ..base
    }
}

/// Applies the fonts' sizes, spacing and colours of one theme.
pub fn apply(ctx: &Context, dark: bool) {
    let p = if dark { DARK } else { LIGHT };
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    let theme = if dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.style_mut_of(theme, |s| {
        s.visuals = visuals_for(&p);
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, medium(13.0)),
            (TextStyle::Heading, semibold(21.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();
        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.interact_size = vec2(40.0, 24.0);
        s.spacing.window_margin = Margin::same(18);
        s.spacing.menu_margin = Margin::same(6);
        s.spacing.combo_height = 320.0;
        s.spacing.slider_width = 120.0;
        s.spacing.scroll = ScrollStyle { floating: true, bar_width: 8.0, floating_width: 3.0, floating_allocated_width: 0.0, ..ScrollStyle::floating() };
        s.interaction.selectable_labels = false;
        s.interaction.tooltip_delay = 0.35;
        s.animation_time = 0.14;
        s.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    });
}
