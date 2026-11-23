//! The Control Room section: the monitoring outputs (Main, Phones) and the
//! functions that act on them (Dim, Recall, Talkback, External Input,
//! Mono, Mute FX), shown beside the hardware output row as in the Windows
//! mixer (docs/ui-spec.md §5).
//!
//! Which outputs are Main and the Phones, and where Cue plays, is the
//! Assign menu's (`Assign`, manual 21.4); by default Main is AN1/2 and
//! Phones PH3/4. DIM lowers Main host-side (`ControlRoom`); Cue, Talkback
//! and External Input are composed onto the hardware with raw crosspoint
//! writes while the model keeps the user's own mix. Mono needs cross
//! crosspoints from the driver, Mute FX needs Reverb/Echo.
//!
//! Solo rides the same engine (`set_solo`): it silences the other strips
//! in the submix being edited only, as the manual has it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use iced::widget::{button, column, container, row, text, tooltip};
use iced::{Element, Length};
use tuxmix_core::{ChannelId, Error, RmeDevice, Scene};

use crate::app::{Message, TuxMix};
use crate::theme;

/// The Dim amount a fresh install starts from, in dB.
pub const DEFAULT_DIM_DB: f32 = -20.0;
/// The Recall (listening) level a fresh install starts from, in dB.
pub const DEFAULT_RECALL_DB: f32 = -20.0;
/// Range of the Dim amount knob (Settings, Main Out), in dB.
pub const DIM_RANGE: (f32, f32) = (-60.0, 0.0);
/// Range of the Recall level knob, in dB: up to the hardware's +6 dB.
pub const RECALL_RANGE: (f32, f32) = (-60.0, 6.0);
/// How far Talkback lowers everything else on the Phones, by default.
pub const DEFAULT_TALKBACK_DIM_DB: f32 = -20.0;
/// The talkback mic a fresh install uses: input 1 (AN1).
pub const DEFAULT_TALKBACK_INPUT: usize = 0;
/// Range of the Talkback dim knob, in dB.
pub const TALKBACK_DIM_RANGE: (f32, f32) = (-60.0, 0.0);
/// How far Listenback lowers the Main mix, by default.
pub const DEFAULT_LISTENBACK_DIM_DB: f32 = -20.0;
/// Range of the Listenback dim knob, in dB.
pub const LISTENBACK_DIM_RANGE: (f32, f32) = (-60.0, 0.0);
/// The stereo input External Input plays on Main by default: AS1/2.
pub const DEFAULT_EXT_IN_PAIR: usize = 2;
/// The External Input gain a fresh install uses, in dB.
pub const DEFAULT_EXT_IN_DB: f32 = 0.0;
/// Range of the External Input gain knob, in dB.
pub const EXT_IN_RANGE: (f32, f32) = (-60.0, 6.0);

/// What the Control Room overrides on top of the user's mix.
///
/// The unit cannot report levels back, so while Dim is engaged the app
/// keeps the user's own Main levels and composes the hardware value from
/// them: hardware = user level x dim gain. A fader move while dimmed
/// changes the user level; releasing Dim writes the user levels back. They
/// are also persisted (`prefs`) while dimmed, so a quit, crash or lost
/// device never leaves Main stuck at the dimmed level.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlRoom {
    /// Dim amount in dB (negative).
    pub dim_db: f32,
    /// The stored listening level RECALL sets Main to, in dB.
    pub recall_db: f32,
    /// The user's Main levels (L, R) while Dim is engaged, `None` otherwise.
    pub dimmed_main: Option<[f32; 2]>,
    /// The output pair whose submix plays on the Cue target (`Assign::cue_target`), if Cue is on.
    /// Its hardware crosspoints are composed with raw writes, so the
    /// model keeps the user's own Phones submix throughout.
    pub cue: Option<usize>,
    /// Talkback: everything else on the Phones lowered by
    /// `talkback_dim_db`, and input `talkback_input` sent there.
    pub talkback: bool,
    pub talkback_input: usize,
    pub talkback_dim_db: f32,
    /// Output pairs flagged Talkback (manual 21.3, an output's Settings):
    /// they get Talkback like the Phones.
    pub talkback_outputs: Vec<usize>,
    /// Listenback (manual 21.8.1): the recording-room mic
    /// `listenback_input` plays on the monitor pair, and the Main mix there
    /// is lowered by `listenback_dim_db`. No mic chosen (the manual's
    /// default), no Listenback.
    pub listenback: bool,
    pub listenback_input: Option<usize>,
    pub listenback_dim_db: f32,
    /// External Input: Main plays input pair `ext_in_pair` at `ext_in_db`
    /// instead of the mix.
    pub ext_in: bool,
    pub ext_in_pair: usize,
    pub ext_in_db: f32,
    /// Soloed input and playback channels (both channels of a linked
    /// pair). The manual (p. 49): "Solo is always activated for the
    /// current submix only."
    pub soloed: HashSet<ChannelId>,
    /// The output pair whose submix carries the solo: the submix being
    /// edited while anything is soloed, `None` otherwise.
    pub solo_pair: Option<usize>,
    /// Live Mode (manual 21.7): the S buttons do PFL, pre-fader listening
    /// on the Cue target, instead of Solo.
    pub live_mode: bool,
    /// Channels in PFL (both channels of a linked pair). While any is, the
    /// Cue target plays only them, at 0 dB before their faders.
    pub pfl: HashSet<ChannelId>,
    /// Which output pairs are Main, Main B and the Phones, and where Cue
    /// plays.
    pub assign: Assign,
    /// Speaker B (manual 21.4): the Main mix plays on Main B instead of
    /// Main. Only one of the two plays at a time.
    pub speaker_b: bool,
    /// Link: the Main and Main B master faders move together.
    pub speaker_b_link: bool,
    /// Mono (manual 21.4): the Main that plays folds left and right
    /// together, every source at (L+R)/2 on both sides
    /// (`RmeDevice::set_mono_out`; issue #11, decoded from a USB capture).
    pub mono: bool,
    /// The M/S key's check on the pair playing the Main mix, and that pair's
    /// own M/S before it (the owner's choice, 2026-11-09: a check like Dim,
    /// on until pressed again and never kept in a snapshot or at quit).
    pub ms_check: Option<(usize, bool)>,
    /// Mute FX (manual 21.4): the FX return is off on these output pairs
    /// (Main, and Main B where there is one), and these are your levels to
    /// bring back. Kept in the prefs while on, as Dim's are, so a crash
    /// never leaves the reverb muted for good. `None` while off.
    pub fx_muted: Option<Vec<(usize, f32)>>,
    /// Dim letting go (`release_dim_gently`): its amount easing back to
    /// 0 dB, with `dimmed_main` keeping your levels until it lands, so the
    /// fader, a save or a second press never take a level part-way up.
    pub dim_easing: Option<DimEase>,
    /// Output pairs whose mix eases back after Talkback, Listenback or
    /// External Input let go (`start_easing`): what each source held there
    /// at the press. Every write to such a pair eases its rising cells
    /// (`cell_level`); drops land at once.
    pub easing: BTreeMap<usize, TargetEase>,
}

/// How long letting go of Dim, Talkback, Listenback or External Input
/// takes to bring the sound back (the owner's pick, 2026-11-11: "Glide
/// back"; TotalMix jumps back at once, Dim by up to 60 dB). Even in dB.
pub(crate) const EASE_FOR: Duration = Duration::from_millis(500);

/// The most one step may move an ease on, as a share of the way: about
/// one and a half of the fast tick's 33 ms. A late tick then takes a
/// longer glide, never a bigger step: 60 dB × 0.099 is under the app's
/// 6 dB (`app::RAMP_OVER_DB`).
const EASE_STEP_MAX: f32 = 0.099;

/// Dim easing back up: since when, how far down it started (dB), and how
/// far along it is (0 to 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimEase {
    pub since: Instant,
    pub from_db: f32,
    pub progress: f32,
}

/// One output pair's mix easing back: since when, the level each source
/// held there when the override let go, and how far along it is (0 to 1).
#[derive(Debug, Clone, PartialEq)]
pub struct TargetEase {
    pub since: Instant,
    pub held: HashMap<ChannelId, f32>,
    pub progress: f32,
    /// The key that let go (its label, as `motion::Bloom::Key` names it):
    /// its light follows this glide and no other (OpenCode's job 35:
    /// matched by time, two keys let go together lit each other).
    pub by: &'static str,
}

/// An ease's next share of the way: the time gone since it started, but
/// never more than `EASE_STEP_MAX` past where it was.
pub(crate) fn ease_progress(since: Instant, was: f32, now: Instant) -> f32 {
    let by_time = now.saturating_duration_since(since).as_secs_f32() / EASE_FOR.as_secs_f32();
    by_time.min(was + EASE_STEP_MAX).clamp(0.0, 1.0)
}

/// A level on its way from `held` up to `to`, `t` of the way (0 to 1),
/// even in dB, and from no more than 60 dB under `to`. A level at or
/// under `held` is a drop: it lands at once.
fn eased(held: f32, to: f32, t: f32) -> f32 {
    // A drop, a level that isn't a number, or the end: the level itself.
    if to.partial_cmp(&held) != Some(std::cmp::Ordering::Greater) || t >= 1.0 {
        return to;
    }
    let to_db = 20.0 * to.log10();
    let floor = to_db - 60.0;
    let from_db = if held > 0.0 { (20.0 * held.log10()).max(floor) } else { floor };
    10f32.powf((from_db + (to_db - from_db) * t) / 20.0)
}

/// Where Cue plays: the manual's "Cue/PFL to" (21.4), "usually Main", or
/// one of the Phones. Audra starts on Phones 1, where Cue has always
/// played here: a mix being built is checked on headphones first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum CueTo {
    Main,
    #[default]
    Phones1,
    Phones2,
}

/// The Assign menu (manual 21.4): the output pair that is Main, Main B
/// (for Speaker B), and up to two Phones. Dim, Recall, External Input and
/// the unit's DIM button act on the monitor pair (Main, or Main B while
/// Speaker B is on, see `monitor_pair`); Talkback on every Phones. An
/// assigned pair moves into the Control Room and takes its role's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Assign {
    pub main: usize,
    /// Main Out B: the second monitor pair Speaker B switches to.
    #[serde(default)]
    pub main_b: Option<usize>,
    pub phones: [Option<usize>; 2],
    pub cue_to: CueTo,
}

impl Default for Assign {
    fn default() -> Self {
        Self {
            main: 0,
            main_b: None,
            phones: [Some(1), None],
            cue_to: CueTo::Phones1,
        }
    }
}

impl Assign {
    /// The Phones pairs, in order.
    pub fn phones_pairs(&self) -> Vec<usize> {
        self.phones.iter().flatten().copied().collect()
    }

    pub fn is_phones(&self, pair: usize) -> bool {
        self.phones.contains(&Some(pair))
    }

    /// The pair that plays the Main mix: Main, or Main B while Speaker B
    /// is on and Main B is assigned.
    pub fn monitor(&self, speaker_b: bool) -> usize {
        match self.main_b {
            Some(b) if speaker_b => b,
            _ => self.main,
        }
    }

    /// The pair Cue plays on: as chosen ("Main" meaning the monitor pair),
    /// or the other Phones, or the monitor pair when the chosen Phones is
    /// not assigned.
    pub fn cue_target_for(&self, speaker_b: bool) -> usize {
        let [p1, p2] = self.phones;
        match self.cue_to {
            CueTo::Main => None,
            CueTo::Phones1 => p1.or(p2),
            CueTo::Phones2 => p2.or(p1),
        }
        .unwrap_or(self.monitor(speaker_b))
    }

    /// `cue_target_for` with Speaker B off.
    pub fn cue_target(&self) -> usize {
        self.cue_target_for(false)
    }

    /// Clears every role `pair` holds besides Main.
    fn free(&mut self, pair: usize) {
        for slot in &mut self.phones {
            if *slot == Some(pair) {
                *slot = None;
            }
        }
        if self.main_b == Some(pair) {
            self.main_b = None;
        }
    }

    /// `pair` as Main: a role it held before is given up.
    pub fn with_main(mut self, pair: usize) -> Self {
        self.free(pair);
        self.main = pair;
        self
    }

    /// Main B as `pair`, or unassigned. Main cannot be Main B too; a
    /// Phones role the pair held is given up.
    pub fn with_main_b(mut self, pair: Option<usize>) -> Self {
        if pair == Some(self.main) {
            return self;
        }
        if let Some(p) = pair {
            self.free(p);
        }
        self.main_b = pair;
        self
    }

    /// Phones `slot` (0 or 1) as `pair`, or unassigned. Main cannot be a
    /// Phones too; a pair moves out of the other Phones slot or Main B.
    pub fn with_phones(mut self, slot: usize, pair: Option<usize>) -> Self {
        if pair == Some(self.main) {
            return self;
        }
        if let Some(p) = pair {
            self.free(p);
        }
        if let Some(s) = self.phones.get_mut(slot) {
            *s = pair;
        }
        self
    }

    pub fn with_cue_to(mut self, to: CueTo) -> Self {
        self.cue_to = to;
        self
    }

    /// Rebuilt through the rules above, so a pair holds one role even when
    /// the value came from a hand-edited prefs file.
    pub fn normalized(self) -> Self {
        Self::default()
            .with_main(self.main)
            .with_phones(0, None)
            .with_phones(0, self.phones[0])
            .with_phones(1, self.phones[1])
            .with_main_b(self.main_b)
            .with_cue_to(self.cue_to)
    }

    /// The role `pair` plays, as its strip and the counter name it.
    pub fn role_name(&self, pair: usize) -> Option<&'static str> {
        if pair == self.main {
            return Some("Main");
        }
        if self.main_b == Some(pair) {
            return Some("Main B");
        }
        match self.phones {
            [Some(p), None] | [None, Some(p)] if p == pair => Some("Phones"),
            [Some(p), _] if p == pair => Some("Phones 1"),
            [_, Some(p)] if p == pair => Some("Phones 2"),
            _ => None,
        }
    }
}

impl Default for ControlRoom {
    fn default() -> Self {
        Self {
            dim_db: DEFAULT_DIM_DB,
            recall_db: DEFAULT_RECALL_DB,
            dimmed_main: None,
            cue: None,
            talkback: false,
            talkback_input: DEFAULT_TALKBACK_INPUT,
            talkback_dim_db: DEFAULT_TALKBACK_DIM_DB,
            talkback_outputs: Vec::new(),
            listenback: false,
            listenback_input: None,
            listenback_dim_db: DEFAULT_LISTENBACK_DIM_DB,
            ext_in: false,
            ext_in_pair: DEFAULT_EXT_IN_PAIR,
            ext_in_db: DEFAULT_EXT_IN_DB,
            soloed: HashSet::new(),
            solo_pair: None,
            live_mode: false,
            pfl: HashSet::new(),
            assign: Assign::default(),
            speaker_b: false,
            speaker_b_link: true,
            mono: false,
            ms_check: None,
            fx_muted: None,
            dim_easing: None,
            easing: BTreeMap::new(),
        }
    }
}

impl ControlRoom {
    /// Dim is on. While it eases back after being let go, it reads off
    /// (the key, the unit's light), though Main is still coming up.
    pub fn is_dimmed(&self) -> bool {
        self.dimmed_main.is_some() && self.dim_easing.is_none()
    }

    /// The linear gain Dim applies to Main: its amount, or where its ease
    /// back has got to.
    pub fn dim_gain(&self) -> f32 {
        let db = self.dim_easing.map_or(self.dim_db, |e| e.from_db * (1.0 - e.progress));
        10f32.powf(db / 20.0)
    }
}

/// The output pair the Main mix plays on: Main, or Main B while Speaker B
/// is on. Dim, Recall, External Input and the unit's DIM button act here.
pub fn monitor_pair(state: &TuxMix) -> usize {
    state.control_room.assign.monitor(state.control_room.speaker_b)
}

/// The output channels of pair `pair`: two per pair on the ALSA backend,
/// one per pair on the USB backend (see `Message::StripClicked`).
pub(crate) fn pair_channels(state: &TuxMix, pair: usize) -> Vec<usize> {
    if state.device.outputs_one_per_pair() {
        vec![pair]
    } else {
        vec![pair * 2, pair * 2 + 1]
    }
}

/// The output channels Dim and Recall act on: the monitor pair's.
pub fn main_channels(state: &TuxMix) -> Vec<usize> {
    pair_channels(state, monitor_pair(state))
}

/// Which side of Main (0 = left, 1 = right) output channel `ch` is.
fn main_side(state: &TuxMix, ch: usize) -> Option<usize> {
    main_channels(state).iter().position(|&c| c == ch)
}

/// Whether Main moves as one level: a linked pair (one write sets both
/// channels, see `RmeDevice::set_output_volume`) or a backend with one
/// output channel per pair.
fn main_is_one_level(state: &TuxMix) -> bool {
    state.device.outputs_one_per_pair() || state.device.output_linked(monitor_pair(state))
}

/// With Link on, the output channel whose master moves with `ch`'s: the
/// same side of the other of Main and Main B.
pub fn linked_master(state: &TuxMix, ch: usize) -> Option<usize> {
    let cr = &state.control_room;
    let b = cr.assign.main_b.filter(|_| cr.speaker_b_link)?;
    let (main, main_b) = (pair_channels(state, cr.assign.main), pair_channels(state, b));
    if let Some(side) = main.iter().position(|&c| c == ch) {
        main_b.get(side).copied()
    } else {
        main_b
            .iter()
            .position(|&c| c == ch)
            .and_then(|side| main.get(side).copied())
    }
}

/// The level output `ch` shows on its fader: the user's own level while
/// Main is dimmed, otherwise what the device holds.
pub fn displayed_level(state: &TuxMix, ch: usize, device_level: f32) -> f32 {
    match (state.control_room.dimmed_main, main_side(state, ch)) {
        (Some(levels), Some(side)) => levels[side],
        _ => device_level,
    }
}

/// A fader write of `user` to output `ch`: for a dimmed Main, records it as
/// the user's level and returns the dimmed value for the hardware;
/// otherwise returns `user` unchanged.
pub fn compose_output_write(state: &mut TuxMix, ch: usize, user: f32) -> f32 {
    let Some(side) = main_side(state, ch) else {
        return user;
    };
    let one_level = main_is_one_level(state);
    let gain = state.control_room.dim_gain();
    match state.control_room.dimmed_main.as_mut() {
        Some(levels) => {
            if one_level {
                *levels = [user, user];
            } else {
                levels[side] = user;
            }
            state.prefs_dirty = true;
            user * gain
        }
        None => user,
    }
}

/// Output channel `ch`'s own level, as the user set it: the remembered one
/// while Dim holds a Main side down, else what the device has.
/// Whether output channel `ch` is one the user listens on: the Main, Main
/// B or a Phones pair. A reset never turns these up (`app::reset_target`,
/// `resetmix`).
pub(crate) fn listened_on(state: &TuxMix, ch: usize) -> bool {
    let assign = state.control_room.assign;
    let pair = crate::mixops::output_pair(state, ch);
    pair == assign.main || Some(pair) == assign.main_b || assign.is_phones(pair)
}

pub fn user_output_level(state: &TuxMix, ch: usize) -> f32 {
    let dimmed = main_side(state, ch).and_then(|side| state.control_room.dimmed_main.map(|levels| levels[side]));
    dimmed.unwrap_or_else(|| state.device.outputs().get(ch).map_or(0.0, |o| o.volume))
}

/// Engages or releases Dim on Main.
/// The Main channels to write: a linked Main once, since that write sets
/// both channels; a split Main side by side.
pub(crate) fn write_channels(state: &TuxMix) -> Vec<usize> {
    let mut channels = main_channels(state);
    if main_is_one_level(state) {
        channels.truncate(1);
    }
    channels
}

/// The user's own Main level (left side): the remembered one while dimmed.
fn main_user_level(state: &TuxMix) -> f32 {
    match state.control_room.dimmed_main {
        Some(levels) => levels[0],
        None => main_channels(state)
            .first()
            .and_then(|&c| state.device.outputs().get(c))
            .map_or(0.0, |o| o.volume),
    }
}

pub fn set_dim(state: &mut TuxMix, on: bool) {
    // A Recall still gliding up stops: Dim, or any key that releases the
    // overrides (`release` comes through here), takes the Main from it.
    state.level_ramps.clear();
    let channels = write_channels(state);
    match (on, state.control_room.dimmed_main) {
        (true, None) => {
            let level = |c: usize| state.device.outputs().get(c).map_or(0.0, |o| o.volume);
            let left = level(channels[0]);
            let right = channels.get(1).map_or(left, |&c| level(c));
            let user = [left, right];
            let gain = state.control_room.dim_gain();
            state.control_room.dimmed_main = Some(user);
            for (side, &c) in channels.iter().enumerate() {
                crate::app::write_output_volume(state, c, user[side] * gain);
            }
        }
        // Pressed again while it eases back: down again at once, with your
        // levels as they were (they never left `dimmed_main`).
        (true, Some(user)) if state.control_room.dim_easing.is_some() => {
            state.control_room.dim_easing = None;
            let gain = state.control_room.dim_gain();
            for (side, &c) in channels.iter().enumerate() {
                crate::app::write_output_volume(state, c, user[side] * gain);
            }
        }
        // Let go by anything but the Dim key (`release`: a snapshot, Undo,
        // an Assign change, quit): your levels at once, an ease included,
        // so nothing is left part-way up.
        (false, Some(user)) => {
            state.control_room.dimmed_main = None;
            state.control_room.dim_easing = None;
            for (side, &c) in channels.iter().enumerate() {
                crate::app::write_output_volume(state, c, user[side]);
            }
        }
        _ => return,
    }
    state.prefs_dirty = true;
}

/// Lets go of Dim gently: the Dim key, the unit's DIM button, a remote's
/// (all `Message::DimToggle`). Main eases back up over `EASE_FOR` instead
/// of jumping up by the Dim amount (the owner's pick, 2026-11-11); the key
/// reads off at once. Everything else that lets go of Dim lands at once
/// (`set_dim`).
pub fn release_dim_gently(state: &mut TuxMix, now: Instant) {
    if !state.control_room.is_dimmed() {
        return;
    }
    let from_db = state.control_room.dim_db;
    if from_db >= 0.0 {
        set_dim(state, false);
        return;
    }
    state.control_room.dim_easing = Some(DimEase { since: now, from_db, progress: 0.0 });
    state.prefs_dirty = true;
}

/// Something is easing back (the tick keeps its fast pace while it does).
pub fn is_easing(state: &TuxMix) -> bool {
    state.control_room.dim_easing.is_some() || !state.control_room.easing.is_empty()
}

/// The next step of every ease, on the tick: Dim's amount on Main, and the
/// rising cells of each pair whose mix eases back. A finished ease lands on
/// the exact level and lets go.
pub fn step_easing(state: &mut TuxMix, now: Instant) {
    match (state.control_room.dim_easing, state.control_room.dimmed_main) {
        (Some(ease), Some(user)) => {
            let progress = ease_progress(ease.since, ease.progress, now);
            if progress >= 1.0 {
                set_dim(state, false);
            } else {
                state.control_room.dim_easing = Some(DimEase { progress, ..ease });
                let gain = state.control_room.dim_gain();
                for (side, &c) in write_channels(state).iter().enumerate() {
                    crate::app::write_output_volume(state, c, user[side] * gain);
                }
            }
        }
        (Some(_), None) => state.control_room.dim_easing = None,
        _ => {}
    }
    let targets: Vec<usize> = state.control_room.easing.keys().copied().collect();
    for target in targets {
        let Some(ease) = state.control_room.easing.get_mut(&target) else {
            continue;
        };
        let progress = ease_progress(ease.since, ease.progress, now);
        if progress >= 1.0 {
            state.control_room.easing.remove(&target);
            let _ = write_target(state, target);
        } else {
            ease.progress = progress;
            write_rising(state, target);
        }
    }
}

/// Tests: every ease run to its end, a step at a time.
#[cfg(test)]
pub(crate) fn settle_easing(state: &mut TuxMix) {
    let start = Instant::now();
    for k in 1..=40u32 {
        if !is_easing(state) {
            break;
        }
        step_easing(state, start + EASE_FOR * k / 10);
    }
}

/// Starts easing `targets` back: what each source holds there now (an
/// ease under way included) is where its rise starts from. Called just
/// before an override lets go of them, so the release's own write already
/// eases (`cell_level`).
fn start_easing(state: &mut TuxMix, targets: &[usize], now: Instant, by: &'static str) {
    for &target in targets {
        let held = sources(state).into_iter().map(|c| (c, cell_level(state, c, target).0)).collect();
        state.control_room.easing.insert(target, TargetEase { since: now, held, progress: 0.0, by });
    }
}

/// The glides on `targets` end without a write of their own: an override
/// pressed there writes the pair whole, at once, as a press does (OpenCode's
/// job 21: Cue on a mix still gliding back rose eased).
fn stop_easing(state: &mut TuxMix, targets: &[usize]) {
    for t in targets {
        state.control_room.easing.remove(t);
    }
}

/// Every ease landed at once: `release` (a snapshot, Undo, an Assign
/// change, quit) leaves nothing part-way.
fn land_easing(state: &mut TuxMix) {
    let targets: Vec<usize> = std::mem::take(&mut state.control_room.easing).into_keys().collect();
    let _ = write_targets(state, &targets);
}

/// A Solo or PFL key about to let go (issue #37, the recommended pick):
/// the pair the solo holds, or the Cue target PFL holds, starts easing, so
/// what it silenced glides back up as Dim's does instead of all at once.
/// Where the press doesn't end up releasing anything, the ease runs to the
/// levels already there and changes nothing.
pub fn ease_solo_release(state: &mut TuxMix, now: Instant) {
    if state.control_room.live_mode {
        if !state.control_room.pfl.is_empty() {
            let target = cue_target(state);
            start_easing(state, &[target], now, "PFL");
        }
    } else if let Some(pair) = state.control_room.solo_pair {
        start_easing(state, &[pair], now, "SOLO");
    }
}

/// Cue let go by its key: the Cue target's own mix eases back (issue #37).
pub fn ease_cue_release(state: &mut TuxMix, now: Instant) {
    if state.control_room.cue.is_some() {
        let target = cue_target(state);
        start_easing(state, &[target], now, "CUE");
    }
}

/// Talkback let go gently (its key): the Phones' mix eases back up.
pub fn talkback_off_gently(state: &mut TuxMix, now: Instant) -> Result<(), Error> {
    if !state.control_room.talkback {
        return Ok(());
    }
    let targets = talkback_pairs(state);
    start_easing(state, &targets, now, "TALKBACK");
    set_talkback(state, false)
}

/// External Input let go gently (its key): the monitor pair's mix eases
/// back up.
pub fn ext_in_off_gently(state: &mut TuxMix, now: Instant) -> Result<(), Error> {
    if !state.control_room.ext_in {
        return Ok(());
    }
    start_easing(state, &[monitor_pair(state)], now, "EXT IN");
    set_ext_in(state, false)
}

/// Listenback let go gently (its key): the monitor pair's mix eases back
/// up.
pub fn listenback_off_gently(state: &mut TuxMix, now: Instant) -> Result<(), Error> {
    if !state.control_room.listenback {
        return Ok(());
    }
    start_easing(state, &[monitor_pair(state)], now, "LISTEN");
    set_listenback(state, false)
}

/// Sets the Dim amount; a dimmed Main follows it: down at once, up more
/// than 6 dB gliding (OpenCode's job 41: turning the knob to 0 while dimmed
/// put Main up by as much as 60 dB in one write).
pub fn set_dim_db(state: &mut TuxMix, db: f32) {
    state.control_room.dim_db = db.clamp(DIM_RANGE.0, DIM_RANGE.1);
    if let Some(user) = state.control_room.dimmed_main {
        let gain = state.control_room.dim_gain();
        let now = std::time::Instant::now();
        let to_db = |v: f32| 20.0 * v.max(1e-6).log10();
        for (side, &c) in write_channels(state).iter().enumerate() {
            let to = user[side] * gain;
            let now_level = state.device.outputs().get(c).map_or(0.0, |o| o.volume);
            if to_db(to) > to_db(now_level) + crate::app::RAMP_OVER_DB as f32 {
                state.level_ramps.insert(c, (to_db(now_level).max(-60.0), to_db(to), now));
            } else {
                state.level_ramps.remove(&c);
                crate::app::write_output_volume(state, c, to);
            }
        }
    }
    state.prefs_dirty = true;
}

/// Sets the stored listening level RECALL goes to.
pub fn set_recall_db(state: &mut TuxMix, db: f32) {
    state.control_room.recall_db = db.clamp(RECALL_RANGE.0, RECALL_RANGE.1);
    state.prefs_dirty = true;
}

/// Stores Main's current level (the user's own, also while dimmed) as the
/// Recall level.
pub fn store_recall_from_main(state: &mut TuxMix) {
    let level = main_user_level(state);
    let db = if level > 0.0 {
        20.0 * level.log10()
    } else {
        RECALL_RANGE.0
    };
    set_recall_db(state, db.round());
}

/// RECALL: releases Dim, then sets Main to the stored listening level.
pub fn recall(state: &mut TuxMix) {
    // Dimmed, or easing back: Dim ends without writing your level back
    // first, so Main goes from where it is to the Recall level, never up
    // to your level and down again when Recall is the lower one.
    if state.control_room.dimmed_main.take().is_some() {
        state.control_room.dim_easing = None;
        state.prefs_dirty = true;
    }
    // The other overrides let go held: what they lowered eases back up
    // (OpenCode's job 44: Talkback's Phones came back in one write).
    release_holding(state, std::time::Instant::now());
    let level = 10f32.powf(state.control_room.recall_db / 20.0).min(2.0);
    let to_db = 20.0 * level.log10();
    let now = std::time::Instant::now();
    for c in write_channels(state) {
        let mut channels = vec![c];
        // Linked, Main and Main B keep the same level.
        channels.extend(linked_master(state, c));
        for ch in channels {
            // A rise of more than `RAMP_OVER_DB` glides up (the reviewer,
            // 2026-11-04: Recall put up to +6 dB on Main at once); a drop
            // lands at once.
            let now_db = state.device.outputs().get(ch).map_or(-120.0, |o| 20.0 * o.volume.max(1e-6).log10());
            if to_db > now_db + crate::app::RAMP_OVER_DB as f32 {
                state.level_ramps.insert(ch, (now_db.max(-60.0), to_db, now));
            } else {
                crate::app::write_output_volume(state, ch, level);
            }
        }
    }
}

/// The Dim key: on at once, off gently (`release_dim_gently`).
pub fn toggle_dim(state: &mut TuxMix) {
    if state.control_room.is_dimmed() {
        release_dim_gently(state, Instant::now());
    } else {
        set_dim(state, true);
    }
}

/// Every source channel feeding the matrix: inputs, then playbacks.
fn sources(state: &TuxMix) -> Vec<ChannelId> {
    let inputs = state.device.inputs().len();
    let playbacks = state.device.playbacks().len();
    (0..inputs)
        .map(ChannelId::Input)
        .chain((0..playbacks).map(ChannelId::Playback))
        .collect()
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The output pair Cue plays on ("Main" meaning the monitor pair).
pub fn cue_target(state: &TuxMix) -> usize {
    let cr = &state.control_room;
    cr.assign.cue_target_for(cr.speaker_b)
}

/// The output pairs whose hardware carries an override rather than the
/// user's own mix: the Cue target while Cue plays, every Phones while
/// Talkback is on, the monitor pair with External Input, the soloed
/// submix, and with Main B assigned whichever of Main and Main B is not
/// playing, plus Main B while it plays Main's mix.
/// The Control Room writes `pair`'s cells itself, composed: a send or pan
/// set there goes to the model alone first (`app::write_send`).
pub(crate) fn holds(state: &TuxMix, pair: usize) -> bool {
    overridden_pairs(state).contains(&pair)
}

fn overridden_pairs(state: &TuxMix) -> Vec<usize> {
    let cr = &state.control_room;
    let mut pairs = Vec::new();
    if cr.cue.is_some() {
        pairs.push(cue_target(state));
    }
    if cr.talkback {
        pairs.extend(talkback_pairs(state));
    }
    if cr.ext_in || cr.listenback {
        pairs.push(monitor_pair(state));
    }
    pairs.extend(cr.solo_pair);
    if !cr.pfl.is_empty() {
        pairs.push(cue_target(state));
    }
    if let Some(b) = cr.assign.main_b {
        pairs.push(b);
        if cr.speaker_b {
            pairs.push(cr.assign.main);
        }
    }
    // A pair easing back is still held: a fader move there is written
    // eased (`after_mix_change`), not at full until the next tick.
    pairs.extend(cr.easing.keys().copied());
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// The pair whose submix `pair` plays when that is not its own: the cued
/// output on the Cue target, Main on Main B while Speaker B is on. A mix
/// change there has to be carried over (`after_mix_change`).
fn plays_from(state: &TuxMix, pair: usize) -> Option<usize> {
    let cr = &state.control_room;
    match cr.cue {
        Some(cued) if pair == cue_target(state) => Some(cued),
        _ if cr.speaker_b && cr.assign.main_b == Some(pair) => Some(cr.assign.main),
        _ => None,
    }
}

/// The output pairs Talkback acts on: every Phones, and every output
/// flagged Talkback in its Settings (manual 21.3).
pub fn talkback_pairs(state: &TuxMix) -> Vec<usize> {
    let cr = &state.control_room;
    let mut pairs = cr.assign.phones_pairs();
    pairs.extend(cr.talkback_outputs.iter().copied());
    pairs.sort_unstable();
    pairs.dedup();
    pairs
}

/// What Talkback does to a Phones mix, in plain words, while it's on:
/// shown under that Phones' level (the scout, 2026-11-12: Dim says so
/// under Main, Talkback said nothing where you listen).
pub fn talkback_words(state: &TuxMix) -> String {
    let mic = talkback_mic(state);
    let db = state.control_room.talkback_dim_db;
    // The knob's bottom is -60 dB, not silence (Qwen's review): the number,
    // always, and nothing claimed at 0 dB. Who "the rest" is, said
    // (critique #38: for a podcaster it's their guest).
    let rest = if db > -0.5 {
        "everything else as it was".to_string()
    } else {
        format!("everything else {:.0} dB quieter", -db)
    };
    format!("Talkback is on: {mic} talks into these headphones, and {rest}.")
}

/// Talkback's few words beside the Phones' name (its sentence on hover:
/// `talkback_words`), short enough to sit beside the level's number.
pub fn talkback_short(state: &TuxMix) -> String {
    // A long mic name would be cut off beside the number (OpenCode's job
    // 53): then the sentence on hover says who.
    let mic = talkback_mic(state);
    if mic.chars().count() > 6 {
        "Talkback on".to_string()
    } else {
        // From the talker's side, who pressed the key (critique #95 A
        // P2-3: "you hear AN1" described the headphones' side).
        format!("Talkback: {mic} talks in here")
    }
}

fn talkback_mic(state: &TuxMix) -> String {
    crate::app::strip_display_name(state, ChannelId::Input(state.control_room.talkback_input))
}

/// Levels by source, then by output pair: [left, right].
pub type SideLevels = Vec<Vec<[f32; 2]>>;

/// The [left, right] level every source sends into every output pair as
/// the hardware carries it, inputs then playbacks, each indexed by pair:
/// the user's mix, with a muted source silent, except on the pairs an
/// override holds (`overridden_pairs`), where it is the override
/// (`composed`). The output meters are summed from this, so they show what
/// actually plays: the cued mix on the Phones, the Talkback dip, a solo,
/// External Input, Speaker B.
pub fn heard_side_levels(state: &TuxMix) -> (SideLevels, SideLevels) {
    let overridden = overridden_pairs(state);
    // A playback pair's channel plays its own side under the pair's
    // balance; an optical input pair's channel its own side as it is.
    let sides = |mono: bool, ch: ChannelId, id: usize, (level, pan): (f32, i8)| -> [f32; 2] {
        let own = match ch {
            ChannelId::Playback(_) => tuxmix_core::channel::balance_side(id, level, pan),
            _ => level,
        };
        if mono {
            let (l, r) = tuxmix_core::channel::pan_split_power(level, pan);
            [l, r]
        } else if id.is_multiple_of(2) {
            [own, 0.0]
        } else {
            [0.0, own]
        }
    };
    let heard = |ch: ChannelId, mono: bool, id: usize, mute: bool, own: Vec<[f32; 2]>| {
        own.into_iter()
            .enumerate()
            .map(|(pair, lr)| {
                if overridden.contains(&pair) {
                    sides(mono, ch, id, composed(state, ch, pair))
                } else if mute {
                    [0.0, 0.0]
                } else {
                    lr
                }
            })
            .collect::<Vec<_>>()
    };
    let inputs = state
        .device
        .inputs()
        .iter()
        .enumerate()
        .map(|(i, c)| heard(ChannelId::Input(i), c.is_mono(), c.id, c.mute, c.side_levels()))
        .collect();
    let playbacks = state
        .device
        .playbacks()
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let own = (0..c.volumes.len())
                .map(|pair| sides(false, ChannelId::Playback(i), c.id, (c.volumes[pair], state.device.playback_balance(i, pair))))
                .collect();
            heard(ChannelId::Playback(i), false, c.id, c.mute, own)
        })
        .collect();
    (inputs, playbacks)
}

/// The (level, pan) the hardware should carry for `ch` into `target`.
///
/// - The monitor pair with Listenback on: the Listenback mic at its own
///   fader in Main's submix (0 dB when that fader is off), everything else
///   there lowered by the Listenback dim, External Input included.
/// - The monitor pair with External Input on: only the chosen stereo
///   input, left channel hard left and right hard right, at the External
///   Input gain.
/// - Main and Main B (when assigned): only the monitor pair plays, and it
///   plays Main's mix; the other is silent (Speaker B, manual 21.4).
/// - The Cue target plays the cued output's submix while Cue is on.
/// - Every Talkback pair (the Phones and flagged outputs), with Talkback
///   on, is lowered by the Talkback dim, except the talkback mic itself,
///   which plays at its own fader in that pair's submix (manual 21.4), or
///   at 0 dB when that fader is off.
/// - A muted source is 0, the way the backends' own mute writes it, and
///   so is one silenced by another strip's solo in the solo's submix.
///   Solo is tested on the submix being played (`from`), so a Cue of the
///   soloed submix carries the solo too.
fn composed(state: &TuxMix, ch: ChannelId, target: usize) -> (f32, i8) {
    let cr = &state.control_room;
    let assign = cr.assign;
    let monitor = monitor_pair(state);
    let listen_here = cr.listenback && target == monitor && cr.listenback_input.is_some();
    if listen_here && cr.listenback_input.map(ChannelId::Input) == Some(ch) {
        let level = state.device.volume(ch, assign.main).unwrap_or(0.0);
        return (if level > 0.0 { level } else { 1.0 }, 0);
    }
    let listen_gain = if listen_here { db_to_gain(cr.listenback_dim_db) } else { 1.0 };
    if target == monitor && cr.ext_in {
        return match ch {
            ChannelId::Input(i) if i / 2 == cr.ext_in_pair => (
                db_to_gain(cr.ext_in_db) * listen_gain,
                if i % 2 == 0 { -100 } else { 100 },
            ),
            _ => (0.0, 0),
        };
    }
    let mut own = target;
    if let Some(b) = assign.main_b {
        if target == assign.main || target == b {
            if target != monitor {
                return (0.0, 0);
            }
            own = assign.main;
        }
    }
    let talkback_here = cr.talkback
        && (assign.is_phones(target) || cr.talkback_outputs.contains(&target));
    if talkback_here && ch == ChannelId::Input(cr.talkback_input) {
        let level = state.device.volume(ch, target).unwrap_or(0.0);
        return (if level > 0.0 { level } else { 1.0 }, 0);
    }
    // PFL (Live Mode, manual 21.7): the Cue target plays only the channels
    // in PFL, before their faders and mute, at 0 dB: a stereo pair's two
    // channels hard left and right, a mono channel in the middle. Talkback
    // still lowers it on a Phones.
    if !cr.pfl.is_empty() && target == cue_target(state) {
        if !cr.pfl.contains(&ch) {
            return (0.0, 0);
        }
        let level = if talkback_here { db_to_gain(cr.talkback_dim_db) } else { 1.0 };
        return (level, pfl_pan(state, ch));
    }
    let from = match cr.cue {
        Some(cued) if target == cue_target(state) => cued,
        _ => own,
    };
    let silent = state.device.mute(ch).unwrap_or(false)
        || (cr.solo_pair == Some(from) && !cr.soloed.is_empty() && !cr.soloed.contains(&ch));
    let mut level = if silent {
        0.0
    } else {
        state.device.volume(ch, from).unwrap_or(0.0)
    };
    if talkback_here {
        level *= db_to_gain(cr.talkback_dim_db);
    }
    // A playback channel carries its pair's balance in `from`, whichever
    // channel of the pair the backend keeps it on.
    let pan = match ch {
        ChannelId::Playback(i) => state.device.playback_balance(i, from),
        _ => state.device.pan(ch, from).unwrap_or(0),
    };
    (level * listen_gain, pan)
}

/// What `write_target` puts in one cell: the composed level, and while the
/// pair's mix eases back (`TargetEase`), a rising cell's level part-way up
/// from what it held when the override let go.
fn cell_level(state: &TuxMix, ch: ChannelId, target: usize) -> (f32, i8) {
    let (level, pan) = composed(state, ch, target);
    match state.control_room.easing.get(&target) {
        Some(ease) => (eased(ease.held.get(&ch).copied().unwrap_or(0.0), level, ease.progress), pan),
        None => (level, pan),
    }
}

/// An easing pair's rising cells only, on the tick: a step writes the few
/// cells that move, not the whole submix.
fn write_rising(state: &mut TuxMix, target: usize) {
    let Some(ease) = state.control_room.easing.get(&target) else {
        return;
    };
    let rising: Vec<ChannelId> = sources(state)
        .into_iter()
        .filter(|&c| composed(state, c, target).0 > ease.held.get(&c).copied().unwrap_or(0.0))
        .collect();
    for c in rising {
        let (level, pan) = cell_level(state, c, target);
        let _ = state.device.write_crosspoint_raw(c, target, level, pan);
    }
}

/// Writes `target`'s whole submix to the hardware as composed, leaving the
/// model alone (eased while its mix eases back, `cell_level`). Returns the
/// first error (a backend without raw writes).
fn write_target(state: &mut TuxMix, target: usize) -> Result<(), Error> {
    let srcs = sources(state);
    let cells: Vec<_> = srcs
        .iter()
        .map(|&c| (c, cell_level(state, c, target)))
        .collect();
    let mut result = Ok(());
    for (c, (level, pan)) in cells {
        let written = state.device.write_crosspoint_raw(c, target, level, pan);
        if result.is_ok() {
            result = written;
        }
    }
    result
}

/// Writes every pair in `targets`; returns the first error.
fn write_targets(state: &mut TuxMix, targets: &[usize]) -> Result<(), Error> {
    let mut result = Ok(());
    for &target in targets {
        let written = write_target(state, target);
        if result.is_ok() {
            result = written;
        }
    }
    result
}

/// Starts or stops Cue: `Some(pair)` plays that output's submix on the Cue
/// target; one Cue at a time, and cueing the target on itself is off.
pub fn set_cue(state: &mut TuxMix, pair: Option<usize>) -> Result<(), Error> {
    let target = cue_target(state);
    let pair = pair.filter(|&p| p != target);
    if state.control_room.cue == pair {
        return Ok(());
    }
    state.control_room.cue = pair;
    state.prefs_dirty = true;
    // Pressing acts at once, also on a mix still gliding back (job 21).
    if pair.is_some() {
        state.control_room.easing.remove(&target);
    }
    let result = write_target(state, target);
    if result.is_err() && pair.is_some() {
        // Nothing reached the hardware: don't claim a Cue that isn't on.
        state.control_room.cue = None;
    }
    result
}

/// Switches Talkback on or off, on every Phones.
pub fn set_talkback(state: &mut TuxMix, on: bool) -> Result<(), Error> {
    if state.control_room.talkback == on {
        return Ok(());
    }
    state.control_room.talkback = on;
    state.prefs_dirty = true;
    let phones = talkback_pairs(state);
    if on {
        stop_easing(state, &phones);
    }
    let result = write_targets(state, &phones);
    if result.is_err() && on {
        state.control_room.talkback = false;
    }
    result
}

/// Switches External Input on or off.
/// Switches Mono on the Main that plays (Main B while Speaker B is on).
pub fn set_mono(state: &mut TuxMix, on: bool) -> Result<(), Error> {
    let target = on.then(|| state.control_room.assign.monitor(state.control_room.speaker_b));
    state.device.set_mono_out(target)?;
    state.control_room.mono = on;
    Ok(())
}

/// Keeps Mono on the Main that plays after Speaker B or the Assign menu
/// moved it.
pub fn refold_mono(state: &mut TuxMix) {
    if !state.control_room.mono {
        return;
    }
    let target = Some(state.control_room.assign.monitor(state.control_room.speaker_b));
    if state.device.mono_out() != target {
        if let Err(e) = state.device.set_mono_out(target) {
            log::warn!("Mono did not follow the Main: {e}");
        }
    }
}

pub fn set_ext_in(state: &mut TuxMix, on: bool) -> Result<(), Error> {
    if state.control_room.ext_in == on {
        return Ok(());
    }
    state.control_room.ext_in = on;
    state.prefs_dirty = true;
    if on {
        stop_easing(state, &[monitor_pair(state)]);
    }
    let result = write_target(state, monitor_pair(state));
    if result.is_err() && on {
        state.control_room.ext_in = false;
    }
    result
}

/// Settings: the talkback mic.
pub fn set_talkback_input(state: &mut TuxMix, input: usize) {
    // While it plays, the change eases in (the level audit: the new mic came
    // in at once).
    if state.control_room.talkback {
        let phones = talkback_pairs(state);
        start_easing(state, &phones, std::time::Instant::now(), "SETTING");
    }
    state.control_room.talkback_input = input;
    state.prefs_dirty = true;
    if state.control_room.talkback {
        let phones = talkback_pairs(state);
        let _ = write_targets(state, &phones);
    }
}

/// Settings: how far Talkback lowers the rest of the Phones.
pub fn set_talkback_dim_db(state: &mut TuxMix, db: f32) {
    // While it plays, what the change raises eases up (held at the levels
    // before it), what it lowers lands (OpenCode's job 41).
    let phones = talkback_pairs(state);
    if state.control_room.talkback {
        start_easing(state, &phones, std::time::Instant::now(), "SETTING");
    }
    state.control_room.talkback_dim_db = db.clamp(TALKBACK_DIM_RANGE.0, TALKBACK_DIM_RANGE.1);
    state.prefs_dirty = true;
    if state.control_room.talkback {
        let _ = write_targets(state, &phones);
    }
}

/// An output's Talkback flag (its Settings, manual 21.3): it gets Talkback
/// like the Phones. The Phones always do, so they are not flagged.
pub fn set_talkback_output(state: &mut TuxMix, pair: usize, on: bool) {
    if state.control_room.assign.is_phones(pair) || pair >= state.device.output_pair_count() {
        return;
    }
    let had = state.control_room.talkback_outputs.contains(&pair);
    if on == had {
        return;
    }
    // While Talkback plays, that output eases to its new state (the level
    // audit: unflagged, its own mix came back up in one write).
    if state.control_room.talkback {
        start_easing(state, &[pair], std::time::Instant::now(), "SETTING");
    }
    let outputs = &mut state.control_room.talkback_outputs;
    if on {
        outputs.push(pair);
    } else {
        outputs.retain(|&p| p != pair);
    }
    state.prefs_dirty = true;
    if state.control_room.talkback {
        let _ = write_target(state, pair);
    }
}

/// Switches Listenback on or off; on needs a Listenback mic.
pub fn set_listenback(state: &mut TuxMix, on: bool) -> Result<(), Error> {
    let on = on && state.control_room.listenback_input.is_some();
    if state.control_room.listenback == on {
        return Ok(());
    }
    state.control_room.listenback = on;
    state.prefs_dirty = true;
    if on {
        stop_easing(state, &[monitor_pair(state)]);
    }
    let result = write_target(state, monitor_pair(state));
    if result.is_err() && on {
        state.control_room.listenback = false;
    }
    result
}

/// Settings: the Listenback mic, or none (which ends a Listenback on).
pub fn set_listenback_input(state: &mut TuxMix, input: Option<usize>) {
    let input = input.filter(|&i| i < state.device.inputs().len());
    // Its mic gone, Listenback lets go gently, as its key does; another mic
    // eases in (the level audit: both landed in one write).
    if input.is_none() {
        let _ = listenback_off_gently(state, std::time::Instant::now());
    } else if state.control_room.listenback {
        start_easing(state, &[monitor_pair(state)], std::time::Instant::now(), "SETTING");
    }
    state.control_room.listenback_input = input;
    state.prefs_dirty = true;
    if state.control_room.listenback {
        let _ = write_target(state, monitor_pair(state));
    }
}

/// Settings: how far Listenback lowers the Main mix.
pub fn set_listenback_dim_db(state: &mut TuxMix, db: f32) {
    if state.control_room.listenback {
        start_easing(state, &[monitor_pair(state)], std::time::Instant::now(), "SETTING");
    }
    state.control_room.listenback_dim_db = db.clamp(LISTENBACK_DIM_RANGE.0, LISTENBACK_DIM_RANGE.1);
    state.prefs_dirty = true;
    if state.control_room.listenback {
        let _ = write_target(state, monitor_pair(state));
    }
}

/// Settings: the stereo input External Input plays.
pub fn set_ext_in_pair(state: &mut TuxMix, pair: usize) {
    if state.control_room.ext_in {
        start_easing(state, &[monitor_pair(state)], std::time::Instant::now(), "SETTING");
    }
    state.control_room.ext_in_pair = pair;
    state.prefs_dirty = true;
    if state.control_room.ext_in {
        let _ = write_target(state, monitor_pair(state));
    }
}

/// Settings: the External Input gain.
pub fn set_ext_in_db(state: &mut TuxMix, db: f32) {
    if state.control_room.ext_in {
        start_easing(state, &[monitor_pair(state)], std::time::Instant::now(), "SETTING");
    }
    state.control_room.ext_in_db = db.clamp(EXT_IN_RANGE.0, EXT_IN_RANGE.1);
    state.prefs_dirty = true;
    if state.control_room.ext_in {
        let _ = write_target(state, monitor_pair(state));
    }
}

/// The Assign menu: a new Main, Phones or Cue target. Every override is
/// released first, so none is left on the outputs it used to act on (a
/// dimmed old Main, Talkback on old Phones); out-of-range pairs are
/// ignored.
pub fn set_assign(state: &mut TuxMix, assign: Assign) {
    let assign = assign.normalized();
    let pairs = state.device.output_pair_count();
    let valid = assign.main < pairs
        && assign.main_b.is_none_or(|b| b < pairs)
        && assign.phones.iter().flatten().all(|&p| p < pairs);
    if !valid || assign == state.control_room.assign {
        return;
    }
    let old = state.control_room.assign;
    // Held, as before a scene: Main glides back from where Dim had it, and
    // what the overrides lowered (a Main B leaving its role included)
    // eases up instead of one write (Dim let go 40 dB at once).
    release_holding(state, std::time::Instant::now());
    state.control_room.assign = assign;
    if assign.main_b.is_none() {
        state.control_room.speaker_b = false;
    }
    state.prefs_dirty = true;
    // Main and Main B, old and new: a pair that left the roles plays its
    // own mix again, a new Main B starts silent.
    let mut touched = vec![old.main, assign.main];
    touched.extend(old.main_b);
    touched.extend(assign.main_b);
    touched.sort_unstable();
    touched.dedup();
    let _ = write_targets(state, &touched);
}

/// Whether `ch`'s strip is soloed. Outputs have no solo.
pub fn is_soloed(state: &TuxMix, ch: ChannelId) -> bool {
    state.control_room.soloed.contains(&ch)
}

/// `ch` and, when its pair is linked, its stereo partner: the channels one
/// strip's solo covers. Outputs give nothing.
fn solo_channels(state: &TuxMix, ch: ChannelId) -> Vec<ChannelId> {
    match ch {
        ChannelId::Input(i) if state.device.input_pair_linked(i / 2) => {
            vec![ch, ChannelId::Input(i ^ 1)]
        }
        ChannelId::Playback(c) if state.device.playback_linked(c / 2) => {
            vec![ch, ChannelId::Playback(c ^ 1)]
        }
        ChannelId::Output(_) => Vec::new(),
        _ => vec![ch],
    }
}

/// Solos (`on`) or releases the strips `channels`. With `exclusive`, a solo
/// first releases every other strip, so only this action's strips (one
/// strip, or a group or selection soloed together) stay soloed. Nothing
/// changes when nothing reached the hardware.
pub fn set_solo(
    state: &mut TuxMix,
    channels: &[ChannelId],
    on: bool,
    exclusive: bool,
) -> Result<(), Error> {
    let before = state.control_room.soloed.clone();
    let action: HashSet<ChannelId> = channels
        .iter()
        .flat_map(|&c| solo_channels(state, c))
        .collect();
    let mut soloed = before.clone();
    if on && exclusive {
        soloed.retain(|c| action.contains(c));
    }
    for c in action {
        if on {
            soloed.insert(c);
        } else {
            soloed.remove(&c);
        }
    }
    if soloed == before {
        return Ok(());
    }
    state.control_room.soloed = soloed;
    let result = sync_solo(state, true);
    if result.is_err() {
        state.control_room.soloed = before;
        let _ = sync_solo(state, true);
    }
    result
}

/// Puts the solo on the submix being edited and writes the hardware: the
/// pair it left goes back to the user's mix, the pair it carries is
/// composed. Called when the submix changes; `force` also rewrites a pair
/// that did not move, for a change to the soloed strips.
pub fn sync_solo(state: &mut TuxMix, force: bool) -> Result<(), Error> {
    let want = (!state.control_room.soloed.is_empty()).then_some(state.sel_out);
    let old = state.control_room.solo_pair;
    if want == old && !force {
        return Ok(());
    }
    // The solo moving to another mix (another mix selected while it's on):
    // the pair it leaves eases back to its whole mix, as a let-go override
    // does (the level audit: every strip the solo had silenced came back in
    // one write). A solo let go by a key eases too: the key's handler
    // starts it first (`ease_solo_release`, issue #37).
    if let (Some(o), Some(w)) = (old, want) {
        if o != w {
            start_easing(state, &[o], Instant::now(), "SOLO");
        }
    }
    state.control_room.solo_pair = want;
    state.prefs_dirty = true;
    let mut targets: Vec<usize> = [old, want].into_iter().flatten().collect();
    targets.dedup();
    // A Cue of a pair the solo entered or left plays it on the Cue target.
    if let Some(cued) = state.control_room.cue {
        if targets.contains(&cued) {
            targets.push(cue_target(state));
        }
    }
    write_targets(state, &targets)
}

/// A channel's pan while in PFL: a linked pair's left channel hard left and
/// right hard right, a mono channel in the middle.
fn pfl_pan(state: &TuxMix, ch: ChannelId) -> i8 {
    let linked = match ch {
        ChannelId::Input(i) => state.device.input_pair_linked(i / 2),
        ChannelId::Playback(c) => state.device.playback_linked(c / 2),
        ChannelId::Output(_) => false,
    };
    match ch {
        ChannelId::Input(i) | ChannelId::Playback(i) if linked => {
            if i % 2 == 0 {
                -100
            } else {
                100
            }
        }
        _ => 0,
    }
}

/// Whether `ch`'s strip is in PFL.
pub fn is_pfl(state: &TuxMix, ch: ChannelId) -> bool {
    state.control_room.pfl.contains(&ch)
}

/// PFL on (`on`) or off for the strips `channels`, as `set_solo` does
/// solo: with `exclusive` ("Exclusive Solo/PFL", manual 21.7), a PFL first
/// releases every other. Writes the Cue target; nothing changes when
/// nothing reached the hardware.
pub fn set_pfl(
    state: &mut TuxMix,
    channels: &[ChannelId],
    on: bool,
    exclusive: bool,
) -> Result<(), Error> {
    let before = state.control_room.pfl.clone();
    let action: HashSet<ChannelId> = channels
        .iter()
        .flat_map(|&c| solo_channels(state, c))
        .collect();
    let mut pfl = before.clone();
    if on && exclusive {
        pfl.retain(|c| action.contains(c));
    }
    for c in action {
        if on {
            pfl.insert(c);
        } else {
            pfl.remove(&c);
        }
    }
    if pfl == before {
        return Ok(());
    }
    state.control_room.pfl = pfl;
    state.prefs_dirty = true;
    let result = write_target(state, cue_target(state));
    if result.is_err() {
        state.control_room.pfl = before;
        let _ = write_target(state, cue_target(state));
    }
    result
}

/// Live Mode on or off (manual 21.7: "PFL replaces Solo"). Whatever Solo
/// or PFL was on is released first, so a switch never leaves either
/// behind where the S buttons no longer show it.
pub fn set_live_mode(state: &mut TuxMix, on: bool) {
    if state.control_room.live_mode == on {
        return;
    }
    if !state.control_room.soloed.is_empty() {
        state.control_room.soloed.clear();
        let _ = sync_solo(state, true);
    }
    if !state.control_room.pfl.is_empty() {
        state.control_room.pfl.clear();
        let _ = write_target(state, cue_target(state));
    }
    state.control_room.live_mode = on;
    state.prefs_dirty = true;
}

/// After a mix change into output pair `out` (`None`: a mute, which
/// touches every pair), keeps overridden pairs in step: a model write into
/// an overridden pair's own crosspoints rewrote the hardware there, and
/// the cued submix may have moved. A change to an overridden pair's own
/// mix also changes what prefs must keep.
pub fn after_mix_change(state: &mut TuxMix, out: Option<usize>) {
    for pair in overridden_pairs(state) {
        let own = out.is_none_or(|o| o == pair);
        let follows = out.is_some() && plays_from(state, pair) == out;
        if own || follows {
            let _ = write_target(state, pair);
        }
        if own {
            state.prefs_dirty = true;
        }
    }
}

/// Writes every overridden pair again: after a scene, snapshot, undo or
/// reconnect rewrote the hardware from the model, and at startup. The
/// overrides `release` ends are off by then; what stays is Main B (silent,
/// or playing Main's mix with Speaker B on).
pub fn reapply(state: &mut TuxMix) {
    let pairs = overridden_pairs(state);
    let _ = write_targets(state, &pairs);
}

/// Speaker B: the Main mix moves from Main to Main B, or back. A Dim that
/// was on moves with it, and the Cue target follows when Cue plays on
/// "Main".
///
/// The order keeps both Mains at the dimmed level throughout (the scout,
/// 2026-11-12: Dim let go first put the old Main back to your level while
/// its mix still played, and the new Main played its mix at full level
/// until Dim came back, each up by the Dim amount for the writes that move
/// the mix). The Main that takes the mix is lowered first; the one that
/// loses it gets its own level back only once its mix is silent. A glide
/// back carries on, on the Main that plays now (OpenCode's job 21).
pub fn set_speaker_b(state: &mut TuxMix, on: bool) {
    let assign = state.control_room.assign;
    let Some(b) = assign.main_b else {
        return;
    };
    if state.control_room.speaker_b == on {
        return;
    }
    // Your levels while Dim is on or gliding back, and the gain it has got
    // to (`dim_easing` stays as it is).
    let held = state.control_room.dimmed_main;
    let gain = state.control_room.dim_gain();
    // A Recall gliding up stops, as Dim's own changes stop it.
    state.level_ramps.clear();
    let old = write_channels(state);
    state.control_room.speaker_b = on;
    state.prefs_dirty = true;
    if held.is_some() {
        let new = write_channels(state);
        let level = |c: usize| state.device.outputs().get(c).map_or(0.0, |o| o.volume);
        let left = level(new[0]);
        let user = [left, new.get(1).map_or(left, |&c| level(c))];
        for (side, &c) in new.iter().enumerate() {
            crate::app::write_output_volume(state, c, user[side] * gain);
        }
        state.control_room.dimmed_main = Some(user);
    }
    let mut pairs = vec![
        assign.main,
        b,
        assign.cue_target_for(false),
        assign.cue_target_for(true),
    ];
    pairs.sort_unstable();
    pairs.dedup();
    let _ = write_targets(state, &pairs);
    // The Main that lost the mix is silent now: its own level back.
    if let Some(user) = held {
        for (side, &c) in old.iter().enumerate() {
            crate::app::write_output_volume(state, c, user[side]);
        }
    }
}

/// Link: the Main and Main B master faders move together.
pub fn set_speaker_b_link(state: &mut TuxMix, on: bool) {
    state.control_room.speaker_b_link = on;
    state.prefs_dirty = true;
}

/// The user's own submix into `target` (inputs then playbacks, level and
/// pan), from the model.
fn user_mix(state: &TuxMix, target: usize) -> Vec<(f32, i8)> {
    sources(state)
        .iter()
        .map(|&c| {
            (
                state.device.volume(c, target).unwrap_or(0.0),
                state.device.pan(c, target).unwrap_or(0),
            )
        })
        .collect()
}

/// The user's own submix in every overridden pair (Cue, Talkback, External
/// Input, solo), for prefs. After a crash the ALSA controls hold the
/// override, which the next start would otherwise read back as the
/// user's mix.
pub fn mixes_to_save(state: &TuxMix) -> Vec<(usize, Vec<(f32, i8)>)> {
    overridden_pairs(state)
        .into_iter()
        .map(|pair| (pair, user_mix(state, pair)))
        .collect()
}

/// Puts every override back to the user's mix, solo and PFL included: before a
/// scene, snapshot, undo or redo is applied, and on quit.
pub fn release(state: &mut TuxMix) {
    let_go(state);
    land_easing(state);
}

/// Lets go of every override as `release` does, for a scene or a Total
/// Reset that writes next, with nothing coming back louder in one write
/// (OpenCode's job 44: letting go put Main back at your level, and the
/// cells Talkback, a solo or the Cue held down back at theirs, a moment
/// before the scene's or the reset's own glides took them down again;
/// 40 dB up with Dim at -40). Main comes back as a glide from where Dim
/// holds it (`level_ramps`, which `keep_levels` reads as your level); the
/// held cells stay held, as eases, for `apply_mix_scene_from` to start
/// its glides from (`held_share`) or a Total Reset to silence. Not for
/// quit: no tick follows to finish them.
pub fn release_holding(state: &mut TuxMix, now: Instant) {
    let held: Vec<usize> = overridden_pairs(state)
        .into_iter()
        .filter(|t| !state.control_room.easing.contains_key(t))
        .collect();
    start_easing(state, &held, now, "SETTING");
    let user = state.control_room.dimmed_main.take();
    state.control_room.dim_easing = None;
    let_go(state);
    if let Some(user) = user {
        let db = |v: f32| 20.0 * v.max(1e-6).log10();
        for (side, &c) in write_channels(state).iter().enumerate() {
            let at = state.device.outputs().get(c).map_or(0.0, |o| o.volume);
            if db(user[side]) > db(at) + crate::app::RAMP_OVER_DB as f32 {
                state.level_ramps.insert(c, (db(at).max(-60.0), db(user[side]), now));
            } else {
                crate::app::write_output_volume(state, c, user[side]);
            }
        }
        state.prefs_dirty = true;
    }
}

/// How much of a cell's level the card plays now: 1, or less while an
/// ease still brings it back up.
pub(crate) fn held_share(state: &TuxMix, ch: ChannelId, target: usize) -> f32 {
    let full = composed(state, ch, target).0;
    if full <= 0.0 {
        return 1.0;
    }
    (cell_level(state, ch, target).0 / full).clamp(0.0, 1.0)
}

/// Every ease ends with no write of its own: a scene has just written
/// every cell, its rises gliding from where the eases had them.
pub(crate) fn drop_easing(state: &mut TuxMix) {
    state.control_room.easing.clear();
}

/// What `release` and `release_holding` share: every override off, each
/// written as it lets go (eased where a pair eases).
fn let_go(state: &mut TuxMix) {
    set_dim(state, false);
    set_ms_check(state, false);
    // Mute FX too: its pairs are Main's, which an Assign change moves, and
    // no override stays on the unit after Audra lets go.
    crate::fx::set_mute(state, false);
    let _ = set_cue(state, None);
    let _ = set_talkback(state, false);
    let _ = set_ext_in(state, false);
    let _ = set_listenback(state, false);
    if !state.control_room.soloed.is_empty() {
        state.control_room.soloed.clear();
        let _ = sync_solo(state, true);
    }
    if !state.control_room.pfl.is_empty() {
        state.control_room.pfl.clear();
        let _ = write_target(state, cue_target(state));
    }
}

/// The M/S key (a check, as Dim is): on, the pair playing the Main mix
/// plays mid and side, its own M/S kept to come back; off, that comes back.
pub fn set_ms_check(state: &mut TuxMix, on: bool) {
    match (on, state.control_room.ms_check) {
        (true, None) => {
            let pair = state.control_room.assign.monitor(state.control_room.speaker_b);
            let before = state.device.settings().output_ms_of(pair);
            state.control_room.ms_check = Some((pair, before));
            let _ = update_ms(state, pair, true);
        }
        (false, Some((pair, before))) => {
            state.control_room.ms_check = None;
            let _ = update_ms(state, pair, before);
        }
        _ => {}
    }
}

fn update_ms(state: &mut TuxMix, pair: usize, on: bool) -> Option<()> {
    state.device.set_output_ms(pair, on).ok()
}

/// Startup recovery after a quit, crash or lost device with an override
/// on: Main levels saved while dimmed (on the assigned Main, so the
/// assignment is restored first), and the submixes saved while an override
/// played on them, are written back through the model.
///
/// A Dim left on stays on (issue #22, the owner's yes): Main comes back at
/// the dimmed level with the Dim key lit and a notice, and the person
/// brings it up, as Dim always does. Undimming here put the saved Main
/// back at once, +20 dB at the default Dim, after a crash or a logout,
/// with autostart with nobody at the screen. The mixes saved under an
/// override come back with any rise over `RAMP_OVER_DB` gliding.
pub fn recover(
    state: &mut TuxMix,
    dimmed_main: Option<[f32; 2]>,
    mixes: Vec<(usize, Vec<(f32, i8)>)>,
) {
    if let Some(user) = dimmed_main {
        state.control_room.dimmed_main = Some(user);
        let gain = state.control_room.dim_gain();
        let now = std::time::Instant::now();
        let db = |v: f32| 20.0 * v.max(1e-6).log10();
        for (side, &c) in write_channels(state).iter().enumerate() {
            // A start's rise on the USB backend (`restore_auto_scene_gently`)
            // would take Main up past the Dim: it rises to the dimmed level
            // instead, gliding if that's more than `RAMP_OVER_DB` up (the
            // level audit: written at once from the start's silence).
            state.level_ramps.remove(&c);
            let to = user[side] * gain;
            let cur = state.device.outputs().get(c).map_or(0.0, |o| o.volume);
            if db(to) > db(cur) + crate::app::RAMP_OVER_DB as f32 {
                state.level_ramps.insert(c, (db(cur).max(-60.0), db(to), now));
            } else {
                crate::app::write_output_volume(state, c, to);
            }
        }
        state.notice = Some((DIMMED_AT_START.to_string(), std::time::Instant::now()));
        state.prefs_dirty = true;
    }
    let now = std::time::Instant::now();
    for (target, mix) in mixes {
        for (&c, &(level, pan)) in sources(state).iter().zip(mix.iter()) {
            let _ = crate::app::write_send(state, target, |d| d.set_pan(c, target, pan));
            crate::app::set_send_gently(state, c, target, level, now);
        }
        state.prefs_dirty = true;
    }
}

/// What `recover` says when Audra comes back with Main still dimmed.
pub const DIMMED_AT_START: &str = "Audra came back dimmed, as it was. Press Dim to bring Main up.";

/// What loading a snapshot leaves as it is (manual 21.7, Preferences,
/// Snapshots "Do not load"). The manual's third choice, Control Room
/// settings, has nothing to act on here: Audra keeps those in its prefs,
/// never in a snapshot, so a snapshot cannot change them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SnapshotKeep {
    /// A snapshot loads everything it holds.
    #[default]
    Nothing,
    /// The Main (and Main B) master levels stay.
    Main,
    /// The Main, Main B and Phones master levels stay.
    MainPhones,
}

impl SnapshotKeep {
    pub const ALL: [SnapshotKeep; 3] = [Self::Nothing, Self::Main, Self::MainPhones];
}

impl std::fmt::Display for SnapshotKeep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Nothing => "Nothing (load it all)",
            Self::Main => "The Main volume",
            Self::MainPhones => "Main and Phones volumes",
        })
    }
}

/// Before a snapshot is applied: the master levels it must leave alone
/// (`keep`) are put into it from the device, so loading it keeps them.
/// Called after `release`, so a Dim is already undone and the device holds
/// the user's own levels.
pub fn keep_levels(state: &TuxMix, scene: &mut Scene, keep: SnapshotKeep) {
    let assign = state.control_room.assign;
    let mut pairs = match keep {
        SnapshotKeep::Nothing => return,
        SnapshotKeep::Main => vec![assign.main],
        SnapshotKeep::MainPhones => {
            let mut pairs = vec![assign.main];
            pairs.extend(assign.phones_pairs());
            pairs
        }
    };
    pairs.extend(assign.main_b);
    for pair in pairs {
        for c in pair_channels(state, pair) {
            if let (Some(kept), Some(now)) = (scene.outputs.get_mut(c), state.device.outputs().get(c)) {
                // A level gliding up is kept where it's going: Dim let go
                // before a scene (`release_holding`) glides back to yours.
                kept.volume = state
                    .level_ramps
                    .get(&c)
                    .map_or(now.volume, |&(_, to_db, _)| 10f32.powf(to_db / 20.0));
            }
        }
    }
}

/// A pick-list entry in the Control Room settings: an index and its label.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub index: usize,
    pub label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The hardware inputs, for the talkback mic picker.
pub fn input_choices(state: &TuxMix) -> Vec<Choice> {
    state
        .device
        .inputs()
        .iter()
        .enumerate()
        .map(|(index, ch)| Choice {
            index,
            label: ch.name.clone(),
        })
        .collect()
}

/// The stereo input pairs, for the External Input picker: "AN1/2",
/// "AS1/2", "ADAT3/4" and so on.
pub fn input_pair_choices(state: &TuxMix) -> Vec<Choice> {
    state
        .device
        .inputs()
        .chunks(2)
        .enumerate()
        .map(|(index, pair)| {
            let left = &pair[0].name;
            let label = match pair.get(1) {
                Some(right) => {
                    let common = left
                        .chars()
                        .zip(right.name.chars())
                        .take_while(|(a, b)| a == b)
                        .count();
                    format!("{left}/{}", &right.name[common..])
                }
                None => left.clone(),
            };
            Choice { index, label }
        })
        .collect()
}

/// The scene to store or push on the undo stack: the device's state with
/// Main at the user's own levels, never the dimmed ones.
pub fn capture_scene(state: &TuxMix) -> Scene {
    let mut scene = state.device.capture_scene();
    // The M/S key's check is not the mix: the pair's own M/S as it was.
    if let Some((pair, before)) = state.control_room.ms_check {
        scene.settings.keep_output_ms(pair, before);
    }
    if let Some(user) = state.control_room.dimmed_main {
        for (side, &c) in main_channels(state).iter().enumerate() {
            if let Some(out) = scene.outputs.get_mut(c) {
                out.volume = user[side];
            }
        }
    }
    // A send still gliding up: where it is going, not where it is. Undo
    // 300 ms after a mirror or a paste kept the half-risen mix for Redo.
    for (&(cid, out), g) in &state.fader_glides {
        let slot = match cid {
            ChannelId::Input(i) => scene.inputs.get_mut(i).and_then(|c| c.volumes.get_mut(out)),
            ChannelId::Playback(i) => scene.playbacks.get_mut(i).and_then(|c| c.volumes.get_mut(out)),
            ChannelId::Output(_) => None,
        };
        if let Some(v) = slot {
            *v = g.to;
        }
    }
    // Every pair's FX return, where there is one, so the scene holds them
    // all (an empty list is a scene from before the FX: it leaves them):
    // your levels, a glide's end and not its middle, Mute FX's kept level
    // and not its 0.
    if crate::fx::in_panels(state) {
        for (pair, volume) in crate::fx::returns_to_keep(state) {
            scene.settings.keep_fx_return(pair, volume);
        }
    }
    scene
}

/// Whether an output pair is shown in the Control Room instead of the
/// hardware output row: it is assigned as Main or a Phones.
pub fn is_control_room_pair(state: &TuxMix, pair: usize) -> bool {
    role_name(state, pair).is_some()
}

/// The role an output pair takes in the Control Room, shown on its strip
/// instead of its channel name (unless the "original" names option is on).
pub fn role_name(state: &TuxMix, pair: usize) -> Option<&'static str> {
    state.control_room.assign.role_name(pair)
}

/// The Control Room keys at the head of the right-hand column, four to a
/// row: the same set, in the same order and carved the same as the column
/// beside Main and the Phones (critique #12's P1, the owner's choice, "One
/// set, both places": the keys had moved between the two homes with
/// different sets). Cue is on each output strip, as in TotalMix.
pub fn keys(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    // Two to a row at the full key size (critique #13: four to a row took
    // the labels to 10.5px, under the type floor), in TotalMix's groups,
    // a wider gap between them: Dim to Mono, Talkback and Ext in, Mute FX
    // and Assign.
    let short_key = crate::app::head_tight(state);
    // Lower still in the column while a learning card has a short
    // window's slot, so the page keys stay in view (critique #28).
    let snug_key = !keys_in_mixer(state) && crate::sidebar::snug(state);
    let mut groups: Vec<Vec<Element<'static, Message>>> = vec![Vec::new()];
    for k in key_specs(state) {
        if (k.label == "TALKBACK" || k.label == "MUTE FX") && !groups.last().is_some_and(|g| g.is_empty()) {
            groups.push(Vec::new());
        }
        let focused = crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::Key(k.label));
        if let Some(group) = groups.last_mut() {
            let key_h = if snug_key {
                SNUG_KEY_H
            } else if short_key {
                SHORT_KEY_H
            } else {
                COLUMN_KEY_H
            };
            group.push(carved_key_h(state, k, focused, theme::TEXT_SM, key_h, scale));
        }
    }
    // On a short window the groups sit closer and the keys a little lower,
    // so the column's head keeps its page keys in view with a card in the
    // slot under it (Learning pass 1's "Next for you").
    let short = crate::app::head_tight(state);
    let mut col = column![].spacing(if short { theme::SPACE_SM } else { theme::SPACE_LG } * scale).width(Length::Fill);
    for group in groups {
        let mut rows = column![].spacing(if short { theme::SPACE_TIGHT } else { theme::SPACE_SM } * scale).width(Length::Fill);
        let mut it = group.into_iter().peekable();
        while it.peek().is_some() {
            // A group's last key alone on its row spans both columns (issue
            // #19, the owner's yes): beside an empty cell, Mid/Side read as
            // a key gone missing.
            let pair: Vec<Element<'static, Message>> = it.by_ref().take(2).collect();
            rows = rows.push(row(pair).spacing(theme::SPACE_SM * scale).width(Length::Fill));
        }
        col = col.push(rows);
    }
    col.into()
}

/// The Cough key's name: held, not pressed (`carved_key_h`).
pub const COUGH: &str = "COUGH (HOLD)";

/// One Control Room key as `keys` draws it and the keyboard reaches it
/// (`keyfocus`): its label, whether it is lit, what a press sends (none:
/// inert), and its tooltip.
pub struct KeySpec {
    pub label: &'static str,
    pub active: bool,
    pub on_press: Option<Message>,
    pub tip: &'static str,
}

/// The Control Room keys in the order they stand, wherever they stand
/// (`keys`, `key_column`), as the keyboard reaches them.
pub fn key_specs(state: &TuxMix) -> Vec<KeySpec> {
    column_specs(state)
}

/// Whether the keys stand beside Main and the Phones (the owner,
/// 2026-11-06, chose TotalMix's place for them): in the Mixer with its
/// outputs row open, the mixer fitting the window, on a wide window. On
/// Quick setup, in the Matrix, with the outputs row folded, where the
/// mixer scrolls, and on a window under `KEYS_BESIDE_MIN_W` wide, they
/// stay at the head of the right-hand column, which always keeps them in
/// view.
pub fn keys_in_mixer(state: &TuxMix) -> bool {
    let wide = state.window_size.is_none_or(|(w, _)| w / state.ui_scale.max(0.1) >= KEYS_BESIDE_MIN_W);
    // Window width alone decides (critique #14: a fold or a third row at
    // 1600 still moved them): on a wide window they stay with the Control
    // Room in the Mixer, its column when the outputs row is open, a compact
    // row in its band when folded (`key_band`).
    wide && state.view == crate::app::View::Mixer
}

/// The narrowest window (at scale 1) whose keys stand beside Main and the
/// Phones (critique #13, the owner's choice "By window size"): narrower,
/// one row shows at a time and every fold would move them, so they stay
/// in the right-hand column.
const KEYS_BESIDE_MIN_W: f32 = 1440.0;

/// The keys as TotalMix stands them in its Control Room column (the
/// owner's reference, 2026-11-06: "Dim, Recall, speaker b, mono, talk
/// back, ext, input, mute fx and assign"): Speaker B always there, greyed
/// until a Main B is assigned, and Assign opening the Control Room's
/// settings. Cue is on each output strip here, so it has no key.
fn column_specs(state: &TuxMix) -> Vec<KeySpec> {
    let cr = &state.control_room;
    let has_b = cr.assign.main_b.is_some();
    let fx = crate::fx::in_panels(state) || cr.fx_muted.is_some();
    let mut keys = vec![
        KeySpec { label: "DIM", active: cr.is_dimmed(), on_press: Some(Message::DimToggle),
            tip: "Turns your speakers down by the Dim amount at once. Press again and they come back up over half a second" },
        KeySpec { label: "RECALL", active: false, on_press: Some(Message::RecallMain),
            tip: "Set Main to your Recall level (Audra settings → Monitor)" },
        KeySpec { label: "SPEAKER B", active: cr.speaker_b, on_press: has_b.then_some(Message::SpeakerBToggle),
            tip: if has_b {
                "Speaker B: play the Main mix on Main B instead of Main"
            } else {
                "Speaker B needs a second pair of speakers: pick a Main B under Assign"
            } },
    ];
    // Link beside Speaker B, in its group (the reviewer).
    if has_b {
        keys.push(KeySpec { label: "LINK", active: cr.speaker_b_link, on_press: Some(Message::SpeakerBLinkToggle),
            tip: "Move the Main and Main B faders together" });
    }
    // Mono: the crossed crosspoints, which the updated driver reaches
    // (issue #11); an older driver keeps it greyed with the reason.
    let can_mono = state.device.can_mono();
    keys.push(KeySpec { label: "MONO", active: cr.mono, on_press: can_mono.then_some(Message::MonoToggle),
        tip: if can_mono {
            "Mono: Main plays left and right together, to check the mix in mono"
        } else {
            "Mono needs a newer Babyface driver (the README says how to install it)"
        } });
    // M/S beside Mono (the owner's choice from mockups, 2026-11-09): the
    // speakers' width checked as Mono checks them in mono, on the pair now
    // playing the Main mix. Its strip and the Now Mixing card say it too.
    let monitor = cr.assign.monitor(cr.speaker_b);
    let can_ms = state.device.can_output_proc() && !state.device.outputs_one_per_pair() && state.device.output_linked(monitor);
    let ms_on = cr.ms_check.is_some();
    // "Mid/Side", not "M/S": one row above the M S F keys and beside every
    // strip's M | S, it read as mute and solo (critique #20; the owner's
    // choice from mockups, 2026-11-10).
    keys.push(KeySpec { label: "Mid/Side", active: ms_on, on_press: can_ms.then_some(Message::MsCheckToggle),
        tip: if can_ms {
            "Mid/Side: Main plays the middle on the left and the sides on the right, to check the mix's width"
        } else {
            "Mid/Side needs the Babyface driver from 2026-11-09, and Main as a stereo pair"
        } });
    // Ext in beside Mid/Side (both change what Main plays), so Talkback and
    // Cough share a row and the keys keep five rows: a sixth hid the page
    // keys under a learning card at 1280x700 (critique #32's pick, "B").
    keys.push(KeySpec { label: "EXT IN", active: cr.ext_in, on_press: Some(Message::ExtInToggle),
        tip: "Play an external stereo input on Main instead of the mix; letting go brings the mix back gently (Audra settings → Monitor → More for studios)" });
    keys.push(KeySpec { label: "TALKBACK", active: cr.talkback, on_press: Some(Message::TalkbackToggle),
        tip: "Lower the Phones and send the talkback mic there; letting go brings the Phones back gently (Audra settings → Monitor → More for studios)" });
    // Cough beside Talkback (the owner's pick after critique #32, "B":
    // always on screen, where the D panel's foot was out of sight on
    // smaller screens). Held, not pressed: `carved_key_h` holds it while
    // the voice runs, greyed until then. The keyboard holds it with C.
    let live = crate::dynamics_panel::cough_live(state);
    keys.push(KeySpec { label: COUGH, active: state.cough && live, on_press: None,
        // As the mix to your apps stands (critique #108 A P2-2: it said the
        // stream went quiet while the stream had the plain mic).
        tip: if live && crate::app::plain_mic_looped(state, state.quick_channel).is_some() {
            "Hold to silence what others hear through Voice (\u{201c}Audra processed mic\u{201d}: a cough, a sneeze); you still hear yourself. The mix to your apps still has your plain mic, so a stream from it still hears you: press Stream again to send your shaped voice instead. Let go and you're back. Or hold C"
        } else if live {
            "Hold to silence what others hear through Voice (\u{201c}Audra processed mic\u{201d}: a cough, a sneeze), a stream that takes it included; you still hear yourself. Let go and you're back. Or hold C"
        } else {
            "Cough works while Voice runs (a mic strip's D, then ON; it needs the unit). Then hold it, or hold C, to silence what others hear"
        } });
    if cr.listenback_input.is_some() {
        keys.push(KeySpec { label: "LISTEN", active: cr.listenback, on_press: Some(Message::ListenbackToggle),
            tip: "Listenback: hear the recording-room mic on Main, the mix lowered; letting go brings it back gently (Audra settings → Monitor → More for studios)" });
    }
    keys.push(KeySpec { label: "MUTE FX", active: cr.fx_muted.is_some(), on_press: fx.then_some(Message::MuteFxToggle),
        tip: if fx {
            "Take Reverb and Echo off Main; your return levels come back when released"
        } else {
            "Reverb & Echo aren't available with this driver: the Reverb & Echo page says what they need"
        } });
    keys.push(KeySpec {
        label: ASSIGN,
        active: state.show_settings && state.settings_page == crate::app::SettingsPage::Monitor,
        on_press: Some(Message::OpenSettingsAt(crate::app::SettingsPage::Monitor)),
        tip: "Which outputs are Main, Main B and the Phones, and where Cue plays (Audra settings → Monitor)",
    });
    keys
}

/// The Assign key's label: it opens a dialog, so it ends in an ellipsis.
const ASSIGN: &str = "ASSIGN\u{2026}";

/// The keys' column beside Main and the Phones (`keys_in_mixer`): one key
/// under another, in TotalMix's groups (Dim to Mono; Talkback and External
/// Input; Mute FX; Assign).
pub fn key_column(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    // On a window under 1000 px tall, the short key: ten keys of 28 px and
    // their gaps needed about 348 px beside strips about 330 tall, and
    // squashed Assign to a sliver at 1440x900 (critique #36).
    let short = state.window_size.is_some_and(|(_, h)| h / state.ui_scale.max(0.1) < 1000.0);
    let key_h = if short { SHORT_KEY_H } else { COLUMN_KEY_H };
    let mut col = column![].width(Length::Fill);
    for (i, k) in key_specs(state).into_iter().enumerate() {
        if i > 0 {
            let group = k.label == "TALKBACK" || k.label == "MUTE FX" || k.label == ASSIGN;
            col = col.push(iced::widget::Space::new().height(Length::Fixed(
                if group { theme::SPACE_XL } else { theme::SPACE_SM } * scale,
            )));
        }
        let focused = crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::Key(k.label));
        col = col.push(carved_key_h(state, k, focused, theme::TEXT_SM, key_h, scale));
    }
    // On a panel of its own, a strip's (the owner, 2026-11-06: "these
    // floating buttons are not good"), as TotalMix's Control Room column
    // stands on the mixer's panel; as tall as the strips beside it.
    container(col)
        .padding(theme::SPACE_SM * scale)
        .width(Length::Fixed(KEY_COLUMN_W * scale))
        .height(Length::Fill)
        .style(theme::strip_panel(false, false, 0.0, false))
        .into()
}

/// The folded band's key that shows the rest, and hides them again.
pub const BAND_MORE: &str = "MORE \u{25be}";
pub const BAND_LESS: &str = "LESS \u{25b4}";

/// Whether the Control Room keys ride a folded outputs row's band (a wide
/// window, the outputs folded).
pub fn keys_in_band(state: &TuxMix) -> bool {
    keys_in_mixer(state) && !crate::app::floor_is_open(state, 2)
}

/// The keys pressed mid-call: the folded band always shows them.
fn mid_call(label: &str) -> bool {
    matches!(label, "DIM" | "TALKBACK" | COUGH)
}

/// The keys at the front of a folded band, after "More ▾": the ones
/// pressed mid-call (Dim, Talkback, Cough) and, while More is shut, any
/// other that's lit (the owner's pick after critique #33, "B").
pub fn band_front(state: &TuxMix) -> Vec<KeySpec> {
    key_specs(state).into_iter().filter(|k| mid_call(k.label) || (k.active && !state.band_more_open)).collect()
}

/// The rarely used keys, shown in the mixes' place while More is open
/// (critique #36: shown beside the mixes, they squeezed every mix chip but
/// one out of the band and slid Talkback under the pointer).
pub fn band_rare(state: &TuxMix) -> Vec<KeySpec> {
    if !state.band_more_open {
        return Vec::new();
    }
    key_specs(state).into_iter().filter(|k| !mid_call(k.label)).collect()
}

/// Every key a folded band shows, in its order after More.
pub fn band_specs(state: &TuxMix) -> Vec<KeySpec> {
    let mut keys = band_front(state);
    keys.extend(band_rare(state));
    keys
}

/// The keys in a folded outputs row's band on a wide window
/// (`keys_in_band`): "More ▾" first, so it never moves under the pointer
/// (critique #36), then the front keys.
pub fn key_band(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let mut band = row![].spacing(theme::SPACE_TIGHT * scale);
    let open = state.band_more_open;
    let more = KeySpec {
        label: if open { BAND_LESS } else { BAND_MORE },
        active: false,
        on_press: Some(Message::BandMore(!open)),
        tip: if open { "Fewer keys here: Dim, Talkback, Cough and any key that's on stay" } else { "Recall, Speaker B, Mono, Mid/Side, Ext in, Mute FX and Assign" },
    };
    let focused = crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::Key(more.label));
    band = band.push(container(carved_key(state, more, focused, theme::TEXT_SM, scale)).width(Length::Fixed(BAND_KEY_W * scale)));
    for k in band_front(state) {
        let focused = crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::Key(k.label));
        // Cough's words are the band's longest (critique #36: they touched
        // both rims in 74 px).
        let w = if k.label == COUGH { BAND_COUGH_W } else { BAND_KEY_W };
        band = band.push(container(carved_key(state, k, focused, theme::TEXT_SM, scale)).width(Length::Fixed(w * scale)));
    }
    band.into()
}

/// Cough's key in the band, at scale 1: its words need more than the rest.
pub const BAND_COUGH_W: f32 = 92.0;

/// The rarely used keys in the band, in the mixes' place while More is
/// open.
pub fn rare_band(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let mut band = row![].spacing(theme::SPACE_TIGHT * scale);
    for k in band_rare(state) {
        let focused = crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::Key(k.label));
        band = band.push(container(carved_key(state, k, focused, theme::TEXT_SM, scale)).width(Length::Fixed(BAND_KEY_W * scale)));
    }
    band.into()
}

/// A key's width in the folded band, at scale 1.
pub const BAND_KEY_W: f32 = 74.0;

/// The keys' column's width beside Main and the Phones, at scale 1, and a
/// key's height in it (well included).
pub const KEY_COLUMN_W: f32 = 96.0;
const COLUMN_KEY_H: f32 = 28.0;

/// A key of the column beside Main and the Phones, carved into its panel
/// as TotalMix's are (the owner: "Follow a bit closer to a carve out
/// button like in totalmix"): the strip's M|S treatment, a cut in the
/// panel (`key_well_round`) with the key sunk in it (`Lamp::inset`); lit,
/// its hue glows inside the cut. A key with nothing to do keeps its cut,
/// its label faded.
fn carved_key(state: &TuxMix, spec: KeySpec, focused: bool, text_size: f32, scale: f32) -> Element<'static, Message> {
    carved_key_h(state, spec, focused, text_size, COLUMN_KEY_H, scale)
}

/// A carved key's height on a short window's column head, at scale 1.
const SHORT_KEY_H: f32 = 24.0;
/// While a learning card has a short window's slot (`sidebar::snug`).
const SNUG_KEY_H: f32 = 22.0;

/// `carved_key`, `height` tall (scale 1, well included).
fn carved_key_h(state: &TuxMix, spec: KeySpec, focused: bool, text_size: f32, height: f32, scale: f32) -> Element<'static, Message> {
    let KeySpec { label, active, on_press, tip } = spec;
    // Cough is held: a press silences others, letting go (or leaving the
    // key) brings them back. Only while the voice runs; greyed otherwise.
    let hold = label == COUGH && crate::dynamics_panel::cough_live(state);
    let h = height * scale;
    let pad = theme::WELL_PAD * scale;
    let round = theme::RADIUS_SM * scale;
    let tile = button(
        text(key_words(label))
            .font(theme::SANS_SEMI)
            .size(text_size * scale)
            .wrapping(iced::advanced::text::Wrapping::None)
            .width(Length::Fill)
            .center(),
    )
    .padding([0.0, theme::SPACE_TIGHT * scale])
    .width(Length::Fill)
    .height(Length::Fixed(h - 2.0 * pad))
    .style({
        let lamp = theme::lamp_button(
            // Mute FX and Cough light in mute's hue, as every mute does.
            if label == "MUTE FX" || label == COUGH { theme::mute_color() } else { theme::accent() },
            theme::Lamp {
                active,
                bloom: crate::motion::bloom(state, crate::motion::Bloom::Key(label)),
                cool: crate::motion::cool(state, crate::motion::Bloom::Key(label)),
                inset: true,
                round,
                ..theme::Lamp::default()
            },
        );
        // A held key has no press of its own (the hold is around it), so
        // it's drawn as a live key, not a disabled one. Its rim firm enough
        // to read off the pixel grid (critique #57 B: top 2.85, foot 2.57
        // on Daylight, the sides 3.13).
        // The status mapped first, so a held key (Cough) gets the firm rim
        // too (Strata's review: it saw "disabled" and kept the faint one).
        // Split-safe now (critique #58 B: at 1920 and at 125% the 1px rim
        // fell across two rows, 2.1 to 2.5; firm only covers a tenth of a
        // pixel off the grid).
        let firm = theme::split_safe_key(lamp, active);
        // Mute FX holding Main, the pointer on Main's strip: the key glows,
        // the link the shared hue implies (critique #63's idea).
        move |t: &iced::Theme, st: iced::widget::button::Status| {
            firm(t, if hold && matches!(st, iced::widget::button::Status::Disabled) { iced::widget::button::Status::Active } else { st })
        }
    })
    .on_press_maybe(on_press);
    // Mute FX holding Main, the pointer on Main's strip: its well's edge
    // rings in the mute hue, the link the shared hue implies (critique #65
    // A 1: a shadow on the key never drew).
    let linked = label == "MUTE FX" && active && main_strip_hovered(state);
    let well_style = theme::key_well_round(round + pad);
    let well = container(tile).padding(pad).width(Length::Fill).style(move |t: &iced::Theme| {
        let mut s = well_style(t);
        if linked {
            s.border.color = theme::link_ring(true);
            s.border.width = 1.5 * scale;
        }
        s
    });
    let well: Element<'static, Message> = if label == "MUTE FX" {
        // Its pointer lights the FX key it holds on Main's strip.
        iced::widget::mouse_area(well).on_enter(Message::MuteFxHover(true)).on_exit(Message::MuteFxHover(false)).into()
    } else if hold {
        iced::widget::mouse_area(well)
            .on_press(Message::Cough(true))
            .on_release(Message::Cough(false))
            .on_exit(Message::Cough(false))
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
    } else {
        well.into()
    };
    let tipped = tooltip(
        well,
        container(text(tip).size(theme::TEXT_SM * scale).color(theme::text_primary()))
            .padding(theme::SPACE_SM * scale)
            .style(crate::widgets::hint_panel),
        tooltip::Position::Left,
    )
    .gap(4.0 * scale)
    .delay(crate::widgets::hint_delay());
    crate::keyfocus::ring(tipped.into(), focused, round + pad)
}

/// Whether the pointer is on the strip of a Main pair that Mute FX holds.
fn main_strip_hovered(state: &TuxMix) -> bool {
    let Some(ChannelId::Output(o)) = state.hovered_strip else { return false };
    let pair = if state.device.outputs_one_per_pair() { o } else { o / 2 };
    state.control_room.fx_muted.as_ref().is_some_and(|kept| kept.iter().any(|(p, _)| *p == pair))
}

/// A key's label as it's shown: in sentence case (2026-11-03, the owner's
/// reference: "Dim", "Talkback", not "DIM", "TALKBACK"), a lone letter kept
/// upper case ("Main B"). The key's name stays upper case inside Audra,
/// where it also names the key's motion (`motion::Bloom::Key`).
/// `key_words`, for the tests of other modules.
#[cfg(test)]
pub(crate) fn key_words_for_tests(label: &str) -> String {
    key_words(label)
}

pub(crate) fn key_words(label: &str) -> String {
    label
        .split(' ')
        .enumerate()
        .map(|(i, word)| {
            // A lone letter and FX stay upper case, and a word with a slash
            // as written ("Main B", "Mute FX", "Mid/Side"; critique #19
            // caught "M/s").
            if word.len() == 1 || word == "FX" || word.contains('/') {
                word.to_string()
            } else if i == 0 {
                let mut chars = word.chars();
                chars.next().map_or(String::new(), |c| c.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect())
            } else {
                word.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_playback_pairs_balance_is_what_the_output_meters_add_up() {
        use tuxmix_core::RmeDevice;
        let mut state = crate::app::new(true, None, None);
        let main = monitor_pair(&state);
        for c in [0, 1] {
            state.device.set_volume(ChannelId::Playback(c), main, 1.0).unwrap();
        }
        state.device.set_pan(ChannelId::Playback(0), main, 50).unwrap();
        let (_, pbs) = heard_side_levels(&state);
        assert!((pbs[0][main][0] - 0.5).abs() < 1e-6, "the left side lowered: {:?}", pbs[0][main]);
        assert_eq!(pbs[1][main], [0.0, 1.0], "the right side at the fader");
        // Straight Playback puts the pair back in the centre.
        crate::resetmix::apply(&mut state, crate::resetmix::ResetMix::StraightPlayback);
        assert_eq!(state.device.playback_balance(0, main), 0);
    }

    #[test]
    fn by_default_main_is_an1_2_and_the_phones_ph3_4() {
        let a = Assign::default();
        assert_eq!(a.role_name(0), Some("Main"));
        assert_eq!(a.role_name(1), Some("Phones"));
        for pair in 2..6 {
            assert_eq!(a.role_name(pair), None);
        }
        assert_eq!(a.cue_target(), 1, "Cue plays on the Phones");
    }

    #[test]
    fn a_pair_holds_one_role() {
        let a = Assign::default().with_main(1);
        assert_eq!((a.main, a.phones), (1, [None, None]), "PH3/4 left the Phones");
        assert_eq!(a.cue_target(), 1, "no Phones left: Cue falls back to Main");
        let a = Assign::default().with_phones(1, Some(1));
        assert_eq!(a.phones, [None, Some(1)], "moved from Phones 1 to Phones 2");
        assert_eq!(a.role_name(1), Some("Phones"));
        let a = Assign::default().with_phones(1, Some(0));
        assert_eq!(a, Assign::default(), "Main cannot also be a Phones");
        // A hand-edited file that gives one pair two roles is put right.
        let edited = Assign {
            main: 1,
            main_b: Some(3),
            phones: [Some(1), Some(3)],
            cue_to: CueTo::Phones1,
        };
        let fixed = edited.normalized();
        assert_eq!(fixed.phones, [None, None], "PH3/4 is Main; ADAT3/4 goes to Main B");
        assert_eq!(fixed.main_b, Some(3));
    }

    #[test]
    fn two_phones_are_numbered_and_cue_follows_its_choice() {
        let a = Assign::default()
            .with_phones(1, Some(2))
            .with_cue_to(CueTo::Phones2);
        assert_eq!(a.role_name(1), Some("Phones 1"));
        assert_eq!(a.role_name(2), Some("Phones 2"));
        assert_eq!(a.cue_target(), 2);
        assert_eq!(a.with_cue_to(CueTo::Main).cue_target(), 0);
    }
}

#[cfg(test)]
mod key_words_tests {
    use super::key_words;

    #[test]
    fn keys_read_in_sentence_case() {
        assert_eq!(key_words("DIM"), "Dim");
        assert_eq!(key_words("TALKBACK"), "Talkback");
        assert_eq!(key_words("EXT IN"), "Ext in");
        assert_eq!(key_words("MAIN B"), "Main B");
        assert_eq!(key_words("MUTE FX"), "Mute FX");
    }
}
