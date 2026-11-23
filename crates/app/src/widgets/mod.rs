pub mod crosspoint;
pub mod dyn_plot;
pub mod eq_plot;
pub mod fader;
pub mod feel;
pub mod knob;
pub mod knob_paint;
pub mod level_fader;
pub mod mini_fader;
pub mod overhang;
pub mod panel_icon;
pub mod strip_icons;
pub mod splitter;
pub mod strip;
pub mod tuner;

thread_local! {
    /// "Disable mouse wheel operation" (manual 21.7): faders, knobs and
    /// the EQ's band handles ignore the wheel, so a scroll moves the page
    /// instead of a level. Set from the app state at the top of every
    /// `view` (`set_wheel_off`); a thread-local rather than a field on
    /// every widget's builder (thirteen of them), and per thread so tests
    /// running in parallel can't see each other's setting.
    static WHEEL_OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    /// How the meters show loudness (`Settings → Meters`): 0 peaks only,
    /// 1 an RMS bar with the peaks lighter above it, 2 the same with RMS
    /// shown 3 dB up (a sine then reads its peak). Set by `view`.
    static RMS_MODE: std::cell::Cell<u8> = const { std::cell::Cell::new(1) };
}

/// See `RMS_MODE`.
pub fn set_rms_mode(mode: u8) {
    RMS_MODE.with(|m| m.set(mode));
}

/// See `RMS_MODE`.
pub fn rms_mode() -> u8 {
    RMS_MODE.with(|m| m.get())
}

thread_local! {
    /// Audra settings' "Animations" switch, for the widgets' own motion
    /// (a fader's glide, a control's splash, the sweat). Set by `view`.
    static ANIMATIONS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    /// Lock User Interface is on: the controls show they won't move (the
    /// not-allowed cursor, no hover lift). Set by `view`.
    static LOCKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A screenshot of a clip's shockwave: how far it has run, held still
    /// (`--open clip-shock` only). Set by `view` from
    /// `TuxMix::capture_shock_t`.
    static FROZEN_SHOCK: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
    /// A hand holds a fader (pressed, not yet let go): no fader lands, so a
    /// group's followers move with the hand as it does, dead under it. Set
    /// by `view`.
    static HAND_ON_FADER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// See `HAND_ON_FADER`.
pub fn set_hand_on_fader(on: bool) {
    HAND_ON_FADER.with(|h| h.set(on));
}

/// See `HAND_ON_FADER`.
pub fn hand_on_fader() -> bool {
    HAND_ON_FADER.with(|h| h.get())
}

/// See `FROZEN_SHOCK`.
pub fn set_frozen_shock(t: Option<f32>) {
    FROZEN_SHOCK.with(|f| f.set(t));
}

/// See `FROZEN_SHOCK`.
pub fn frozen_shock() -> Option<f32> {
    FROZEN_SHOCK.with(|f| f.get())
}

/// See `ANIMATIONS`.
pub fn set_animations(on: bool) {
    ANIMATIONS.with(|a| a.set(on));
}

/// See `ANIMATIONS`.
pub fn animations() -> bool {
    ANIMATIONS.with(|a| a.get())
}

thread_local! {
    /// "What's this?" mode (`app::TuxMix::explain`): every hint shows at
    /// once, in a lit card.
    static EXPLAIN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// See `EXPLAIN`.
pub fn set_explain(on: bool) {
    EXPLAIN.with(|e| e.set(on));
}

/// See `EXPLAIN`.
pub fn explain() -> bool {
    EXPLAIN.with(|e| e.get())
}

/// How long a hint waits: at once in "What's this?" mode, else 400 ms.
pub fn hint_delay() -> std::time::Duration {
    std::time::Duration::from_millis(if explain() { 0 } else { 400 })
}

/// A hint's card: the panel, its rim lit in the accent in "What's this?"
/// mode so the answer reads as one.
pub fn hint_panel(t: &iced::Theme) -> iced::widget::container::Style {
    let base = crate::theme::panel(t);
    if explain() {
        iced::widget::container::Style {
            border: iced::Border { color: crate::theme::readable_on_floors(crate::theme::accent()), width: 2.0, ..base.border },
            ..base
        }
    } else {
        base
    }
}

/// See `LOCKED`.
pub fn set_locked(on: bool) {
    LOCKED.with(|l| l.set(on));
}

/// See `LOCKED`.
pub fn locked() -> bool {
    LOCKED.with(|l| l.get())
}

/// See `WHEEL_OFF`.
pub fn set_wheel_off(off: bool) {
    WHEEL_OFF.with(|w| w.set(off));
}

/// Whether the wheel is off for values (`WHEEL_OFF`).
pub fn wheel_off() -> bool {
    WHEEL_OFF.with(|w| w.get())
}

/// The status dot's diameter at scale 1.
const STATUS_DOT_D: f32 = 7.0;

/// A filled dot for a state at a glance: the device's connection, a
/// snapshot with unsaved changes, the banners. Drawn rather than set as a
/// "●" character, whose size and baseline depend on whichever font
/// supplies the glyph.
/// `status_dot` as a hollow ring: an information marker, kept apart from
/// a warning's filled dot (critique #70 A 3).
pub fn status_ring<'a, Message: 'a>(color: iced::Color, scale: f32) -> iced::Element<'a, Message> {
    let d = STATUS_DOT_D * scale;
    iced::widget::container(iced::widget::Space::new())
        .width(d)
        .height(d)
        .style(move |_theme: &iced::Theme| iced::widget::container::Style {
            border: iced::Border { color, width: 1.5 * scale, radius: (d / 2.0).into() },
            ..iced::widget::container::Style::default()
        })
        .into()
}

pub fn status_dot<'a, Message: 'a>(color: iced::Color, scale: f32) -> iced::Element<'a, Message> {
    let d = STATUS_DOT_D * scale;
    iced::widget::container(iced::widget::Space::new())
        .width(d)
        .height(d)
        .style(move |_theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(color)),
            border: iced::border::rounded(d / 2.0),
            ..iced::widget::container::Style::default()
        })
        .into()
}
