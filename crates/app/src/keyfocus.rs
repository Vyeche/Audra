//! The keyboard beyond the strips (the audit, 2026-11-05: "the keyboard
//! reaches mixer strips and nothing else"; the owner chose to fix it).
//!
//! iced 0.14 gives its buttons, pick lists and toggles no keyboard focus,
//! so Audra keeps its own: where the keyboard is (`TuxMix::key_focus`,
//! beside `focused_strip`), the items each region offers in the order they
//! stand on screen, and what a key does there. The views ask the same
//! lists whether an item has the focus (`ring`), so the keyboard never
//! reaches something the screen doesn't show.
//!
//! - **The Mixer:** Tab and Shift+Tab walk the strips and then the counter
//!   (the Main and Phones levels, the Control Room keys, the snapshot
//!   slots), round and round. With the strip's Settings panel open (Space),
//!   Tab goes into the panel's rows first. The arrows keep their meaning
//!   on a strip (left and right walk, up and down nudge its fader).
//! - **A strip's panel and the counter:** up and down move between items,
//!   left and right (or + and −) change a level, the gain or the preset
//!   to load, and Space or Enter presses, switches or loads.
//! - **The Matrix:** the arrows move a cell, Tab walks the cells in reading
//!   order, + and − change its gain, and Space switches it between 0 dB
//!   and off.
//! - Esc lets go, a step at a time; a click hands over to the mouse.
//!
//! **Surfaces with many controls** (Audra settings, the Quick setup, a
//! strip's EQ and Trim panels) don't keep a hand-made list: each control
//! registers itself as the view draws it (`reach` and its helpers), with
//! the messages its keys send, inside the region being drawn (`in_region`).
//! The keyboard then walks the controls of the last frame in the order
//! they were drawn, so it reaches exactly what the screen shows. Tab is
//! held inside Audra settings while it is open.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use iced::widget::{container, stack, Space};
use iced::{Element, Length};
use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix, View};
use crate::widgets::strip::FlyoutKind;

/// Where the keyboard is, when not on a strip of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyFocus {
    /// An item of the counter (`counter_items`).
    Counter(CounterItem),
    /// A row of the focused strip's open Settings panel (`panel_items`).
    Panel(PanelItem),
    /// A Matrix cell: its row (`None`: the Outputs row) and its output
    /// channel.
    Matrix(Option<ChannelId>, usize),
    /// A control that registered itself as it was drawn (`reach`).
    Item(u64),
}

/// A surface whose controls register themselves (`reach`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    Settings,
    /// About: its guides' Start keys and its links (critique #6: F1
    /// opened About, and nothing in it could be reached).
    About,
    Quick,
    Eq(ChannelId),
    /// A strip's D panel (issue #27).
    Dyn(ChannelId),
    /// The device card (clock, sample rate, pitch, the global flags):
    /// critique #20, F3 opened it and nothing in it could be reached.
    Device,
}

/// A control drawn in the last frame that the keyboard can reach, with what
/// its keys send: Space or Enter (`press`), and left/− and right/+.
#[derive(Clone, Debug)]
pub struct Reach {
    pub id: u64,
    pub region: Region,
    /// Its name as it registered, for `focus_words`.
    pub name: String,
    pub press: Option<Message>,
    pub less: Option<Message>,
    pub more: Option<Message>,
}

thread_local! {
    /// The controls of the frame drawn last, in the order drawn: what the
    /// keys act on (no key is handled while a frame is being drawn).
    static DRAWN: RefCell<Vec<Reach>> = const { RefCell::new(Vec::new()) };
    /// How often each name came up this frame, so twins get their own ids.
    static SEEN: RefCell<HashMap<u64, u32>> = RefCell::new(HashMap::new());
    /// The region being drawn, if any.
    static REGION: Cell<Option<Region>> = const { Cell::new(None) };
    /// The item the keyboard is on, as the view starts.
    static FOCUS: Cell<Option<u64>> = const { Cell::new(None) };
    /// The last frame's controls, in the order they were drawn: their
    /// names and keys for `focus_words` (the card naming them is built
    /// before some regions are drawn, so it reads the frame before), and
    /// where the keyboard's focus stood.
    static LAST: RefCell<Vec<Reach>> = const { RefCell::new(Vec::new()) };
    /// `LAST` is a frame kept for the repair (the focused control went),
    /// not the frame just before: the card names nothing from it.
    static KEPT: Cell<bool> = const { Cell::new(false) };
}

/// The view starts: a new frame's controls, and the item with the keyboard
/// noted for the rings.
pub fn begin_frame(state: &TuxMix) {
    let focused = match state.key_focus {
        Some(KeyFocus::Item(id)) => Some(id),
        _ => None,
    };
    DRAWN.with(|d| {
        let mut d = d.borrow_mut();
        if !d.is_empty() {
            LAST.with(|l| {
                let mut l = l.borrow_mut();
                // The keyboard's control gone from the frame just drawn (a
                // key that renamed itself: Turn on Voice became Turn off
                // Voice): the frame where it last stood is kept, so the
                // repair (`settle_focus`, on the tick) lands on the stop now
                // at its place. Overwritten, the next frame forgot it and the
                // focus let go (critique #37).
                let lost = focused.is_some_and(|id| !d.iter().any(|r| r.id == id) && l.iter().any(|r| r.id == id));
                if !lost {
                    *l = d.clone();
                }
                KEPT.with(|k| k.set(lost));
            });
        }
        d.clear();
    });
    SEEN.with(|s| s.borrow_mut().clear());
    FOCUS.with(|f| f.set(match state.key_focus {
        Some(KeyFocus::Item(id)) => Some(id),
        _ => None,
    }));
}

/// What the keyboard is on, said for someone who can't hover for its
/// tooltip (issue #21, the owner's yes): its name, whose it is, and its
/// keys, for the column's card slot (`app::keyboard_card`). `None` while
/// the keyboard is on nothing.
pub fn focus_words(state: &TuxMix) -> Option<(String, String, &'static str)> {
    let strip = |cid: ChannelId| crate::app::strip_display_name(state, cid);
    let focus = state.key_focus;
    match focus {
        Some(KeyFocus::Panel(item)) => {
            let (name, keys) = panel_item_words(item);
            // A mic's D row is Voice; an output's is Loudness, as its panel's
            // title (critique #108 A P3-27: "Dynamics" was engineer talk).
            let name = match (item, state.focused_strip) {
                (PanelItem::Dynamics, Some(ChannelId::Input(_))) => "Voice",
                (PanelItem::Dynamics, Some(ChannelId::Output(_))) => "Loudness",
                _ => name,
            };
            Some((name.to_string(), state.focused_strip.map(strip).unwrap_or_default(), keys))
        }
        Some(KeyFocus::Item(id)) => {
            // The control of this frame, so a control that is gone is not
            // named (job 14). The regions the card is drawn before (the
            // Help, Audra settings and the device card) are read from the
            // frame before: the card stands before they are drawn.
            let r = DRAWN
                .with(|d| d.borrow().iter().find(|r| r.id == id).cloned())
                .or_else(|| {
                    LAST.with(|l| {
                        l.borrow()
                            .iter()
                            // Not from a frame kept for the repair: that
                            // control is gone (OpenCode's job 34).
                            .find(|r| r.id == id && drawn_after_the_card(r.region) && !KEPT.with(Cell::get))
                            .cloned()
                    })
                })?;
            let keys = if r.less.is_some() || r.more.is_some() { "\u{2190} \u{2192} change it, Esc leaves" } else { "Space or Enter, Esc leaves" };
            let place = match r.region {
                Region::Settings => "Audra settings".to_string(),
                Region::About => "Help".to_string(),
                Region::Quick => "Quick setup".to_string(),
                Region::Eq(cid) => format!("{}'s EQ", strip(cid)),
                Region::Dyn(cid @ ChannelId::Output(_)) => format!("{}'s Loudness", strip(cid)),
                Region::Dyn(cid) => format!("{}'s Voice", strip(cid)),
                Region::Device => "the device card".to_string(),
            };
            Some((r.name.clone(), place, keys))
        }
        Some(KeyFocus::Counter(item)) => {
            let (name, keys): (String, &'static str) = match item {
                CounterItem::Level(pair) => (format!("{} level", crate::sidebar::mix_name(state, pair)), "\u{2190} \u{2192} a step"),
                // As the key is lettered ("Dim", not the spec's "DIM"; critique
                // #95 A P3-9).
                CounterItem::Key(label) => (crate::control_room::key_words(label), "Space or Enter"),
                CounterItem::Slot(n) => (
                    state.snapshot_names.get(n as usize - 1).cloned().flatten().unwrap_or_else(|| format!("Snapshot {n}")),
                    "Space recalls it",
                ),
                CounterItem::Store => ("Store".into(), "Space, then a snapshot"),
                CounterItem::Page(tab) => (format!("{} page", crate::sidebar::page_title(tab)), "Space opens it"),
                CounterItem::FxKnob(..) => ("Effect knob".into(), "\u{2190} \u{2192} change it"),
                CounterItem::LayoutSlot(n) => (format!("Layout {n}"), "Space loads it"),
                CounterItem::MixName => (format!("Mix: {}", crate::sidebar::mix_name(state, state.sel_out)), "\u{2190} \u{2192} another mix"),
                CounterItem::FedBy => ("The map of this mix".into(), "Space opens it"),
                // Named for its output (critique #90 A P3-8: "The output's
                // note" named the widget).
                CounterItem::OutputNote => (
                    crate::sidebar::output_note_of(state)
                        .map_or("The output's note".to_string(), |(pair, _)| format!("{}\u{2019}s note", crate::sidebar::mix_name(state, pair))),
                    "Space opens its Settings",
                ),
                _ => return None,
            };
            // A note named for its output says where it is already.
            // (No place for any of the column's items: it told the reader
            // where they already were, and broke the line after a "·";
            // critique #92 A P3-5.)
            let place = String::new();
            Some((name, place, keys))
        }
        Some(KeyFocus::Matrix(row, col)) => {
            let from = row.map_or_else(|| "Outputs row".to_string(), strip);
            // As the pointer's readout names a box (critique #28: "output 1"
            // here, "Main 1/2 (left)" under the pointer).
            let side = if col % 2 == 0 { "left" } else { "right" };
            Some((
                format!("Matrix: {from} into {} ({side})", crate::matrix::column_head(state, col / 2)),
                String::new(),
                "\u{2191} \u{2193} change it, Space turns it off (or to 0 dB)",
            ))
        }
        None => state.focused_strip.map(|cid| {
            (strip(cid), "strip".to_string(), "M mutes, S solos, \u{2191} \u{2193} its fader, Shift+\u{2190} \u{2192} its pan, N narrows, Space opens Settings")
        }),
    }
}

/// A Settings panel row's name, as its row says it, and its keys.
fn panel_item_words(item: PanelItem) -> (&'static str, &'static str) {
    const SWITCH: &str = "Space switches it, Esc leaves";
    const TURN: &str = "\u{2190} \u{2192} change it, Esc leaves";
    const PRESS: &str = "Space or Enter, Esc leaves";
    match item {
        PanelItem::Stereo => ("Stereo pair", SWITCH),
        PanelItem::TrimGains => ("Trim Gains (T)", SWITCH),
        PanelItem::FollowsTrim => ("Follows Trim Gains", SWITCH),
        PanelItem::GetsTalkback => ("Gets Talkback", SWITCH),
        PanelItem::FxReturn => ("FX return", TURN),
        PanelItem::FxSend => ("FX send", TURN),
        PanelItem::Gain => ("Preamp gain, dB", "\u{2190} \u{2192} one dB a press, Esc leaves"),
        PanelItem::AutoSet => ("AutoSet", SWITCH),
        PanelItem::Dynamics => ("Dynamics", "Space opens it"),
        PanelItem::Phantom => ("Phantom power (48V)", SWITCH),
        PanelItem::Pad => ("PAD, \u{2212}11 dB", SWITCH),
        PanelItem::An12 => ("AN1\u{2192}2: AN1 on both sides", SWITCH),
        PanelItem::RefLevel => ("Reference level", "\u{2190} \u{2192} picks, Esc leaves"),
        PanelItem::Sensitivity => ("Sensitivity", "\u{2190} \u{2192} picks, Esc leaves"),
        PanelItem::Phase => ("Phase invert (\u{d8})", SWITCH),
        PanelItem::OutPhaseL => ("Phase invert, left", SWITCH),
        PanelItem::OutPhaseR => ("Phase invert, right", SWITCH),
        PanelItem::OutMs | PanelItem::SrcMs => ("M/S processing", SWITCH),
        PanelItem::SrcWidth => ("Width", "\u{2190} \u{2192} 0.05 a step, Space resets"),
        PanelItem::Eq => ("EQ", "Space opens it"),
        PanelItem::MicPreset => ("Mic preset", PRESS),
        PanelItem::Copy => ("Copy its sends", PRESS),
        PanelItem::Paste => ("Paste sends", PRESS),
        PanelItem::Mirror => ("Mirror", PRESS),
        PanelItem::Clear => ("Clear its sends", PRESS),
        PanelItem::StopMirror => ("Stop mirroring", PRESS),
        PanelItem::Hide => ("Hide this strip", PRESS),
        PanelItem::Flow => ("The map of where its sound goes", "Space opens it beside the panel, Esc leaves"),
    }
}

/// Draws `build` with its controls registered in `region`.
pub fn in_region<'a>(region: Region, build: impl FnOnce() -> Element<'a, Message>) -> Element<'a, Message> {
    let outer = REGION.with(|r| r.replace(Some(region)));
    let el = build();
    REGION.with(|r| r.set(outer));
    el
}

/// The id of the control named `key` in `region`; a name that comes up
/// again in the same frame gets the next id of its own.
fn next_id(region: Region, key: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    region.hash(&mut h);
    key.hash(&mut h);
    let base = h.finish();
    let n = SEEN.with(|s| {
        let mut s = s.borrow_mut();
        let n = s.entry(base).or_insert(0);
        *n += 1;
        *n
    });
    let mut h = std::collections::hash_map::DefaultHasher::new();
    base.hash(&mut h);
    n.hash(&mut h);
    h.finish()
}

/// The id the first control named `key` in `region` gets (for a screenshot
/// that starts with the keyboard on it).
pub fn first_id(region: Region, key: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    region.hash(&mut h);
    key.hash(&mut h);
    let base = h.finish();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    base.hash(&mut h);
    1u32.hash(&mut h);
    h.finish()
}

/// A control the keyboard can reach, drawn in the region being drawn:
/// registered with what its keys send, and ringed while it has the
/// keyboard. Outside a region it is drawn as it is.
pub fn reach<'a>(
    key: &str,
    el: Element<'a, Message>,
    press: Option<Message>,
    less: Option<Message>,
    more: Option<Message>,
    radius: f32,
) -> Element<'a, Message> {
    let Some(region) = REGION.with(Cell::get) else { return el };
    if press.is_none() && less.is_none() && more.is_none() {
        return el;
    }
    let id = next_id(region, key);
    DRAWN.with(|d| d.borrow_mut().push(Reach { id, region, name: key.to_string(), press, less, more }));
    ring(el, FOCUS.with(Cell::get) == Some(id), radius)
}

/// A key or a switch: Space or Enter sends `message`.
pub fn reach_press<'a>(key: &str, el: Element<'a, Message>, message: Option<Message>, radius: f32) -> Element<'a, Message> {
    reach(key, el, message, None, None, radius)
}

/// A pick list: left and right choose the option before or after.
pub fn reach_pick<'a, T: PartialEq + Clone>(
    key: &str,
    el: Element<'a, Message>,
    options: &[T],
    selected: Option<&T>,
    on_select: impl Fn(T) -> Message,
    radius: f32,
) -> Element<'a, Message> {
    let at = selected.and_then(|s| options.iter().position(|o| o == s));
    let pick = |dir: i32| -> Option<Message> {
        let n = options.len() as i32;
        if n == 0 {
            return None;
        }
        let to = match at {
            Some(i) => i as i32 + dir,
            None if dir > 0 => 0,
            None => n - 1,
        };
        (0..n).contains(&to).then(|| on_select(options[to as usize].clone()))
    };
    reach(key, el, None, pick(-1), pick(1), radius)
}

/// A knob or a level: left and right a fiftieth of its sweep (`step`, if
/// given, instead), along a log sweep where it has one.
#[allow(clippy::too_many_arguments)]
pub fn reach_knob<'a>(
    key: &str,
    el: Element<'a, Message>,
    value: f32,
    range: (f32, f32),
    log: bool,
    step: Option<f32>,
    on_change: impl Fn(f32) -> Message,
    radius: f32,
) -> Element<'a, Message> {
    let (lo, hi) = range;
    let to = |dir: f32| -> Option<Message> {
        let v = if log && lo > 0.0 {
            let t = (value / lo).ln() / (hi / lo).ln() + dir / 50.0;
            lo * (hi / lo).powf(t.clamp(0.0, 1.0))
        } else {
            value + dir * step.unwrap_or((hi - lo) / 50.0)
        }
        .clamp(lo.min(hi), hi.max(lo));
        (v != value).then(|| on_change(v))
    };
    reach(key, el, None, to(-1.0), to(1.0), radius)
}

/// The controls of `region` drawn in the last frame, in the order they
/// were drawn: the keyboard's stops in that region (the Quick setup's own
/// scroll, job 14; the D panel's, job 13).
pub fn drawn_in(region: Region) -> Vec<Reach> {
    DRAWN.with(|l| l.borrow().iter().filter(|r| r.region == region).cloned().collect())
}

/// Whether a region's controls are drawn after the keyboard's card, which
/// stands in the column's foot: the card names them from the frame
/// before, because they are not drawn yet when the card is.
fn drawn_after_the_card(region: Region) -> bool {
    matches!(region, Region::About | Region::Settings | Region::Device)
}

/// The drawn control the keyboard is on.
fn focused_reach(state: &TuxMix) -> Option<Reach> {
    let Some(KeyFocus::Item(id)) = state.key_focus else { return None };
    DRAWN.with(|l| l.borrow().iter().find(|r| r.id == id).cloned())
}

/// The control the keyboard is on is gone: its panel closed, or it was
/// renamed. The focus moves to the stop that now stands at its place in
/// the same region (the same place, or the last one if the list got
/// shorter). With the region gone too, the focus lets go (job 14).
/// A frame is kept for the repair: the tick runs fast until it has moved
/// the focus, so the keyboard card is blank for one fast tick, not an idle
/// half second (OpenCode's job 35).
pub fn kept() -> bool {
    KEPT.with(Cell::get)
}

/// On the tick: a focus whose control is gone (renamed, or removed by its
/// own press) moves to the stop now at its place, at once rather than at
/// the next key press, so the ring never vanishes (critique #37).
pub fn settle_focus(state: &mut TuxMix) {
    let Some(KeyFocus::Item(id)) = state.key_focus else { return };
    // Only a control that was drawn and is gone: one a handler just put
    // the focus on is drawn by the next frame, and a tick before it must
    // not let it go (the advisor: "Keep it on…", Show me).
    let gone = !DRAWN.with(|d| d.borrow().iter().any(|r| r.id == id)) && LAST.with(|l| l.borrow().iter().any(|r| r.id == id));
    if gone {
        repair_focus(state);
    }
}

fn repair_focus(state: &mut TuxMix) {
    let Some(KeyFocus::Item(id)) = state.key_focus else { return };
    // Still drawn: nothing to do.
    if DRAWN.with(|d| d.borrow().iter().any(|r| r.id == id)) {
        return;
    }
    // Where the focus stood, and in which region.
    let Some((region, at)) = LAST.with(|l| {
        let l = l.borrow();
        let region = l.iter().find(|r| r.id == id).map(|r| r.region)?;
        let at = l.iter().filter(|r| r.region == region).position(|r| r.id == id)?;
        Some((region, at))
    }) else {
        // The control was never drawn, or not in the frame before: there
        // is nothing to stand on.
        state.key_focus = None;
        return;
    };
    // The region's stops now.
    let now: Vec<u64> = DRAWN.with(|d| d.borrow().iter().filter(|r| r.region == region).map(|r| r.id).collect());
    match now.len() {
        // The region is gone with it: the focus lets go.
        0 => state.key_focus = None,
        // The stop that now stands at its place (the last one if the list
        // got shorter).
        n => state.key_focus = Some(KeyFocus::Item(now[at.min(n - 1)])),
    }
}

/// In Help, whether the keyboard is on one of the links at its foot
/// (`Some(true)`) or on a guide's key at its top (`Some(false)`): where its
/// body scrolls to (`app::help_scroll_to_focus`).
pub(crate) fn help_focus_at_foot(state: &TuxMix) -> Option<bool> {
    let r = focused_reach(state).filter(|r| r.region == Region::About)?;
    Some(matches!(r.press, Some(Message::OpenWikiUrl | Message::OpenRepoUrl)))
}

/// Tab and Shift+Tab through `region`: round and round (`wrap`), or `None`
/// past either end.
fn region_step(state: &mut TuxMix, region: Region, step: i32, wrap: bool) -> Option<()> {
    let items = drawn_in(region);
    let n = items.len() as i32;
    if n == 0 {
        return None;
    }
    let at = match state.key_focus {
        Some(KeyFocus::Item(id)) => items.iter().position(|r| r.id == id).map(|i| i as i32),
        _ => None,
    };
    let next = match at {
        Some(i) if wrap => (i + step).rem_euclid(n),
        Some(i) => i + step,
        None if step > 0 => 0,
        None => n - 1,
    };
    if !(0..n).contains(&next) {
        return None;
    }
    state.key_focus = Some(KeyFocus::Item(items[next as usize].id));
    Some(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterItem {
    /// A monitor level: the Main (or Main B playing it), a Phones pair.
    Level(usize),
    /// A Control Room key, by its label (`control_room::key_specs`).
    Key(&'static str),
    /// A snapshot slot, 1 to 8.
    Slot(u8),
    /// Reverb & Echo's page, while it is open: the key for the next step,
    /// an effect's On key, its type, its preset, and its knobs by index.
    FxAction,
    /// The page's "Faders show FX sends" key.
    FxFaders,
    FxOn(crate::fx::Unit),
    FxType(crate::fx::Unit),
    FxPreset(crate::fx::Unit),
    FxKnob(crate::fx::Unit, u8),
    /// An effect's "In recordings" key, after its knobs.
    FxRec(crate::fx::Unit),
    /// A page's bar (Snapshots, Reverb & Echo, Groups, Layouts, View): it
    /// opens or closes its page (critique #6: none could be reached).
    Page(crate::sidebar::Tab),
    /// The Snapshots page's Store….
    Store,
    /// The guide card's "Show me", while one stands in the counter's slot,
    /// and its × (a guide under way stops only by it).
    GuideGo,
    GuideStop,
    /// The one-time hint card's "Show me" and "Got it", while it stands in
    /// the counter's slot (OpenCode's job 12: only the mouse could answer it).
    DHintShow,
    DHintDone,
    /// The placard's "Only these" / "Show all", and the open-mic warning's
    /// "Take it out" and "Keep it" while it shows.
    OnlyThese,
    OpenMicTakeOut,
    OpenMicKeep,
    OpenMicExpand,
    /// The placard's "Reverb not heard · …" row (`sidebar::fx_unheard`).
    FxUnheard,
    /// The placard's mix name (left and right choose the mix), its "Fed
    /// by" line (opens the map) and an output's phase / M-S note (opens its
    /// Settings) (OpenCode's job 74: the three were mouse-only).
    MixName,
    FedBy,
    OutputNote,
    /// The FX-sends placard's "Back to …" and the key beside it, while the
    /// faders show FX sends (`sidebar::fx_sends_second`).
    FxSendsBack,
    FxSendsKey,
    /// The Layouts page's slots (1-based) and its Store….
    LayoutSlot(u8),
    LayoutStore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelItem {
    Stereo,
    TrimGains,
    FollowsTrim,
    GetsTalkback,
    /// Reverb and Echo: an output's return, AN1/2's send.
    FxReturn,
    FxSend,
    Gain,
    /// AutoSet under the gain (issue #26).
    AutoSet,
    /// The door to the D panel (issue #27).
    Dynamics,
    Phantom,
    Pad,
    An12,
    RefLevel,
    Sensitivity,
    /// Phase invert (Settings panel).
    Phase,
    /// An output's phase: its left side (or its one channel), its right.
    OutPhaseL,
    OutPhaseR,
    /// An output pair's M/S processing.
    OutMs,
    /// A stereo input's or playback's M/S processing.
    SrcMs,
    /// A linked stereo pair's width (Settings panel), 0.05 a step.
    SrcWidth,
    /// Opens the strip's EQ panel (the audit, 2026-11-07: EQ had no
    /// keyboard path; Tab goes on into the EQ panel once it is open).
    Eq,
    MicPreset,
    Copy,
    Paste,
    Mirror,
    Clear,
    StopMirror,
    Hide,
    /// "Where it goes" / "What plays here": the map beside the panel
    /// (`flow::popup`, issue #23).
    Flow,
}

/// The counter's items in the order they stand. None while it is folded
/// to its rail; the slots only while the Snapshots page is open.
pub fn counter_items(state: &TuxMix) -> Vec<CounterItem> {
    if !state.sidebar_open {
        return Vec::new();
    }
    // The placard first, where it stands: its "Only these", then the
    // open-mic warning's two answers while it shows.
    // With the faders on FX sends the placard is Reverb & Echo's: its Back
    // and the key beside it, no "Only these" and no not-heard row.
    let mut items = if state.fader_fx {
        let mut keys = vec![CounterItem::FxSendsBack];
        if crate::sidebar::fx_sends_second(state).is_some() {
            keys.push(CounterItem::FxSendsKey);
        }
        keys
    } else {
        vec![CounterItem::MixName, CounterItem::FedBy, CounterItem::OnlyThese]
    };
    if crate::sidebar::open_mic_shown(state).is_some() {
        items.push(CounterItem::OpenMicExpand);
        if state.open_mic_expanded {
            items.extend([CounterItem::OpenMicTakeOut, CounterItem::OpenMicKeep]);
        }
    }
    // Under the open-mic warning, as drawn (critique #70 A 3).
    // On the FX-sends placard too, for the hazard (critique #111 A P2-1).
    if (!state.fader_fx || crate::fx::mic_reverb_hazard(state).is_some()) && crate::sidebar::fx_unheard(state).is_some() {
        items.push(CounterItem::FxUnheard);
    }
    if crate::sidebar::output_note_press(state).is_some() {
        items.push(CounterItem::OutputNote);
    }
    items.push(CounterItem::Level(crate::control_room::monitor_pair(state)));
    items.extend(state.control_room.assign.phones_pairs().into_iter().map(CounterItem::Level));
    // In a folded band, only the keys it shows, then its More key (critique
    // #33's pick, "B"): Tab never lands on a key out of sight.
    if crate::control_room::keys_in_band(state) {
        items.push(CounterItem::Key(if state.band_more_open { crate::control_room::BAND_LESS } else { crate::control_room::BAND_MORE }));
        items.extend(crate::control_room::band_specs(state).into_iter().filter(|k| k.on_press.is_some()).map(|k| CounterItem::Key(k.label)));
    } else {
        items.extend(
            crate::control_room::key_specs(state)
                .into_iter()
                .filter(|k| k.on_press.is_some())
                .map(|k| CounterItem::Key(k.label)),
        );
    }
    // Where the pages float: the snapshot keys and Store in the head, then
    // the pages' keys, an open card's stops after its key.
    let float = crate::sidebar::pages_float(state);
    use crate::sidebar::Tab;
    if float {
        // The Snapshots section's title, its keys and Store; listed in the
        // head (issue #18) its title opens nothing, and its rows are the
        // keys.
        if !crate::sidebar::snapshots_listed(state) {
            items.push(CounterItem::Page(Tab::Snapshots));
        }
        items.extend((1..=8).map(CounterItem::Slot));
        items.push(CounterItem::Store);
    }
    // Each page's bar, its page's stops under it while it is open, in the
    // order they stand.
    for tab in [Tab::Snapshots, Tab::Fx, Tab::Groups, Tab::Layouts, Tab::View] {
        if !(float && tab == Tab::Snapshots) {
            items.push(CounterItem::Page(tab));
        }
        let open = if float { crate::sidebar::page_open(state) == Some(tab) } else { state.sidebar_tab == Some(tab) };
        if !open {
            continue;
        }
        match tab {
            // The card's slots are the head's keys: reached once, there.
            Tab::Snapshots if float => {}
            Tab::Snapshots => {
                items.extend((1..=8).map(CounterItem::Slot));
                items.push(CounterItem::Store);
            }
            Tab::Fx => {
                if crate::fx::status_action(state).is_some() {
                    items.push(CounterItem::FxAction);
                }
                if crate::fx::in_panels(state) {
                    items.push(CounterItem::FxFaders);
                }
                for unit in [crate::fx::Unit::Reverb, crate::fx::Unit::Echo] {
                    items.extend([CounterItem::FxOn(unit), CounterItem::FxType(unit), CounterItem::FxPreset(unit)]);
                    items.extend((0..crate::fx::knob_count(state, unit) as u8).map(|i| CounterItem::FxKnob(unit, i)));
                    items.push(CounterItem::FxRec(unit));
                }
            }
            Tab::Layouts => {
                items.extend((1..=crate::sidebar::LAYOUT_COUNT).map(CounterItem::LayoutSlot));
                items.push(CounterItem::LayoutStore);
            }
            _ => {}
        }
    }
    // The guide card's "Show me" and ×, last, where the card stands.
    if crate::guides::show_me_here(state) {
        items.push(CounterItem::GuideGo);
    }
    if crate::guides::under_way_here(state) {
        items.push(CounterItem::GuideStop);
    }
    if crate::dynamics_panel::hint_stops_here(state) {
        items.extend([CounterItem::DHintShow, CounterItem::DHintDone]);
    }
    items
}

/// Left/right and +/− on a Reverb & Echo item that turns: a knob a step,
/// the type, or the next preset (loaded at once, to audition them).
fn fx_turn(state: &TuxMix, item: CounterItem, dir: i32) -> Option<Option<Message>> {
    let m = match item {
        CounterItem::FxKnob(unit, i) => crate::fx::knob_nudge(state, unit, i as usize, dir),
        CounterItem::FxType(unit) => Some(crate::fx::type_step(state, unit, dir)),
        CounterItem::FxPreset(unit) => crate::fx::preset_step(state, unit, dir),
        _ => return None,
    };
    // Tagged as the keyboard's (`tricks`): the same messages as a click.
    Some(m.map(|m| Message::KeyShortcut("fx-keys", Box::new(m))))
}

/// The rows of strip `cid`'s Settings panel the keyboard can reach, in the
/// order the panel shows them (`app::settings_popover`); inert ones left
/// out.
pub fn panel_items(state: &TuxMix, cid: ChannelId) -> Vec<PanelItem> {
    use PanelItem::*;
    let p = crate::app::strip_params(state, cid, state.sel_out);
    let mut items = Vec::new();
    if !matches!(cid, ChannelId::Playback(_)) || p.protocol_features {
        items.push(Stereo);
    }
    // A source's "Stereo" group: the pair, its M/S and its Width
    // (`settings_popover`).
    if matches!(cid, ChannelId::Input(_) | ChannelId::Playback(_)) {
        if crate::app::strip_has_source_ms(state, cid) {
            items.push(SrcMs);
        }
        if state.device.can_source_width(cid) {
            items.push(SrcWidth);
        }
    }
    // Every source's Trim Gains stands under the pair, in "In the mixer"
    // (`settings_popover`, critique #20).
    if !matches!(cid, ChannelId::Output(_)) && !state.trim_all {
        items.push(TrimGains);
    }
    // "Where it goes" (a source), under Trim Gains (`settings_popover`).
    if !matches!(cid, ChannelId::Output(_)) {
        items.push(Flow);
    }
    // A playback's send, where the driver has it (`settings_popover`).
    if matches!(cid, ChannelId::Playback(_)) && crate::fx::in_panels(state) && state.device.has_source_fx_send(cid) {
        items.push(FxSend);
    }
    if let ChannelId::Output(ch) = cid {
        items.push(FollowsTrim);
        let pair = crate::mixops::output_pair(state, ch);
        if !state.control_room.assign.is_phones(pair) {
            items.push(GetsTalkback);
        }
        // "What plays here", under them (`settings_popover`).
        items.push(Flow);
        if state.device.can_output_proc() {
            items.extend(crate::app::output_phase_rows(state, ch).into_iter().map(|r| r.0));
            if crate::app::output_ms_pair(state, ch).is_some() {
                items.push(OutMs);
            }
        }
        if state.device.can_output_eq(ch) {
            items.push(Eq);
        }
        if crate::dynamics_panel::has_d(state, cid) {
            items.push(Dynamics);
        }
        if crate::fx::in_panels(state) {
            items.push(FxReturn);
        }
    }
    if let ChannelId::Input(idx) = cid {
        if p.has_gain {
            items.push(Gain);
            items.push(AutoSet);
        }
        if p.has_48v {
            items.push(Phantom);
        }
        if p.has_pad {
            items.push(Pad);
        }
        if idx < 2 && p.protocol_features {
            items.push(An12);
        }
        if p.has_phase && p.protocol_features {
            items.push(Phase);
        }
        if p.has_eq && p.protocol_features {
            items.push(Eq);
        }
        if crate::dynamics_panel::has_d(state, cid) {
            items.push(Dynamics);
        }
        if p.has_sensitivity && state.device.has_ref_level_boost() {
            items.push(RefLevel);
        } else if p.has_sensitivity && state.device.has_sensitivity_control() {
            items.push(Sensitivity);
        }
        // "To Reverb & Echo" after the unit's controls (`settings_popover`).
        if (idx < 2 || state.device.has_source_fx_send(cid)) && crate::fx::in_panels(state) {
            items.push(FxSend);
        }
        if p.has_gain && !state.mic_presets.is_empty() {
            items.push(MicPreset);
        }
    }
    items.push(Copy);
    if crate::mixops::can_paste(state, cid) {
        items.push(Paste);
    }
    if let ChannelId::Output(ch) = cid {
        if crate::app::mirror_message(state, cid).is_some() {
            items.push(Mirror);
        }
        items.push(Clear);
        if crate::mixops::mirrored_from(state, crate::mixops::output_pair(state, ch)).is_some() {
            items.push(StopMirror);
        }
    } else {
        items.push(Clear);
    }
    if crate::app::hideable(state, cid) {
        items.push(Hide);
    }
    items
}

/// Whether counter item `item` has the keyboard.
pub fn counter_focused(state: &TuxMix, item: CounterItem) -> bool {
    state.key_focus == Some(KeyFocus::Counter(item))
}

/// Whether row `item` of strip `cid`'s panel has the keyboard.
pub fn panel_focused(state: &TuxMix, cid: ChannelId, item: PanelItem) -> bool {
    state.focused_strip == Some(cid) && state.key_focus == Some(KeyFocus::Panel(item))
}

/// Whether the Matrix cell at `row`, `col` has the keyboard.
pub fn matrix_focused(state: &TuxMix, row: Option<ChannelId>, col: usize) -> bool {
    state.view == View::Matrix && state.key_focus == Some(KeyFocus::Matrix(row, col))
}

/// The focus ring on a placard key: outside the key with a 1 px gap, in a
/// 3 px slot every placard key keeps, so nothing moves when the focus does
/// (critique #72 A 1: in the halo's colour it equalled the card's face and
/// a focused key only looked smaller; inside, it ran through the words of
/// keys padded 2 px). The card's ink: it reads on the card's face, around
/// a filled key as around an outlined one.
pub fn placard_ring<'a>(content: Element<'a, Message>, focused: bool, radius: f32) -> Element<'a, Message> {
    container(content)
        .padding(PLACARD_RING_SLOT)
        .style(move |_t: &iced::Theme| if focused { (crate::theme::placard_focus_ring(radius + PLACARD_RING_SLOT))(_t) } else { iced::widget::container::Style::default() })
        .into()
}

/// `placard_ring` for a key whose own line height leaves room above and
/// below its words (the placard's mix name, its "Fed by" line): the slot
/// only at the sides, so the card is no taller (a 3 px slot all round made
/// it 12 px taller and cost the page keys at 1280x700).
pub fn placard_ring_flat<'a>(content: Element<'a, Message>, focused: bool, radius: f32) -> Element<'a, Message> {
    container(content)
        .padding([0.0, PLACARD_RING_SLOT])
        .style(move |_t: &iced::Theme| if focused { (crate::theme::placard_focus_ring(radius))(_t) } else { iced::widget::container::Style::default() })
        .into()
}

/// The room around each placard key for its focus ring: a 1 px gap and a
/// 2 px ring.
pub const PLACARD_RING_SLOT: f32 = 3.0;

/// The focus ring: an inner ring over `content` in the halo's colour
/// (`theme::focus_ring`), drawn over it so nothing moves when the focus
/// does; it takes no events, so the control under it works as before.
pub fn ring<'a>(content: Element<'a, Message>, focused: bool, radius: f32) -> Element<'a, Message> {
    if !focused {
        return content;
    }
    stack![
        content,
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(crate::theme::focus_ring(radius)),
    ]
    .into()
}

/// `ring` with the mark washed over the face (`theme::focus_ring_tinted`).
pub fn ring_tinted<'a>(content: Element<'a, Message>, focused: bool, radius: f32) -> Element<'a, Message> {
    if !focused {
        return content;
    }
    stack![
        content,
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(crate::theme::focus_ring_tinted(radius)),
    ]
    .into()
}

/// The strip whose Settings panel is open, when it is the keyboard's.
pub(crate) fn panel_strip(state: &TuxMix) -> Option<ChannelId> {
    state
        .focused_strip
        .filter(|cid| state.flyout_open.contains(&(*cid, FlyoutKind::Settings)))
}

/// One step through `list` from `at` (by `step`), or `None` past its ends.
fn stepped<T: PartialEq + Copy>(list: &[T], at: Option<T>, step: i32) -> Option<T> {
    let n = list.len() as i32;
    let i = at.and_then(|a| list.iter().position(|&x| x == a)).map_or(if step > 0 { -1 } else { n }, |i| i as i32);
    let next = i + step;
    (0..n).contains(&next).then(|| list[next as usize])
}

/// Tab (`step` 1) and Shift+Tab (-1). `None`: handled; `Some`: the message
/// to dispatch instead (the views' own Tab, outside the Mixer and Matrix).
pub fn tab(state: &mut TuxMix, step: i32) -> Option<Message> {
    // A control that is gone: the focus moves to the stop that now stands
    // at its place (job 14).
    repair_focus(state);
    // About holds the keyboard while it is open, and so does Audra settings
    // (they close each other, so only one stands).
    if state.show_about {
        let _ = region_step(state, Region::About, step, true);
        return None;
    }
    if state.show_settings {
        let _ = region_step(state, Region::Settings, step, true);
        return None;
    }
    if state.show_device_panel {
        let _ = region_step(state, Region::Device, step, true);
        return None;
    }
    match state.view {
        View::Matrix => {
            matrix_step(state, step, true);
            None
        }
        // The Quick setup's controls, round and round (Ctrl+Tab steps the
        // views).
        View::Quick => {
            let _ = region_step(state, Region::Quick, step, true);
            None
        }
        View::Mixer => {
            mixer_tab(state, step);
            None
        }
    }
}

/// One of a strip's open panels, as the keyboard passes through them:
/// its Settings panel, then its EQ, then its Trim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    Settings,
    Drawn(Region),
}

/// The open panels of strip `cid` with something for the keyboard, in the
/// order Tab passes through them.
fn stops_of(state: &TuxMix, cid: ChannelId) -> Vec<Stop> {
    let open = |kind| state.flyout_open.contains(&(cid, kind));
    let mut stops = Vec::new();
    if open(FlyoutKind::Settings) && state.focused_strip == Some(cid) && !panel_items(state, cid).is_empty() {
        stops.push(Stop::Settings);
    }
    let eq = Region::Eq(cid);
    if open(FlyoutKind::Eq) && !drawn_in(eq).is_empty() {
        stops.push(Stop::Drawn(eq));
    }
    let d = Region::Dyn(cid);
    if open(FlyoutKind::Dynamics) && !drawn_in(d).is_empty() {
        stops.push(Stop::Drawn(d));
    }
    stops
}

/// The keyboard into `stop` of strip `cid`: its first item going forward,
/// its last going back.
fn enter(state: &mut TuxMix, cid: ChannelId, stop: Stop, step: i32) {
    state.focused_strip = Some(cid);
    match stop {
        Stop::Settings => {
            let items = panel_items(state, cid);
            state.key_focus = if step > 0 { items.first() } else { items.last() }.map(|&i| KeyFocus::Panel(i));
        }
        Stop::Drawn(region) => {
            let items = drawn_in(region);
            state.key_focus = if step > 0 { items.first() } else { items.last() }.map(|r| KeyFocus::Item(r.id));
        }
    }
}

/// The strip whose EQ (or D) panel the keyboard is in, if it is.
pub fn focus_in_eq(state: &TuxMix) -> Option<ChannelId> {
    match in_panel(state)? {
        (cid, Stop::Drawn(Region::Eq(_) | Region::Dyn(_))) => Some(cid),
        _ => None,
    }
}

/// Where in a strip's open panels the keyboard is, if it is.
fn in_panel(state: &TuxMix) -> Option<(ChannelId, Stop)> {
    match state.key_focus? {
        KeyFocus::Panel(_) => panel_strip(state).map(|cid| (cid, Stop::Settings)),
        KeyFocus::Item(_) => match focused_reach(state)?.region {
            r @ (Region::Eq(cid) | Region::Dyn(cid)) => Some((cid, Stop::Drawn(r))),
            _ => None,
        },
        _ => None,
    }
}

fn mixer_tab(state: &mut TuxMix, step: i32) {
    let order = crate::app::mixer_strip_order(state);
    let counter = counter_items(state);
    if let Some((cid, stop)) = in_panel(state) {
        // A step inside the panel the keyboard is in.
        let moved = match (stop, state.key_focus) {
            (Stop::Settings, Some(KeyFocus::Panel(item))) => match stepped(&panel_items(state, cid), Some(item), step) {
                Some(next) => {
                    state.key_focus = Some(KeyFocus::Panel(next));
                    true
                }
                None => false,
            },
            (Stop::Drawn(region), _) => region_step(state, region, step, false).is_some(),
            _ => false,
        };
        if moved {
            return;
        }
        // Past its end: the strip's next open panel (or the one before).
        let stops = stops_of(state, cid);
        let at = stops.iter().position(|s| *s == stop).map_or(-1, |i| i as i32);
        if let Some(&next) = usize::try_from(at + step).ok().and_then(|i| stops.get(i)) {
            enter(state, cid, next, step);
            return;
        }
        // Out of the panels: on to the next strip, or back on this one.
        state.key_focus = None;
        state.focused_strip = Some(cid);
        if step > 0 {
            strip_or_counter(state, &order, &counter, Some(cid), step);
        }
        return;
    }
    match state.key_focus {
        Some(KeyFocus::Counter(item)) => match stepped(&counter, Some(item), step) {
            Some(next) => state.key_focus = Some(KeyFocus::Counter(next)),
            None => {
                state.key_focus = None;
                let end = if step > 0 { order.first() } else { order.last() };
                state.focused_strip = end.copied();
                crate::app::open_row_of_focus(state);
            }
        },
        _ => {
            // Into the strip's open panels first.
            if step > 0 {
                if let Some(cid) = state.focused_strip {
                    if let Some(&first) = stops_of(state, cid).first() {
                        enter(state, cid, first, step);
                        return;
                    }
                }
            }
            let from = state.focused_strip;
            strip_or_counter(state, &order, &counter, from, step);
        }
    }
}

/// The next strip from `from`, or past either end of the strips the
/// counter's first or last item.
fn strip_or_counter(state: &mut TuxMix, order: &[ChannelId], counter: &[CounterItem], from: Option<ChannelId>, step: i32) {
    state.key_focus = None;
    match stepped(order, from, step) {
        Some(next) => {
            state.focused_strip = Some(next);
            crate::app::open_row_of_focus(state);
        }
        None => {
            let end = if step > 0 { counter.first() } else { counter.last() };
            match end {
                Some(&item) => {
                    state.focused_strip = None;
                    state.key_focus = Some(KeyFocus::Counter(item));
                }
                // No counter (folded): round the strips.
                None => {
                    state.focused_strip = if step > 0 { order.first() } else { order.last() }.copied();
                    crate::app::open_row_of_focus(state);
                }
            }
        }
    }
}

/// Left and right (`step` -1 and 1) where the keyboard is beyond a strip.
/// `false`: not here; the arrows keep their strip meaning.
pub fn left_right(state: &mut TuxMix, step: i32) -> Option<Option<Message>> {
    repair_focus(state);
    match state.key_focus? {
        KeyFocus::Item(_) => Some(focused_reach(state).and_then(|r| if step > 0 { r.more } else { r.less })),
        KeyFocus::Panel(item) => Some(adjust_panel(state, item, step)),
        KeyFocus::Counter(CounterItem::Level(pair)) => Some(level_nudge(state, pair, step as f32)),
        KeyFocus::Counter(CounterItem::MixName) => Some(crate::sidebar::mix_step(state, step)),
        KeyFocus::Counter(item) if fx_turn(state, item, step).is_some() => fx_turn(state, item, step),
        KeyFocus::Counter(item) => {
            let items = counter_items(state);
            if let Some(next) = stepped(&items, Some(item), step) {
                state.key_focus = Some(KeyFocus::Counter(next));
            }
            Some(None)
        }
        KeyFocus::Matrix(..) => {
            matrix_move(state, 0, step);
            Some(None)
        }
    }
}

/// Up and down (`dir` 1 and -1, as the fader nudge has them).
pub fn up_down(state: &mut TuxMix, dir: f32) -> Option<Option<Message>> {
    repair_focus(state);
    let step = if dir > 0.0 { -1 } else { 1 };
    match state.key_focus? {
        KeyFocus::Item(id) => {
            let region = DRAWN.with(|l| l.borrow().iter().find(|r| r.id == id).map(|r| r.region));
            if let Some(region) = region {
                let wrap = matches!(region, Region::Settings | Region::Quick | Region::Device);
                if region_step(state, region, step, wrap).is_none() {
                    // Past a panel's ends, up and down stay put.
                }
            }
            Some(None)
        }
        KeyFocus::Panel(item) => {
            let cid = panel_strip(state)?;
            if let Some(next) = stepped(&panel_items(state, cid), Some(item), step) {
                state.key_focus = Some(KeyFocus::Panel(next));
            }
            Some(None)
        }
        KeyFocus::Counter(item) => {
            let items = counter_items(state);
            if let Some(next) = stepped(&items, Some(item), step) {
                state.key_focus = Some(KeyFocus::Counter(next));
            }
            Some(None)
        }
        KeyFocus::Matrix(..) => {
            matrix_move(state, step, 0);
            Some(None)
        }
    }
}

/// + and − (`dir` 1 and -1).
pub fn adjust(state: &mut TuxMix, dir: i32) -> Option<Message> {
    repair_focus(state);
    match state.key_focus {
        Some(KeyFocus::Panel(item)) => adjust_panel(state, item, dir),
        Some(KeyFocus::Counter(CounterItem::Level(pair))) => level_nudge(state, pair, dir as f32),
        Some(KeyFocus::Counter(CounterItem::MixName)) => crate::sidebar::mix_step(state, dir),
        Some(KeyFocus::Matrix(row, col)) => matrix_nudge(state, row, col, dir as f32),
        Some(KeyFocus::Counter(item)) => fx_turn(state, item, dir).flatten(),
        Some(KeyFocus::Item(_)) => focused_reach(state).and_then(|r| if dir > 0 { r.more } else { r.less }),
        // On a strip, or nowhere: the fader nudge, as the arrows do.
        None => Some(Message::FaderNudge(dir as f32)),
    }
}

/// Space and Enter beyond a strip. `None` from the outer option: not
/// here (a strip's Space opens its panel).
pub fn press(state: &mut TuxMix) -> Option<Option<Message>> {
    match state.key_focus? {
        KeyFocus::Item(_) => {
            if focused_reach(state).is_none() {
                // The control is gone: the focus moves to the stop that now
                // stands at its place, but this press presses nothing (the
                // user meant the one that is gone, job 14).
                repair_focus(state);
                return Some(None);
            }
            Some(focused_reach(state).and_then(|r| r.press))
        }
        KeyFocus::Panel(item) => {
            let cid = panel_strip(state)?;
            Some(press_panel(state, cid, item))
        }
        KeyFocus::Counter(CounterItem::Key(label)) if label == crate::control_room::BAND_MORE || label == crate::control_room::BAND_LESS => {
            Some(Some(Message::BandMore(!state.band_more_open)))
        }
        KeyFocus::Counter(CounterItem::Key(label)) => Some(
            crate::control_room::key_specs(state)
                .into_iter()
                .find(|k| k.label == label)
                .and_then(|k| k.on_press),
        ),
        KeyFocus::Counter(CounterItem::Slot(n)) => Some(Some(Message::SnapshotSlotPressed(n))),
        KeyFocus::Counter(CounterItem::Page(tab)) => Some(Some(Message::SidebarTab(tab))),
        KeyFocus::Counter(CounterItem::Store) => Some(Some(Message::SnapshotStore)),
        KeyFocus::Counter(CounterItem::GuideGo) => Some(Some(Message::GuideGo)),
        KeyFocus::Counter(CounterItem::GuideStop) => Some(Some(Message::GuideStop)),
        KeyFocus::Counter(CounterItem::DHintShow) => Some(Some(Message::DHint(true))),
        KeyFocus::Counter(CounterItem::DHintDone) => Some(Some(Message::DHint(false))),
        KeyFocus::Counter(CounterItem::OnlyThese) => Some(Some(Message::SetChannelView(
            if state.channel_view == crate::app::ChannelView::Submix { crate::app::ChannelView::Layout } else { crate::app::ChannelView::Submix },
        ))),
        KeyFocus::Counter(CounterItem::OpenMicTakeOut) => Some(crate::sidebar::open_mic_shown(state).map(|(c, p)| Message::OpenMicTakeOut(c, p))),
        KeyFocus::Counter(CounterItem::OpenMicKeep) => Some(crate::sidebar::open_mic_shown(state).map(|(c, p)| Message::OpenMicKeep(c, p))),
        KeyFocus::Counter(CounterItem::OpenMicExpand) => Some(Some(Message::OpenMicExpand)),
        KeyFocus::Counter(CounterItem::FxUnheard) => Some(crate::sidebar::fx_unheard(state).map(|(press, _, _)| press)),
        KeyFocus::Counter(CounterItem::MixName) => Some(None),
        KeyFocus::Counter(CounterItem::FedBy) => Some(Some(Message::FlowOpen(crate::flow::FlowOf::Mix(state.sel_out)))),
        KeyFocus::Counter(CounterItem::OutputNote) => Some(crate::sidebar::output_note_press(state)),
        KeyFocus::Counter(CounterItem::FxSendsBack) => Some(Some(Message::FaderFxToggle)),
        KeyFocus::Counter(CounterItem::FxSendsKey) => Some(crate::sidebar::fx_sends_second(state)),
        KeyFocus::Counter(CounterItem::LayoutSlot(n)) => Some(Some(Message::LayoutClicked(n))),
        KeyFocus::Counter(CounterItem::LayoutStore) => Some(Some(Message::LayoutStore)),
        KeyFocus::Counter(CounterItem::Level(_)) => Some(None),
        KeyFocus::Counter(CounterItem::FxAction) => Some(crate::fx::status_action(state)),
        KeyFocus::Counter(CounterItem::FxFaders) => Some(Some(Message::FaderFxToggle)),
        KeyFocus::Counter(CounterItem::FxRec(unit)) => Some(Some(Message::SetRecordFx(unit, !crate::fx::rec_includes(state, unit)))),
        KeyFocus::Counter(CounterItem::FxOn(unit)) => Some(Some(Message::FxOn(unit, !crate::fx::is_on(state, unit)))),
        KeyFocus::Counter(CounterItem::FxType(_) | CounterItem::FxPreset(_) | CounterItem::FxKnob(..)) => Some(None),
        KeyFocus::Matrix(row, col) => {
            let (cid, pair, now) = matrix_cell(state, row, col)?;
            let to = if now > 0.0 { 0.0 } else { 1.0 };
            Some(Some(Message::MatrixSet(cid, pair, to, false)))
        }
    }
}

/// Esc: the keyboard lets go of the counter or the Matrix cell. `false`:
/// nothing of its here (a panel's Esc closes the panel).
pub fn let_go(state: &mut TuxMix) -> bool {
    // An item in Audra settings lets go with the dialog (Esc closes it).
    if (state.show_settings || state.show_device_panel) && matches!(state.key_focus, Some(KeyFocus::Item(_))) {
        return false;
    }
    if matches!(state.key_focus, Some(KeyFocus::Counter(_) | KeyFocus::Matrix(..) | KeyFocus::Item(_))) {
        state.key_focus = None;
        true
    } else {
        false
    }
}

fn press_panel(state: &mut TuxMix, cid: ChannelId, item: PanelItem) -> Option<Message> {
    use PanelItem::*;
    let p = crate::app::strip_params(state, cid, state.sel_out);
    let input = match cid {
        ChannelId::Input(i) => Some(i),
        _ => None,
    };
    match item {
        Stereo => Some(Message::StereoLinkChanged(cid, !p.stereo_linked)),
        TrimGains => (!state.trim_all).then_some(Message::TrimStripToggle(cid)),
        FollowsTrim => {
            let ChannelId::Output(ch) = cid else { return None };
            Some(Message::NoTrimToggle(crate::mixops::output_pair(state, ch)))
        }
        GetsTalkback => {
            let ChannelId::Output(ch) = cid else { return None };
            let pair = crate::mixops::output_pair(state, ch);
            let flagged = state.control_room.talkback_outputs.contains(&pair);
            Some(Message::TalkbackOutputChanged(pair, !flagged))
        }
        Gain | FxReturn | FxSend => None,
        // Space puts Width back to +1.00 (as recorded), as Pitch's does 0.
        SrcWidth => (state.device.settings().source_width_of(cid) < 1.0).then_some(Message::SourceWidth(cid, 1.0)),
        Phase => Some(Message::PhaseChanged(cid, !p.phase)),
        AutoSet => {
            let ChannelId::Input(idx) = cid else { return None };
            Some(Message::SetAutoSet(idx, !crate::autoset::is_on(state, idx)))
        }
        Dynamics => Some(Message::ToggleFlyout(cid, FlyoutKind::Dynamics)),
        OutPhaseL | OutPhaseR => {
            let ChannelId::Output(ch) = cid else { return None };
            let (_, och, ..) = crate::app::output_phase_rows(state, ch).into_iter().find(|r| r.0 == item)?;
            Some(Message::OutputPhaseSide(och, !state.device.settings().output_phase_of(och)))
        }
        SrcMs => Some(Message::SourceMs(cid, !state.device.settings().source_ms_of(cid))),
        OutMs => {
            let ChannelId::Output(ch) = cid else { return None };
            let pair = crate::app::output_ms_pair(state, ch)?;
            Some(Message::OutputMs(pair, !state.device.settings().output_ms_of(pair)))
        }
        Eq => Some(Message::ToggleFlyout(cid, FlyoutKind::Eq)),
        // Its two clicks stay two presses: the first arms it.
        Phantom => input.map(Message::PhantomPress),
        Pad => input.map(|i| Message::Pad(i, !p.pad)),
        An12 => Some(Message::An12Changed(!state.device.settings().an12)),
        RefLevel => adjust_panel(state, item, 1),
        Sensitivity => input.map(|i| Message::Sensitivity(i, !p.sensitivity_plus4)),
        MicPreset => {
            let idx = input?;
            let n = state.kb_preset_pick.or_else(|| current_preset(state, idx)).unwrap_or(0);
            (n < state.mic_presets.len()).then_some(Message::MicPresetLoad(idx, n))
        }
        Copy => Some(Message::MixCopy(cid)),
        Paste => Some(Message::MixPaste(cid)),
        Mirror => crate::app::mirror_message(state, cid),
        Clear => Some(Message::MixClear(cid)),
        StopMirror => Some(Message::MixMirrorStop(cid)),
        Hide => Some(Message::HideStrip(cid)),
        Flow => Some(if crate::flow::open_for(state, cid) {
            Message::FlowClose
        } else {
            Message::FlowOpen(crate::flow::of_strip(state, cid))
        }),
    }
}

/// The preset input `idx` is set to now, as the panel's menu names it.
fn current_preset(state: &TuxMix, idx: usize) -> Option<usize> {
    state.mic_presets.iter().position(|p| crate::app::mic_preset_matches(state, idx, p))
}

fn adjust_panel(state: &mut TuxMix, item: PanelItem, dir: i32) -> Option<Message> {
    let cid = panel_strip(state)?;
    // Reverb and Echo's knobs: a dB a step, off at the bottom.
    let step = |db: f32, top: f32| {
        let to = (db + dir as f32).clamp(crate::fx::OFF_DB, top);
        (to != db).then_some(to)
    };
    match (item, cid) {
        (PanelItem::FxReturn, ChannelId::Output(ch)) => {
            let pair = crate::mixops::output_pair(state, ch);
            let db = crate::fx::vol_db(crate::fx::user_return(state, pair));
            return step(db, 6.0).map(|to| Message::FxReturn(pair, crate::fx::db_vol(to)));
        }
        (PanelItem::FxSend, ChannelId::Input(0 | 1)) => {
            return step(crate::fx::send_db(state), 0.0).map(Message::FxSend);
        }
        (PanelItem::FxSend, _) if state.device.has_source_fx_send(cid) => {
            let db = state.device.settings().source_fx_send_of(cid);
            return step(db, 0.0).map(|to| Message::SourceFxSend(cid, to));
        }
        // Width: 0.05 a step, as a turn of the knob.
        (PanelItem::SrcWidth, _) => {
            let w = state.device.settings().source_width_of(cid);
            let to = ((w + 0.05 * dir as f32) * 100.0).round() / 100.0;
            let to = to.clamp(-1.0, 1.0);
            return (to != w).then_some(Message::SourceWidth(cid, to));
        }
        _ => {}
    }
    let ChannelId::Input(idx) = cid else {
        return None;
    };
    let p = crate::app::strip_params(state, cid, state.sel_out);
    match item {
        // A dB a step, as the knob's own steps.
        PanelItem::Gain => {
            let to = (p.gain as i64 + dir as i64).clamp(0, p.gain_max as i64) as u32;
            (to != p.gain).then_some(Message::Gain(idx, to))
        }
        // +4dBu, -10dBV, Boost, round.
        PanelItem::RefLevel => {
            let current = match state.device.inputs().get(idx).map_or(0, |c| c.ref_level) {
                0 if p.sensitivity_plus4 => 1,
                0 => 2,
                raw => raw,
            };
            let next = ((current as i32 - 1 + dir).rem_euclid(3) + 1) as u16;
            Some(Message::RefLevel(idx, next))
        }
        // Which preset Enter loads; nothing changes until then.
        PanelItem::MicPreset => {
            let n = state.mic_presets.len() as i32;
            if n > 0 {
                let at = state.kb_preset_pick.or_else(|| current_preset(state, idx)).map_or(-1, |i| i as i32);
                state.kb_preset_pick = Some((at + dir).rem_euclid(n) as usize);
            }
            None
        }
        _ => None,
    }
}

/// A monitor level one wheel detent along the faders' taper, as the
/// counter's slider moves it.
fn level_nudge(state: &TuxMix, pair: usize, dir: f32) -> Option<Message> {
    let ch = if state.device.outputs_one_per_pair() { pair } else { pair * 2 };
    let now = crate::control_room::user_output_level(state, ch);
    let t = (crate::widgets::fader::vol_to_t(now) + dir * crate::app::NUDGE_STEP_T).clamp(0.0, 1.0);
    Some(Message::VolumeChanged(ChannelId::Output(ch), ch, crate::widgets::fader::t_to_vol(t)))
}

/// What the Matrix cell at `row`, `col` sets: its channel, the pair it
/// sends to, and its level now.
fn matrix_cell(state: &TuxMix, row: Option<ChannelId>, col: usize) -> Option<(ChannelId, usize, f32)> {
    match row {
        Some(cid) => {
            let pair = col / 2;
            Some((cid, pair, state.device.volume(cid, pair).unwrap_or(0.0)))
        }
        None => {
            let out = state.device.outputs().get(col)?;
            Some((ChannelId::Output(col), 0, out.volume))
        }
    }
}

fn matrix_nudge(state: &TuxMix, row: Option<ChannelId>, col: usize, dir: f32) -> Option<Message> {
    let (cid, pair, now) = matrix_cell(state, row, col)?;
    let t = (crate::widgets::fader::vol_to_t(now) + dir * crate::app::NUDGE_STEP_T).clamp(0.0, 1.0);
    Some(Message::MatrixSet(cid, pair, crate::widgets::fader::t_to_vol(t), false))
}

/// The Matrix's arrows: the cell by rows and columns, from the first.
pub fn matrix_arrow(state: &mut TuxMix, drow: i32, dcol: i32) {
    matrix_move(state, drow, dcol);
}

/// Moves the Matrix cell by rows and columns, starting at the first cell.
fn matrix_move(state: &mut TuxMix, drow: i32, dcol: i32) {
    let rows = crate::matrix::cursor_rows(state);
    let cols = crate::matrix::cursor_cols(state);
    if rows.is_empty() || cols.is_empty() {
        return;
    }
    let (r, c) = match state.key_focus {
        Some(KeyFocus::Matrix(row, col)) => (
            rows.iter().position(|&x| x == row).unwrap_or(0) as i32 + drow,
            cols.iter().position(|&x| x == col).unwrap_or(0) as i32 + dcol,
        ),
        _ => (0, 0),
    };
    let r = r.clamp(0, rows.len() as i32 - 1) as usize;
    let c = c.clamp(0, cols.len() as i32 - 1) as usize;
    state.key_focus = Some(KeyFocus::Matrix(rows[r], cols[c]));
}

/// Tab through the Matrix in reading order, round and round.
fn matrix_step(state: &mut TuxMix, step: i32, wrap: bool) {
    let rows = crate::matrix::cursor_rows(state);
    let cols = crate::matrix::cursor_cols(state);
    let (nr, nc) = (rows.len() as i32, cols.len() as i32);
    if nr == 0 || nc == 0 {
        return;
    }
    let at = match state.key_focus {
        Some(KeyFocus::Matrix(row, col)) => {
            let r = rows.iter().position(|&x| x == row).unwrap_or(0) as i32;
            let c = cols.iter().position(|&x| x == col).unwrap_or(0) as i32;
            r * nc + c
        }
        _ => if step > 0 { -1 } else { nr * nc },
    };
    let mut next = at + step;
    if wrap {
        next = next.rem_euclid(nr * nc);
    }
    let next = next.clamp(0, nr * nc - 1);
    state.key_focus = Some(KeyFocus::Matrix(rows[(next / nc) as usize], cols[(next % nc) as usize]));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{new, Message, View};
    use crate::widgets::strip::FlyoutKind;

    /// A mock state with the D panel of `cid` open and drawn (the "More"
    /// section shown or not), so the keyboard's stops are the panel's.
    fn with_the_d_panel(cid: ChannelId, more: bool) -> TuxMix {
        let mut state = new(true, None, None);
        state.view = View::Mixer;
        crate::app::open_flyout(&mut state, cid, FlyoutKind::Dynamics);
        let _ = crate::app::update(&mut state, Message::Dyn(cid, crate::dynamics_panel::DynMsg::More(more)));
        let _ = crate::app::view(&state, iced::window::Id::unique());
        state
    }

    #[test]
    fn a_key_that_renames_itself_keeps_the_keyboard() {
        // Critique #37: Turn on Voice pressed became Turn off Voice, the old
        // id was in no frame by the next key press, and the focus let go.
        let an1 = ChannelId::Input(0);
        let mut state = with_the_d_panel(an1, false);
        let turn_on = first_id(Region::Dyn(an1), "Turn on Voice");
        assert!(drawn_in(Region::Dyn(an1)).iter().any(|r| r.id == turn_on), "a stop while off");
        state.key_focus = Some(KeyFocus::Item(turn_on));
        let _ = crate::app::update(&mut state, Message::Dyn(an1, crate::dynamics_panel::DynMsg::Power(true)));
        // Several frames go by before the tick, as the window redraws.
        for _ in 0..3 {
            let _ = crate::app::view(&state, iced::window::Id::unique());
        }
        settle_focus(&mut state);
        let turn_off = first_id(Region::Dyn(an1), "Turn off Voice");
        assert_eq!(state.key_focus, Some(KeyFocus::Item(turn_off)), "on the key at its place, now Turn off Voice");
    }

    #[test]
    fn a_tick_before_the_frame_keeps_a_focus_just_placed() {
        // A handler puts the focus on a control the next frame draws; a tick
        // that comes first must leave it.
        let an1 = ChannelId::Input(0);
        let mut state = with_the_d_panel(an1, false);
        let placed = first_id(Region::Dyn(an1), "Not drawn yet");
        state.key_focus = Some(KeyFocus::Item(placed));
        let _ = crate::app::update(&mut state, Message::Tick);
        assert_eq!(state.key_focus, Some(KeyFocus::Item(placed)));
    }

    #[test]
    fn tab_meets_the_voice_card_before_the_sound_box_and_step_4_in_its_order() {
        // Critique #37: stops are reached in the order they're built.
        let an1 = ChannelId::Input(0);
        let _state = with_the_d_panel(an1, false);
        let names: Vec<String> = drawn_in(Region::Dyn(an1)).iter().map(|r| r.name.clone()).collect();
        let at = |n: &str| names.iter().position(|x| x == n).unwrap_or_else(|| panic!("{n} in {names:?}"));
        assert!(at("Turn on Voice") < at("Preset"), "the card's key, then the Sound box: {names:?}");
        let mut state = new(true, None, None);
        state.view = View::Quick;
        state.quick_channel = an1;
        let _ = crate::app::view(&state, iced::window::Id::unique());
        let names: Vec<String> = drawn_in(Region::Quick).iter().map(|r| r.name.clone()).collect();
        let at = |n: &str| names.iter().position(|x| x == n).unwrap_or_else(|| panic!("{n} in {names:?}"));
        assert!(at("Sound: Podcast") < at("Voice: shapes how others hear you"), "the sounds, then the switch: {names:?}");
    }

    #[test]
    fn a_control_that_is_gone_moves_the_focus_to_its_neighbour() {
        // Job 14: the keyboard must not sit on a control that is gone.
        let an2 = ChannelId::Input(1);
        // The D panel of AN2 is open while Voice runs on AN1: it shows a
        // card (critique #33's pick, "B") with "Move it to AN2".
        let mut state = with_the_d_panel(an2, false);
        state.dyn_input = Some(0);
        let _ = crate::app::view(&state, iced::window::Id::unique());
        // The card's "Move it to AN2" is a stop of the panel's.
        let move_it = first_id(Region::Dyn(an2), "Move it to AN2");
        assert!(drawn_in(Region::Dyn(an2)).iter().any(|r| r.id == move_it), "the card's key is a stop");
        state.key_focus = Some(KeyFocus::Item(move_it));
        // Pressed: Voice runs here, the card goes, and its keys with it.
        let _ = crate::app::update(&mut state, Message::Dyn(an2, crate::dynamics_panel::DynMsg::MoveHere));
        assert_eq!(state.dyn_input, Some(1), "Voice moved to AN2");
        let _ = crate::app::view(&state, iced::window::Id::unique());
        assert!(!drawn_in(Region::Dyn(an2)).iter().any(|r| r.id == move_it), "the card's key is gone");
        repair_focus(&mut state);
        let Some(KeyFocus::Item(id)) = state.key_focus else { panic!("the focus let go") };
        assert_ne!(id, move_it, "not the control that is gone");
        // And the focus is on a control that is there.
        assert!(drawn_in(Region::Dyn(an2)).iter().any(|r| r.id == id), "the neighbour is drawn");
    }

    #[test]
    fn a_region_that_is_gone_lets_the_focus_go() {
        // Job 14: with the panel closed there is nothing to stand on.
        let cid = ChannelId::Input(0);
        let mut state = with_the_d_panel(cid, false);
        let power = first_id(Region::Dyn(cid), "Power");
        state.key_focus = Some(KeyFocus::Item(power));
        // The panel closes: its controls are gone with it.
        crate::app::open_flyout(&mut state, cid, FlyoutKind::Settings);
        let _ = crate::app::view(&state, iced::window::Id::unique());
        repair_focus(&mut state);
        assert_eq!(state.key_focus, None, "the focus let go");
    }

    #[test]
    fn the_card_stays_quiet_about_a_control_that_is_gone() {
        // Job 14: the card names what the keyboard is on; a control that is
        // gone is not named.
        let cid = ChannelId::Input(0);
        let mut state = with_the_d_panel(cid, false);
        let power = first_id(Region::Dyn(cid), "Power");
        state.key_focus = Some(KeyFocus::Item(power));
        assert!(focus_words(&state).is_some(), "the card names it while it is there");
        // The panel closes: the control is gone.
        crate::app::open_flyout(&mut state, cid, FlyoutKind::Settings);
        let _ = crate::app::view(&state, iced::window::Id::unique());
        assert!(focus_words(&state).is_none(), "the card says nothing about a control that is gone");
    }
}
