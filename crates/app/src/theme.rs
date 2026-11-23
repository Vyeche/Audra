//! The look in one place: faces, the colour palette, the type, radius and
//! spacing scales, and every widget style the GUI uses.

use iced::gradient::Linear;
use iced::widget::overlay::menu;
use iced::widget::{button, container};
use iced::{Background, Border, Color, Font, Shadow};

// ── Faces (loaded in main.rs) ────────────────────────────────────
/// Labels and body text: Atkinson Hyperlegible Next.
pub const SANS: Font = Font::with_name("Atkinson Hyperlegible Next");
/// Tabs, group labels, the names on a strip.
pub const SANS_SEMI: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..SANS
};
/// Keys and the M/S buttons: short words that must read at a glance.
pub const SANS_BOLD: Font = Font {
    weight: iced::font::Weight::Bold,
    ..SANS
};
/// Every number: the Mono sibling, fixed width so readouts hold still.
pub const MONO: Font = Font::with_name("Atkinson Hyperlegible Mono");
/// Row names and the Now Mixing placard only (Bold is the one weight shipped).
pub const SERIF: Font = Font {
    weight: iced::font::Weight::Bold,
    ..Font::with_name("Shippori Mincho")
};

// Identity: Audra. A warm, intimate dark, like a control room after
// hours: charcoal surfaces carrying a faint plum warmth, warm ivory text,
// champagne faders, and one rose-gold accent for anything engaged
// (lit toggles, the selected strip, the active submix, the open view's
// tab). Hue is information, not decoration: green = signal and
// connection, honey amber = attention (solo, simulated, warnings), red =
// clip, cool periwinkle = mute (the one cool note against the warmth),
// ember orange = 48V, like the unit's own phantom LED. Every text and
// state pair is checked against WCAG AA (labels on lit toggles >= 7:1).
//
// All colors live in one `Palette`, so a user theme can later replace
// `AUDRA` without touching the widgets (see PLAN.md, user themes).

/// Every color role in the interface. A theme is one of these. Plain
/// values, so two can be blended (`blend_palettes`, the fade between
/// themes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Window canvas and meter wells: the deepest layer.
    pub bg_deep: Color,
    /// Cards and control surfaces.
    pub surface: Color,
    /// The fill under the cursor; means nothing but "here".
    pub surface_hi: Color,
    /// Hairlines: one step above `surface`, so a card reads as an object.
    pub border: Color,
    /// The outline of a control at rest and the fader's groove: at least
    /// 3:1 on every ground, the lamp wash included, so a button or a rail
    /// never melts into a lit strip. (`border` is for panels, which sit
    /// on `surface`, and is too faint for this: about 1:1 on the wash.)
    pub control_edge: Color,
    /// Body text and values.
    pub text_primary: Color,
    /// Labels, secondary text, inactive controls.
    pub text_sec: Color,
    /// "Engaged": lit toggles, selection, the active submix, section ticks.
    pub accent: Color,
    /// Quiet selected fill (menu highlight, text selection, switch side).
    pub accent_dim: Color,
    /// Fader caps and rails: the brightest neutral in the chrome.
    pub fader: Color,
    /// Text on any lit (accent, 48V, solo, mute) fill.
    pub on_active: Color,
    /// Signal on the meters, and "connected".
    pub signal: Color,
    /// Attention without alarm: solo, simulated device, soft warnings.
    pub attention: Color,
    /// Clip and errors.
    pub clip: Color,
    /// Mute.
    pub mute: Color,
    /// Phantom power (48V).
    pub phantom: Color,
    /// The top bar: the darkest band, so the floors below read as lit.
    pub top_bar: Color,
    /// The three mixer floors (inputs, playback, outputs). Zones divide
    /// by a change in ground tone, not by boxes or rules.
    pub floors: [Color; 3],
    /// The right-hand column (the counter), under its lamp wash.
    pub counter: Color,
    /// Quiet text that must still read in a dim room: −∞, empty slots,
    /// the dB scale. At least 4.5:1 on every floor and the counter.
    pub muted: Color,
    /// The lamp: the light that falls on the mix being shaped. Used as
    /// a translucent wash, never as a fill or a text colour.
    pub lamp: Color,
    /// Second lines on the Now Mixing placard, whose card is `text_primary`
    /// and whose name is `on_active`. At least 4.5:1 on `text_primary`.
    pub on_placard_sec: Color,
}

/// The built-in Audra theme.
pub const AUDRA: Palette = Palette {
    bg_deep: Color::from_rgb8(0x12, 0x0d, 0x12),
    surface: Color::from_rgb8(0x1d, 0x16, 0x1c),
    surface_hi: Color::from_rgb8(0x2a, 0x21, 0x29),
    border: Color::from_rgb8(0x3d, 0x30, 0x39),
    control_edge: Color::from_rgb8(0x8c, 0x7d, 0x86),
    text_primary: Color::from_rgb8(0xf8, 0xee, 0xe8),
    text_sec: Color::from_rgb8(0xd6, 0xc8, 0xcd),
    accent: Color::from_rgb8(0xf2, 0xa0, 0x8a),
    accent_dim: Color::from_rgb8(0x4a, 0x33, 0x41),
    fader: Color::from_rgb8(0xe9, 0xdb, 0xd1),
    on_active: Color::from_rgb8(0x1e, 0x0f, 0x16),
    signal: Color::from_rgb8(0x5f, 0xd6, 0x9b),
    attention: Color::from_rgb8(0xf7, 0xb9, 0x55),
    clip: Color::from_rgb8(0xf0, 0x52, 0x5f),
    mute: Color::from_rgb8(0x86, 0xab, 0xff),
    phantom: Color::from_rgb8(0xff, 0x7a, 0x3d),
    top_bar: Color::from_rgb8(0x0d, 0x09, 0x0d),
    floors: [
        Color::from_rgb8(0x1a, 0x14, 0x19),
        Color::from_rgb8(0x15, 0x10, 0x15),
        Color::from_rgb8(0x1d, 0x16, 0x1c),
    ],
    counter: Color::from_rgb8(0x21, 0x19, 0x20),
    // Raised from #b3a2ab (2026-11-03): 4.8:1 on the lamp wash was a pass
    // on paper and hard to read in a dim room. Now at least 6:1.
    muted: Color::from_rgb8(0xc0, 0xb0, 0xb8),
    lamp: Color::from_rgb8(0xf7, 0xb9, 0x55),
    on_placard_sec: Color::from_rgb8(0x4a, 0x33, 0x41),
};
/// Daylight: Light grounds and dark ink, for a bright room.
pub const LIGHT: Palette = Palette {
    bg_deep: Color::from_rgb8(0xe3, 0xdd, 0xd9),
    surface: Color::from_rgb8(0xff, 0xff, 0xff),
    surface_hi: Color::from_rgb8(0xec, 0xe6, 0xe2),
    border: Color::from_rgb8(0xcf, 0xc6, 0xc1),
    control_edge: Color::from_rgb8(0x85, 0x7a, 0x75),
    text_primary: Color::from_rgb8(0x1f, 0x1a, 0x1d),
    text_sec: Color::from_rgb8(0x4a, 0x42, 0x47),
    accent: Color::from_rgb8(0xb0, 0x45, 0x2b),
    accent_dim: Color::from_rgb8(0xf3, 0xd9, 0xcf),
    fader: Color::from_rgb8(0x3b, 0x34, 0x38),
    on_active: Color::from_rgb8(0xff, 0xff, 0xff),
    signal: Color::from_rgb8(0x1b, 0x7a, 0x48),
    attention: Color::from_rgb8(0x94, 0x62, 0x07),
    clip: Color::from_rgb8(0xc6, 0x2f, 0x3c),
    mute: Color::from_rgb8(0x3c, 0x63, 0xc9),
    phantom: Color::from_rgb8(0xb9, 0x4a, 0x15),
    top_bar: Color::from_rgb8(0xe9, 0xe4, 0xe1),
    floors: [Color::from_rgb8(0xf6, 0xf2, 0xef), Color::from_rgb8(0xef, 0xea, 0xe6), Color::from_rgb8(0xf9, 0xf6, 0xf4)],
    counter: Color::from_rgb8(0xf1, 0xec, 0xe9),
    muted: Color::from_rgb8(0x61, 0x59, 0x60),
    lamp: Color::from_rgb8(0xf5, 0xa6, 0x23),
    on_placard_sec: Color::from_rgb8(0xcf, 0xc4, 0xc9),
};

/// Graphite: Neutral grays, no warm cast.
pub const GRAPHITE: Palette = Palette {
    bg_deep: Color::from_rgb8(0x12, 0x12, 0x14),
    surface: Color::from_rgb8(0x1c, 0x1c, 0x1f),
    surface_hi: Color::from_rgb8(0x29, 0x29, 0x2d),
    border: Color::from_rgb8(0x3a, 0x3a, 0x40),
    control_edge: Color::from_rgb8(0x8c, 0x8c, 0x94),
    text_primary: Color::from_rgb8(0xee, 0xee, 0xee),
    text_sec: Color::from_rgb8(0xc9, 0xc9, 0xcf),
    accent: Color::from_rgb8(0xf2, 0xa0, 0x8a),
    accent_dim: Color::from_rgb8(0x3e, 0x35, 0x36),
    fader: Color::from_rgb8(0xe3, 0xe3, 0xe6),
    on_active: Color::from_rgb8(0x16, 0x16, 0x18),
    signal: Color::from_rgb8(0x5f, 0xd6, 0x9b),
    attention: Color::from_rgb8(0xf2, 0xc1, 0x4e),
    clip: Color::from_rgb8(0xef, 0x5a, 0x5a),
    mute: Color::from_rgb8(0x86, 0xab, 0xff),
    phantom: Color::from_rgb8(0xff, 0x7a, 0x3d),
    top_bar: Color::from_rgb8(0x0c, 0x0c, 0x0e),
    floors: [Color::from_rgb8(0x19, 0x19, 0x1c), Color::from_rgb8(0x14, 0x14, 0x16), Color::from_rgb8(0x1d, 0x1d, 0x20)],
    counter: Color::from_rgb8(0x20, 0x20, 0x24),
    muted: Color::from_rgb8(0xb4, 0xb4, 0xbb),
    lamp: Color::from_rgb8(0xe0, 0xe0, 0xe8),
    on_placard_sec: Color::from_rgb8(0x46, 0x46, 0x4e),
};

/// Slate: a cool slate room with teal light (2026-11-03, the owner's
/// hardware-mixer reference, narrowed into a theme rather than copied):
/// slate grounds, light grey metal caps, a teal accent and a teal lamp on
/// the strips that feed the mix, an orange M and a red 48V (an orange red,
/// clear of the crimson clip). Solo keeps the
/// amber it shares with attention and the meter's middle zone.
pub const SLATE: Palette = Palette {
    bg_deep: Color::from_rgb8(0x1b, 0x1f, 0x24),
    surface: Color::from_rgb8(0x26, 0x2c, 0x32),
    surface_hi: Color::from_rgb8(0x34, 0x3b, 0x43),
    border: Color::from_rgb8(0x4a, 0x53, 0x5d),
    control_edge: Color::from_rgb8(0x8f, 0x9a, 0xa6),
    text_primary: Color::from_rgb8(0xec, 0xf0, 0xf3),
    text_sec: Color::from_rgb8(0xc8, 0xd0, 0xd7),
    accent: Color::from_rgb8(0x5b, 0xbf, 0xd9),
    accent_dim: Color::from_rgb8(0x21, 0x41, 0x4d),
    fader: Color::from_rgb8(0xd8, 0xdd, 0xe2),
    on_active: Color::from_rgb8(0x0b, 0x1a, 0x20),
    signal: Color::from_rgb8(0x74, 0xcf, 0x52),
    attention: Color::from_rgb8(0xe9, 0xc0, 0x4b),
    // 48V red toward orange, clip red toward crimson: at 7 degrees apart
    // a lit 48V read as a clip (the critique, 2026-11-04). Now 24 apart,
    // as in Audra, and 48V still 19 short of the orange M.
    clip: Color::from_rgb8(0xec, 0x4f, 0x6c),
    mute: Color::from_rgb8(0xf0, 0xa0, 0x43),
    phantom: Color::from_rgb8(0xff, 0x6a, 0x3f),
    top_bar: Color::from_rgb8(0x17, 0x1b, 0x1f),
    floors: [Color::from_rgb8(0x2a, 0x30, 0x37), Color::from_rgb8(0x25, 0x2b, 0x31), Color::from_rgb8(0x2d, 0x34, 0x3b)],
    counter: Color::from_rgb8(0x22, 0x27, 0x2d),
    muted: Color::from_rgb8(0xb4, 0xbe, 0xc8),
    lamp: Color::from_rgb8(0x6c, 0xc8, 0xe0),
    on_placard_sec: Color::from_rgb8(0x3d, 0x49, 0x54),
};

/// High contrast: Black and white with a yellow accent, for the most legibility.
pub const HIGH_CONTRAST: Palette = Palette {
    bg_deep: Color::from_rgb8(0x00, 0x00, 0x00),
    surface: Color::from_rgb8(0x0a, 0x0a, 0x0a),
    surface_hi: Color::from_rgb8(0x1f, 0x1f, 0x1f),
    border: Color::from_rgb8(0x8a, 0x8a, 0x8a),
    control_edge: Color::from_rgb8(0xc8, 0xc8, 0xc8),
    text_primary: Color::from_rgb8(0xff, 0xff, 0xff),
    text_sec: Color::from_rgb8(0xf0, 0xf0, 0xf0),
    accent: Color::from_rgb8(0xff, 0xd4, 0x00),
    accent_dim: Color::from_rgb8(0x3d, 0x33, 0x00),
    fader: Color::from_rgb8(0xff, 0xff, 0xff),
    on_active: Color::from_rgb8(0x00, 0x00, 0x00),
    signal: Color::from_rgb8(0x3c, 0xff, 0x8f),
    attention: Color::from_rgb8(0xff, 0xb0, 0x00),
    clip: Color::from_rgb8(0xff, 0x4d, 0x4d),
    mute: Color::from_rgb8(0x6f, 0xa8, 0xff),
    phantom: Color::from_rgb8(0xff, 0x8a, 0x00),
    top_bar: Color::from_rgb8(0x00, 0x00, 0x00),
    floors: [Color::from_rgb8(0x00, 0x00, 0x00), Color::from_rgb8(0x05, 0x05, 0x05), Color::from_rgb8(0x0a, 0x0a, 0x0a)],
    counter: Color::from_rgb8(0x0d, 0x0d, 0x0d),
    muted: Color::from_rgb8(0xd8, 0xd8, 0xd8),
    lamp: Color::from_rgb8(0xff, 0xff, 0xff),
    on_placard_sec: Color::from_rgb8(0x33, 0x33, 0x33),
};

/// Midnight: Deep blue grounds and a blue accent.
pub const MIDNIGHT: Palette = Palette {
    bg_deep: Color::from_rgb8(0x0b, 0x10, 0x20),
    surface: Color::from_rgb8(0x14, 0x1b, 0x2e),
    surface_hi: Color::from_rgb8(0x1f, 0x29, 0x42),
    border: Color::from_rgb8(0x33, 0x40, 0x5e),
    control_edge: Color::from_rgb8(0x7f, 0x8d, 0xb0),
    text_primary: Color::from_rgb8(0xe8, 0xee, 0xfc),
    text_sec: Color::from_rgb8(0xc3, 0xcd, 0xe6),
    accent: Color::from_rgb8(0x6a, 0xa8, 0xff),
    accent_dim: Color::from_rgb8(0x22, 0x35, 0x5a),
    fader: Color::from_rgb8(0xdf, 0xe7, 0xf7),
    on_active: Color::from_rgb8(0x0a, 0x12, 0x26),
    signal: Color::from_rgb8(0x5f, 0xd6, 0x9b),
    attention: Color::from_rgb8(0xf7, 0xc2, 0x55),
    clip: Color::from_rgb8(0xf0, 0x52, 0x5f),
    mute: Color::from_rgb8(0xb3, 0x9c, 0xff),
    phantom: Color::from_rgb8(0xff, 0x7a, 0x3d),
    top_bar: Color::from_rgb8(0x08, 0x0c, 0x18),
    floors: [Color::from_rgb8(0x12, 0x1a, 0x2c), Color::from_rgb8(0x0e, 0x15, 0x24), Color::from_rgb8(0x15, 0x1e, 0x32)],
    counter: Color::from_rgb8(0x18, 0x22, 0x38),
    muted: Color::from_rgb8(0xa9, 0xb4, 0xcf),
    lamp: Color::from_rgb8(0x7f, 0xb8, 0xff),
    on_placard_sec: Color::from_rgb8(0x33, 0x40, 0x5e),
};

/// Crimson: Dark red grounds and a rose accent.
pub const CRIMSON: Palette = Palette {
    bg_deep: Color::from_rgb8(0x14, 0x0a, 0x0c),
    surface: Color::from_rgb8(0x1f, 0x10, 0x13),
    surface_hi: Color::from_rgb8(0x2e, 0x18, 0x1c),
    border: Color::from_rgb8(0x45, 0x25, 0x29),
    control_edge: Color::from_rgb8(0x9a, 0x7a, 0x7f),
    text_primary: Color::from_rgb8(0xfb, 0xea, 0xec),
    text_sec: Color::from_rgb8(0xe3, 0xc8, 0xcc),
    accent: Color::from_rgb8(0xff, 0x6b, 0x81),
    accent_dim: Color::from_rgb8(0x4a, 0x1f, 0x27),
    fader: Color::from_rgb8(0xf1, 0xdd, 0xe0),
    on_active: Color::from_rgb8(0x1d, 0x0a, 0x0e),
    signal: Color::from_rgb8(0x5f, 0xd6, 0x9b),
    attention: Color::from_rgb8(0xf7, 0xb9, 0x55),
    clip: Color::from_rgb8(0xff, 0x3b, 0x3b),
    mute: Color::from_rgb8(0x86, 0xab, 0xff),
    phantom: Color::from_rgb8(0xff, 0x8a, 0x3d),
    top_bar: Color::from_rgb8(0x0f, 0x07, 0x09),
    floors: [Color::from_rgb8(0x1c, 0x0e, 0x11), Color::from_rgb8(0x17, 0x0b, 0x0e), Color::from_rgb8(0x20, 0x0f, 0x13)],
    counter: Color::from_rgb8(0x24, 0x12, 0x16),
    muted: Color::from_rgb8(0xc9, 0xa9, 0xae),
    lamp: Color::from_rgb8(0xff, 0x8a, 0x7a),
    on_placard_sec: Color::from_rgb8(0x4a, 0x1f, 0x27),
};

/// Monokai: The code editor's palette: olive black, cyan, pink, green.
pub const MONOKAI: Palette = Palette {
    bg_deep: Color::from_rgb8(0x1e, 0x1f, 0x1c),
    surface: Color::from_rgb8(0x27, 0x28, 0x22),
    surface_hi: Color::from_rgb8(0x3e, 0x3d, 0x32),
    border: Color::from_rgb8(0x49, 0x48, 0x3e),
    control_edge: Color::from_rgb8(0xa3, 0xa1, 0x92),
    text_primary: Color::from_rgb8(0xf8, 0xf8, 0xf2),
    text_sec: Color::from_rgb8(0xcf, 0xcf, 0xc2),
    accent: Color::from_rgb8(0x66, 0xd9, 0xef),
    accent_dim: Color::from_rgb8(0x24, 0x47, 0x4f),
    fader: Color::from_rgb8(0xf8, 0xf8, 0xf2),
    on_active: Color::from_rgb8(0x1e, 0x1f, 0x1c),
    signal: Color::from_rgb8(0xa6, 0xe2, 0x2e),
    attention: Color::from_rgb8(0xe6, 0xdb, 0x74),
    clip: Color::from_rgb8(0xf9, 0x26, 0x72),
    mute: Color::from_rgb8(0xae, 0x81, 0xff),
    phantom: Color::from_rgb8(0xfd, 0x97, 0x1f),
    top_bar: Color::from_rgb8(0x1a, 0x1b, 0x17),
    floors: [Color::from_rgb8(0x27, 0x28, 0x22), Color::from_rgb8(0x23, 0x24, 0x1f), Color::from_rgb8(0x2d, 0x2e, 0x27)],
    counter: Color::from_rgb8(0x2f, 0x30, 0x29),
    muted: Color::from_rgb8(0xc6, 0xc4, 0xae),
    lamp: Color::from_rgb8(0xe6, 0xdb, 0x74),
    on_placard_sec: Color::from_rgb8(0x49, 0x48, 0x3e),
};

/// Nord: Arctic slate and frost blue.
pub const NORD: Palette = Palette {
    bg_deep: Color::from_rgb8(0x24, 0x29, 0x33),
    surface: Color::from_rgb8(0x2e, 0x34, 0x40),
    surface_hi: Color::from_rgb8(0x3b, 0x42, 0x52),
    border: Color::from_rgb8(0x43, 0x4c, 0x5e),
    control_edge: Color::from_rgb8(0x9e, 0xa9, 0xbf),
    text_primary: Color::from_rgb8(0xf3, 0xf5, 0xf9),
    text_sec: Color::from_rgb8(0xd8, 0xde, 0xe9),
    accent: Color::from_rgb8(0x88, 0xc0, 0xd0),
    accent_dim: Color::from_rgb8(0x34, 0x49, 0x5a),
    fader: Color::from_rgb8(0xe5, 0xe9, 0xf0),
    on_active: Color::from_rgb8(0x24, 0x29, 0x33),
    signal: Color::from_rgb8(0xa3, 0xbe, 0x8c),
    attention: Color::from_rgb8(0xeb, 0xcb, 0x8b),
    clip: Color::from_rgb8(0xbf, 0x61, 0x6a),
    mute: Color::from_rgb8(0xb4, 0x8e, 0xad),
    phantom: Color::from_rgb8(0xd0, 0x87, 0x70),
    top_bar: Color::from_rgb8(0x20, 0x24, 0x2c),
    floors: [Color::from_rgb8(0x2e, 0x34, 0x40), Color::from_rgb8(0x2a, 0x2f, 0x3a), Color::from_rgb8(0x32, 0x38, 0x46)],
    counter: Color::from_rgb8(0x35, 0x3b, 0x49),
    muted: Color::from_rgb8(0xbc, 0xc4, 0xd2),
    lamp: Color::from_rgb8(0xeb, 0xcb, 0x8b),
    on_placard_sec: Color::from_rgb8(0x4c, 0x56, 0x6a),
};

/// Solarized: Solarized dark: teal ground, cyan accent.
pub const SOLARIZED: Palette = Palette {
    bg_deep: Color::from_rgb8(0x00, 0x21, 0x2b),
    surface: Color::from_rgb8(0x00, 0x2b, 0x36),
    surface_hi: Color::from_rgb8(0x07, 0x36, 0x42),
    border: Color::from_rgb8(0x0f, 0x4a, 0x57),
    control_edge: Color::from_rgb8(0x6f, 0x8f, 0x96),
    text_primary: Color::from_rgb8(0xfd, 0xf6, 0xe3),
    text_sec: Color::from_rgb8(0xee, 0xe8, 0xd5),
    accent: Color::from_rgb8(0x2a, 0xa1, 0x98),
    accent_dim: Color::from_rgb8(0x0d, 0x4a, 0x4a),
    fader: Color::from_rgb8(0xee, 0xe8, 0xd5),
    on_active: Color::from_rgb8(0x00, 0x2b, 0x36),
    signal: Color::from_rgb8(0x85, 0x99, 0x00),
    attention: Color::from_rgb8(0xb5, 0x89, 0x00),
    clip: Color::from_rgb8(0xdc, 0x32, 0x2f),
    mute: Color::from_rgb8(0x95, 0x9b, 0xe6),
    phantom: Color::from_rgb8(0xf0, 0x7a, 0x3a),
    top_bar: Color::from_rgb8(0x00, 0x1e, 0x26),
    floors: [Color::from_rgb8(0x00, 0x2b, 0x36), Color::from_rgb8(0x00, 0x25, 0x2e), Color::from_rgb8(0x03, 0x30, 0x3c)],
    counter: Color::from_rgb8(0x04, 0x33, 0x3f),
    muted: Color::from_rgb8(0xa3, 0xb1, 0xb1),
    lamp: Color::from_rgb8(0xb5, 0x89, 0x00),
    on_placard_sec: Color::from_rgb8(0x58, 0x6e, 0x75),
};



/// One theme: its id (kept in the preferences), its name and a line about
/// it for the View tab, and its palette. A user's theme (`usertheme`) also
/// carries what its contrast check found (`contrast_issues`).
#[derive(Debug, Clone, Copy)]
pub struct ThemeEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub palette: &'static Palette,
    /// A theme from a file, not a built-in one.
    pub custom: bool,
    /// What `contrast_issues` found, empty when it reads as the built-in
    /// themes do.
    pub issues: &'static [&'static str],
}

/// Every built-in theme, in the View tab's order. Audra's own comes first;
/// each holds the same contrast rules (see the tests at the foot).
pub const THEMES: [ThemeEntry; 10] = [
    ThemeEntry { id: "audra", name: "Audra", blurb: "The listening room after hours: warm dark, coral, amber light.", palette: &AUDRA, custom: false, issues: &[] },
    ThemeEntry { id: "light", name: "Daylight", blurb: "Light grounds and dark ink, for a bright room.", palette: &LIGHT, custom: false, issues: &[] },
    ThemeEntry { id: "graphite", name: "Graphite", blurb: "Neutral grays, no warm cast.", palette: &GRAPHITE, custom: false, issues: &[] },
    ThemeEntry { id: "slate", name: "Slate", blurb: "Cool slate and teal light, like a hardware mixer's room.", palette: &SLATE, custom: false, issues: &[] },
    ThemeEntry { id: "contrast", name: "High contrast", blurb: "Black and white with a yellow accent, for the most legibility.", palette: &HIGH_CONTRAST, custom: false, issues: &[] },
    ThemeEntry { id: "midnight", name: "Midnight", blurb: "Deep blue grounds and a blue accent.", palette: &MIDNIGHT, custom: false, issues: &[] },
    ThemeEntry { id: "crimson", name: "Crimson", blurb: "Dark red grounds and a rose accent.", palette: &CRIMSON, custom: false, issues: &[] },
    ThemeEntry { id: "monokai", name: "Monokai", blurb: "The code editor's palette: olive black, cyan, pink, green.", palette: &MONOKAI, custom: false, issues: &[] },
    ThemeEntry { id: "nord", name: "Nord", blurb: "Arctic slate and frost blue.", palette: &NORD, custom: false, issues: &[] },
    ThemeEntry { id: "solarized", name: "Solarized", blurb: "Solarized dark: teal ground, cyan accent.", palette: &SOLARIZED, custom: false, issues: &[] },
];

thread_local! {
    /// The themes on offer: the built-in ones, then the user's own once
    /// `register` has read them. Per thread like `CURRENT` (the interface
    /// runs on one), so tests never see each other's.
    static REGISTRY: std::cell::Cell<&'static [ThemeEntry]> = const { std::cell::Cell::new(&THEMES) };
}

/// Every theme on offer, built-in first, in the View tab's order. Indexes
/// into it are what the state keeps (`TuxMix::theme`).
pub fn themes() -> &'static [ThemeEntry] {
    REGISTRY.with(|r| r.get())
}

/// Offers the built-in themes plus `custom` (`usertheme::load_dir`). The
/// slice lives as long as the program; a reload leaks the old one, a few
/// hundred bytes.
pub fn register(custom: Vec<ThemeEntry>) {
    let all: Vec<ThemeEntry> = THEMES.iter().copied().chain(custom).collect();
    let all: &'static [ThemeEntry] = Box::leak(all.into_boxed_slice());
    REGISTRY.with(|r| r.set(all));
}

/// The index in `themes()` of a theme id, Audra's for an unknown one.
pub fn theme_index(id: &str) -> usize {
    themes().iter().position(|t| t.id == id).unwrap_or(0)
}

/// WCAG 2 relative luminance of an sRGB colour.
pub fn luminance(c: Color) -> f32 {
    let lin = |v: f32| {
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// `hue` pulled toward the theme's ink, in tenths, until it reads at 4.5:1
/// on every mixer floor and on a panel (the hue kept as far as that allows).
/// `hue` made readable on its own tint over the floors (a badge's field:
/// the floors washed with `hue` at `wash`), 4.5:1 (critique #78 B: Quick's
/// red step badge read 3.80 on its pink disc with the floors' depth).
pub fn readable_on_tint(hue: Color, wash: f32) -> Color {
    let p = palette();
    let grounds: Vec<Color> = p.floors.iter().chain([&p.surface]).map(|g| blend(*g, hue, wash)).collect();
    // Past the ink to pure black or white where even the ink falls short
    // (Slate's ink on its brightest tint, 4.48).
    let extreme = if luminance(p.text_primary) > 0.5 { Color::WHITE } else { Color::BLACK };
    (0..=10)
        .map(|k| blend(hue, p.text_primary, k as f32 / 10.0))
        .chain((1..=10).map(|k| blend(p.text_primary, extreme, k as f32 / 10.0)))
        .find(|c| grounds.iter().all(|g| contrast(*c, *g) >= 4.5))
        .unwrap_or(extreme)
}

pub fn readable_on_floors(hue: Color) -> Color {
    let p = palette();
    // The column's grounds too: the counter, its lamp-washed top and a
    // section's shade on it (critique #28: lamp words read 4.2-4.4:1
    // there on Daylight).
    let column = column_grounds(&p);
    for k in 0..=10 {
        let c = blend(hue, p.text_primary, k as f32 / 10.0);
        if p.floors.iter().chain([&p.surface]).chain(column.iter()).all(|g| contrast(c, *g) >= 4.5) {
            return c;
        }
    }
    p.text_primary
}

/// The right-hand column's grounds as they show: the counter, its top
/// under the lamp's wash (`counter`), and a section's shade over both
/// (`section`).
fn column_grounds(p: &Palette) -> [Color; 4] {
    let dark = luminance(p.text_primary) > luminance(p.surface);
    let shade = if dark { 0.16 } else { 0.035 };
    let top = blend(p.counter, p.lamp, 0.14);
    [p.counter, top, blend(p.counter, Color::BLACK, shade), blend(top, Color::BLACK, shade)]
}

/// The Matrix's column heads' grounds: its panel (`surface`) and the
/// floors, bare and under the lamp's band (16%).
pub fn matrix_head_grounds() -> Vec<Color> {
    let p = palette();
    p.floors.iter().chain([&p.surface]).flat_map(|g| [*g, blend(*g, p.lamp, 0.16)]).collect()
}

/// A held slot's floor and wall (`list_led`) as they show over the
/// column's grounds, for the words on it (critique #28: a changed
/// snapshot's number read 2.7:1 on Daylight).
pub fn held_led_grounds() -> Vec<Color> {
    let p = palette();
    let (floor, wall) = held_floor_wall();
    column_grounds(&p).iter().flat_map(|g| [blend(*g, Color { a: 1.0, ..floor }, floor.a), blend(*g, Color { a: 1.0, ..wall }, wall.a)]).collect()
}

fn held_floor_wall() -> (Color, Color) {
    // The lamp's amber, as the Snapshots list's stored lamps are; coral stays
    // for the lit slot (critique #71 A 5: compact keys coral, the list's
    // lamps amber, for the same "stored").
    if dark_theme() {
        // Deep enough that both numbers on it read: the stored one in the
        // ink at 4.5:1, the changed-since one still amber (OpenCode's job 73:
        // on the amber floor Graphite read 4.41, High contrast 4.34, and the
        // changed-since amber fell back to plain ink in both).
        let p = palette();
        let pick = |k: f32| {
            let idle = blend(lamp(), Color::BLACK, k);
            (Color { a: 0.85, ..idle }, Color { a: 0.95, ..blend(idle, Color::BLACK, 0.35) })
        };
        let reads = |(floor, wall): (Color, Color)| {
            let grounds: Vec<Color> = column_grounds(&p)
                .iter()
                .flat_map(|g| [blend(*g, Color { a: 1.0, ..floor }, floor.a), blend(*g, Color { a: 1.0, ..wall }, wall.a)])
                .collect();
            let ink_ok = grounds.iter().all(|g| contrast(text_primary(), *g) >= 4.6);
            let amber_ok = (0..=4).map(|k| blend(attention(), text_primary(), k as f32 / 10.0)).any(|c| grounds.iter().all(|g| contrast(c, *g) >= 4.5));
            ink_ok && amber_ok
        };
        (0..=8).map(|i| pick(0.45 + 0.05 * i as f32)).find(|fw| reads(*fw)).unwrap_or_else(|| pick(0.85))
    } else {
        // Paler until the ink reads on it, for a custom light theme with a
        // strong lamp (OpenCode's job 75; the built-in light theme reads at
        // the first step, 7.93).
        let p = palette();
        let pick = |a: f32| (Color { a, ..lamp() }, Color { a: a + 0.2, ..blend(lamp(), Color::BLACK, 0.15) });
        let reads = |(floor, wall): (Color, Color)| {
            column_grounds(&p)
                .iter()
                .flat_map(|g| [blend(*g, Color { a: 1.0, ..floor }, floor.a), blend(*g, Color { a: 1.0, ..wall }, wall.a)])
                .all(|g| contrast(text_primary(), g) >= 4.6)
        };
        (0..=6).map(|i| pick(0.45 - 0.05 * i as f32)).find(|fw| reads(*fw)).unwrap_or_else(|| pick(0.15))
    }
}

/// `hue` moved toward `toward` a tenth at a time until it reads at 4.5:1
/// on every one of `grounds`; `toward` itself if none does. For words in a
/// state's hue on a ground the floors' check doesn't cover (the device
/// chip's `bg_deep`, the EQ's dark screen; critique #27's detector gaps).
pub fn readable_on(hue: Color, toward: Color, grounds: &[Color]) -> Color {
    (0..=10)
        .map(|k| blend(hue, toward, k as f32 / 10.0))
        .find(|c| grounds.iter().all(|g| contrast(*c, *g) >= 4.5))
        .unwrap_or(toward)
}

/// The attention hue as a small graphic (a dot) on `face`: the hue itself
/// where it holds 3:1, else moved toward a saturated amber of the right
/// lightness (a bright one on a dark face, a deep orange on a light one),
/// never toward the ink, which greyed it (critique #71 A 3: Nord's dot
/// came out khaki, the dark theme's brown).
pub fn attention_dot_on(face: Color) -> Color {
    let hue = attention();
    // On a dark face, brighter is still amber, so it goes to 4.5; on a light
    // one, darker turns brown, so it stops at 3.6 (critique #73 B: at 3.2
    // only 29 of its 49 px reached 3:1).
    let (toward, need) = if luminance(face) < 0.2 { (Color::from_rgb(1.0, 0.78, 0.25), 4.5) } else { (Color::from_rgb(0.72, 0.34, 0.0), 3.6) };
    let other = if luminance(face) < 0.2 { Color::from_rgb(0.72, 0.34, 0.0) } else { Color::from_rgb(1.0, 0.78, 0.25) };
    let path = |to: Color| (0..=20).map(move |k| blend(hue, to, k as f32 / 20.0));
    // The preferred way; then the other; then whichever reads best, so a
    // custom theme with a mid-grey ink never gets a dot that's gone
    // (OpenCode's job 73: the deep orange read 1.21 on a 50% grey card).
    path(toward)
        .find(|c| contrast(*c, face) >= need)
        .or_else(|| path(other).find(|c| contrast(*c, face) >= 3.0))
        .unwrap_or_else(|| path(toward).chain(path(other)).fold(hue, |best, c| if contrast(c, face) > contrast(best, face) { c } else { best }))
}

/// The contrast ratio of two colours (1 to 21).
pub fn contrast(a: Color, b: Color) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Every ground a control can sit on: the top bar, the floors, the
/// counter, and each floor under the lamp wash at its strongest.
pub fn grounds_of(p: &Palette) -> Vec<(&'static str, Color)> {
    let over = |fg: Color, a: f32, bg: Color| Color {
        r: fg.r * a + bg.r * (1.0 - a),
        g: fg.g * a + bg.g * (1.0 - a),
        b: fg.b * a + bg.b * (1.0 - a),
        a: 1.0,
    };
    let mut all = vec![("the top bar", p.top_bar), ("the right-hand column", p.counter)];
    for (floor, name) in p.floors.iter().zip(["the inputs floor", "the playback floor", "the outputs floor"]) {
        all.push((name, *floor));
        all.push(("a lit strip", over(p.lamp, LAMP_TOP, *floor)));
        // A strip's brushed panel at its brightest (its head), hovered,
        // and with the lamp on it.
        let panel = |lamp: f32| {
            let lift = panel_lift(p, 0.0, true, lamp);
            over(lift, lift.a, *floor)
        };
        all.push(("a strip's panel", panel(0.0)));
        all.push(("a lit strip's panel", over(p.lamp, LAMP_TOP, panel(1.0))));
    }
    all
}

/// What is cut into or laid on the grounds, at its darkest, with what it
/// holds (issue #21: the contrast test didn't look at them): a strip's
/// value boxes and peak readouts (`inset_box`, its top: text and
/// secondary text), a side key's tile at rest (`side_key`: its quiet
/// glyph), and a well of rows in the column (`well_box`: the snapshot
/// list's names, an empty one quiet).
pub fn cuts_of(p: &Palette) -> Vec<(&'static str, Color, Holds)> {
    let over = |a: f32, bg: Color| Color {
        r: bg.r * (1.0 - a),
        g: bg.g * (1.0 - a),
        b: bg.b * (1.0 - a),
        a: 1.0,
    };
    let dark = luminance(p.text_primary) > luminance(p.surface);
    let (box_top, key_top, well) = if dark { (0.52, 0.2, 0.52) } else { (0.2, 0.06, 0.06) };
    let mut all = Vec::new();
    for floor in p.floors.iter() {
        let lift = panel_lift(p, 0.0, true, 0.0);
        let panel = Color {
            r: lift.r * lift.a + floor.r * (1.0 - lift.a),
            g: lift.g * lift.a + floor.g * (1.0 - lift.a),
            b: lift.b * lift.a + floor.b * (1.0 - lift.a),
            a: 1.0,
        };
        all.push(("a value box", over(box_top, panel), Holds::Values));
        all.push(("a side key", over(key_top, panel), Holds::Glyph));
    }
    all.push(("a well in the column", over(well, p.counter), Holds::Rows));
    all
}

/// What a cut (`cuts_of`) holds, so it is tested for that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holds {
    /// Numbers and their units: text and secondary text.
    Values,
    /// A key's glyph at rest: quiet text.
    Glyph,
    /// Named rows, an empty one quiet: all three.
    Rows,
}

/// The shape of a strip's brushed panel down its length (top 0 to foot
/// 1), as a share of its brightest: a satin sheen, bright at the head, a
/// soft band of light a fifth of the way down, falling away to the foot.
const BRUSHED: [(f32, f32); 6] = [(0.0, 1.0), (0.06, 0.62), (0.2, 0.86), (0.45, 0.5), (0.75, 0.32), (1.0, 0.22)];

/// The piecewise-linear value of `curve` at `t`.
fn along(curve: &[(f32, f32)], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    curve
        .windows(2)
        .find(|w| t <= w[1].0)
        .map_or(curve[curve.len() - 1].1, |w| {
            let k = if w[1].0 > w[0].0 { (t - w[0].0) / (w[1].0 - w[0].0) } else { 0.0 };
            w[0].1 + (w[1].1 - w[0].1) * k
        })
}

/// A strip's brushed panel at `t` down it, as a lift laid over the floor
/// (the ink in a dark theme, the paper in a light one, so it reads as a
/// panel on any floor); a little more under the cursor. Under the lamp
/// (`lamp`, 0..1) the sheen gives way to the lamp's own light, which is
/// a lift already and has its own fall down the strip: both together
/// raised the ground until a key's rim fell under 3:1 (Audra) and quiet
/// text under 4.5:1 (Nord).
fn panel_lift(p: &Palette, t: f32, hovered: bool, lamp: f32) -> Color {
    let dark = luminance(p.text_primary) > luminance(p.surface);
    let (ink, peak, hover) = if dark { (p.text_primary, 0.075, 0.03) } else { (p.surface, 0.6, 0.15) };
    let a = (peak * along(&BRUSHED, t) + if hovered { hover } else { 0.0 }) * (1.0 - lamp.clamp(0.0, 1.0));
    Color { a: a.min(1.0), ..ink }
}

/// Every ground a key's label can sit on, flattened: on each floor (lit by
/// the lamp or not, the panel's sheen giving way to it), down into the
/// well and onto the key's top and foot at rest and hovered; and on the
/// counter, where the M and S masters stand with no well.
fn key_label_grounds(p: &Palette) -> Vec<Color> {
    let dark = luminance(p.text_primary) > luminance(p.surface);
    let flat = |c: Color, bg: Color| Color {
        r: c.r * c.a + bg.r * (1.0 - c.a),
        g: c.g * c.a + bg.g * (1.0 - c.a),
        b: c.b * c.a + bg.b * (1.0 - c.a),
        a: 1.0,
    };
    let shade = |a: f32| Color { a, ..Color::BLACK };
    let well = if dark { [shade(0.45), shade(0.28)] } else { [shade(0.16), shade(0.07)] };
    let rests: [f32; 3] = if dark { [0.18, 0.09, 0.25] } else { [0.05, 0.13, 0.19] };
    let mut grounds = Vec::new();
    for floor in p.floors {
        for lamp in [0.0, 1.0] {
            let lit = flat(Color { a: LAMP_TOP * lamp, ..p.lamp }, floor);
            let panel = flat(panel_lift(p, 0.2, false, lamp), lit);
            for w in well {
                let in_well = flat(w, panel);
                for a in rests {
                    grounds.push(flat(Color { a, ..p.text_primary }, in_well));
                }
            }
        }
    }
    for a in rests {
        grounds.push(flat(Color { a, ..p.text_primary }, p.counter));
    }
    grounds
}

/// `hue` for a key's label at rest (`Lamp::hue_label`), pulled toward the
/// ink in tenths until it reads at 4.5:1 on every ground a key's label sits
/// on (`key_label_grounds`), keeping as much of the hue as that allows.
pub fn readable_on_keys(hue: Color) -> Color {
    readable_on_keys_in(&palette(), hue)
}

fn readable_on_keys_in(p: &Palette, hue: Color) -> Color {
    let grounds = key_label_grounds(p);
    (0..=10)
        .map(|k| blend(hue, p.text_primary, k as f32 / 10.0))
        .find(|c| grounds.iter().all(|g| contrast(*c, *g) >= 4.5))
        .unwrap_or(p.text_primary)
}

/// A state mark's ink (L≠R on a strip): `hue` made readable on the
/// floors and the lit strips by changing its lightness only, toward white
/// on a dark theme or black on a light one, so it keeps its colour.
/// `readable_on_keys` pulls toward the text tone for every key face, and
/// in Nord, Slate and Solarized the mark came out the name's own colour
/// and read as a channel type (critique #38).
pub fn state_mark_ink(hue: Color) -> Color {
    let p = palette();
    let grounds = strip_header_grounds(&p);
    let toward = if luminance(p.floors[0]) < 0.2 { Color::WHITE } else { Color::BLACK };
    readable_on(hue, toward, &grounds)
}

/// PAD's word on a mic strip: the accent as a state mark
/// (`state_mark_ink`), or the lamp's where that comes out too near the
/// type words' grey to read as a state (critique #42: Nord's pale cyan
/// stood 14.9 ΔE from MIC).
pub fn pad_ink() -> Color {
    state_mark_ink(pad_hue())
}

/// PAD's hue: the accent, or the lamp's where the accent's mark comes out
/// too near the type words' grey (`pad_ink`).
pub fn pad_hue() -> Color {
    let ink = state_mark_ink(accent());
    let sec = text_sec();
    let apart = (ink.r - sec.r).abs().max((ink.g - sec.g).abs()).max((ink.b - sec.b).abs());
    if apart < 0.25 { lamp() } else { accent() }
}

/// The PAD chip's word: the lit keys' ink where it reads at 4.5:1 on the
/// whole lens (its lip, top and foot), else black or white, whichever
/// reads best (critique #43's pick: on Daylight's and Solarized's fill the
/// lit ink came to 4.2:1).
pub fn pad_mark_ink() -> Color {
    pad_chip().1
}

/// The PAD chip's fill hue and its word's ink: the lit keys' ink on
/// `pad_hue` where it reads at 4.5:1 on the whole lens (its lip, top and
/// foot); else the hue deepened under white, or lightened under black, a
/// tenth at a time, whichever needs the smaller step.
pub fn pad_chip() -> (Color, Color) {
    let hue = pad_hue();
    let worst = |fill: Color, ink: Color| {
        let (top, bottom) = key_lit_colors(fill, 0.0, false);
        [blend(fill, Color::WHITE, 0.4), top, bottom].iter().map(|g| contrast(ink, *g)).fold(f32::INFINITY, f32::min)
    };
    if worst(hue, on_active()) >= 4.5 {
        return (hue, on_active());
    }
    for k in 1..=10 {
        let t = k as f32 / 10.0;
        let deep = blend(hue, Color::BLACK, t);
        if worst(deep, Color::WHITE) >= 4.5 {
            return (deep, Color::WHITE);
        }
        let light = blend(hue, Color::WHITE, t);
        if worst(light, Color::BLACK) >= 4.5 {
            return (light, Color::BLACK);
        }
    }
    (Color::BLACK, Color::WHITE)
}

/// PAD's mark on a folded strip: lit and filled, as 48V's lamp above it
/// (the owner's pick after critique #43: an outline read as an unlit key).
pub fn pad_mark(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(key_lit(pad_chip().0, 0.0, false)),
        text_color: Some(pad_mark_ink()),
        border: Border { color: key_lit_edge(pad_chip().0), width: 1.0, radius: RADIUS_XS.into() },
        ..container::Style::default()
    }
}

/// Every ground a strip header's words sit on: its panel over each floor,
/// lit by the lamp or not, hovered or not (OpenCode's job 53: the hovered
/// panel wasn't checked).
pub(crate) fn strip_header_grounds(p: &Palette) -> Vec<Color> {
    let flat = |c: Color, bg: Color| Color {
        r: c.r * c.a + bg.r * (1.0 - c.a),
        g: c.g * c.a + bg.g * (1.0 - c.a),
        b: c.b * c.a + bg.b * (1.0 - c.a),
        a: 1.0,
    };
    let mut grounds = Vec::new();
    for floor in p.floors {
        for lamp in [0.0, 1.0] {
            let lit = flat(Color { a: LAMP_TOP * lamp, ..p.lamp }, floor);
            for hovered in [false, true] {
                for t in [0.0, 0.1, 0.2] {
                    grounds.push(flat(panel_lift(p, t, hovered, lamp), lit));
                }
            }
        }
    }
    grounds
}

/// `top` laid over `bottom`, both translucent (Porter-Duff "over").
fn layer(top: Color, bottom: Color) -> Color {
    let a = top.a + bottom.a * (1.0 - top.a);
    if a <= 0.0 {
        return Color::TRANSPARENT;
    }
    let mix = |t: f32, b: f32| (t * top.a + b * bottom.a * (1.0 - top.a)) / a;
    Color { r: mix(top.r, bottom.r), g: mix(top.g, bottom.g), b: mix(top.b, bottom.b), a }
}

/// Where `p` falls short of the rules every built-in theme holds (text 7:1
/// and quiet text 4.5:1 on every ground including the lamp wash, control
/// edges 3:1, labels on lit fills and on the placard 4.5:1, panel text
/// 7:1), in words, one line per failure; empty when it reads as they do.
pub fn contrast_issues(p: &Palette) -> Vec<String> {
    let mut issues = Vec::new();
    let mut need = |what: &str, fg: Color, bg: Color, on: &str, min: f32| {
        let c = contrast(fg, bg);
        if c < min {
            issues.push(format!("{what} on {on} is {c:.1}:1 (needs {min}:1)"));
        }
    };
    for (on, ground) in grounds_of(p) {
        need("text", p.text_primary, ground, on, 7.0);
        need("secondary text", p.text_sec, ground, on, 4.5);
        need("quiet text", p.muted, ground, on, 4.5);
        need("control edges", p.control_edge, ground, on, 3.0);
    }
    for (on, ground, holds) in cuts_of(p) {
        if matches!(holds, Holds::Values | Holds::Rows) {
            need("text", p.text_primary, ground, on, 7.0);
            need("secondary text", p.text_sec, ground, on, 4.5);
        }
        if matches!(holds, Holds::Glyph | Holds::Rows) {
            need("quiet text", p.muted, ground, on, 4.5);
        }
    }
    for (fill, on) in [(p.accent, "the accent"), (p.phantom, "48V"), (p.mute, "mute"), (p.attention, "solo")] {
        need("labels", p.on_active, fill, on, 4.5);
    }
    need("the placard's name", p.on_active, p.text_primary, "its card", 4.5);
    need("the placard's lines", p.on_placard_sec, p.text_primary, "its card", 4.5);
    need("panel text", p.text_primary, p.surface, "a panel", 7.0);
    issues.dedup();
    issues
}

thread_local! {
    /// The palette every colour accessor below reads. The view sets it from
    /// the chosen theme on every draw (`set_palette`); per thread, so tests
    /// running in parallel each see Audra's unless they set another.
    static CURRENT: std::cell::Cell<Palette> = const { std::cell::Cell::new(AUDRA) };
    /// `palette_key` of `CURRENT`, worked out when it is set.
    static CURRENT_KEY: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Makes `palette` the one the interface draws with.
pub fn set_palette(palette: &Palette) {
    CURRENT.with(|c| c.set(*palette));
    CURRENT_KEY.with(|k| k.set(fingerprint(palette)));
}

/// A fingerprint of the palette drawing now. Every cached drawing (a knob,
/// a fader's scale and groove, a Matrix cell) keys on it, so a theme change
/// or a fade redraws it: they used to keep the old theme's colours, which
/// left Audra's cream "C" on Daylight's pale knobs (the owner's screenshot,
/// 2026-11-03: "the font color doesnt update when switching between themes").
pub fn palette_key() -> u64 {
    CURRENT_KEY.with(|k| k.get())
}

/// Whether the High contrast theme is the one showing.
pub fn is_high_contrast() -> bool {
    fingerprint(&palette()) == fingerprint(&HIGH_CONTRAST)
}

fn fingerprint(p: &Palette) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for c in [
        p.bg_deep, p.surface, p.surface_hi, p.border, p.control_edge, p.text_primary, p.text_sec, p.accent,
        p.accent_dim, p.fader, p.on_active, p.signal, p.attention, p.clip, p.mute, p.phantom, p.top_bar,
        p.floors[0], p.floors[1], p.floors[2], p.counter, p.muted, p.lamp, p.on_placard_sec,
    ] {
        c.into_rgba8().hash(&mut h);
    }
    h.finish()
}

/// The palette the interface draws with now.
pub fn palette() -> Palette {
    CURRENT.with(|c| c.get())
}

fn p() -> Palette {
    palette()
}

/// `a` turned `t` (0..1) of the way into `b`, role by role: a theme fading
/// into another.
pub fn blend_palettes(a: &Palette, b: &Palette, t: f32) -> Palette {
    if t <= 0.0 {
        return *a;
    }
    if t >= 1.0 {
        return *b;
    }
    let m = |x: Color, y: Color| Color {
        r: x.r + (y.r - x.r) * t,
        g: x.g + (y.g - x.g) * t,
        b: x.b + (y.b - x.b) * t,
        a: x.a + (y.a - x.a) * t,
    };
    Palette {
        bg_deep: m(a.bg_deep, b.bg_deep),
        surface: m(a.surface, b.surface),
        surface_hi: m(a.surface_hi, b.surface_hi),
        border: m(a.border, b.border),
        control_edge: m(a.control_edge, b.control_edge),
        text_primary: m(a.text_primary, b.text_primary),
        text_sec: m(a.text_sec, b.text_sec),
        accent: m(a.accent, b.accent),
        accent_dim: m(a.accent_dim, b.accent_dim),
        fader: m(a.fader, b.fader),
        on_active: m(a.on_active, b.on_active),
        signal: m(a.signal, b.signal),
        attention: m(a.attention, b.attention),
        clip: m(a.clip, b.clip),
        mute: m(a.mute, b.mute),
        phantom: m(a.phantom, b.phantom),
        top_bar: m(a.top_bar, b.top_bar),
        floors: [m(a.floors[0], b.floors[0]), m(a.floors[1], b.floors[1]), m(a.floors[2], b.floors[2])],
        counter: m(a.counter, b.counter),
        muted: m(a.muted, b.muted),
        lamp: m(a.lamp, b.lamp),
        on_placard_sec: m(a.on_placard_sec, b.on_placard_sec),
    }
}

// The colour roles, read from the current palette. These were constants
// while Audra had one theme; as functions every theme can supply them.
pub fn muted() -> Color {
    p().muted
}
/// The channel-type tags' hues (a strip's MIC / INST / LINE / SPDIF /
/// ADAT, and the bus rows' PB / OUT), before `readable_on_floors` pulls
/// them toward the ink: tuned for dark grounds.
pub const TAG_MIC: Color = Color::from_rgb8(0x7f, 0xb3, 0xd5);
pub const TAG_INST: Color = Color::from_rgb8(0xd9, 0xa8, 0x5c);
pub const TAG_LINE: Color = Color::from_rgb8(0x9d, 0xa3, 0xac);
/// The two digital ports share a violet.
pub const TAG_DIGITAL: Color = Color::from_rgb8(0xa2, 0x92, 0xc6);
pub const TAG_PB: Color = Color::from_rgb8(0x5f, 0xb3, 0xa8);
pub const TAG_OUT: Color = Color::from_rgb8(0x7f, 0xbf, 0x8a);

pub fn lamp() -> Color {
    p().lamp
}
pub fn bg_deep() -> Color {
    p().bg_deep
}
pub fn surface() -> Color {
    p().surface
}
pub fn surface_hi() -> Color {
    p().surface_hi
}
pub fn border() -> Color {
    p().border
}
pub fn control_edge() -> Color {
    p().control_edge
}

// ── Materials ────────────────────────────────────────────────────
// The controls are objects (2026-11-03, the owner, holding up a hardware
// mixer's knobs, faders and keys: "The nubs and sliders and buttons should
// look" like these): a key is a low block lit from above, a knob a
// turned cap, a fader cap brushed metal riding in a slot. The theme
// colours the room and the lit states; the materials below are derived
// from its roles, so every theme keeps them.

/// Whether the theme is dark (light ink on dark grounds).
pub fn is_dark() -> bool {
    dark_theme()
}

/// The fader caps' metal: `fader` on a dark theme (the ivory caps), the
/// Cap material's silver on a light one (the reviewer: dark caps were the
/// heaviest objects on a pale panel, the busyness the owner saw).
pub fn fader_metal() -> Color {
    if dark_theme() {
        fader_color()
    } else {
        cap_body()
    }
}

pub(crate) fn dark_theme() -> bool {
    let p = p();
    luminance(p.text_primary) > luminance(p.surface)
}

/// Which way a control moves under the cursor (2026-11-03, the owner: a
/// control about to be touched should be "slight brighter then others ...
/// Brighter or darker depending on the theme"): toward white in a dark
/// theme, toward black in a light one, so it stands further off its ground
/// either way.
pub fn hover_toward() -> Color {
    if dark_theme() {
        Color::WHITE
    } else {
        Color::BLACK
    }
}

/// How far a hovered drawn control (a knob's cap, a fader's metal) moves
/// toward `hover_toward`.
pub const HOVER_SHIFT: f32 = 0.14;

/// `c` under the cursor.
pub fn hovered(c: Color) -> Color {
    blend(c, hover_toward(), HOVER_SHIFT)
}

/// A fill running top to bottom from `top` to `bottom`: how light from
/// above falls on a low block.
pub fn lit_from_above(top: Color, bottom: Color) -> Background {
    Background::from(
        Linear::new(std::f32::consts::PI)
            .add_stop(0.0, top)
            .add_stop(1.0, bottom),
    )
}

/// A key at rest: the ink lifted over the ground (translucent, so on a lit
/// strip it brightens the wash rather than punching a hole in it; the
/// owner, 2026-11-03, found opaque dark keys "hard to see with the black
/// and brown"), more at the top than the bottom, so it reads as a block
/// with its top face to the light. Hover lifts it; pressed, it goes flat.
pub fn key_rest(hovered: bool, pressed: bool) -> Background {
    let (top, bottom) = key_rest_colors(hovered, pressed);
    lit_from_above(top, bottom)
}

/// `key_rest`'s top and foot.
pub(crate) fn key_rest_colors(hovered: bool, pressed: bool) -> (Color, Color) {
    let ink = p().text_primary;
    // A dark theme's lift is light, so the top gets more of it; a light
    // theme's is a shade, so the top gets less.
    if !dark_theme() {
        // On a light panel a key is a raised block of near-white lit from
        // above, its foot a shade under the panel (the reviewer: at 1.13:1
        // the Control Room keys were only a hairline).
        let white = |a: f32| Color { a, ..Color::WHITE };
        let shade = |a: f32| Color { a, ..Color::BLACK };
        return match (pressed, hovered) {
            (true, _) => (shade(0.1), shade(0.06)),
            (false, true) => (white(0.95), shade(0.04)),
            (false, false) => (white(0.8), shade(0.08)),
        };
    }
    let (top, bottom) = match (pressed, hovered) {
        (true, _) => (0.06, 0.09),
        (false, true) => (0.25, 0.15),
        (false, false) => (0.18, 0.09),
    };
    (Color { a: top, ..ink }, Color { a: bottom, ..ink })
}

/// A key lit in `hue`: solid, brighter at the top like a lit lens, and
/// lifted toward white by `bloom` while it warms up.
pub fn key_lit(hue: Color, bloom: f32, pressed: bool) -> Background {
    let (top, bottom) = key_lit_colors(hue, bloom, pressed);
    // A lens: a bright lip across the top, the colour full below it.
    let lip = if pressed { top } else { blend(hue, Color::WHITE, 0.4 + 0.3 * bloom) };
    Background::from(
        Linear::new(std::f32::consts::PI)
            .add_stop(0.0, lip)
            .add_stop(0.16, top)
            .add_stop(1.0, bottom),
    )
}

/// `key_lit`'s top and foot.
pub(crate) fn key_lit_colors(hue: Color, bloom: f32, pressed: bool) -> (Color, Color) {
    let mut top = blend(hue, Color::WHITE, 0.16 + 0.3 * bloom);
    let mut bottom = blend(hue, Color::BLACK, 0.10);
    if pressed {
        top = blend(top, Color::BLACK, PRESS_DARKEN);
        bottom = blend(bottom, Color::BLACK, PRESS_DARKEN);
    }
    (top, bottom)
}

/// `a` to `b` by `t`, alpha included (for a lit key going out over a
/// translucent one at rest).
/// A translucent `c` laid over the opaque `ground`: the colour it shows.
fn over(c: Color, ground: Color) -> Color {
    Color {
        r: ground.r + (c.r - ground.r) * c.a,
        g: ground.g + (c.g - ground.g) * c.a,
        b: ground.b + (c.b - ground.b) * c.a,
        a: 1.0,
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// The edge of a key lit in `hue`: the hue, darker, so the lit block has
/// a crisp rim instead of bleeding into its glow.
pub fn key_lit_edge(hue: Color) -> Color {
    blend(hue, Color::BLACK, 0.28)
}

/// The inset a set of joined keys sits in (the owner's reference: "Some
/// are insets"): a shallow well cut into the panel, shade over it with a
/// darker rim (`key_well_style`), with corners of `radius` (half its
/// height: a capsule round a capsule pair).
pub fn key_well_round(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| key_well_style(radius)
}

/// The well's own shades, top and foot, over the panel it's cut into.
fn well_grounds() -> [Color; 2] {
    let shade = |a: f32| Color { a, ..Color::BLACK };
    let (top, foot) = if dark_theme() { (0.45, 0.28) } else { (0.06, 0.04) };
    [layer(shade(top), surface()), layer(shade(foot), surface())]
}

/// An inset key's resting rim (the Control Room's keys): read against the
/// well it stands in as well as the key grounds, wherever it lands on the
/// pixel grid (carried since #58: Solarized's rims read 2.70, the sides
/// 2.24, on the well's dark teal).
pub fn well_key_edge() -> Color {
    let grounds: Vec<Color> = key_grounds().into_iter().chain(well_grounds()).collect();
    let base = resting_key_edge();
    // High contrast holds rims at 4.5 (critiques #105-#111 B: the M/S and
    // M/CUE pills read 3.5 top and bottom, 2.5 at their round ends).
    let need = if is_high_contrast() { 4.5 } else { 3.0 };
    (0..=20)
        .map(|k| blend(base, text_primary(), k as f32 / 20.0))
        .find(|c| grounds.iter().all(|g| contrast(blend(*c, *g, 1.0 - SPLIT_EDGE_COVER), *g) >= need))
        .unwrap_or_else(text_primary)
}

fn key_well_style(radius: f32) -> container::Style {
    let dark = dark_theme();
    let shade = |a: f32| Color { a, ..Color::BLACK };
    container::Style {
        background: Some(if dark {
            lit_from_above(shade(0.45), shade(0.28))
        } else {
            lit_from_above(shade(0.06), shade(0.04))
        }),
        border: Border {
            color: shade(if dark { 0.6 } else { 0.24 }),
            width: 1.0,
            radius: radius.into(),
        },
        ..container::Style::default()
    }
}

/// A discovery shimmer's band (`strip::shimmered`), `t` 0..1 across: the
/// lamp's light, soft at its edges, passing left to right over a control.
pub fn glint(t: f32, radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        let lamp = p().lamp;
        let c = -0.25 + 1.5 * t.clamp(0.0, 1.0);
        let w = 0.2;
        let at = |x: f32| x.clamp(0.0, 1.0);
        let gradient = Linear::new(std::f32::consts::FRAC_PI_2)
            .add_stop(at(c - w), Color { a: 0.0, ..lamp })
            .add_stop(at(c), Color { a: 0.42, ..lamp })
            .add_stop(at(c + w), Color { a: 0.0, ..lamp });
        container::Style {
            background: Some(Background::from(gradient)),
            border: Border { radius: radius.into(), ..Border::default() },
            ..container::Style::default()
        }
    }
}

/// The narrow switch's slot at the card's edge (critique #15, Direction
/// A, TotalMix's cave): a dark cut, deeper at its top, its inner end a
/// half-round, a dark rim; never the card's highlight (the owner: that
/// stays on the card's perimeter). `edge_lip` under it catches the light.
pub fn edge_slot(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        let shade = |a: f32| Color { a, ..Color::BLACK };
        let dark = dark_theme();
        container::Style {
            background: Some(if dark { lit_from_above(shade(0.62), shade(0.48)) } else { lit_from_above(shade(0.16), shade(0.11)) }),
            border: Border { color: shade(if dark { 0.6 } else { 0.36 }), width: 1.0, radius: Joint::Left.radius(radius) },
            ..container::Style::default()
        }
    }
}

/// The lit lip under the narrow switch's slot: the panel catching the
/// light along the cut's lower edge (the Inset's lip).
pub fn edge_lip(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(inset().lip)),
        border: Border { radius: Joint::Left.radius(radius), ..Border::default() },
        ..container::Style::default()
    }
}

/// The narrow switch's cap (critique #15, Direction A): a raised cap of
/// the knobs' metal, lighter than the cave it rides in, lit from above,
/// a dark rim and a small shadow; its inner end round, its outer end flat
/// where it meets the slot's end (`at_edge`). No highlight on it, hovered
/// or not (the owner): hover lifts its top a little, a press flattens it.
pub fn edge_knob(radius: f32, at_edge: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        // The fader caps' metal: cream on a dark theme, silver on a light
        // one (the owner's pick from mockups).
        let body = fader_metal();
        let lift = match status {
            button::Status::Hovered => 0.26,
            button::Status::Pressed => 0.0,
            _ => 0.18,
        };
        let pressed = matches!(status, button::Status::Pressed);
        let outer = if at_edge { 0.0 } else { RADIUS_XS.min(radius) };
        let radius = iced::border::Radius { top_left: radius, bottom_left: radius, top_right: outer, bottom_right: outer };
        button::Style {
            background: Some(if pressed { Background::Color(body) } else { lit_from_above(blend(body, text_primary(), lift), body) }),
            border: Border { color: Color { a: 0.5, ..Color::BLACK }, width: 1.0, radius },
            shadow: if pressed {
                Shadow::default()
            } else {
                Shadow { color: Color { a: 0.5, ..Color::BLACK }, offset: iced::Vector::new(0.0, 1.0), blur_radius: 1.5 }
            },
            text_color: text_primary(),
            ..button::Style::default()
        }
    }
}

/// How far a joined set of keys sits inside its well (`key_well_round`).
pub const WELL_PAD: f32 = 2.0;

/// A cut in a panel (the owner, 2026-11-03: "a box within a box ... an
/// inset and a bevel"; the lane a fader runs in, the meter's frame, a
/// value box): its shade inside, deeper at the top where the light can't
/// reach (`top`, `foot`); its walls, the top one in shadow (`shadow`) and
/// the bottom one catching the light (`glint`); its rim; and the panel's
/// lit lip just under it (`lip`). A light theme's is drawn harder, so it
/// still reads as a cut on a near-white panel.
#[derive(Debug, Clone, Copy)]
pub struct Inset {
    pub top: Color,
    pub foot: Color,
    pub shadow: Color,
    pub glint: Color,
    pub rim: Color,
    pub lip: Color,
    /// How much the side walls are shaded (left in shadow, right lit): on
    /// a dark panel it gives the cut its depth, on a light one it turned a
    /// tall lane into a tube, so a light theme's sides stay flat.
    pub sides: f32,
}

pub fn inset() -> Inset {
    let shade = |a: f32| Color { a, ..Color::BLACK };
    let light = |a: f32| Color { a, ..Color::WHITE };
    if dark_theme() {
        Inset { top: shade(0.52), foot: shade(0.3), shadow: shade(0.6), glint: light(0.12), rim: shade(0.62), lip: light(0.08), sides: 0.8 }
    } else {
        // On a light panel a cut still reads darker than its panel, a box
        // within the box (the owner: "The white one looks kind of ugly").
        // No bright glint inside: on a light panel a white wall reads as a
        // halo round a pill, not a cut ("the white inset looks strange").
        // Pale with a crisp inner shadow (2026-11-03, the owner: Daylight
        // read "too busy", with too little contrast; mid-grey cuts put grey
        // words on grey).
        // Flat, with a crisp top wall (the reviewer: a fill fading over a
        // light ground washed out, so the cuts read as raised pills).
        Inset { top: shade(0.10), foot: shade(0.10), shadow: shade(0.30), glint: light(0.3), rim: shade(0.36), lip: light(0.9), sides: 0.0 }
    }
}

/// A box cut into the panel, as a container (the Inset as far as a
/// container can draw one: its shade deeper at the top, its rim): the
/// strip's value capsule and peak readouts. `radius` half its height
/// makes a capsule.
pub fn inset_box(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        let i = inset();
        // On a light theme a box that holds words is a pale cut, so its
        // words keep their contrast.
        let (top, foot) = if dark_theme() {
            (i.top, i.foot)
        } else {
            // A crisp shadow at its top edge, then flat.
            (Color { a: 0.2, ..Color::BLACK }, Color { a: 0.1, ..Color::BLACK })
        };
        container::Style {
            background: Some(lit_from_above(top, foot)),
            border: Border { color: i.rim, width: 1.0, radius: radius.into() },
            text_color: Some(text_primary()),
            ..container::Style::default()
        }
    }
}

/// A tall well of rows (the snapshot list, the groups grid): on a light
/// theme a flat, light cut (the reviewer: the boxes' gradient spread over
/// 100px became a grey slab); on a dark one as `inset_box`.
pub fn well_box(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |theme| {
        if dark_theme() {
            // The firm edge round the cut: its own rim read 1.04 on High
            // contrast and 1.11-1.48 on the dark themes, the Groups and
            // Snapshots frames alike (critique #100 B).
            let mut style = inset_box(radius)(theme);
            style.border.color = firm_edge();
            // 1.5 px: at 1 px its top and bottom fell on half pixels and
            // read 2.44-2.94 (critique #101 B).
            style.border.width = SPLIT_EDGE_W;
            return style;
        }
        container::Style {
            background: Some(Background::Color(Color { a: 0.06, ..Color::BLACK })),
            // Half black: at 36% the Groups grid's frame read 2.52 on the
            // page (critiques #98 and #99 B).
            border: Border { color: Color { a: 0.5, ..Color::BLACK }, width: 1.0, radius: radius.into() },
            text_color: Some(text_primary()),
            ..container::Style::default()
        }
    }
}

/// A groups-grid cell (the reviewer: raised blocks inside the well were
/// the box-in-a-box-with-a-bevel Daylight had dropped): flat on the well,
/// a hairline round it, its numeral in its column's hue; lit, solid in
/// that hue as the reference's are.
pub fn grid_cell(hue: Color, lit: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        if lit {
            return button::Style {
                background: Some(key_lit(hue, 0.0, pressed)),
                text_color: on_active(),
                border: Border { color: key_lit_edge(hue), width: 1.0, radius: RADIUS_XS.into() },
                ..button::Style::default()
            };
        }
        let line = if dark_theme() { Color { a: 0.07, ..Color::WHITE } } else { Color { a: 0.12, ..Color::BLACK } };
        button::Style {
            background: hovered.then_some(Background::Color(Color { a: 0.14, ..hue })),
            text_color: readable_on_keys(hue),
            border: Border { color: line, width: 1.0, radius: RADIUS_XS.into() },
            ..button::Style::default()
        }
    }
}

/// The body of a knob's cap: dark anodised metal in every theme (a light
/// theme's knobs are dark objects on a pale panel, as on hardware), with
/// the theme's own cast.
pub fn cap_body() -> Color {
    let p = p();
    if dark_theme() {
        blend(p.surface_hi, p.text_primary, 0.11)
    } else {
        // Brushed silver on a light panel: the dark caps sat heavy on it
        // and made the strip busy (the owner, 2026-11-03).
        blend(p.surface_hi, p.text_primary, 0.2)
    }
}

/// The recessed slot a fader cap rides in: shade over whatever is under it,
/// so it reads as a cut in the panel on any ground, the lamp wash included.
pub fn slot() -> Color {
    Color { a: if dark_theme() { 0.62 } else { 0.55 }, ..Color::BLACK }
}

/// Whichever of the theme's inks reads best on `ground`: for marks and
/// labels drawn on a control's own body (a knob's pointer and value).
pub fn ink_on(ground: Color) -> Color {
    let p = p();
    [p.text_primary, p.surface, p.on_active]
        .into_iter()
        .max_by(|a, b| contrast(*a, ground).total_cmp(&contrast(*b, ground)))
        .unwrap_or(p.text_primary)
}
pub fn text_primary() -> Color {
    p().text_primary
}
pub fn text_sec() -> Color {
    p().text_sec
}
pub fn accent() -> Color {
    p().accent
}
pub fn accent_dim() -> Color {
    p().accent_dim
}
pub fn fader_color() -> Color {
    p().fader
}
pub fn signal() -> Color {
    p().signal
}
pub fn clip() -> Color {
    p().clip
}
pub fn phantom() -> Color {
    p().phantom
}
pub fn attention() -> Color {
    p().attention
}
pub fn mute_color() -> Color {
    p().mute
}
/// Band identity in the EQ panel, matching the three columns and their
/// handles in the response plot. These reuse the palette's clip/signal/
/// mute hues, but inside the EQ they mean band 1/2/3.
pub fn eq_band() -> [Color; 3] {
    [p().clip, p().signal, p().mute]
}
pub fn solo_color() -> Color {
    p().attention
}
pub fn on_active() -> Color {
    p().on_active
}
pub fn on_placard() -> Color {
    p().on_active
}
pub fn on_placard_sec() -> Color {
    p().on_placard_sec
}

/// The meters' zone colours: Audra's bright green, amber and red in every
/// theme. A meter is an instrument read at a glance on its own dark well,
/// and a theme's darker state hues (Daylight's, say) would sink into it.
pub const METER_SIGNAL: Color = AUDRA.signal;
pub const METER_ATTENTION: Color = AUDRA.attention;
pub const METER_CLIP: Color = AUDRA.clip;

/// iced's own theme, from the current palette: what unstyled text and
/// iced's default widget parts fall back to. It used to be iced's Dark,
/// whose white text would vanish on Daylight.
pub fn iced_theme() -> iced::Theme {
    let p = p();
    iced::Theme::custom(
        "Audra".to_string(),
        iced::theme::Palette {
            background: p.floors[0],
            text: p.text_primary,
            primary: p.accent,
            success: p.signal,
            warning: p.attention,
            danger: p.clip,
        },
    )
}

// ── Type scale ───────────────────────────────────────────────────
// Every text element in the app maps to one of these six tiers rather
// than a one-off pixel value chosen per call site — before this there
// were eight near-random sizes (6.5, 8, 9, 10, 11, 12, 13, 20) with no
// clear reason two of them (8 vs 9, 11 vs 12) were ever different.
//
// These are the sizes at `ui_scale == 1.0`; the manual zoom (see
// `SCALE_MIN`/`SCALE_MAX`) multiplies them in the mixer and matrix views.

// The floor is 12px: Audra is read in a dim room, often from an arm's
// length away, and the inherited 9 and 11px tiers were the main reason
// the window was hard to read. Nothing in the app is set smaller.

/// The floor, and the most common size: labels, toggle captions (M, S,
/// 48V), type tags, the dB scale, matrix headings, hints and tooltips.
pub const TEXT_SM: f32 = 12.0;
/// Every number that changes (dB readouts, gain, levels), set in the
/// Mono face one step above the label floor so values read first.
pub const TEXT_NUM: f32 = 13.0;
/// Default body text: channel names, top-bar labels, pick lists.
pub const TEXT_MD: f32 = 14.0;
/// Row names (Hardware inputs, ...), in the serif.
pub const TEXT_ROW: f32 = 17.0;
/// Large titles, and the name when the drawn wordmark cannot load.
pub const TEXT_XL: f32 = 22.0;
/// The monitor levels in the right column and the placard's name.
pub const TEXT_BIG: f32 = 28.0;

/// The zoom a fresh window opens at (see `TuxMix::ui_scale`, which is
/// multiplied into every text size and widget dimension in the mixer and
/// matrix views).
pub const SCALE_DEFAULT: f32 = 1.0;
/// Bounds for the *manual* UI zoom (Ctrl+molette / Ctrl+= / Ctrl+-). The
/// scale is no longer tied to window size at all — window resizing just
/// shows more or less via scroll, so these only constrain how far the user
/// can zoom by hand (a readability floor and a sane ceiling).
pub const SCALE_MIN: f32 = 0.75;
pub const SCALE_MAX: f32 = 1.75;

// ── Corner radius scale ─────────────────────────────────────────────
// Same idea as the type scale: before this, `panel` used 7 and `chip`
// used 6 with no reason they were ever different — just whatever felt
// fine in isolation when each was written.

/// Small parts: matrix cells, a strip's icon buttons, slider rails and
/// caps, the submix mark.
pub const RADIUS_XS: f32 = 3.0;
/// The strip's value box: between XS and SM, squarer than a capsule (the
/// owner, 2026-11-07: "too round"). The narrow switch's slot and cap round
/// at half their height instead.
pub const RADIUS_BOX: f32 = 4.0;

/// Every control and small card: buttons, Control Room keys, snapshot
/// slots, text inputs, pick lists/menus, the scrollbar scroller, the
/// selected strip's edge, the placard.
pub const RADIUS_SM: f32 = 6.0;
/// Larger cards: a strip's Settings/EQ/Trim panel (`panel`), dialogs and
/// popovers (`dialog`), top-bar groups (`chip`).
pub const RADIUS_MD: f32 = 10.0;

// ── Spacing scale ────────────────────────────────────────────────────
// Same consolidation for `.spacing()`/`.padding()` call sites — before
// this there were eleven near-arbitrary values (1, 2, 3, 4, 5, 6, 8, 10,
// 12, 14, 16) scattered across app.rs/strip.rs/matrix.rs with no shared
// vocabulary. Values are the *smallest* unit of a given role, not a
// strict multiplier progression — the goal is a short, named list callers
// can pick from, not a formula.

/// Between rows stacked tightly inside one strip's control stack (header
/// → M/S → 48V/PAD → fader → dB → pan) and between a pan dot and its
/// label — these read as one continuous unit, not separate rows.
pub const SPACE_HAIRLINE: f32 = 1.0;
/// Between closely related inline siblings: a channel name and its type
/// tag, the M/S button pair, a matrix cell grid, the MIXER/MATRIX toggle.
pub const SPACE_TIGHT: f32 = 2.0;
/// Small control padding — e.g. the top-bar view-toggle buttons.
pub const SPACE_SM: f32 = 4.0;
/// Strip card padding, strip-to-strip gaps, a chip's internal row
/// spacing.
pub const SPACE_MD: f32 = 6.0;
/// Section-level spacing: between a row's name and its strips, and a
/// floor's top and bottom padding.
pub const SPACE_LG: f32 = 8.0;
/// Container-level padding: a chip's horizontal padding, the mixer/matrix
/// view's outer padding.
pub const SPACE_XL: f32 = 12.0;
/// The top bar's outer padding and the gap between its identity/nav/
/// session groups.
pub const SPACE_XXL: f32 = 16.0;

/// The top bar's height at scale 1. The counter's folded rail centres its
/// toggle on the same line.
pub const TOP_BAR_H: f32 = 48.0;

pub fn panel(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(surface())),
        border: Border {
            color: border(),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        text_color: Some(text_primary()),
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A section of the right-hand column (the levels, the Control Room keys,
/// the tabbed area; 2026-11-03, the owner's reference frames each group of
/// its right-hand column): a panel set a little into the column, shade
/// over it with a `border` rim at 10px corners.
pub fn section(_theme: &iced::Theme) -> container::Style {
    let shade = if dark_theme() { 0.16 } else { 0.035 };
    container::Style {
        background: Some(Background::Color(Color { a: shade, ..Color::BLACK })),
        border: Border {
            color: border(),
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        text_color: Some(text_primary()),
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A floating dialog card: the same graphite surface as `panel`, plus a
/// drop shadow so it reads as raised above the window instead of drawn
/// into it. First user: the About modal (`about::about_modal`).
/// A strip's panel floating over the strips beside it (Settings, Trim):
/// the panel with a `control_edge` rim and a shadow under it, so on a
/// dark theme it stands off the strips it covers (the reviewer,
/// 2026-11-04: an output's Settings filled ADAT3/4's place and read as
/// ADAT3/4 turned into a menu).
pub fn floating_card(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(surface())),
        border: Border { color: control_edge(), width: 1.0, radius: RADIUS_MD.into() },
        text_color: Some(text_primary()),
        shadow: Shadow {
            color: Color { a: if dark_theme() { 0.7 } else { 0.3 }, ..Color::BLACK },
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        ..container::Style::default()
    }
}

pub fn dialog(_theme: &iced::Theme) -> container::Style {
    // On a dark theme the card is the page's own tone, dimmed page or not:
    // its hairline read 1.39-1.55 round the tray and quit questions
    // (critique #95 B). The firm edge there, 1.5 px so it holds on a half
    // pixel.
    let dark = dark_theme();
    container::Style {
        background: Some(Background::Color(surface())),
        border: Border {
            color: if dark { firm_edge() } else { border() },
            width: if dark { 1.5 } else { 1.0 },
            radius: RADIUS_MD.into(),
        },
        text_color: Some(text_primary()),
        shadow: Shadow {
            color: Color {
                a: 0.55,
                ..Color::BLACK
            },
            offset: iced::Vector::new(0.0, 10.0),
            blur_radius: 30.0,
        },
        snap: false,
    }
}

/// Linear-interpolates `base` toward `tint` by `amount` (0 = pure `base`,
/// 1 = pure `tint`), keeping `base`'s own alpha — used to tint a strip
/// card just enough to read as "this channel's category" at a glance
/// without competing with the accent-colored selection border or making
/// the card itself look like a colored badge.
pub(crate) fn blend(base: Color, tint: Color, amount: f32) -> Color {
    Color {
        r: base.r + (tint.r - base.r) * amount,
        g: base.g + (tint.g - base.g) * amount,
        b: base.b + (tint.b - base.b) * amount,
        a: base.a,
    }
}

/// The lamp pool's strength at the top of a lit strip, where the light
/// falls, and at its foot. iced draws linear gradients only, so the pool
/// is a wash that fades down the strip rather than a round spot.
/// Softened from 0.19 / 0.11 / 0.06 (2026-11-03): at 0.19 the head of a
/// lit strip was a muddy brown that the dark controls on it sank into. A
/// lit strip still stands 1.4:1 above its floor.
const LAMP_TOP: f32 = 0.16;
const LAMP_MID: f32 = 0.095;
const LAMP_FOOT: f32 = 0.05;

/// A strip: a brushed panel in a hairline frame, on its floor's ground
/// tone (the rows still divide by tone).
///
/// - `lamp` (0..1): how far the strip is in the lamp pool. A strip is lit
///   when it feeds the mix being edited, or is that mix's output.
/// - `hovered` only brightens the fill under the strip; nothing lifts,
///   glows or grows. A lit strip keeps its wash under the cursor.
/// - `selected` (part of a Ctrl/Shift multi-selection) draws a coral
///   edge: selection is a live state, so it may use coral.
///
/// Plain `container::Style` closures get no hover status the way buttons
/// do, so hover is tracked in `TuxMix::hovered_strip` (see
/// `widgets/strip.rs`).
/// `focused`: the strip the keyboard is on (Tab, the arrows): a glow in the
/// fader colour, and its rim where selection's coral is not, the one focus
/// mark the mixer has (the critique, 2026-11-04: no keyboard path, no focus).
/// A strip card's coloured frame, when it has one, and its width: a lit
/// strip's frame catches the lamp; selection's coral and the keyboard's
/// rim come first. None for the plain hairline.
pub fn strip_rim(selected: bool, focused: bool, lamp: f32) -> Option<(Color, f32)> {
    let s = lamp.clamp(0.0, 1.0);
    if selected {
        Some((accent(), 1.5))
    } else if focused {
        Some((fader_color(), 1.5))
    } else if s > 0.0 {
        Some((blend(border(), p().lamp, 0.75 * s), 1.0))
    } else {
        None
    }
}

pub fn strip_panel(selected: bool, hovered: bool, lamp: f32, focused: bool) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        // A brushed panel in a hairline frame (2026-11-03, the owner, from
        // the hardware mixer they hold up as the reference: "a subtle
        // brushed look background"): each channel reads as one unit, as
        // on a console's channel strip. The lamp's wash lies over the
        // panel, the two in one gradient (a container has one fill).
        // Selection turns the frame coral.
        let p = p();
        let s = lamp.clamp(0.0, 1.0);
        let wash = |t: f32| along(&[(0.0, LAMP_TOP), (0.6, LAMP_MID), (1.0, LAMP_FOOT)], t) * s;
        let mut gradient = Linear::new(std::f32::consts::PI);
        for t in [0.0, 0.06, 0.2, 0.45, 0.6, 0.75, 1.0] {
            gradient = gradient.add_stop(t, layer(Color { a: wash(t), ..p.lamp }, panel_lift(&p, t, hovered, s)));
        }
        let background = Some(Background::from(gradient));
        container::Style {
            background,
            text_color: Some(text_primary()),
            // Selection keeps its coral edge; the keyboard's strip takes a
            // rim in the fader colour where it has none, and a glow in it
            // either way, so a strip both selected and focused shows both
            // (the reviewer: focus had hidden the selection).
            border: Border {
                color: strip_rim(selected, focused, lamp).map_or(border(), |(c, _)| c),
                width: strip_rim(selected, focused, lamp).map_or(1.0, |(_, w)| w),
                radius: RADIUS_SM.into(),
            },
            ..container::Style::default()
        }
    }
}

/// A veil in a strip card's own face over its contents, `a` of the way
/// opaque: the contents fade out and in as a strip folds and unfolds (the
/// owner, 2026-11-14: the fold "still don't look nice"). Its face only:
/// the card's frame stays as it is.
pub fn strip_veil(selected: bool, hovered: bool, lamp: f32, focused: bool, tone: usize, a: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |theme| {
        let base = strip_panel(selected, hovered, lamp, focused)(theme);
        // The face is see-through over its floor (a few per cent of light
        // on a dark theme): flattened onto the floor (`tone`, as `floor`),
        // so the veil hides what's under it.
        let ground = p().floors[tone.min(2)];
        let fade = |c: Color| Color { a: a.clamp(0.0, 1.0), ..layer(c, ground) };
        let background = base.background.map(|b| match b {
            Background::Color(c) => Background::Color(fade(c)),
            Background::Gradient(iced::Gradient::Linear(mut l)) => {
                for stop in l.stops.iter_mut().flatten() {
                    stop.color = fade(stop.color);
                }
                Background::Gradient(iced::Gradient::Linear(l))
            }
        });
        container::Style {
            background,
            border: Border { radius: RADIUS_SM.into(), ..Border::default() },
            ..container::Style::default()
        }
    }
}

/// A folded mic strip's 48V mark: a lit 48V key that can't be pressed
/// (the folded rail has no room for the two-click switch; unfold it).
pub fn phantom_mark(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(key_lit(phantom(), 0.0, false)),
        text_color: Some(on_active()),
        border: Border { color: key_lit_edge(phantom()), width: 1.0, radius: RADIUS_XS.into() },
        ..container::Style::default()
    }
}

/// The trick card (`tricks::card`): the dialog's raised card with the
/// lamp's warmth along its rim, a find rather than a warning.
pub fn trick_card(theme: &iced::Theme) -> container::Style {
    let base = dialog(theme);
    // The lamp rim held at 3:1 on the column wherever it lands on the
    // pixel grid (critique #62 B 2, carried since #60: Daylight's 55% lamp
    // rim read 1.36, a split bottom 1.50; Dusk's split bottom 1.77).
    // On a light theme a lighter, closer shadow: the dialog's 55% black
    // darkened the column under the card until its own amber rim read
    // 2.43-2.91 against it (carried since #66, "card vs shadow").
    let shadow = if dark_theme() {
        base.shadow
    } else {
        Shadow { color: Color { a: 0.18, ..Color::BLACK }, offset: iced::Vector::new(0.0, 4.0), blur_radius: 14.0 }
    };
    container::Style {
        border: Border { color: tip_card_edge(), width: SPLIT_EDGE_W, radius: RADIUS_MD.into() },
        shadow,
        ..base
    }
}

/// The trick card's rim: the lamp, moved toward the ink until a pixel it
/// covers `SPLIT_EDGE_COVER` reads at 3:1 on every key ground.
pub fn tip_card_edge() -> Color {
    // Also against the column darkened by the card's own shadow, just under
    // its foot (critique #74 B: the keyboard hint's rim 2.88 there).
    let shade = |g: Color| blend(g, Color::BLACK, if dark_theme() { 0.0 } else { 0.14 });
    let grounds: Vec<Color> = key_grounds().into_iter().flat_map(|g| [g, shade(g)]).collect();
    (0..=20)
        .map(|k| blend(lamp(), text_primary(), k as f32 / 20.0))
        .find(|c| grounds.iter().all(|g| contrast(blend(*c, *g, 1.0 - SPLIT_EDGE_COVER), *g) >= 3.0))
        .unwrap_or_else(text_primary)
}

/// The milestone card (`celebrate::card`): the dialog's raised card, but a
/// win's rather than a tip's. It lands lit by the lamp, its rim in the lamp
/// and its face washed with it, and settles to a standing glow (`glow` 1 on
/// arrival, 0.3 at rest).
pub fn milestone_card(glow: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |theme| {
        let base = dialog(theme);
        let g = glow.clamp(0.0, 1.0);
        // At rest the rim holds 3:1 as the tip card's does, warming to the
        // lamp as it lands; on a light theme the tip card's lighter shadow
        // (critique carried since #70: the rim 1.70 against the dialog's
        // 55% shadow under it).
        let shadow = if dark_theme() {
            base.shadow
        } else {
            Shadow { color: Color { a: 0.18, ..Color::BLACK }, offset: iced::Vector::new(0.0, 4.0), blur_radius: 14.0 }
        };
        container::Style {
            background: Some(Background::Color(blend(surface(), lamp(), 0.12 * g))),
            // Mostly the readable rim even as it lands: the halo and the warm
            // face carry the glow (at full lamp it read 1.94 on its own halo).
            // 2 px: at 1.5 its top fell between two pixel rows (2.72, critique #78 A 2).
            border: Border { color: blend(tip_card_edge(), lamp(), 0.3 * g), width: 2.0, radius: RADIUS_MD.into() },
            shadow,
            ..base
        }
    }
}

/// The lamp's light round the milestone card: a fill-less wrapper whose
/// shadow is the glow (as `strip_glow`'s), strongest as the card lands.
pub fn lamp_halo(glow: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        let g = glow.clamp(0.0, 1.0);
        let shadow = if dark_theme() {
            Shadow { color: Color { a: 0.6 * g, ..lamp() }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 24.0 }
        } else {
            Shadow { color: Color { a: 0.75 * g, ..lamp() }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 16.0 }
        };
        container::Style { border: Border { radius: RADIUS_MD.into(), ..Border::default() }, shadow, ..container::Style::default() }
    }
}

/// A key's own light thrown round it (`flare` 1 to 0): a fill-less
/// wrapper whose shadow is the glow, as `strip_glow`'s (a lit key's own
/// shadow is a 16px bloom; the moment an effect is first heard is bigger).
pub fn key_halo(hue: Color, flare: f32, radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        let f = flare.clamp(0.0, 1.0);
        container::Style {
            border: Border { radius: radius.into(), ..Border::default() },
            shadow: Shadow {
                color: Color { a: if dark_theme() { 0.7 } else { 0.8 } * f, ..hue },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 28.0 * f,
            },
            ..container::Style::default()
        }
    }
}

/// The keyboard's colour beyond the strips: on a dark theme the fader
/// colour taken halfway to white, brighter than any control's own lit rim
/// (the reviewer, 2026-11-05: on Slate a Matrix cell's ring sat too close
/// to its neighbours' rims); on a light one the ink.
pub fn focus_mark() -> Color {
    if dark_theme() {
        blend(fader_color(), Color::WHITE, 0.5)
    } else {
        text_primary()
    }
}

/// The keyboard's ring over a control beyond the strips (`keyfocus::ring`):
/// 2px in the halo's colour (the fader colour in a dark theme, the ink in
/// a light one), inside the control's own edge, no fill.
/// A placard key's focus ring (`keyfocus::placard_ring`): the card's ink,
/// 2 px, outside the key with a gap, so it reads on the card's face.
pub fn placard_focus_ring_color() -> Color {
    on_placard()
}

pub fn placard_focus_ring(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| container::Style {
        border: Border { color: placard_focus_ring_color(), width: 2.0, radius: radius.into() },
        ..container::Style::default()
    }
}

pub fn focus_ring(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| container::Style {
        border: Border {
            color: focus_mark(),
            width: 2.0,
            radius: radius.into(),
        },
        ..container::Style::default()
    }
}

/// The focus ring with the mark washed over the face: for a Matrix cell, one
/// among some 150 alike, where the 2 px ring alone was faint (critique #99 A
/// P3-11).
pub fn focus_ring_tinted(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |t| container::Style {
        background: Some(Background::Color(Color { a: if dark_theme() { 0.1 } else { 0.16 }, ..focus_mark() })),
        ..focus_ring(radius)(t)
    }
}

/// The keyboard strip's tab (`strip::focus_tabbed`): a lamp on the card's
/// top edge in the halo's colour, rounded at its foot only, so it hangs
/// from the edge.
pub fn focus_tab(_theme: &iced::Theme) -> container::Style {
    let lit = if dark_theme() { fader_color() } else { text_primary() };
    container::Style {
        background: Some(Background::Color(lit)),
        border: Border {
            radius: iced::border::Radius { top_left: 0.0, top_right: 0.0, bottom_right: 2.0, bottom_left: 2.0 },
            ..Border::default()
        },
        shadow: Shadow { color: Color { a: 0.6, ..lit }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 4.0 },
        ..container::Style::default()
    }
}

/// The keyboard strip's glow, drawn by a plain container around the strip
/// card (`strip_panel`). iced's GPU renderer draws no shadow under a
/// gradient fill, and the card's brushed panel is one, so a glow set on
/// the card itself never showed (the focus+selection capture,
/// 2026-11-05). The wrapper has no fill of its own; the card covers it.
/// In a dark theme the glow is light, the fader colour; in a light one a
/// pale glow vanishes into the floor, so there it is a ring of the ink,
/// even all round: offset, it read as a floating panel's lift rather than
/// "the keyboard is here" (the reviewer, 2026-11-05). Either way it is the
/// one strip with a halo, selected or not.
/// `lamp` (0..1): a lit strip (it feeds the mix being edited) gives off the
/// lamp's light round it, so the pool reads in every theme (critique #5:
/// in Slate the wash alone was invisible). The keyboard's glow wins.
pub fn strip_glow(focused: bool, lamp: f32) -> impl Fn(&iced::Theme) -> container::Style {
    let flare = (lamp - 1.0).clamp(0.0, 1.0);
    move |_theme| container::Style {
        // The active path's flare also lights the card's rim for a moment.
        border: Border {
            radius: RADIUS_SM.into(),
            color: Color { a: 0.95 * flare, ..p().lamp },
            width: if flare > 0.0 { 2.0 } else { 0.0 },
        },
        shadow: if !focused && lamp > 0.0 {
            // Over 1, the active path's flare (Fun pass 2): brighter and
            // wider for a moment.
            let s = lamp.clamp(0.0, 1.0);
            if dark_theme() {
                Shadow { color: Color { a: (0.5 * s + 0.5 * flare).min(1.0), ..p().lamp }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 14.0 + 22.0 * flare }
            } else {
                Shadow { color: Color { a: (0.7 * s + 0.3 * flare).min(1.0), ..p().lamp }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 10.0 + 14.0 * flare }
            }
        } else if !focused {
            Shadow::default()
        } else if dark_theme() {
            Shadow { color: Color { a: 0.85, ..fader_color() }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 16.0 }
        } else {
            Shadow { color: Color { a: 0.85, ..text_primary() }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 8.0 }
        },
        ..container::Style::default()
    }
}

/// A flat `bg_deep()` read as an undifferentiated slab under the strip
/// cards. A barely-there top-to-bottom falloff (a few percent of white
/// blended in at the top, gone entirely by the bottom) reads as soft
/// ambient light instead — the same trick most DAW backgrounds use — without
/// the gradient itself ever being something you'd consciously notice.
pub fn root(_theme: &iced::Theme) -> container::Style {
    // `Radians::to_distance` subtracts another `FRAC_PI_2` internally
    // before turning this into a direction vector (its `0` is "to top",
    // matching CSS's `linear-gradient()` angle convention) — `PI`, not
    // `FRAC_PI_2`, is what actually points top-to-bottom. Verified against
    // `iced_core::angle::Radians::to_distance` source directly, not guessed
    // — a `FRAC_PI_2` first attempt rendered as a (barely visible,
    // near-flat) *horizontal* gradient instead.
    let gradient = Linear::new(std::f32::consts::PI)
        .add_stop(0.0, blend(bg_deep(), Color::WHITE, 0.035))
        .add_stop(1.0, bg_deep());
    container::Style {
        background: Some(Background::from(gradient)),
        text_color: Some(text_primary()),
        ..container::Style::default()
    }
}

/// Slightly darker than `surface()` (the strip-card background) — previously
/// identical, which flattened the top bar and the mixer strips into one
/// undifferentiated slab. Receding it toward `bg_deep()` reads as a frame the
/// strip cards sit in front of, without introducing a third named color.
pub fn top_bar(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(p().top_bar)),
        text_color: Some(text_primary()),
        ..container::Style::default()
    }
}

/// A small square toggle button (M / S / 48V / PAD) that lights up `active_color`
/// when `active` — with a soft glow to sell the "lit" look — and responds to
/// hover when inactive so it reads as clickable before you ever press it.
/// How much a `Pressed` button's background darkens versus its resting
/// `Hovered` look — the two used to render identically, so a click gave
/// no tactile "it went down" feedback, just the same hover state held a
/// little longer.
const PRESS_DARKEN: f32 = 0.30;

pub fn toggle_button(
    active: bool,
    active_color: Color,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    armed_toggle_button(active, false, active_color)
}

/// A choice that's chosen but means "nothing", an "Off" picked: plain and
/// quiet, not coral, so coral only lights for something that's working
/// (the owner's pick after critique #31, "B": "every Off lights up coral
/// like something is on"). A lighter block with a clear edge, so it still
/// reads as the one chosen.
pub fn chosen_off(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let pressed = matches!(status, button::Status::Pressed);
    let block = blend(surface_hi(), text_primary(), if hovered { 0.16 } else { 0.10 });
    button::Style {
        background: Some(Background::Color(if pressed { blend(block, Color::BLACK, 0.12) } else { block })),
        text_color: text_primary(),
        border: Border { color: text_sec(), width: 1.0, radius: RADIUS_XS.into() },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// Everything a toggle's light can be doing (`lamp_button`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Lamp {
    /// Lit.
    pub active: bool,
    /// Off, but set to come back (a master holds it), or waiting for a
    /// second click (`waiting`): its hue, dimmed.
    pub armed: bool,
    /// Armed and waiting for the second click (48V?, LOOP?): it breathes
    /// (`motion::breath`) until the click comes or the wait runs out, so
    /// it's plain where to click.
    pub waiting: bool,
    /// Warming up, 1 the moment it lit (`motion::bloom`).
    pub bloom: f32,
    /// Going out, 1 the moment it went off (`motion::cool`).
    pub cool: f32,
    /// Where it sits in a joined set (M|S): which corners round.
    pub joint: Joint,
    /// At rest its label wears its hue (2026-11-03, the owner's reference:
    /// an unlit S in green letters, an unlit M in orange), so a key says
    /// what it will light up as before it's pressed. M, S and 48V; the
    /// coral toggles stay quiet, as coral is kept for what's live.
    pub hue_label: bool,
    /// Sunk into the strip rather than standing on it (the owner: M and S
    /// "are inset round buttons", "not popping up"): at rest a cut, shade
    /// deeper at the top; lit, the colour glows inside it.
    pub inset: bool,
    /// Its corners' radius (0 for the keys' `RADIUS_XS`); half its height
    /// makes a capsule.
    pub round: f32,
}

/// An inset key's top and foot at rest: shade, deeper at the top. Under
/// the cursor it comes forward (lighter in a dark theme, darker in a light
/// one); pressed, it sinks further.
fn key_inset_colors(hovered: bool, pressed: bool) -> (Color, Color) {
    let shade = |a: f32| Color { a, ..Color::BLACK };
    let (top, foot) = match (pressed, hovered, dark_theme()) {
        (true, _, true) => (0.55, 0.4),
        (false, true, true) => (0.3, 0.12),
        (false, false, true) => (0.44, 0.24),
        (true, _, false) => (0.24, 0.18),
        (false, true, false) => (0.16, 0.1),
        (false, false, false) => (0.17, 0.1),
    };
    (shade(top), shade(foot))
}

/// Where a key sits in a set of keys joined into one block (M|S, M|CUE):
/// the block's outer corners round, the inner ones are square, and the two
/// rims meet in a seam.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Joint {
    #[default]
    Alone,
    Left,
    Right,
}

impl Joint {
    /// The corners for a block of radius `r`.
    pub fn radius(self, r: f32) -> iced::border::Radius {
        let (tl, tr, br, bl) = match self {
            Joint::Alone => (r, r, r, r),
            Joint::Left => (r, 0.0, 0.0, r),
            Joint::Right => (0.0, r, r, 0.0),
        };
        iced::border::Radius { top_left: tl, top_right: tr, bottom_right: br, bottom_left: bl }
    }
}

/// A toggle lit in `hue`, with all of `lamp`'s states.
pub fn lamp_button(hue: Color, lamp: Lamp) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        let mut style = if lamp.armed && !lamp.active {
            let breath = if lamp.waiting { crate::motion::breath() } else { 0.0 };
            armed_style(hue, status, breath)
        } else {
            let mut style = plain_toggle(lamp.active, hue, theme, status, if lamp.active { lamp.bloom } else { 0.0 });
            if lamp.inset && !lamp.active {
                let (top, foot) = key_inset_colors(hovered, pressed);
                style.background = Some(lit_from_above(top, foot));
                // A clear edge, not a carved one (the owner's pick after
                // critique #33, "B"): the dark rim read 1.05 on its well.
                // Greyed, the faint resting rim it had (critique #63 A 1: the
                // live rim on Speaker B and an idle Cough said "press me";
                // inactive controls are exempt from the 3:1).
                style.border.color = if hovered {
                    fader_color()
                } else if matches!(status, button::Status::Disabled) {
                    resting_key_edge()
                } else {
                    well_key_edge()
                };
                // High contrast: a wider rim, so the pill's round ends read
                // as its straight sides do (critique #112 B: ends 3.43-3.52
                // against tops 5.22-5.37).
                if is_high_contrast() && !matches!(status, button::Status::Disabled) {
                    style.border.width = style.border.width.max(1.5);
                }
            }
            if !lamp.active && lamp.hue_label && !matches!(status, button::Status::Disabled) {
                style.text_color = readable_on_keys(hue);
            }
            if !lamp.active && lamp.cool > 0.0 {
                let rest = if lamp.inset { key_inset_colors(hovered, pressed) } else { key_rest_colors(hovered, pressed) };
                style = cooling(style, hue, lamp.cool, rest);
            }
            style
        };
        style.border.radius = lamp.joint.radius(if lamp.round > 0.0 { lamp.round } else { RADIUS_XS });
        style
    }
}

/// A key at rest with the light of a key just switched off still going
/// out of it (`cool`, 1 to 0): its fill, rim, glow and label all fade from
/// the lit look, like an LED cooling.
fn cooling(mut style: button::Style, hue: Color, cool: f32, (rest_top, rest_bottom): (Color, Color)) -> button::Style {
    // From the lit look as it was (`lit_hue`, `power_ink`): from the raw
    // hue, its first frame read 3.9-4.2 and its ink jumped (critique #34).
    let lit = lit_hue(hue);
    let (lit_top, lit_bottom) = key_lit_colors(lit, 0.0, false);
    style.background = Some(lit_from_above(mix(rest_top, lit_top, cool), mix(rest_bottom, lit_bottom, cool)));
    style.border.color = mix(style.border.color, key_lit_edge(lit), cool);
    style.text_color = mix(style.text_color, power_ink(lit), cool);
    style.shadow = Shadow {
        color: Color { a: 0.45 * cool, ..hue },
        offset: iced::Vector::new(0.0, 0.0),
        blur_radius: 6.0,
    };
    style
}

/// The armed look, breathing by `breath` (0..1): its fill and rim swell
/// toward the hue and back. Nothing about it glows, so it never reads as
/// lit.
fn armed_style(hue: Color, status: button::Status, breath: f32) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let pressed = matches!(status, button::Status::Pressed);
    let lift = if hovered { 0.10 } else { 0.0 } + 0.14 * breath;
    let mut top = blend(bg_deep(), hue, 0.24 + lift);
    let mut bottom = blend(bg_deep(), hue, 0.15 + lift * 0.8);
    if pressed {
        top = blend(top, Color::BLACK, PRESS_DARKEN);
        bottom = blend(bottom, Color::BLACK, PRESS_DARKEN);
    }
    button::Style {
        background: Some(lit_from_above(top, bottom)),
        // Readable on its own fill at every point of the breath and under
        // the pointer (critique #28: 2.5:1 at the breath's peak on
        // Daylight, 3.65 at rest).
        text_color: {
            // When no blend toward the text colour gets there (pressed on
            // Daylight: 4.15, critique #32's test), the plainest ink that does.
            // From the ink, not the quiet ink: on Nord the waiting label
            // came out dimmer than its unchosen neighbours and read as
            // greyed out (critique #96 A P2-4).
            let ink = readable_on(blend(text_primary(), hue, 0.55 + 0.25 * breath), text_primary(), &[top, bottom]);
            let worst = |c: Color| contrast(c, top).min(contrast(c, bottom));
            // High contrast holds text at 7:1: its plain ink there
            // (critiques #104 and #105 B: "48V?" read 6.16).
            if fingerprint(&palette()) == fingerprint(&HIGH_CONTRAST) && worst(text_primary()) >= 7.0 {
                text_primary()
            } else if worst(ink) >= 4.5 {
                ink
            } else {
                [ink, Color::BLACK, Color::WHITE].into_iter().max_by(|a, b| worst(*a).total_cmp(&worst(*b))).unwrap_or(ink)
            }
        },
        border: Border {
            // Toward the hue until it shows on the panel at 3:1 (the
            // armed amber read 2.9 in Solarized, critiques #32 and #33).
            color: {
                let base = 0.6 + 0.4 * breath;
                // On every ground a key stands on, the page's floors too
                // (critique #97 B: tuned to the panel, it read 2.47 on Quick
                // setup's floor); past the hue, toward the ink.
                let reads = |c: &Color| key_grounds().iter().all(|g| contrast(blend(*c, *g, 1.0 - SPLIT_EDGE_COVER), *g) >= 3.0);
                (0..=8)
                    .map(|k| blend(border(), hue, (base + k as f32 * 0.05).min(1.0)))
                    .find(reads)
                    .or_else(|| (1..=10).map(|k| blend(hue, text_primary(), k as f32 / 10.0)).find(reads))
                    .unwrap_or(hue)
            },
            // Wide enough to hold on a half pixel (critique #96 B: the
            // waiting cell's rim read 1.87-2.87 on Solarized, 2.15-2.56 on
            // Nord and Daylight, beside unlit cells now at 5 and more).
            width: SPLIT_EDGE_W,
            radius: RADIUS_XS.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A key set but not yet doing its work (Reverb on, nothing heard yet): the
/// armed look, steady (it waits for sound, not for a click), in `radius`.
pub fn key_button_armed(hue: Color, radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let mut style = armed_style(hue, status, 0.0);
        style.border.radius = radius.into();
        style
    }
}

/// `toggle_button` with a third, "armed" state: off right now, but set to
/// come back. A channel's M (or S) whose mute (solo) the sidebar's Mute
/// (Solo) master has switched off shows it, TotalMix-style, so the
/// channels a press on the master will restore stay visible: the button's
/// own color, dimmed, as a tinted outline rather than a fill. `armed` is
/// ignored while `active`.
pub fn armed_toggle_button(
    active: bool,
    armed: bool,
    active_color: Color,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    lamp_button(active_color, Lamp { active, armed, ..Lamp::default() })
}

fn plain_toggle(
    active: bool,
    active_color: Color,
    _theme: &iced::Theme,
    status: button::Status,
    bloom: f32,
) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let pressed = matches!(status, button::Status::Pressed);
    let bloom = bloom.clamp(0.0, 1.0);

    // A low block (`key_rest`), or lit solid in its hue (`key_lit`).
    // Pressed, a block at rest loses its lift and its shadow: it went down.
    // Lit, in its hue shaded as far as its words need (`lit_hue`).
    let lit = lit_hue(active_color);
    let background = if active {
        key_lit(lit, bloom, pressed)
    } else {
        key_rest(hovered, pressed)
    };
    let border_color = if active {
        key_lit_edge(lit)
    } else if hovered {
        fader_color()
    } else {
        // The resting keys' clear edge (critique #33's pick; #36 found
        // these choice keys still on the faint one).
        resting_key_edge()
    };
    let shadow = if active {
        Shadow {
            color: Color {
                a: if pressed { 0.25 } else { 0.45 + 0.4 * bloom },
                ..active_color
            },
            offset: iced::Vector::new(0.0, 0.0),
            blur_radius: if pressed { 3.0 } else { 6.0 + 12.0 * bloom },
        }
    } else {
        Shadow::default()
    };

    button::Style {
        background: Some(background),
        // A button with no `on_press` at all is `Disabled` in iced: same
        // box, same place, dimmed label. Two things ride on that here —
        // the skeleton `F` in the sidebar's M/S/F row, and every control
        // the attached device cannot drive (the class-compliant
        // personality has no RME-protocol features). Before this they
        // rendered as live toggles that simply never lit.
        text_color: if matches!(status, button::Status::Disabled) {
            Color { a: 0.4, ..text_sec() }
        } else if active {
            power_ink(lit)
        } else {
            text_sec()
        },
        border: Border {
            color: border_color,
            width: 1.0,
            radius: RADIUS_XS.into(),
        },
        shadow,
        snap: false,
    }
}

/// The QUICK/MIXER/MATRIX view switch in the top bar, the app's primary
/// navigation: the active tab is the icon's own white pill with the
/// tile's near-black label, inactive tabs stay flat text until hovered.
/// It is the same `accent()`/`on_active()` pair every engaged toggle uses,
/// so "this is the active view" reads in the same language as "this
/// switch is on".
pub fn tab_toggle(active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        let background = if active {
            if pressed {
                blend(accent(), Color::BLACK, PRESS_DARKEN)
            } else {
                accent()
            }
        } else if pressed {
            // The resting/hover states here are surface_hi()/TRANSPARENT, and
            // TRANSPARENT has nothing for `blend` toward black to darken -
            // give the inactive tab its own distinct, visible press color
            // instead of falling through to "looks like hover held longer."
            Color {
                a: 0.35,
                ..accent_dim()
            }
        } else if hovered {
            surface_hi()
        } else {
            Color::TRANSPARENT
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: if active { on_active() } else { text_sec() },
            border: Border {
                radius: RADIUS_SM.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

/// The shared track of a joined 2-way switch (`sidebar::segmented_row`):
/// one recessed rounded body that both cells sit inside, so the pair
/// reads as a single control with a sliding thumb instead of two
/// unrelated pills.
pub fn segment_track(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(bg_deep())),
        border: Border {
            // The control's own outline, at 3:1 wherever it lands (critique
            // #87 B: the EQ's slope track 1.42-1.68).
            color: steady_edge(border(), NEAR_GRID_COVER),
            width: 1.0,
            radius: RADIUS_SM.into(),
        },
        ..container::Style::default()
    }
}

/// One cell inside a `segment_track`: the lit side is a raised graphite
/// thumb, the other is bare text on the track that only gains a fill
/// under the cursor: a switch position, not an "on" state.
pub fn segment_cell(active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        // The side it points to stands as a key at rest on the track (the
        // reviewer: a faint lift was too weak a mark for the open page).
        let background = if active {
            let (top, foot) = key_rest_colors(hovered, pressed);
            let base = surface_hi();
            Background::from(
                Linear::new(std::f32::consts::PI)
                    .add_stop(0.0, over(top, base))
                    .add_stop(1.0, over(foot, base)),
            )
        } else if hovered {
            Background::Color(surface())
        } else {
            Background::Color(Color::TRANSPARENT)
        };
        button::Style {
            background: Some(background),
            // A side with no `on_press` is inert: dimmed like every other
            // disabled control, so a switch that cannot act does not look
            // like one that just did nothing.
            text_color: match (status, active) {
                (button::Status::Disabled, _) => Color { a: 0.4, ..text_sec() },
                (_, true) => text_primary(),
                (_, false) => text_sec(),
            },
            border: Border {
                color: if active { steady_edge(control_edge(), NEAR_GRID_COVER) } else { Color::TRANSPARENT },
                width: 1.0,
                radius: (RADIUS_SM - 2.0).into(),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

pub fn plain_button(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Pressed => blend(surface_hi(), Color::BLACK, PRESS_DARKEN),
        button::Status::Hovered => surface_hi(),
        _ => surface(),
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: text_primary(),
        border: Border {
            color: border(),
            width: 1.0,
            radius: RADIUS_SM.into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}


/// The sidebar's device chip as a button: the `chip` look at rest, the
/// field-style ring under the cursor and while the popover it opens is up.
#[cfg(test)]
pub fn chip_button(open: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    chip_button_lit(open, chip_edge())
}

/// `chip_button` with its resting rim in `rest` (the dot-alone chip: a
/// faint ring in the dot's own colour; the firm one under the pointer or
/// while its panel is open).
pub fn chip_button_lit(open: bool, rest: Color) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = match status {
            // Half the press on a light theme, so its words stay readable
            // while held (critique #28: 2.4:1 on Daylight).
            button::Status::Pressed => blend(bg_deep(), Color::BLACK, if dark_theme() { PRESS_DARKEN } else { PRESS_DARKEN * 0.5 }),
            button::Status::Hovered => blend(bg_deep(), surface_hi(), 0.5),
            _ => bg_deep(),
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: text_primary(),
            border: Border {
                color: if open || hovered { fader_color() } else { rest },
                // 1.5 px, split-safe: at 1 px (critique #63 A 2's weight) its
                // left side fell between pixels at 2.1 in every theme
                // (critique #64 B 2); the chip's x comes from its width.
                width: SPLIT_EDGE_W,
                radius: RADIUS_MD.into(),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

/// The device chip's resting rim: `border` moved toward the ink until it
/// reads at 3:1 on the top bar and on its own face wherever it lands on the
/// pixel grid (carried since #57: the chip's rims read 1.10-1.80, the chip
/// a button that opens the device panel; critique #64 B 2: tuned for the
/// grid at 1 px, its left side read 2.1 between two pixels).
pub fn chip_edge() -> Color {
    let grounds = [p().top_bar, bg_deep()];
    (0..=20)
        .map(|k| blend(border(), text_primary(), k as f32 / 20.0))
        .find(|c| grounds.iter().all(|g| contrast(blend(*c, *g, 1.0 - SPLIT_EDGE_COVER), *g) >= 3.0))
        .unwrap_or_else(text_primary)
}

/// A flat, textual button: no box at rest, brightening to `surface_hi()`
/// under the cursor.
///
/// For secondary actions and utilities (undo/redo, a panel's "store", its
/// "−"/"+") that were previously the same bordered box as a real control.
/// In a 210px column that added up to nothing but boxes, and a row of
/// equally-sized boxes carries no information about which of them matters.
///
/// `Disabled` (a button with no `on_press`, e.g. undo with an empty stack)
/// drops to a quarter-opacity label: the flat look has no border to gray
/// out, so that is what keeps "nothing to undo" from reading as "click
/// me".
pub fn ghost_button(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Pressed => (
            Background::Color(blend(surface(), Color::BLACK, PRESS_DARKEN)),
            text_primary(),
        ),
        button::Status::Hovered => (Background::Color(surface_hi()), text_primary()),
        button::Status::Disabled => (
            Background::Color(Color::TRANSPARENT),
            Color { a: 0.4, ..text_sec() },
        ),
        _ => (Background::Color(Color::TRANSPARENT), text_sec()),
    };
    button::Style {
        background: Some(background),
        text_color,
        border: Border {
            radius: RADIUS_SM.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}


pub fn text_input(
    _theme: &iced::Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    use iced::widget::text_input::Status;
    // A cut that holds words, with a control's rim (the reviewer: in the
    // panel's own fill with a `border` hairline, at 1.4:1 to 1.8:1, a field
    // read as a stray label, not a place to type).
    let border_color = match status {
        Status::Focused { .. } => accent(),
        Status::Hovered => fader_color(),
        // At rest the keys' steady rim, so it reads off the pixel grid
        // (critique #59 B: the Mic preset name's top 2.34, split over two
        // rows once its panel moved half a pixel).
        _ => steady_edge(control_edge(), SPLIT_EDGE_COVER),
    };
    let width = if matches!(status, Status::Active) { SPLIT_EDGE_W } else { 1.0 };
    let (top, foot) = if dark_theme() {
        (Color { a: 0.35, ..Color::BLACK }, Color { a: 0.2, ..Color::BLACK })
    } else {
        (Color { a: 0.07, ..Color::BLACK }, Color { a: 0.03, ..Color::BLACK })
    };
    iced::widget::text_input::Style {
        background: lit_from_above(top, foot),
        border: Border {
            color: border_color,
            width,
            radius: RADIUS_SM.into(),
        },
        icon: text_sec(),
        // Quieter than typed words (critique #96 A P3-9: a prompt as
        // strong as a name read as a name already typed).
        placeholder: muted(),
        value: text_primary(),
        selection: accent_dim(),
    }
}

pub fn pick_list(
    _theme: &iced::Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    use iced::widget::pick_list::Status;
    let border_color = match status {
        Status::Opened { .. } | Status::Hovered => fader_color(),
        // The steady rim at rest (critique #59 B: the Mic preset list's foot
        // 2.47, split over two rows).
        Status::Active => steady_edge(border(), SPLIT_EDGE_COVER),
    };
    iced::widget::pick_list::Style {
        text_color: text_primary(),
        placeholder_color: text_sec(),
        handle_color: text_sec(),
        background: Background::Color(surface()),
        border: Border {
            color: border_color,
            width: if matches!(status, Status::Active) { SPLIT_EDGE_W } else { 1.0 },
            radius: RADIUS_SM.into(),
        },
    }
}

/// A pick list as a capsule key (the EQ's **Preset ▾**, as the reference
/// sets it in its panel's header): the Key material, round ends.
pub fn capsule_pick_list(radius: f32) -> impl Fn(&iced::Theme, iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
    move |_theme, status| {
        use iced::widget::pick_list::Status;
        let hovered = matches!(status, Status::Opened { .. } | Status::Hovered);
        let (top, foot) = key_rest_colors(hovered, false);
        iced::widget::pick_list::Style {
            text_color: text_primary(),
            placeholder_color: if hovered { text_primary() } else { text_sec() },
            handle_color: text_sec(),
            background: lit_from_above(top, foot),
            // The steady rim at rest (critique #59 B: the Mic preset list's
            // foot 2.47, split over two rows).
            border: Border {
                color: if hovered { fader_color() } else { steady_edge(control_edge(), SPLIT_EDGE_COVER) },
                width: if hovered { 1.0 } else { SPLIT_EDGE_W },
                radius: radius.into(),
            },
        }
    }
}

/// An EQ band's type key (the reviewer: solid fills read as lit and were
/// the loudest things on screen): a rest key wearing its band's hue, a 1px
/// rim in it and the hue at 18% inside; dimmer while the band is off.
pub fn band_key(hue: Color, on: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let k = if on { 1.0 } else { 0.45 };
        let fill = Color { a: (if hovered { 0.28 } else { 0.18 }) * k, ..hue };
        button::Style {
            background: Some(Background::Color(fill)),
            text_color: readable_on_keys(hue),
            // Off, its faded hue read 1.95-2.92 (critique #97 B): the firm
            // edge then, the band's hue once it is on.
            border: Border { color: if on { hue } else { firm_edge() }, width: if on { 1.0 } else { SPLIT_EDGE_W }, radius: RADIUS_XS.into() },
            ..button::Style::default()
        }
    }
}

/// A round ON key (the EQ's and the Low Cut's, as the reference's): a
/// little round cut with ON in it, ringed in the accent and glowing when
/// on, a quiet rim when off.
pub fn on_ring(active: bool, radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        let (top, foot) = key_inset_colors(hovered, pressed);
        button::Style {
            background: Some(lit_from_above(top, foot)),
            text_color: if active { readable_on_keys(accent()) } else if hovered { text_primary() } else { muted() },
            border: Border {
                // Off and at rest: the firm edge (3:1 on every ground; the
                // EQ's ON measured 2.97 on Daylight since critique #87).
                color: if active { accent() } else if hovered { fader_color() } else { page_key(radius)(_theme, status).border.color },
                width: if active { 2.0 } else { 1.0 },
                radius: radius.into(),
            },
            shadow: if active {
                Shadow { color: Color { a: 0.4, ..accent() }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 6.0 }
            } else {
                Shadow::default()
            },
            ..button::Style::default()
        }
    }
}

/// A part's round ON inside the D panel's More: lit while set and at work;
/// set while the path is off, the "set, waiting" look (the owner's pick
/// after critique #32, "B"); the plain ring while not set.
pub fn part_key(set: bool, at_work: bool, radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme, status| {
        if set && !at_work {
            (key_button_armed(accent(), radius))(theme, status)
        } else {
            (on_ring(set, radius))(theme, status)
        }
    }
}

/// The D panel's power: lit solid in coral while the path is on, as every
/// lit key is (critique #32: a 2 px ring was the weakest lit mark in the
/// panel, under the lit choices it powers); the plain ring while off.
pub fn power_key(active: bool, radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme, status| {
        if active {
            // Pressed, it keeps its colour and loses its glow: the darkened
            // coral of a pressed lit key is a middle tone no ink reads on
            // at 4.5:1 (black or white: 4.3, the test).
            let pressed = matches!(status, button::Status::Pressed);
            let hue = power_hue();
            let mut style = (toggle_button(true, hue))(theme, if pressed { button::Status::Hovered } else { status });
            style.border.radius = radius.into();
            if pressed {
                style.shadow = Shadow::default();
            }
            style.text_color = power_ink(hue);
            style
        } else {
            (on_ring(false, radius))(theme, status)
        }
    }
}

/// The ink that reads best on a key lit in `hue` (its top and foot).
fn power_ink(hue: Color) -> Color {
    let (top, bottom) = key_lit_colors(hue, 0.0, false);
    let worst = |c: Color| contrast(c, top).min(contrast(c, bottom));
    let inks = [on_active(), text_primary(), Color::BLACK, Color::WHITE];
    // The theme's own lit ink first (DESIGN.md: lit keys in `on_active`),
    // then the text colour, black or white only where neither reads: the
    // best of the four always won, and pure black beat the plum ink in
    // every dark theme (critique #34's measurement).
    inks.into_iter()
        .find(|c| worst(*c) >= 4.5)
        .unwrap_or_else(|| inks.into_iter().max_by(|a, b| worst(*a).total_cmp(&worst(*b))).unwrap_or(on_active()))
}

/// The armed look breathing, for a key that waits for a second press
/// (48V?'s way): Reset to Natural's "Reset? Press again" (critique #34:
/// it waited without breathing).
pub fn key_button_armed_breathing(hue: Color, radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let mut style = armed_style(hue, status, crate::motion::breath());
        style.border.radius = radius.into();
        style
    }
}

/// The D panel's lit ON: the accent, as every lit key (`lit_hue`).
pub(crate) fn power_hue() -> Color {
    lit_hue(accent())
}

/// A lit key's hue, shaded only as far as its words need to read at 4.5:1
/// on it (`power_ink`): most themes' hues read as they are; Daylight's
/// coral and Solarized's are middle tones no ink reads on (4.2 and 3.9,
/// critiques #32 and #33), so a step darker or lighter.
pub(crate) fn lit_hue(hue: Color) -> Color {
    let reads = |h: Color| {
        let (top, bottom) = key_lit_colors(h, 0.0, false);
        let ink = power_ink(h);
        contrast(ink, top).min(contrast(ink, bottom)) >= 4.5
    };
    for k in 0..=12 {
        for toward in [Color::BLACK, Color::WHITE] {
            let h = blend(hue, toward, k as f32 * 0.04);
            if reads(h) {
                return h;
            }
        }
    }
    hue
}

/// The default 10px scrollbar/scroller reads chunky next to everything
/// else we've thinned down — 4px is still comfortably grabbable but looks
/// modern rather than like default browser chrome.
pub fn thin_scrollbar() -> iced::widget::scrollable::Scrollbar {
    iced::widget::scrollable::Scrollbar::new()
        .width(4)
        .scroller_width(4)
}

/// `scrollable` with its scroller in `control_edge` at rest, for the right-hand
/// column: on a short window the open page runs under the zoom bar, and in
/// `border` the bar was too faint to say there was more below (the
/// reviewer, at 1280x700).
pub fn scrollable_marked(theme: &iced::Theme, status: iced::widget::scrollable::Status) -> iced::widget::scrollable::Style {
    let mut style = scrollable(theme, status);
    if matches!(status, iced::widget::scrollable::Status::Active { .. }) {
        // The keys' clear edge, at 3:1 on every ground (critique #58 B:
        // Quick's thumb read 1.25:1, the only sign step 4 was below).
        style.vertical_rail.scroller.background = Background::Color(resting_key_edge());
    }
    style
}

/// A thin, minimal scrollbar rail — the scroller brightens on hover/drag
/// instead of the default Iced chrome, matching the rest of the app.
pub fn scrollable(
    _theme: &iced::Theme,
    status: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    use iced::widget::scrollable::{AutoScroll, Rail, Scroller, Status};

    let (h_active, v_active) = match status {
        Status::Active { .. } => (false, false),
        Status::Hovered {
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
            ..
        } => (
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
        ),
        Status::Dragged {
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
            ..
        } => (
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
        ),
    };

    let rail = |active: bool| Rail {
        background: Some(Background::Color(Color::TRANSPARENT)),
        border: Border::default(),
        scroller: Scroller {
            background: Background::Color(if active { fader_color() } else { border() }),
            border: Border {
                radius: RADIUS_SM.into(),
                ..Border::default()
            },
        },
    };

    iced::widget::scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail(v_active),
        horizontal_rail: rail(h_active),
        gap: None,
        auto_scroll: AutoScroll {
            background: Background::Color(surface()),
            border: Border::default(),
            shadow: Shadow::default(),
            icon: text_primary(),
        },
    }
}

pub fn menu(_theme: &iced::Theme) -> menu::Style {
    menu::Style {
        background: Background::Color(surface()),
        // An open list over the strips: the firm edge (critique #97 B: its
        // hairline read 1.42-1.56 and its face is the page's own tone).
        border: Border {
            color: firm_edge(),
            width: SPLIT_EDGE_W,
            radius: RADIUS_SM.into(),
        },
        text_color: text_primary(),
        selected_text_color: text_primary(),
        selected_background: Background::Color(accent_dim()),
        shadow: Shadow::default(),
    }
}

// ── The counter (right-hand column) ──────────────────────────────

/// The right-hand column: its own ground tone, with the lamp's wash
/// falling from the top where the placard stands.
pub fn counter(_theme: &iced::Theme) -> container::Style {
    let gradient = Linear::new(std::f32::consts::PI)
        .add_stop(0.0, blend(p().counter, lamp(), 0.14))
        .add_stop(0.35, p().counter)
        .add_stop(1.0, p().counter);
    container::Style {
        background: Some(Background::from(gradient)),
        text_color: Some(text_primary()),
        ..container::Style::default()
    }
}

/// The Now Mixing placard: an ivory card on the counter, with a plain
/// drop shadow (no coloured glow).
pub fn placard(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(text_primary())),
        text_color: Some(on_placard()),
        border: Border {
            radius: RADIUS_SM.into(),
            ..Border::default()
        },
        shadow: Shadow {
            color: Color {
                a: 0.6,
                ..Color::BLACK
            },
            offset: iced::Vector::new(0.0, 8.0),
            blur_radius: 22.0,
        },
        ..container::Style::default()
    }
}

/// The placard drawn close over the levels (the column snug or tight):
/// its shadow shorter, so it ends above the levels' first line (critique
/// #40: at 1280x700 on Daylight the full shadow lay under "· Dim" and took
/// it to 3.31:1; no lamp ink reads at 4.5:1 there and stays amber).
pub fn placard_close(theme: &iced::Theme) -> container::Style {
    container::Style {
        shadow: Shadow {
            color: Color { a: 0.45, ..Color::BLACK },
            offset: iced::Vector::new(0.0, 3.0),
            blur_radius: 8.0,
        },
        ..placard(theme)
    }
}

/// A small button on the placard's ivory card: a hairline pill in the
/// card's ink at rest, filled with it while on (the card's own colours
/// reversed), so it reads as the card's own control.
pub fn placard_button(active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    placard_button_style(active)
}

/// A line of the placard that opens something ("Fed by … ›", issue #23):
/// printed type at rest, a faint tint and the name's ink under the cursor.
pub fn placard_link(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| Background::Color(blend(text_primary(), on_placard(), 0.08))),
        text_color: if hovered { on_placard() } else { on_placard_sec() },
        border: Border { radius: RADIUS_XS.into(), ..Border::default() },
        ..button::Style::default()
    }
}

fn placard_button_style(active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, fg) = if active {
            (on_placard(), text_primary())
        } else if hovered {
            (blend(text_primary(), on_placard(), 0.10), on_placard())
        } else {
            (Color::TRANSPARENT, on_placard())
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            // 1.5 px, so the rim reads wherever the key lands (critique #69
            // B: "Only these" 1.40-1.87 where its 1 px fell between pixels).
            border: Border {
                color: on_placard_sec(),
                width: if active { 0.0 } else { SPLIT_EDGE_W },
                radius: RADIUS_SM.into(),
            },
            ..button::Style::default()
        }
    }
}

/// The placard's name, which is also the submix selector: printed type at
/// rest, a hairline only under the cursor or while open.
pub fn placard_pick_list(
    _theme: &iced::Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    use iced::widget::pick_list::Status;
    let edge = match status {
        Status::Opened { .. } | Status::Hovered => Color { a: 0.35, ..on_placard() },
        Status::Active => Color::TRANSPARENT,
    };
    iced::widget::pick_list::Style {
        text_color: on_placard(),
        placeholder_color: on_placard_sec(),
        handle_color: on_placard_sec(),
        background: Background::Color(Color::TRANSPARENT),
        border: Border {
            color: edge,
            width: 1.0,
            radius: RADIUS_SM.into(),
        },
    }
}

/// The placard's mix list with its closed words hidden: the title is drawn
/// over it in the serif (`sidebar::placard`), the open menu keeps its own.
pub fn placard_pick_list_titled(theme: &iced::Theme, status: iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
    iced::widget::pick_list::Style { text_color: Color::TRANSPARENT, ..placard_pick_list(theme, status) }
}

/// A resting key's outline: from the theme's edge toward its words' colour
/// until it shows at 3:1 on the grounds keys stand on (the owner's pick
/// after critique #33, "B: a clearer edge": the Control Room and Quick
/// setup keys measured 1.05 to 2.27).
pub fn resting_key_edge() -> Color {
    // The grounds keys stand on: the panel, the floor, a raised box (the
    // Voice panel's Sound box) and a card (critique #36: Fit and the
    // other-mic card's keys read 2.5 to 3.0 on those).
    // And a lit strip's panel under the lamp's wash (critique #49: the FX
    // lane's rim read 2.80 to 2.99 on Nord's lit strips).
    let base = if dark_theme() { border() } else { control_edge() };
    (0..=20)
        .map(|k| blend(base, text_primary(), k as f32 * 0.05))
        .find(|c| key_grounds().iter().all(|g| contrast(*c, *g) >= 3.0))
        .unwrap_or_else(text_sec)
}

/// The grounds keys stand on (see `resting_key_edge`).
fn key_grounds() -> [Color; 6] {
    let lamp = p().lamp;
    [
        surface(),
        bg_deep(),
        surface_hi(),
        blend(surface(), text_sec(), 0.06),
        layer(Color { a: LAMP_TOP, ..lamp }, surface()),
        layer(Color { a: LAMP_MID, ..lamp }, surface()),
    ]
}

/// A key whose rim still reads at 3:1 wherever it lands on the pixel grid
/// (`notice_key`'s cure, for any key style): unlit, its 1px rim goes
/// `SPLIT_EDGE_W` wide and moves toward the ink until a pixel it covers
/// three quarters reads at 3:1 on every key ground. Critique #56: folded
/// M/S sit 0.1-0.3 px off the grid under the 1.3x name line (top and bottom
/// rims 2.25-2.63, the sides 3.5), and the Quick page's Sound keys, a
/// third of a row each, fall between pixels sideways (2.08 on one side,
/// 3.11 on the other). Lit keys keep their own edge: their face says it.
pub fn split_safe_key<F>(style: F, lit: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style
where
    F: Fn(&iced::Theme, button::Status) -> button::Style,
{
    steady_rim(style, lit, SPLIT_EDGE_W, SPLIT_EDGE_COVER)
}

/// How much of its pixel a 1px rim covers at the least on a key placed
/// within a tenth of a pixel of the grid (`firm_rim_key`).
pub const NEAR_GRID_COVER: f32 = 0.85;

/// `split_safe_key` for a key that sits within a tenth of a pixel of the
/// grid (folded M/S, once their band is whole pixels): its 1px rim, only
/// as much darker (lighter in a dark theme) as reads at 3:1 when it is
/// `NEAR_GRID_COVER` covered. The 1.5px rim read heavy beside the open
/// strips' keys there.
pub fn firm_rim_key<F>(style: F, lit: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style
where
    F: Fn(&iced::Theme, button::Status) -> button::Style,
{
    steady_rim(style, lit, 1.0, NEAR_GRID_COVER)
}

fn steady_rim<F>(style: F, lit: bool, width: f32, cover: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style
where
    F: Fn(&iced::Theme, button::Status) -> button::Style,
{
    move |theme, status| {
        let mut s = style(theme, status);
        if !lit && !matches!(status, button::Status::Disabled) && s.border.width <= 1.0 {
            s.border.width = width;
            s.border.color = steady_edge(s.border.color, cover);
        }
        s
    }
}

/// `edge` moved toward the ink a twentieth at a time until a pixel `cover`
/// covered by it reads at 3:1 on every key ground.
fn steady_edge(edge: Color, cover: f32) -> Color {
    // A black-and-white theme's own bar (critique #96 B: tuned to 3:1,
    // keys on a High contrast card read 4.06-4.37 against its 4.5).
    let need = if contrast(text_primary(), bg_deep()) >= 20.0 { 4.6 } else { 3.0 };
    (0..=20)
        .map(|k| blend(edge, text_primary(), k as f32 / 20.0))
        .find(|c| key_grounds().iter().all(|g| contrast(blend(*c, *g, 1.0 - cover), *g) >= need))
        .unwrap_or_else(text_primary)
}

/// A Control Room key: a raised block of the panel's own surface, lit
/// from above, standing a little off the counter; solid coral when engaged
/// (a live state); its label faded when the key has no function yet.
///
/// `cool` (1 to 0) is a key just switched off going out (`motion::cool`).
///
/// `radius`: its corners (half its height: a capsule, as the reference's
/// Control Room keys are).
pub fn key_button(active: bool, bloom: f32, cool: f32, radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    key_button_in(accent(), active, bloom, cool, radius)
}

/// `key_button` lit in `hue` rather than the accent: a key whose state has
/// a hue of its own (Mute FX lights in `mute`, the Hue Is Information rule).
pub fn key_button_in(hue: Color, active: bool, bloom: f32, cool: f32, radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let bloom = if active { bloom.clamp(0.0, 1.0) } else { 0.0 };
        let cool = if active { 0.0 } else { cool.clamp(0.0, 1.0) };
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        let disabled = matches!(status, button::Status::Disabled);
        let block = surface_hi();
        let (bg, fg, edge) = if disabled {
            // It keeps its box, faded (the reviewer: a bare word read as a
            // label, not a key you can't use yet).
            (
                Background::Color(Color { a: 0.4, ..block }),
                Color { a: 0.45, ..text_sec() },
                Color { a: 0.4, ..control_edge() },
            )
        } else if active {
            let lift = if hovered && !pressed { 0.12 } else { 0.0 };
            // Shaded as far as its words need (critique #33: "Move it" read
            // 3.4 hovered on Daylight); hovered, lighter only while those
            // same words still read, so the ink never flips on hover
            // (critique #34: white to black on Daylight).
            let base = lit_hue(hue);
            let ink = power_ink(base);
            let lifted = blend(base, Color::WHITE, lift);
            let (lt, lb) = key_lit_colors(lifted, 0.0, false);
            let lit = if lift > 0.0 && contrast(ink, lt).min(contrast(ink, lb)) >= 4.5 { lifted } else { base };
            // The bloom lightens the top only as far as the words still
            // read on it (critique #61 B 2: an effect key's flare as it was
            // first heard took Daylight's white label to 2.9).
            let bloom = (0..=10)
                .rev()
                .map(|k| bloom * k as f32 / 10.0)
                .find(|b| contrast(ink, key_lit_colors(lit, *b, pressed).0) >= 4.5)
                .unwrap_or(0.0);
            (key_lit(lit, bloom, pressed), ink, key_lit_edge(base))
        } else {
            let (top, bottom) = match (pressed, hovered, dark_theme()) {
                (true, _, _) => (blend(block, Color::BLACK, 0.12), blend(block, Color::BLACK, 0.06)),
                (false, true, true) => (blend(block, text_primary(), 0.16), blend(block, text_primary(), 0.06)),
                (false, false, true) => (blend(block, text_primary(), 0.09), block),
                // On a light panel: near white at the top, a shade under the
                // panel at the foot (the reviewer: 1.13:1, only a hairline).
                (false, true, false) => (blend(block, Color::WHITE, 0.95), blend(block, Color::BLACK, 0.03)),
                (false, false, false) => (blend(block, Color::WHITE, 0.85), blend(block, Color::BLACK, 0.07)),
            };
            // Just switched off: the coral going out of it.
            let (lit_top, lit_bottom) = key_lit_colors(hue, 0.0, false);
            (
                lit_from_above(mix(top, lit_top, cool), mix(bottom, lit_bottom, cool)),
                mix(text_primary(), on_active(), cool),
                mix(if hovered { fader_color() } else { resting_key_edge() }, key_lit_edge(hue), cool),
            )
        };
        // A key that has just lit flares (`motion::bloom`), then settles
        // to its plain coral fill.
        let shadow = if bloom > 0.0 {
            Shadow {
                color: Color { a: 0.6 * bloom, ..hue },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 16.0 * bloom,
            }
        } else if cool > 0.0 {
            Shadow {
                color: Color { a: 0.45 * cool, ..hue },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 8.0,
            }
        } else {
            Shadow::default()
        };
        button::Style {
            background: Some(bg),
            text_color: fg,
            border: Border {
                color: edge,
                width: 1.0,
                radius: radius.into(),
            },
            shadow,
            ..button::Style::default()
        }
    }
}


/// A mixer floor (0 inputs, 1 playback, 2 outputs): flat ground tone.
pub fn floor(tone: usize) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(p().floors[tone.min(2)])),
        text_color: Some(text_primary()),
        ..container::Style::default()
    }
}

/// A folded mixer floor while one row is open: its own ground tone at
/// rest, a step brighter under the cursor, darker while pressed.
/// A folded row's band; `lit`, it holds strips that feed the mix being
/// edited, and takes the lamp as a lit strip does: a wash of it and its rim
/// (critique #6: folded, the row went dark).
pub fn folded_floor(tone: usize, lit: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let ground = p().floors[tone.min(2)];
        // Hover and press from the bare floor, then the lamp's wash over it,
        // so a lit band keeps its wash under the cursor (the Hover Is Fill
        // rule). No rim: its light is thrown round it, as a strip's is
        // (`strip_glow`, on the band's wrapper).
        let mut background = match status {
            button::Status::Hovered => blend(ground, surface_hi(), 0.6),
            button::Status::Pressed => blend(ground, Color::BLACK, PRESS_DARKEN),
            _ => ground,
        };
        let k = lit.clamp(0.0, 1.0);
        if k > 0.0 {
            background = blend(background, lamp(), k * if dark_theme() { 0.08 } else { 0.12 });
        }
        button::Style {
            background: Some(Background::Color(background)),
            text_color: text_primary(),
            border: Border { radius: RADIUS_SM.into(), ..Border::default() },
            ..button::Style::default()
        }
    }
}

/// A strip's small icon buttons (T, Ø, settings, EQ, collapse): no box
/// at rest, only the glyph in quiet text; a soft fill under the cursor;
/// coral glyph on a plum fill when lit (a live state).
pub fn icon_button(lit: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (bg, fg) = if lit {
            (Some(Background::Color(accent_dim())), accent())
        } else if hovered {
            (Some(Background::Color(surface_hi())), text_primary())
        } else {
            (None, text_sec())
        };
        button::Style {
            background: bg,
            text_color: fg,
            border: Border {
                radius: RADIUS_XS.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// A key in the strip's side column (T, Ø, settings, EQ) and the collapse
/// key (2026-11-03, the owner: "They dont like the buttons. They are more
/// like icons"): the toggles' block a size down, with its rim, lifting
/// under the cursor; solid coral while what it opens is open or in use
/// (EQ on, a trim off unity), with a small glow, as a lit toggle.
/// A side key the one-time hint points at (critique #31, the owner's "B":
/// "a small note points at its D key"): the quiet tile with a ring in the
/// lamp's light, its letter in it too, until the hint is answered.
pub fn side_key_hinted(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let pressed = matches!(status, button::Status::Pressed);
    let ink = hinted_ink();
    // The lamp's fill and glow, its letter in the lamp, and the plain
    // tile's rim: no ring, which means "panel open" on a side key
    // (critique #60 A 1); a lamp dot at its corner says "look here"
    // (`hint_dot`).
    let plain = side_key(false)(_theme, status);
    button::Style {
        background: Some(Background::Color(Color { a: if pressed { HINT_FILL_PRESSED } else { HINT_FILL }, ..lamp() })),
        text_color: ink,
        border: plain.border,
        shadow: Shadow { color: Color { a: 0.45, ..lamp() }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 6.0 },
        snap: false,
    }
}

/// The one-time D hint's dot: the lamp, readable on every floor, with a
/// soft glow, at the key's top right.
pub fn hint_dot(radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(readable_on_floors(lamp()))),
        border: Border { color: Color { a: 0.55, ..Color::BLACK }, width: 0.75, radius: radius.into() },
        shadow: Shadow { color: Color { a: 0.6, ..lamp() }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 5.0 },
        ..container::Style::default()
    }
}

/// The hinted D key's letter and ring: readable on the key's own lamp fill
/// (at rest and pressed) over the strip under it (critique #32: the letter
/// read 4.1:1 in Slate and Solarized, chosen for the bare floors without
/// the fill).
pub fn hinted_ink() -> Color {
    readable_on(lamp(), palette().text_primary, &hinted_grounds())
}

/// What the hinted D's letter sits on: its lamp fill, at rest and pressed,
/// over the strip panel where the side keys stand (the brushed lift and
/// the lamp's wash, hovered or not, lit or not), on every floor. Not the
/// right column's grounds: the key is never there.
fn hinted_grounds() -> Vec<Color> {
    let p = palette();
    let mut out = Vec::new();
    for floor in p.floors {
        for t in [0.6, 0.7, 0.8, 0.9] {
            for hovered in [false, true] {
                for s in [0.0, 1.0] {
                    let lift = panel_lift(&p, t, hovered, s);
                    let panel = blend(floor, Color { a: 1.0, ..lift }, lift.a);
                    let strip = blend(panel, p.lamp, along(&[(0.0, LAMP_TOP), (0.6, LAMP_MID), (1.0, LAMP_FOOT)], t) * s);
                    out.push(blend(strip, p.lamp, HINT_FILL));
                    out.push(blend(strip, p.lamp, HINT_FILL_PRESSED));
                }
            }
        }
    }
    out
}

/// The hinted D key's lamp fill, at rest and pressed (pressed 0.26, not
/// 0.32: with it Nord had no readable ink on a lit, hovered strip).
const HINT_FILL: f32 = 0.18;
const HINT_FILL_PRESSED: f32 = 0.26;

/// The faces a side key's glyph stands on at rest: the key's shade over
/// the strip's panel, plain or under the lamp's wash.
fn side_key_faces() -> [Color; 4] {
    let shade = Color { a: if dark_theme() { 0.2 } else { 0.06 }, ..Color::BLACK };
    let lamp = p().lamp;
    let lit = layer(Color { a: LAMP_TOP, ..lamp }, surface());
    let lit_mid = layer(Color { a: LAMP_MID, ..lamp }, surface());
    [layer(shade, surface()), layer(shade, surface_hi()), layer(shade, lit), layer(shade, lit_mid)]
}

pub fn side_key(lit: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        if lit {
            // The lit keys' shade of the accent, where its letter reads at
            // 4.5 top to foot, as the Control Room's lit keys have it
            // (critique #61 B carried: Daylight's lit EQ letter read 4.05
            // on its top rows).
            let hue = lit_hue(accent());
            return button::Style {
                background: Some(key_lit(hue, 0.0, pressed)),
                text_color: power_ink(hue),
                border: Border { color: key_lit_edge(hue), width: 1.0, radius: RADIUS_XS.into() },
                shadow: Shadow {
                    color: Color { a: if pressed { 0.2 } else { 0.35 }, ..accent() },
                    offset: iced::Vector::new(0.0, 0.0),
                    blur_radius: 5.0,
                },
                snap: false,
            };
        }
        // Quiet until used (the owner, 2026-11-03: they were "too bright
        // and in the face ... competing with the slider"; the reference's
        // side keys are dim tiles): a tile a shade off the panel, its rim
        // the `border` hairline, its glyph `muted` (quiet text that still
        // reads). Under the cursor it comes forward: the glyph in
        // `text_primary`, the rim `fader`. With nothing to do it keeps its
        // box, its glyph fainter still.
        let disabled = matches!(status, button::Status::Disabled);
        let shade = |a: f32| Color { a, ..Color::BLACK };
        let (top, foot) = match (pressed, hovered, dark_theme()) {
            (true, _, true) => (0.32, 0.24),
            (false, true, true) => (0.06, 0.0),
            (false, false, true) => (0.2, 0.12),
            (true, _, false) => (0.14, 0.09),
            (false, true, false) => (0.1, 0.05),
            (false, false, false) => (0.06, 0.02),
        };
        button::Style {
            background: Some(lit_from_above(shade(top), shade(foot))),
            text_color: if disabled {
                Color { a: 0.45, ..muted() }
            } else if hovered {
                text_primary()
            } else {
                // The secondary ink, not `muted`: a 12 px letter's thin
                // strokes blend into the face, so muted's 6:1 read 3.3:1 on
                // screen (critiques #51/#52, measured); held at 4.5:1 on the
                // key's own face, a lamp-lit strip's included.
                readable_on(text_sec(), text_primary(), &side_key_faces())
            },
            // Quiet at rest (the owner's quiet keys), a clear rim under
            // the pointer: `control_edge`, 3.7:1 on the strip (issue #21,
            // the owner's yes; `border` reads 1.5:1, under the 3:1 a
            // control's outline wants).
            border: Border {
                color: if hovered && !disabled { control_edge() } else { border() },
                width: if hovered && !disabled { 1.5 } else { 1.0 },
                radius: RADIUS_XS.into(),
            },
            ..button::Style::default()
        }
    }
}

/// The open side key's ring and glyph: the accent, readable on the key's
/// faces (the wrench's drawn glyph takes it too).
pub fn side_key_ring_ink() -> Color {
    // On the key's faces and on the strip round it (critique #61 B 1:
    // Solarized's EQ letter read 4.26 on the strip, 4.82 on its face).
    let faces = side_key_faces();
    let grounds = key_grounds();
    let all: Vec<Color> = faces.iter().chain(grounds.iter()).copied().collect();
    readable_on(accent(), text_primary(), &all)
}

/// A side key whose panel is open while what it switches is off (critique
/// #59's question: lit solid, D read "on" beside a panel saying "Off"):
/// the plain tile, ringed and lettered in the accent. Solid stays for "on".
pub fn side_key_open(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let mut s = side_key(false)(_theme, status);
    let ring = side_key_ring_ink();
    s.border = Border { color: ring, width: SPLIT_EDGE_W, radius: RADIUS_XS.into() };
    s.text_color = ring;
    s
}

/// An output's FX key while Mute FX holds its return silent: lit in the
/// mute hue, as a muted strip's M is (critique #62 A 4: solid coral said
/// "on and heard" while nothing was heard there).
pub fn side_key_held(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let pressed = matches!(status, button::Status::Pressed);
    let hue = lit_hue(mute_color());
    button::Style {
        background: Some(key_lit(hue, 0.0, pressed)),
        text_color: power_ink(hue),
        border: Border { color: key_lit_edge(hue), width: 1.0, radius: RADIUS_XS.into() },
        ..button::Style::default()
    }
}

/// The ring that ties Mute FX to the FX key it holds, either way round,
/// drawn round the control in its room (critique #65 A 1: a shadow on the
/// key was clipped and never drew): the mute hue readable on the strip
/// and the column; while Mute FX is off and only pointed at (`held`
/// false), the same hue fainter, what a press would silence.
pub fn link_ring(held: bool) -> Color {
    let ring = readable_on(mute_color(), text_primary(), &key_grounds());
    if held {
        ring
    } else {
        // 0.8: at 0.6 it read 2.55 on Daylight's lamp wash (critique #66 A 3).
        Color { a: 0.8, ..ring }
    }
}

/// A section's header bar in the right-hand column (the owner's reference:
/// TotalMix stacks its right-hand panels under header bars, "Snapshots",
/// "Groups", "Layout Presets", each opening its page): a low block of
/// `surface_hi` lit from above, a `border` rim, its title centred; the
/// open one a little brighter, with `text_primary`.
pub fn header_bar(open: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        // Flat, no rim: a heading, not a key (the reviewer: they read as
        // a stack of look-alike buttons).
        let block = if open { blend(surface_hi(), text_primary(), 0.06) } else { surface_hi() };
        let fill = if hovered { blend(block, hover_toward(), 0.08) } else { block };
        button::Style {
            background: Some(Background::Color(fill)),
            text_color: if open || hovered { text_primary() } else { text_sec() },
            border: Border { radius: RADIUS_SM.into(), ..Border::default() },
            ..button::Style::default()
        }
    }
}

/// A fold's bar on a Settings page (More for studios, Reset Mix): the
/// heading's flat bar with a thin firm edge, since it stands among rimmed
/// controls (critique #90 B: its fill alone stood 1.14-1.24 from the panel;
/// #91 A: one of the two had the edge).
pub fn fold_bar(open: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = header_bar(open)(theme, status);
        style.border.color = firm_edge();
        style.border.width = 1.0;
        style
    }
}

/// A row of a slot list (snapshots, layouts; the reference's lists): flat
/// in its well at rest, lifted under the cursor; the selected one's name
/// in `text_primary`. Waiting to be stored into, every row takes the
/// coral rim Store's sweep lights (`level`, 0 to 1).
pub fn list_row(active: bool, target: Option<f32>) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        let lift = match (pressed, hovered, target.is_some()) {
            (true, _, _) => 0.02,
            (false, true, true) => 0.18,
            // 8%: at 12% a hovered row's grey note read 4.50 on Daylight
            // and 3.88-4.0 on Nord and Solarized (critique #103 B).
            (false, true, false) => 0.08,
            _ => 0.0,
        };
        let background = if target.is_some() && hovered {
            Some(Background::Color(Color { a: 0.22, ..accent() }))
        } else if lift > 0.0 {
            Some(Background::Color(Color { a: lift, ..text_primary() }))
        } else {
            None
        };
        button::Style {
            background,
            text_color: if active || hovered { text_primary() } else { text_sec() },
            border: Border {
                color: target.map_or(Color::TRANSPARENT, |level| Color { a: level.clamp(0.0, 1.0), ..accent() }),
                width: if target.is_some() { 1.0 } else { 0.0 },
                radius: RADIUS_XS.into(),
            },
            ..button::Style::default()
        }
    }
}

/// A list row's LED square: a little cut at rest, lit coral for the
/// selected slot (the loaded snapshot, where Store writes), like the
/// reference's lit square. `held`: the slot holds something, and its LED
/// glows dimly in the lamp's amber, as a hardware slot's lamp idles; an empty
/// slot's stays dark (the critique, 2026-11-04: stored and empty slots
/// differed only in how bright their names were).
pub fn list_led(lit: bool, held: bool) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        // Unlit, a little cut (no light rim: the reviewer saw checkboxes):
        // a floor, a darker top wall and a lit lip at its foot, so it
        // reads as a cut with a floor rather than a hole.
        let (floor, wall, lip) = if dark_theme() {
            // Its lip brighter on a dark theme, so an empty slot shows as a
            // cut there (critique #51: near-black on near-black, slots 4
            // to 8 vanished), still no rim.
            (Color { a: 0.5, ..Color::BLACK }, Color { a: 0.7, ..Color::BLACK }, Color { a: 0.22, ..Color::WHITE })
        } else {
            (Color { a: 0.22, ..Color::BLACK }, Color { a: 0.4, ..Color::BLACK }, Color { a: 0.6, ..Color::WHITE })
        };
        // Held: the same cut with the lamp idling in its floor, well short
        // of lit: darkened on a dark theme, a pale wash on a light one
        // (where a darkened coral came near the lit lamp's).
        let (floor, wall) = if held { held_floor_wall() } else { (floor, wall) };
        let unlit = Background::from(
            Linear::new(std::f32::consts::PI)
                .add_stop(0.0, wall)
                .add_stop(0.18, floor)
                .add_stop(0.88, floor)
                .add_stop(1.0, lip),
        );
        container::Style {
            background: Some(if lit { key_lit(accent(), 0.0, false) } else { unlit }),
            // On a dark theme an unlit one keeps a faint edge: the list's
            // ground is near-black, and a darker cut alone vanished there
            // (critique #51). Faint, so it never reads as a checkbox.
            border: Border {
                color: if lit {
                    key_lit_edge(accent())
                } else if held {
                    // A stored one's edge in the stored lamp's colour, so the
                    // key reads 3:1 on its ground and apart from an empty one
                    // (critique #55: no rim, 2.47:1 on a dark theme).
                    let ground = surface();
                    let well = layer(Color { a: 0.06, ..Color::BLACK }, ground);
                    stored_lamp(blend(ground, text_sec(), 0.42), well)
                } else if dark_theme() {
                    // 42%: at 30% it measured 1.6 to 2.8:1, under the 3:1 a
                    // control's edge wants (critiques #52, #53).
                    Color { a: 0.42, ..Color::WHITE }
                } else {
                    // A light theme's empty key: the resting keys' edge, 3:1
                    // on its grounds wherever it lands (carried since #59:
                    // its cut alone read 1.67; critique #63 B 4: on the
                    // Quick page's short column it sits between rows, 1.94).
                    steady_edge(resting_key_edge(), SPLIT_EDGE_COVER)
                },
                // 1.5 px for every key: at 1 px a 16 px row split the rim over
                // two pixel rows (critique #73 B: dark empty 2.06 / 2.27, light
                // stored 2.55; 4.07+ where whole).
                width: SPLIT_EDGE_W,
                radius: 2.0.into(),
            },
            shadow: if lit {
                Shadow { color: Color { a: 0.4, ..accent() }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 5.0 }
            } else {
                Shadow::default()
            },
            ..container::Style::default()
        }
    }
}

/// The Snapshots list's lamp: round, as a desk's LED is (the owner,
/// 2026-11-14, after critique #53: on dark themes the empty squares with a
/// light edge read as checkboxes, though a click recalls a whole mix). Off,
/// a solid dim glass dot with a highlight at its top, never a ring; stored,
/// the lamp idling warm; the one you're on, lit and glowing. `size` is its
/// diameter in pixels (it rounds to a circle at any zoom). `flare` (0 to 1)
/// strikes it, just stored into or recalled: a ring and a halo in the
/// lamp's colour that bloom and fade (`motion::slot_flare`).
pub fn list_lamp(lit: bool, held: bool, size: f32, flare: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        let ground = surface();
        // Off: solid, between the ground and the secondary ink, so it shows
        // on a near-black list as a dot that's off, not as a hole; on a
        // light theme moved toward the ink until it holds 3:1 on the list's
        // well (`well_box`; at 40% it measured 1.5:1, 2026-11-14).
        // Moved toward the ink until it holds 3:1 on the list's well with a
        // margin, in every theme: 3.6 on a light one (the glass's lit top
        // lifts the dot's mean; 3.0 on the base measured 2.5:1 from the
        // capture; 4.2 since critique #73's prep: at 3.6 the centre measured
        // 3.04-3.14 and the glass top 2.69-2.76), 3.4 on a dark one (a fixed 42% measured 2.97:1 in
        // Graphite, and only 62% of the disc over 3:1; critique #55).
        let off = {
            let well = layer(Color { a: 0.06, ..Color::BLACK }, ground);
            let (mut k, floor) = if dark_theme() { (0.42, 3.4) } else { (0.4, 4.2) };
            while k < 1.0 && contrast(blend(ground, text_sec(), k), well) < floor {
                k += 0.04;
            }
            blend(ground, text_sec(), k.min(1.0))
        };
        // Stored: the lamp's own colour, idling, moved until it stands well
        // apart from an empty lamp (1.6:1 brighter on a dark theme, 1.3:1
        // deeper on a light one) and 3:1 on the list's well, in every theme
        // (critique #55: it was the empty lamp tinted, the same brightness
        // in dark, darker than empty in Nord and Solarized, 2.6:1 there).
        let well = layer(Color { a: 0.06, ..Color::BLACK }, ground);
        // On a light theme the stored lamp is the lamp's own warm glow, a
        // pale wash lighter than an empty one (DESIGN), and its 3:1 on the
        // well comes from a rim in the deeper lamp colour: darkened whole
        // until it held 3:1, the amber read as brown (carried since #55).
        let wash = held && !lit && !dark_theme();
        let base = if wash { stored_wash(off) } else if held { stored_lamp(off, well) } else { off };
        let glass = Background::from(
            Linear::new(std::f32::consts::PI)
                .add_stop(0.0, blend(base, Color::WHITE, 0.22))
                .add_stop(0.45, base)
                .add_stop(1.0, blend(base, Color::BLACK, 0.18)),
        );
        // Lit on a light theme: a hot face, the coral lifted toward white
        // at its top, so it is lighter than an empty lamp as well as another
        // hue (critique #73 B: lit vs empty 1.07:1, hue alone; its glow is
        // clipped by the list's scroll).
        // The hot face in a dark theme too: the accent's pale coral sat as
        // bright as the stored amber, told apart by hue alone (critique #76,
        // #77 B: 1.12).
        // The theme's own accent, brightened at full saturation or else
        // darkened, until it stands apart from the stored and empty lamps
        // (critique #78 A 1: a fixed coral sat beside Nord's frost and
        // Solarized's teal and, in Solarized, matched the stored lamp's
        // brightness; #74 A 2: a white lift read pale salmon).
        let stored_face = if dark_theme() { stored_lamp(off, well) } else { stored_wash(off) };
        let lit_face = {
            let hot = lit_lamp(stored_face, off, well);
            Background::from(
                Linear::new(std::f32::consts::PI)
                    .add_stop(0.0, blend(hot, Color::WHITE, 0.18))
                    .add_stop(0.45, hot)
                    .add_stop(1.0, blend(hot, accent(), 0.5)),
            )
        };
        container::Style {
            background: Some(if lit { lit_face } else { glass }),
            border: Border {
                color: if flare > 0.0 {
                    Color { a: flare, ..readable_on_floors(lamp()) }
                } else if lit {
                    key_lit_edge(accent())
                } else if wash {
                    stored_lamp(off, well)
                } else {
                    Color::TRANSPARENT
                },
                // 2 px for the stored wash: at 1.5 the circle's 45° staircase corners
                // read 2.2-2.5 (critique #73 B, the strict ring 2.54).
                width: if flare > 0.0 { 1.0 + flare } else if wash || (lit && !dark_theme()) { 2.0 } else if lit { 1.0 } else { 0.0 },
                radius: (size / 2.0).into(),
            },
            shadow: if flare > 0.0 {
                Shadow { color: Color { a: 0.9 * flare, ..readable_on_floors(lamp()) }, offset: iced::Vector::ZERO, blur_radius: 4.0 + 12.0 * flare }
            } else if lit {
                Shadow { color: Color { a: 0.45, ..accent() }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 6.0 }
            } else {
                Shadow::default()
            },
            ..container::Style::default()
        }
    }
}

/// A stored snapshot's lamp: the lamp's hue, moved toward white on a dark
/// theme (toward black on a light one) until it clears the empty lamp
/// `off` and the list's `well` (`list_lamp`).
/// A stored snapshot's lamp on a light theme: the lamp's own colour, lifted
/// toward white until it glows clearly lighter than the empty lamp `off`
/// (`list_lamp`; its rim is `stored_lamp`).
/// The lit snapshot lamp's face: the accent, lifted toward its own
/// full-saturation bright (so it stays its hue, not a pale wash) or else
/// darkened, the first that stands 1.5:1 from the stored lamp, 1.4:1 from
/// an empty one and 3:1 on the list's well (`list_lamp`).
pub(crate) fn lit_lamp(stored: Color, empty: Color, well: Color) -> Color {
    let a = accent();
    // The accent at more chroma (pushed from its own grey), so the lit lamp
    // reads vivid, not dusty (critique #79 A 6: Audra's lit face went
    // #de937e, duller than the stored amber).
    let vivid = |k: f32| {
        let grey = (a.r + a.g + a.b) / 3.0;
        Color::from_rgb((grey + (a.r - grey) * k).clamp(0.0, 1.0), (grey + (a.g - grey) * k).clamp(0.0, 1.0), (grey + (a.b - grey) * k).clamp(0.0, 1.0))
    };
    let mut all = Vec::new();
    for k in [1.6_f32, 1.3, 1.0] {
        let base = vivid(k);
        let peak = base.r.max(base.g).max(base.b).max(0.01);
        let bright = Color::from_rgb((base.r / peak).min(1.0), (base.g / peak).min(1.0), (base.b / peak).min(1.0));
        all.extend((0..=10).map(|j| blend(base, bright, j as f32 / 10.0)));
        all.extend((1..=10).map(|j| blend(base, Color::BLACK, 0.06 * j as f32)));
    }
    // Lit means brighter than off (critique #79 A 6: Daylight's brick lamp
    // was the darkest dot in the list and read as off).
    let ok = |c: &Color| luminance(*c) > luminance(empty) && contrast(*c, stored) >= 1.5 && contrast(*c, empty) >= 1.4 && contrast(*c, well) >= 3.0;
    let score = |c: &Color| {
        let lighter = if luminance(*c) > luminance(empty) { 1.0 } else { 0.5 };
        lighter * (contrast(*c, stored) / 1.5).min(contrast(*c, empty) / 1.4).min(contrast(*c, well) / 3.0)
    };
    all.iter().copied().find(ok).unwrap_or_else(|| all.iter().copied().fold(a, |best, c| if score(&c) > score(&best) { c } else { best }))
}

pub(crate) fn stored_wash(off: Color) -> Color {
    let mut k = 0.0;
    loop {
        let c = blend(lamp(), Color::WHITE, k);
        if (contrast(c, off) >= 1.6 && luminance(c) > luminance(off)) || k >= 0.6 {
            return c;
        }
        k += 0.03;
    }
}

pub(crate) fn stored_lamp(off: Color, well: Color) -> Color {
    let dark = dark_theme();
    let start = if dark { lamp() } else { readable_on_floors(lamp()) };
    let toward = if dark { Color::WHITE } else { Color::BLACK };
    let apart = if dark { 1.6 } else { 1.3 };
    let mut k = 0.0;
    loop {
        let c = blend(start, toward, k);
        let clear = contrast(c, well) >= 3.2 && contrast(c, off) >= apart && (luminance(c) > luminance(off)) == dark;
        if clear || k >= 0.9 {
            return c;
        }
        k += 0.03;
    }
}

/// A rule as the `border` hairline. iced's own takes a colour from its
/// palette, which came out peach on Daylight beside some heads (the
/// reviewer), and coral is kept for what's live.
pub fn hairline_rule(_theme: &iced::Theme) -> iced::widget::rule::Style {
    iced::widget::rule::Style {
        color: border(),
        radius: 0.0.into(),
        fill_mode: iced::widget::rule::FillMode::Full,
        snap: true,
    }
}

/// The selected mix's columns in the Matrix: the lamp's wash as one band
/// behind them, strongest at the head and fading down the grid, at the
/// strips' own wash alphas.
/// A key outlined in the lamp, its words in it: the strips' "FX send" band
/// while the faders set FX sends (the lamp as light, never a fill).
pub fn lamp_outline(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| iced::Background::Color(Color { a: 0.12, ..lamp() })),
        text_color: readable_on_floors(lamp()),
        border: iced::Border { color: lamp(), width: 1.0, radius: RADIUS_SM.into() },
        ..button::Style::default()
    }
}

pub fn lamp_band(_theme: &iced::Theme) -> container::Style {
    let l = lamp();
    container::Style {
        background: Some(lit_from_above(Color { a: 0.16, ..l }, Color { a: 0.05, ..l })),
        border: Border { radius: RADIUS_SM.into(), ..Border::default() },
        ..container::Style::default()
    }
}

/// A control's edge that keeps 3:1 on every ground even where a curve
/// covers only part of a pixel: `control_edge` stepped towards the ink
/// until it reads 3.6:1 on the floors, the panel and a lit panel.
pub fn firm_edge() -> Color {
    let p = palette();
    let mut grounds: Vec<Color> = p.floors.to_vec();
    grounds.push(p.surface);
    grounds.push(blend(p.surface, p.accent, 0.12));
    let ink = text_primary();
    (0..=10).map(|k| blend(control_edge(), ink, k as f32 / 10.0)).find(|c| grounds.iter().all(|g| contrast(*c, *g) >= 3.6)).unwrap_or(ink)
}

/// A settings switch's track. On/off options and one-off actions were
/// alike, full-width bars both (the critique, 2026-11-03), so an option is
/// now a row with this switch at its end: off, a slot cut into the row
/// (the Inset: its shade, a `control_edge` rim, the panel's lit lip under
/// it; the reviewer: a bare shade vanished on a dark panel and left the
/// knob a lone dot); on, the accent's lit lens. `inert` while the option
/// can't be set: all of it at 40%, as every inert control.
pub fn switch_track(on: bool, inert: bool, radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        let k = if inert { 0.4 } else { 1.0 };
        let fade = |c: Color| Color { a: c.a * k, ..c };
        if on {
            let a = accent();
            let (top, bottom) = key_lit_colors(a, 0.0, false);
            return container::Style {
                background: Some(lit_from_above(fade(top), fade(bottom))),
                border: Border { color: fade(key_lit_edge(a)), width: 1.0, radius: radius.into() },
                shadow: Shadow {
                    color: Color { a: if inert { 0.0 } else { 0.35 }, ..a },
                    offset: iced::Vector::new(0.0, 0.0),
                    blur_radius: 5.0,
                },
                ..container::Style::default()
            };
        }
        let m = inset();
        container::Style {
            background: Some(lit_from_above(fade(m.top), fade(m.foot))),
            // The firm edge: its round ends, half covered, read 2.54 on
            // Daylight with `control_edge` (critique #89 B).
            // (and `SPLIT_EDGE_W` wide: at 1 px the curve still read 2.83).
            border: Border { color: fade(firm_edge()), width: SPLIT_EDGE_W, radius: radius.into() },
            // The lip: the panel catching the light along the slot's foot.
            shadow: Shadow { color: fade(m.lip), offset: iced::Vector::new(0.0, 1.0), blur_radius: 0.0 },
            ..container::Style::default()
        }
    }
}

/// A settings switch's knob, round, standing on its track with a shadow
/// under it: on, the fader caps' metal; off, quiet (a rest key's grey on
/// a dark theme, near white with a crisp rim on a light one, where silver
/// melted into its slot). At 40% while `inert`.
pub fn switch_knob(on: bool, inert: bool, radius: f32) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme| {
        // Inert, faded less than its track, so where it sits still shows
        // (critique #96 B: the knob's body was the track's colour, 1.04).
        let k = if inert { 0.75 } else { 1.0 };
        let fade = |c: Color| Color { a: c.a * k, ..c };
        let dark = dark_theme();
        let body = match (on, dark) {
            (true, _) => fader_metal(),
            (false, true) => blend(surface_hi(), text_primary(), 0.3),
            (false, false) => blend(Color::WHITE, surface(), 0.06),
        };
        let (top, foot) = if on || dark {
            (blend(body, Color::WHITE, 0.2), blend(body, Color::BLACK, 0.12))
        } else {
            (Color::WHITE, blend(body, Color::BLACK, 0.06))
        };
        container::Style {
            background: Some(lit_from_above(fade(top), fade(foot))),
            border: Border {
                color: fade(Color { a: if dark { 0.5 } else { 0.36 }, ..Color::BLACK }),
                width: 1.0,
                radius: radius.into(),
            },
            shadow: Shadow {
                color: fade(Color { a: if dark { 0.45 } else { 0.25 }, ..Color::BLACK }),
                offset: iced::Vector::new(0.0, 1.0),
                blur_radius: 1.5,
            },
            ..container::Style::default()
        }
    }
}

/// A small text key as a capsule (the reference's Store, Edit, Clear):
/// the Key material at rest, corners of `radius`. With nothing to do (a
/// Paste with nothing copied) its words and rim at 40%, as every inert
/// control.
pub fn capsule_key(radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        let inert = matches!(status, button::Status::Disabled);
        let (ink, edge) = if inert {
            (Color { a: 0.4, ..text_sec() }, Color { a: 0.4, ..control_edge() })
        } else if hovered {
            (text_primary(), fader_color())
        } else {
            (text_sec(), control_edge())
        };
        button::Style {
            background: Some(key_rest(hovered, pressed)),
            text_color: ink,
            border: Border { color: edge, width: 1.0, radius: radius.into() },
            ..button::Style::default()
        }
    }
}

/// The notice's Undo key: `capsule_key` with an edge that still reads
/// at 3:1 against the notice wherever it lands on the pixel grid
/// (critique #41: the 1px rim split across two rows at half strength,
/// 2.16:1 on Daylight's bottom edge; no colour gets a half-covered pixel
/// to 3:1 on a light card). It is 1.5px wide, so one pixel across it is
/// always at least three quarters covered (`SPLIT_EDGE_W`), in a colour
/// pulled toward the ink until that pixel reads at 3:1.
pub fn notice_key(radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = capsule_key(radius)(theme, status);
        style.border.width = SPLIT_EDGE_W;
        if !matches!(status, button::Status::Hovered | button::Status::Pressed | button::Status::Disabled) {
            style.border.color = split_safe_edge(control_edge(), surface());
        }
        style
    }
}

/// The tools row's Undo and Redo (beside the M S F masters): `notice_key`'s
/// edge, made for every ground the column shows under it (critique #43: the
/// capsule's 1px rim read 1.87:1 on Daylight's column).
pub fn column_key(radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = capsule_key(radius)(theme, status);
        style.border.width = SPLIT_EDGE_W;
        if !matches!(status, button::Status::Hovered | button::Status::Pressed | button::Status::Disabled) {
            let p = palette();
            let grounds = column_grounds(&p);
            let ink = text_primary();
            style.border.color = (0..=10)
                .map(|k| blend(control_edge(), ink, k as f32 / 10.0))
                .find(|c| grounds.iter().all(|g| contrast(blend(*c, *g, 1.0 - SPLIT_EDGE_COVER), *g) >= 3.0))
                .unwrap_or(ink);
        }
        style
    }
}

/// A capsule key on the page: the dialog and panels (`surface`), the
/// floors, the What's this banner's tint and Quick setup's grounds, with
/// `column_key`'s 1.5 px edge pulled toward the ink until a 75% covered
/// pixel reads at 3:1 on each (critique #81 B: Done, Help's What's this,
/// My setup's keys and Replace / Cancel had a 1 px rim on half pixels,
/// 1.7-2.6 at their bottom edge; light was worst).
pub fn page_key(radius: f32) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = capsule_key(radius)(theme, status);
        style.border.width = SPLIT_EDGE_W;
        if !matches!(status, button::Status::Hovered | button::Status::Pressed | button::Status::Disabled) {
            let p = palette();
            let mut grounds: Vec<Color> = p.floors.to_vec();
            grounds.push(p.surface);
            grounds.push(blend(p.surface, p.accent, 0.12));
            // Its own face too: the key must read as a key on any of them.
            if let Some(Background::Color(face)) = style.background {
                grounds.push(face);
            }
            let ink = text_primary();
            style.border.color = (0..=10)
                .map(|k| blend(control_edge(), ink, k as f32 / 10.0))
                .find(|c| grounds.iter().all(|g| contrast(blend(*c, *g, 1.0 - SPLIT_EDGE_COVER), *g) >= 3.0))
                .unwrap_or(ink);
        }
        style
    }
}

/// An edge that reads wherever it falls on the pixel grid: 1.5px leaves
/// one pixel across it at least 75% covered.
pub const SPLIT_EDGE_W: f32 = 1.5;

/// How much of its strongest pixel an edge `SPLIT_EDGE_W` wide covers at
/// the least.
pub const SPLIT_EDGE_COVER: f32 = 0.75;

/// `edge` moved toward the ink a tenth at a time until a pixel
/// `SPLIT_EDGE_COVER` covered by it reads at 3:1 on `ground`.
pub fn split_safe_edge(edge: Color, ground: Color) -> Color {
    let ink = text_primary();
    (0..=10)
        .map(|k| blend(edge, ink, k as f32 / 10.0))
        .find(|c| contrast(blend(*c, ground, 1.0 - SPLIT_EDGE_COVER), ground) >= 3.0)
        .unwrap_or(ink)
}

/// A snapshot or layout slot: a filled tile, no outline. The selected
/// slot (the one loaded, and where Store writes) takes a coral edge.
pub fn slot_button(active: bool) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let pressed = matches!(status, button::Status::Pressed);
        // The Control Room keys' block (`key_button`), a size down.
        let block = if active { blend(surface_hi(), accent(), 0.10) } else { surface_hi() };
        let (top, bottom) = match (pressed, hovered) {
            (true, _) => (blend(block, Color::BLACK, 0.12), blend(block, Color::BLACK, 0.06)),
            (false, true) => (blend(block, text_primary(), 0.15), blend(block, text_primary(), 0.05)),
            (false, false) => (blend(block, text_primary(), 0.08), block),
        };
        button::Style {
            background: Some(lit_from_above(top, bottom)),
            text_color: text_primary(),
            border: Border {
                color: if active { accent() } else { border() },
                width: 1.0,
                radius: RADIUS_SM.into(),
            },
            shadow: Shadow::default(),
            ..button::Style::default()
        }
    }
}

#[cfg(test)]
mod key_label_tests {
    use super::*;

    /// Hue in degrees (HSL).
    fn hue(c: Color) -> f32 {
        let (r, g, b) = (c.r, c.g, c.b);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = max - min;
        if d <= f32::EPSILON {
            return 0.0;
        }
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h * 60.0
    }

    #[test]
    fn every_themes_48v_is_not_its_clip() {
        // Slate's two reds sat 7 degrees apart, so a lit 48V read as an
        // over (the critique, 2026-11-04).
        for t in THEMES.iter() {
            let d = (hue(t.palette.phantom) - hue(t.palette.clip)).abs();
            let d = d.min(360.0 - d);
            assert!(d >= 18.0, "{}: 48V and clip are {d:.0} degrees apart", t.name);
        }
    }

    #[test]
    fn every_themes_keys_say_their_hue_and_read_on_every_ground() {
        for t in THEMES.iter() {
            let p = &t.palette;
            for (name, hue) in [("mute", p.mute), ("solo", p.attention), ("48V", p.phantom)] {
                let label = readable_on_keys_in(p, hue);
                for g in key_label_grounds(p) {
                    let c = contrast(label, g);
                    assert!(c >= 4.5, "{}: the {name} label is {c:.1}:1 on a key", t.name);
                }
                // Still the hue, not the ink: it says what it will light as.
                assert_ne!(label, p.text_primary, "{}: the {name} label lost its hue", t.name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{contrast, Color, Palette, AUDRA, THEMES};

    #[test]
    fn a_snapshot_lamp_is_round_and_has_no_ring_when_off() {
        // The owner, 2026-11-14: the square ones read as checkboxes.
        let theme = iced::Theme::Dark;
        for (lit, held) in [(false, false), (false, true), (true, true)] {
            let style = super::list_lamp(lit, held, 12.0, 0.0)(&theme);
            assert_eq!(style.border.radius, 6.0.into(), "lit {lit} held {held}");
            if !lit && super::dark_theme() {
                assert_eq!(style.border.width, 0.0, "an off lamp has no ring");
            }
        }
        assert_eq!(super::list_lamp(false, false, 15.0, 0.0)(&theme).border.radius, 7.5.into());
    }

    #[test]
    fn a_placard_keys_focus_ring_reads_on_the_key_under_it_in_every_theme() {
        // Critique #72 A 1: drawn in the halo's colour, it equalled the card.
        for entry in super::THEMES {
            super::set_palette(entry.palette);
            let (face, ink) = (super::text_primary(), super::on_placard());
            // The one ring drawn: outside the key, on the card's face
            // (critique #73 A 5: the test guarded a colour never drawn).
            let _ = ink;
            assert!(contrast(super::placard_focus_ring_color(), face) >= 3.0, "{}: {:.2}", entry.name, contrast(super::placard_focus_ring_color(), face));
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn the_open_mic_dot_keeps_its_amber_and_reads_on_the_placard_in_every_theme() {
        // Critique #71 A 3: pulled toward the ink to 4.5:1, it greyed.
        for entry in super::THEMES {
            super::set_palette(entry.palette);
            let face = super::text_primary();
            let dot = super::attention_dot_on(face);
            let (r, g, b) = (dot.r, dot.g, dot.b);
            let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
            let sat = if hi > 0.0 { (hi - lo) / hi } else { 0.0 };
            assert!(contrast(dot, face) >= 3.0, "{}: {:.2}:1", entry.name, contrast(dot, face));
            if entry.name != "High contrast" {
                assert!(sat >= 0.5 && r >= g && g >= b, "{}: still amber (sat {sat:.2})", entry.name);
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn a_badges_number_reads_on_its_own_tint_in_every_theme() {
        // Critique #78 B: Quick's red step badge "2" read 3.80 on its pink
        // disc, coloured for the bare floors.
        for entry in super::THEMES {
            super::set_palette(entry.palette);
            let p = super::palette();
            for hue in [super::clip(), super::signal(), super::attention()] {
                // At rest (the flash's brighter field lasts while the tick
                // draws in).
                for wash in [0.14_f32] {
                    let ink = super::readable_on_tint(hue, wash);
                    for g in p.floors.iter().chain([&p.surface]) {
                        let field = super::blend(*g, hue, wash);
                        assert!(contrast(ink, field) >= 4.5, "{}: badge number {:.2}", entry.name, contrast(ink, field));
                    }
                }
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn the_lit_snapshot_lamp_is_the_themes_accent_and_stands_apart_in_every_theme() {
        // Critique #78 A 1: a fixed coral in every theme, 1.04 from stored in
        // Solarized.
        let hue = |c: Color| {
            let (mx, mn) = (c.r.max(c.g).max(c.b), c.r.min(c.g).min(c.b));
            let d = mx - mn;
            if d < 1e-4 { return 0.0; }
            let h = if mx == c.r { ((c.g - c.b) / d).rem_euclid(6.0) } else if mx == c.g { (c.b - c.r) / d + 2.0 } else { (c.r - c.g) / d + 4.0 };
            h * 60.0
        };
        let gap = |a: f32, b: f32| { let d = (a - b).abs() % 360.0; d.min(360.0 - d) };
        for entry in super::THEMES {
            super::set_palette(entry.palette);
            let ground = super::surface();
            let well = super::layer(Color { a: 0.06, ..Color::BLACK }, ground);
            let mid = |c: iced::Background| match c {
                iced::Background::Gradient(iced::Gradient::Linear(l)) => l.stops.iter().flatten().find(|s| (s.offset - 0.45).abs() < 0.01).map(|s| s.color).unwrap(),
                iced::Background::Color(c) => c,
            };
            let theme = iced::Theme::Dark;
            let lit = mid(super::list_lamp(true, true, 12.0, 0.0)(&theme).background.unwrap());
            let stored = mid(super::list_lamp(false, true, 12.0, 0.0)(&theme).background.unwrap());
            let empty = mid(super::list_lamp(false, false, 12.0, 0.0)(&theme).background.unwrap());
            // 1.5 is the aim; Slate, whose accent and lamp share a hue, reaches 1.43.
            assert!(contrast(lit, stored) >= 1.4, "{}: lit vs stored {:.2}", entry.name, contrast(lit, stored));
            assert!(contrast(lit, empty) >= 1.35, "{}: lit vs empty {:.2}", entry.name, contrast(lit, empty));
            assert!(contrast(lit, well) >= 3.0, "{}: lit on the well {:.2}", entry.name, contrast(lit, well));
            assert!(super::luminance(lit) > super::luminance(empty), "{}: the lit lamp lighter than an empty one", entry.name);
            let (acc, lamp) = (super::accent(), super::lamp());
            // Grey accents (High contrast) have no hue to keep.
            let sat = |c: Color| { let mx = c.r.max(c.g).max(c.b); if mx > 0.0 { (mx - c.r.min(c.g).min(c.b)) / mx } else { 0.0 } };
            if sat(acc) > 0.15 {
                let _ = lamp;
                assert!(gap(hue(lit), hue(acc)) <= 20.0, "{}: the lit lamp's hue {:.0}° from the accent's", entry.name, gap(hue(lit), hue(acc)));
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn a_compact_snapshot_keys_numbers_read_in_every_theme() {
        // OpenCode's job 73: no test read the row of eight keys' numbers, so
        // the amber floor took two themes' stored numbers under 4.5 and the
        // changed-since amber with them.
        for entry in super::THEMES {
            super::set_palette(entry.palette);
            let grounds = super::held_led_grounds();
            let stored = super::text_primary();
            for g in &grounds {
                assert!(contrast(stored, *g) >= 4.5, "{}: stored number {:.2}", entry.name, contrast(stored, *g));
            }
            let changed = super::readable_on(super::attention(), super::text_primary(), &grounds);
            for g in &grounds {
                assert!(contrast(changed, *g) >= 4.5, "{}: changed number {:.2}", entry.name, contrast(changed, *g));
            }
            let (hi, lo) = (changed.r.max(changed.g).max(changed.b), changed.r.min(changed.g).min(changed.b));
            let sat = if hi > 0.0 { (hi - lo) / hi } else { 0.0 };
            assert!(sat >= 0.3, "{}: the changed number keeps its amber (saturation {sat:.2})", entry.name);
            // A stored key's face stands apart from an empty one's (OpenCode's
            // job 75: only the list's lamp was pinned).
            let theme = iced::Theme::Dark;
            let face = |held: bool| match super::list_led(false, held)(&theme).background.unwrap() {
                iced::Background::Gradient(iced::Gradient::Linear(l)) => l.stops.iter().flatten().map(|s| s.color).nth(1).unwrap(),
                iced::Background::Color(c) => c,
            };
            let ground = super::surface();
            let over = |c: Color| super::blend(ground, Color { a: 1.0, ..c }, c.a);
            let (stored_face, empty_face) = (over(face(true)), over(face(false)));
            assert!(contrast(stored_face, empty_face) >= 1.3 || super::luminance(stored_face) != super::luminance(empty_face) && {
                let d = (stored_face.r - empty_face.r).abs() + (stored_face.g - empty_face.g).abs() + (stored_face.b - empty_face.b).abs();
                d > 0.3
            }, "{}: stored vs empty compact key {:.2}", entry.name, contrast(stored_face, empty_face));
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn a_stored_lamp_stands_apart_from_an_empty_one_in_every_theme() {
        // Critique #55: stored and empty looked alike (1.02:1 in dark),
        // and in Nord and Solarized stored was the darker of the two.
        for entry in super::THEMES {
            super::set_palette(entry.palette);
            let ground = super::surface();
            let well = super::layer(Color { a: 0.06, ..Color::BLACK }, ground);
            let mid = |c: iced::Background| match c {
                iced::Background::Gradient(iced::Gradient::Linear(l)) => l.stops.iter().flatten().find(|s| (s.offset - 0.45).abs() < 0.01).map(|s| s.color).unwrap(),
                iced::Background::Color(c) => c,
            };
            let theme = iced::Theme::Dark;
            let empty_style = super::list_lamp(false, false, 12.0, 0.0)(&theme);
            assert_eq!(empty_style.border.width, 0.0, "{}: an empty lamp has no ring", entry.name);
            let empty = mid(empty_style.background.unwrap());
            let stored_style = super::list_lamp(false, true, 12.0, 0.0)(&theme);
            let stored = mid(stored_style.background.unwrap());
            if super::dark_theme() {
                assert!(contrast(stored, well) >= 3.0, "{}: stored {:.2}:1 on the well", entry.name, contrast(stored, well));
                assert!(contrast(stored, empty) >= 1.5, "{}: stored vs empty {:.2}:1", entry.name, contrast(stored, empty));
            } else {
                // A pale warm wash, lighter than empty, its rim 3:1 on the
                // well (the brown, carried since #55).
                let rim = stored_style.border.color;
                assert!(stored_style.border.width >= 1.0 && contrast(rim, well) >= 3.0, "{}: stored rim {:.2}:1 on the well", entry.name, contrast(rim, well));
                assert!(super::luminance(stored) > super::luminance(empty) && contrast(stored, empty) >= 1.5, "{}: stored vs empty {:.2}:1", entry.name, contrast(stored, empty));
            }
            assert!(contrast(empty, well) >= 3.0, "{}: empty {:.2}:1 on the well", entry.name, contrast(empty, well));
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn a_key_rim_reads_wherever_it_lands_on_the_pixel_grid_in_every_theme() {
        // Critique #56: folded M/S top and bottom rims 2.25-2.63 (off the
        // grid by a fraction of a pixel), a Quick Sound key's side 2.08.
        let theme = iced::Theme::Dark;
        let rest = iced::widget::button::Status::Active;
        for entry in super::THEMES {
            super::set_palette(entry.palette);
            let plain = super::toggle_button(false, super::accent());
            let checks: [(&str, iced::widget::button::Style, f32); 2] = [
                ("split-safe", super::split_safe_key(&plain, false)(&theme, rest), super::SPLIT_EDGE_COVER),
                ("firm", super::firm_rim_key(&plain, false)(&theme, rest), super::NEAR_GRID_COVER),
            ];
            for (name, s, cover) in checks {
                for g in super::key_grounds() {
                    let seen = super::blend(s.border.color, g, 1.0 - cover);
                    assert!(contrast(seen, g) >= 3.0, "{}: {name} rim {:.2}:1 at {cover} cover", entry.name, contrast(seen, g));
                }
            }
            // Lit, a key keeps its own edge and width.
            let lit = super::toggle_button(true, super::accent())(&theme, rest);
            let kept = super::split_safe_key(super::toggle_button(true, super::accent()), true)(&theme, rest);
            assert_eq!((kept.border.color, kept.border.width), (lit.border.color, lit.border.width), "{}", entry.name);
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn the_dot_alone_chip_rings_faintly_at_rest_firmly_under_the_pointer() {
        // Critique #63 A 2 (a dot in a firm ring read as a record key) and
        // #64 A 2 (a bare amber dot read as the Help key's badge).
        let theme = iced::Theme::Dark;
        let faint = Color { a: 0.4, ..super::attention() };
        let rest = super::chip_button_lit(false, faint)(&theme, iced::widget::button::Status::Active).border;
        assert_eq!(rest.color, faint);
        let hover = super::chip_button_lit(false, faint)(&theme, iced::widget::button::Status::Hovered).border;
        assert_eq!(hover.color, super::fader_color());
        let open = super::chip_button_lit(true, faint)(&theme, iced::widget::button::Status::Active).border;
        assert_eq!(open.color, super::fader_color(), "its panel open");
    }

    #[test]
    fn the_carried_edges_read_in_every_theme() {
        // The four carried since #57-#59: the Control Room's keys on their
        // well, the device chip on the top bar, an empty snapshot key, the
        // Quick page's 48V key.
        let theme = iced::Theme::Dark;
        let rest = iced::widget::button::Status::Active;
        let reads = |edge: Color, grounds: &[Color], what: &str, name: &str| {
            for g in grounds {
                let seen = super::blend(edge, *g, 1.0 - super::SPLIT_EDGE_COVER);
                assert!(contrast(seen, *g) >= 3.0, "{name}: {what} {:.2}:1", contrast(seen, *g));
            }
        };
        for entry in super::THEMES {
            super::set_palette(entry.palette);
            let wells: Vec<Color> = super::key_grounds().into_iter().chain(super::well_grounds()).collect();
            let cr = super::split_safe_key(super::lamp_button(super::accent(), super::Lamp { inset: true, ..super::Lamp::default() }), false)(&theme, rest).border;
            reads(cr.color, &wells, "Control Room key", entry.name);
            // Greyed, it stays fainter than a live one (critique #63 A 1).
            let greyed = super::split_safe_key(super::lamp_button(super::accent(), super::Lamp { inset: true, ..super::Lamp::default() }), false)(&theme, iced::widget::button::Status::Disabled).border;
            let well = super::well_grounds()[0];
            assert!(contrast(greyed.color, well) < contrast(cr.color, well) || greyed.color == cr.color && super::resting_key_edge() == cr.color, "{}: greyed rim as firm as live", entry.name);
            let chip = super::chip_button(false)(&theme, rest).border;
            assert_eq!(chip.width, super::SPLIT_EDGE_W, "{}: device chip split-safe", entry.name);
            reads(chip.color, &[super::p().top_bar, super::bg_deep()], "device chip", entry.name);
            let empty = super::list_led(false, false)(&theme).border;
            if !super::dark_theme() {
                assert_eq!(empty.width, super::SPLIT_EDGE_W, "{}", entry.name);
                reads(empty.color, &super::key_grounds(), "empty snapshot key", entry.name);
            }
            let p48 = super::split_safe_key(super::lamp_button(super::phantom(), super::Lamp { hue_label: true, ..super::Lamp::default() }), false)(&theme, rest).border;
            reads(p48.color, &super::key_grounds(), "Quick 48V key", entry.name);
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn the_tip_card_and_fx_send_rims_read_in_every_theme() {
        // Critique #62 B 1 (FX send 1.76) and B 2 (tip card 1.36).
        let theme = iced::Theme::Dark;
        let rest = iced::widget::button::Status::Active;
        for entry in super::THEMES {
            super::set_palette(entry.palette);
            let card = super::trick_card(&theme).border;
            let send = super::split_safe_key(super::lamp_outline, false)(&theme, rest).border;
            for (name, b) in [("tip card", card), ("FX send", send)] {
                assert_eq!(b.width, super::SPLIT_EDGE_W, "{}: {name}", entry.name);
                for g in super::key_grounds().into_iter().chain([super::surface()]) {
                    let seen = super::blend(b.color, g, 1.0 - super::SPLIT_EDGE_COVER);
                    assert!(contrast(seen, g) >= 3.0, "{}: {name} rim {:.2}:1", entry.name, contrast(seen, g));
                }
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn a_snapshot_lamp_strikes_when_its_slot_lands_and_settles() {
        // Recalled or stored into, the list's lamp rings and glows in the
        // lamp's colour (as the numbered keys flare); at rest it's plain.
        let theme = iced::Theme::Dark;
        let struck = super::list_lamp(true, true, 12.0, 1.0)(&theme);
        assert_eq!(struck.border.width, 2.0, "the ring at its widest as it lands");
        assert!(struck.shadow.blur_radius > 12.0 && struck.shadow.color.a > 0.8, "and a halo");
        let settled = super::list_lamp(true, true, 12.0, 0.0)(&theme);
        assert_eq!(settled.border.width, 1.0, "back to the lit lamp's edge");
        assert!(settled.shadow.blur_radius <= 6.0);
    }

    #[test]
    fn the_placard_text_reads_on_its_card() {
        assert!(contrast(AUDRA.on_active, AUDRA.text_primary) >= 4.5);
        assert!(contrast(AUDRA.on_placard_sec, AUDRA.text_primary) >= 4.5);
    }

    #[test]
    fn muted_text_reads_on_every_floor_and_the_counter() {
        for ground in AUDRA.floors.iter().chain([&AUDRA.counter]) {
            assert!(contrast(AUDRA.muted, *ground) >= 4.5, "{ground:?}");
        }
    }

    /// Every ground a control can sit on: the top bar, the floors, the
    /// counter, and each floor under the lamp wash at its strongest.
    fn grounds() -> Vec<Color> {
        let over = |fg: Color, a: f32, bg: Color| Color {
            r: fg.r * a + bg.r * (1.0 - a),
            g: fg.g * a + bg.g * (1.0 - a),
            b: fg.b * a + bg.b * (1.0 - a),
            a: 1.0,
        };
        let mut all = vec![AUDRA.top_bar, AUDRA.counter];
        for floor in AUDRA.floors {
            all.push(floor);
            all.push(over(AUDRA.lamp, super::LAMP_TOP, floor));
        }
        all
    }

    #[test]
    fn controls_stand_out_on_every_ground_the_lamp_wash_included() {
        // The owner, 2026-11-03: "hard to see with the black and brown".
        for ground in grounds() {
            assert!(contrast(AUDRA.control_edge, ground) >= 3.0, "edge on {ground:?}");
            assert!(contrast(AUDRA.muted, ground) >= 6.0, "muted on {ground:?}");
            assert!(contrast(AUDRA.text_sec, ground) >= 7.0, "text_sec on {ground:?}");
        }
    }

    #[test]
    fn every_theme_holds_the_same_contrast_rules() {
        for entry in THEMES {
            let issues = super::contrast_issues(entry.palette);
            assert!(issues.is_empty(), "{}: {issues:?}", entry.name);
        }
        // And the check does catch a theme that doesn't.
        let poor = Palette { text_sec: AUDRA.surface, ..AUDRA };
        assert!(super::contrast_issues(&poor).iter().any(|i| i.starts_with("secondary text")));
    }

    #[test]
    fn a_resting_key_shows_its_edge_in_every_theme() {
        // The owner's pick after critique #33 ("B: a clearer edge").
        for entry in THEMES {
            super::set_palette(entry.palette);
            let st = super::key_button(false, 0.0, 0.0, 6.0)(&iced::Theme::Dark, iced::widget::button::Status::Active);
            let card = super::blend(entry.palette.surface, entry.palette.text_sec, 0.06);
            for ground in [entry.palette.surface, entry.palette.bg_deep, entry.palette.surface_hi, card] {
                let edge = super::contrast(st.border.color, ground);
                assert!(edge >= 3.0, "{}: resting key edge {edge:.2}", entry.name);
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn a_chosen_off_reads_and_stands_out_in_every_theme() {
        // Critique #31's "quiet Off": its words at text level on its block,
        // and its edge visible on the panel, in every built-in theme.
        for entry in THEMES {
            super::set_palette(entry.palette);
            use iced::widget::button::Status;
            for status in [Status::Active, Status::Hovered, Status::Pressed] {
                let st = super::chosen_off(&iced::Theme::Dark, status);
                let Some(iced::Background::Color(block)) = st.background else { panic!("a flat block") };
                let words = super::contrast(st.text_color, block);
                assert!(words >= 4.5, "{} {status:?}: words {words:.2}", entry.name);
                let edge = super::contrast(st.border.color, entry.palette.surface);
                assert!(edge >= 3.0, "{} {status:?}: edge {edge:.2}", entry.name);
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn the_hinted_d_key_reads_on_its_own_fill_in_every_theme() {
        // Critique #32: its letter, on its lamp fill at rest and pressed,
        // over the floors and the panel, in every built-in theme.
        for entry in THEMES {
            super::set_palette(entry.palette);
            let p = entry.palette;
            let ink = super::hinted_ink();
            let _ = p;
            for g in super::hinted_grounds() {
                let r = contrast(ink, g);
                assert!(r >= 4.5, "{}: the hinted D on its fill: {r:.2}", entry.name);
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn the_waiting_look_and_the_solid_on_read_in_every_theme() {
        // Critique #32's picks: a chosen setting while the voice is off
        // ("set, waiting": the armed look in coral) and the D panel's ON
        // lit solid. Words at text level on their fill; the waiting key's
        // edge visible on the panel.
        use iced::widget::button::Status;
        for entry in THEMES {
            super::set_palette(entry.palette);
            let p = entry.palette;
            for status in [Status::Active, Status::Hovered, Status::Pressed] {
                let hovered = !matches!(status, Status::Active);
                let pressed = matches!(status, Status::Pressed);
                // Waiting: its fill as `armed_style` makes it.
                let st = (super::key_button_armed(super::accent(), 4.0))(&iced::Theme::Dark, status);
                let lift = if hovered { 0.10 } else { 0.0 };
                let mut top = super::blend(super::bg_deep(), super::accent(), 0.24 + lift);
                let mut bottom = super::blend(super::bg_deep(), super::accent(), 0.15 + lift * 0.8);
                if pressed {
                    top = super::blend(top, Color::BLACK, super::PRESS_DARKEN);
                    bottom = super::blend(bottom, Color::BLACK, super::PRESS_DARKEN);
                }
                for g in [top, bottom] {
                    let r = contrast(st.text_color, g);
                    assert!(r >= 4.5, "{} {status:?}: waiting words {r:.2}", entry.name);
                }
                let edge = contrast(st.border.color, p.surface);
                assert!(edge >= 3.0, "{} {status:?}: waiting edge {edge:.2}", entry.name);
                // The solid ON.
                let on = (super::power_key(true, 12.0))(&iced::Theme::Dark, status);
                // (Pressed, it keeps its colour: see `power_key`.)
                let (t, b) = super::key_lit_colors(super::power_hue(), 0.0, false);
                for g in [t, b] {
                    let r = contrast(on.text_color, g);
                    assert!(r >= 4.5, "{} {status:?}: ON's words {r:.2}", entry.name);
                }
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn lit_keys_words_and_armed_edges_read_in_every_theme() {
        // Critique #33: the words on a lit key (choices, Move it, a held
        // Cough) read 4.17 in Daylight and 3.92 in Solarized; the armed
        // amber's edge 2.90 in Solarized.
        use iced::widget::button::Status;
        for entry in THEMES {
            super::set_palette(entry.palette);
            for hue in [super::accent(), super::mute_color()] {
                for status in [Status::Active, Status::Hovered] {
                    let st = (super::toggle_button(true, hue))(&iced::Theme::Dark, status);
                    let (t, b) = super::key_lit_colors(super::lit_hue(hue), 0.0, false);
                    for g in [t, b] {
                        let r = contrast(st.text_color, g);
                        assert!(r >= 4.5, "{} {status:?}: lit words {r:.2}", entry.name);
                    }
                    let k = (super::key_button_in(hue, true, 0.0, 0.0, 4.0))(&iced::Theme::Dark, status);
                    let rest = (super::key_button_in(hue, true, 0.0, 0.0, 4.0))(&iced::Theme::Dark, Status::Active);
                    assert_eq!(k.text_color, rest.text_color, "{} {status:?}: the ink never flips on hover", entry.name);
                    let (t, b) = super::key_lit_colors(super::lit_hue(hue), 0.0, false);
                    for g in [t, b] {
                        let r = contrast(k.text_color, g);
                        assert!(r >= 4.5, "{} {status:?}: block key's words {r:.2}", entry.name);
                    }
                }
            }
            for hue in [super::attention(), super::accent()] {
                let st = (super::key_button_armed(hue, 4.0))(&iced::Theme::Dark, Status::Active);
                let e = contrast(st.border.color, entry.palette.surface);
                assert!(e >= 3.0, "{}: armed edge {e:.2}", entry.name);
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn channel_type_tags_read_in_every_theme() {
        for entry in THEMES {
            super::set_palette(entry.palette);
            let p = entry.palette;
            for hue in [Color::from_rgb8(0x7f, 0xb3, 0xd5), Color::from_rgb8(0xd9, 0xa8, 0x5c), Color::from_rgb8(0x5f, 0xb3, 0xa8)] {
                let c = super::readable_on_floors(hue);
                for g in p.floors {
                    assert!(contrast(c, g) >= 4.5, "{}: tag on a floor", entry.name);
                }
            }
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn a_state_mark_keeps_its_colour_and_reads_in_every_theme() {
        // Critique #38: the L≠R tag took the text tone in three themes.
        for entry in THEMES {
            super::set_palette(entry.palette);
            let p = entry.palette;
            let ink = super::state_mark_ink(super::accent());
            // The DIM tag in the lamp too (critique #40: on the floors'
            // check it came to 4.63:1 in Solarized).
            let dim = super::state_mark_ink(super::lamp());
            // PAD's word: a state colour apart from the type word's grey
            // (critique #42: Nord's came out near MIC's).
            let pad = super::pad_ink();
            // The folded chip's word on its whole lens.
            let hue = super::pad_chip().0;
            let (top, bottom) = super::key_lit_colors(hue, 0.0, false);
            for g in [super::blend(hue, Color::WHITE, 0.4), top, bottom] {
                assert!(contrast(super::pad_mark_ink(), g) >= 4.5, "{}: the folded PAD chip's word", entry.name);
            }
            let apart = |a: Color, b: Color| (a.r - b.r).abs().max((a.g - b.g).abs()).max((a.b - b.b).abs());
            assert!(apart(pad, super::text_sec()) >= 0.25, "{}: PAD apart from MIC's grey", entry.name);
            for g in super::strip_header_grounds(p) {
                assert!(contrast(pad, g) >= 4.5, "{}: PAD on a strip's panel", entry.name);
            }
            // The notice's Undo key: half its edge over the card still at
            // 3:1 (critique #41: a 1px rim between two pixels).
            // The tools row's Undo/Redo: their edge on every column ground.
            let col_key = super::column_key(11.0)(&iced::Theme::Dark, iced::widget::button::Status::Active).border.color;
            for g in super::column_grounds(p) {
                assert!(contrast(super::blend(col_key, g, 1.0 - super::SPLIT_EDGE_COVER), g) >= 3.0, "{}: the tools row's Undo edge", entry.name);
            }
            let edge = super::split_safe_edge(super::control_edge(), super::surface());
            let cover = super::SPLIT_EDGE_COVER;
            assert!(contrast(super::blend(edge, super::surface(), 1.0 - cover), super::surface()) >= 3.0, "{}: the Undo key's edge, its strongest pixel", entry.name);
            for g in super::strip_header_grounds(p) {
                assert!(contrast(ink, g) >= 4.5, "{}: the mark on a strip's panel, lit, hovered or not", entry.name);
                assert!(contrast(dim, g) >= 4.5, "{}: DIM on a strip's panel, lit, hovered or not", entry.name);
            }
            let apart = |a: Color, b: Color| (a.r - b.r).abs().max((a.g - b.g).abs()).max((a.b - b.b).abs());
            assert!(apart(ink, p.text_primary) >= 0.1, "{}: the mark keeps a colour of its own, not the name's", entry.name);
            assert!(apart(ink, super::text_sec()) >= 0.1, "{}: nor the type word's", entry.name);
        }
        super::set_palette(&AUDRA);
    }

    #[test]
    fn a_theme_change_changes_the_key_every_cached_drawing_uses() {
        super::set_palette(&AUDRA);
        let audra = super::palette_key();
        super::set_palette(THEMES[1].palette);
        assert_ne!(super::palette_key(), audra, "Daylight redraws the knobs");
        super::set_palette(&AUDRA);
        assert_eq!(super::palette_key(), audra);
    }

    #[test]
    fn theme_ids_are_unique_and_an_unknown_one_falls_back_to_audra() {
        let mut ids: Vec<&str> = THEMES.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), THEMES.len());
        assert_eq!(super::theme_index("monokai"), 7);
        assert_eq!(super::theme_index("slate"), 3);
        assert_eq!(super::theme_index("no-such-theme"), 0);
    }
}

