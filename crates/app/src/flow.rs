//! Signal-flow cards (Learning pass 2, the owner's pick from mockups):
//! where a sound goes, drawn. From a strip, its sound fans out to every mix
//! it can reach ("Where AN1 goes"); from an output, every source playing
//! into its mix fans in ("What plays in Phones 3/4"). A live path is a
//! line with dots running along it and its level; a cut path is a faint
//! dashed line that says why it stops (muted, down, not in that mix).

use std::time::{Duration, Instant};

use iced::advanced::text as advanced_text;
use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::widget::{button, column, container, row, text, Space};
use iced::{mouse, Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};
use crate::theme;

/// What a card shows the flow of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowOf {
    /// A strip's sound, out to the mixes.
    Source(ChannelId),
    /// An output pair's mix, from its sources.
    Mix(usize),
}

/// One path on the card.
#[derive(Debug, Clone)]
struct Lane {
    label: String,
    /// The level when live, else why it stops.
    note: String,
    live: bool,
    /// "+n more": a line of words, no box and no path (critique #26: in a
    /// box it read as one more source).
    more: bool,
}

/// What the card draws.
#[derive(Debug, Clone)]
struct Flow {
    title: String,
    hub: String,
    hub_note: String,
    lanes: Vec<Lane>,
    /// The hub at the left, paths fanning out (a source); else at the
    /// right, paths fanning in (a mix).
    fan_out: bool,
    /// A line under the drawing, where a box's figure needs its why.
    foot: Option<String>,
}

/// One trip of a dot along a path.
const DOT_TRIP: Duration = Duration::from_millis(1600);

fn db(v: f32) -> String {
    if v <= 0.0 {
        "\u{2212}\u{221e}".into()
    } else {
        let d = 20.0 * v.log10();
        if d.abs() < 0.05 {
            "0 dB".into()
        } else if d < 0.0 {
            format!("\u{2212}{:.1} dB", -d)
        } else {
            format!("+{d:.1} dB")
        }
    }
}

fn pair_ch(state: &TuxMix, pair: usize) -> usize {
    if state.device.outputs_one_per_pair() { pair } else { pair * 2 }
}

fn muted(state: &TuxMix, cid: ChannelId) -> bool {
    match cid {
        ChannelId::Input(i) => state.device.inputs().get(i).is_some_and(|c| c.mute),
        ChannelId::Playback(i) => state.device.playbacks().get(i).is_some_and(|c| c.mute),
        ChannelId::Output(i) => state.device.outputs().get(i).is_some_and(|c| c.mute),
    }
}

/// A dB level as the lanes say it ("−10.5 dB").
fn db_level(d: f32) -> String {
    if d.abs() < 0.05 {
        "0 dB".into()
    } else if d < 0.0 {
        format!("\u{2212}{:.1} dB", -d)
    } else {
        format!("+{d:.1} dB")
    }
}

/// The effects' name as the lanes say it: the one that's on, or both.
fn fx_label(state: &TuxMix) -> &'static str {
    match (state.fx.reverb_on, state.fx.echo_on) {
        (true, false) => "Reverb",
        (false, true) => "Echo",
        _ => "Reverb & Echo",
    }
}

/// A source's FX send in dB, where it has one: AN1/2's as the unit gets it
/// (following its faders), any other pair's fixed level.
fn source_send_db(state: &TuxMix, cid: ChannelId) -> Option<f32> {
    match cid {
        ChannelId::Input(0 | 1) => Some(crate::fx::send_heard_db(state)),
        c if crate::fx::SEND_PAIR_CHANNELS.contains(&c) && state.device.has_source_fx_send(c) => Some(state.device.settings().source_fx_send_of(c)),
        _ => None,
    }
}

/// The Reverb & Echo lane of a mix's map, by the one "what you hear" model
/// (`fx::block_in`): its return's level where they're heard, else why not,
/// in words that fit a box (the critiques carried "the flow map has no
/// Reverb & Echo" since #66). None while both are off.
fn fx_return_lane(state: &TuxMix, p: usize, out_muted: bool) -> Option<Lane> {
    use crate::fx::Block;
    if !state.fx.any_on() {
        return None;
    }
    let (live, note) = match crate::fx::block_in(state, p) {
        // The simulation runs no effects: never drawn as playing.
        None if state.device.is_mock() => (false, "not in simulation".to_string()),
        None if out_muted => (false, "output muted".to_string()),
        None => (true, format!("{} return", db(crate::fx::user_return(state, p)))),
        Some(Block::HeldByMute) => (false, "held by Mute FX".into()),
        Some(Block::NoReturn | Block::NoSendNoReturn) => (false, "its FX is down".into()),
        Some(Block::NoSend) => (false, "nothing sent yet".into()),
        Some(Block::SendFollowsDown) => (false, "AN1/2 is down".into()),
        Some(Block::NoEngine) => (false, "driver can't play".into()),
        // (critique #110 B: it read "not running".)
        Some(Block::KeptForMic) => (false, "kept out (open mic)".into()),
        Some(_) => (false, "not running".into()),
    };
    Some(Lane { label: fx_label(state).into(), note, live, more: false })
}

/// A source named as the placard's "Fed by" line and its strip name it:
/// "PH3/4 PB", not "PCM PH3/4" (critique #26: one pair, three names).
fn source_name(state: &TuxMix, cid: ChannelId) -> String {
    let name = crate::app::strip_display_name(state, cid);
    let short = crate::app::short_label(&name);
    if matches!(cid, ChannelId::Playback(_)) {
        format!("{short} PB")
    } else {
        short.to_string()
    }
}

/// A playback the plain words name "computer audio", as the placard's Fed
/// by line does: the switch on and no name of the user's own. Its box then
/// reads "computer audio" with its code in the note (critique #84 A P2-2b:
/// "AN1/2 PB" under a placard's "computer audio"; #85 A P3-1: "computer
/// (AN1/2)" was a third name, and read as the mic pair).
pub(crate) fn plain_playback(state: &TuxMix, cid: ChannelId) -> bool {
    state.plain_names && matches!(cid, ChannelId::Playback(_)) && crate::app::user_label(state, cid).is_none()
}

fn build(state: &TuxMix, of: FlowOf) -> Flow {
    match of {
        FlowOf::Source(cid) => {
            let name = source_name(state, cid);
            let strip_muted = muted(state, cid);
            let lanes = (0..state.device.output_pair_count())
                .map(|p| {
                    let level = state.device.volume(cid, p).unwrap_or(0.0);
                    let out_muted = muted(state, ChannelId::Output(pair_ch(state, p)));
                    let (live, note) = if strip_muted {
                        (false, "muted on its strip".to_string())
                    } else if level <= 0.0 {
                        // The listener's outputs by their words here too,
                        // as the setup's card just said "Your speakers
                        // don't play the mic" (critique #86 A P3-6).
                        let a = &state.control_room.assign;
                        let words = if !state.plain_names {
                            "not in this mix"
                        } else if p == a.main || a.main_b == Some(p) {
                            "not in your speakers"
                        } else if a.is_phones(p) {
                            "not in your headphones"
                        } else {
                            "not in this mix"
                        };
                        (false, words.to_string())
                    } else if out_muted {
                        (false, format!("{} here, but the output is muted", db(level)))
                    } else {
                        // Whose it is, where the listener has a word for it
                        // (critique #85 A P3-2): "speakers · −6.0 dB".
                        let a = &state.control_room.assign;
                        let role = if !state.plain_names {
                            None
                        } else if p == a.main {
                            Some("speakers")
                        } else if a.is_phones(p) {
                            Some("headphones")
                        } else {
                            None
                        };
                        (true, role.map_or_else(|| db(level), |r| format!("{r} \u{b7} {}", db(level))))
                    };
                    Lane { label: crate::sidebar::mix_name(state, p), note, live, more: false }
                })
                .collect::<Vec<_>>();
            // Into the effects too, where the source has a send and one is
            // on: its send's level, or why nothing goes in.
            let mut lanes = lanes;
            let mut foot = None;
            if let Some(send) = source_send_db(state, cid).filter(|_| state.fx.any_on()) {
                let (live, note) = if strip_muted {
                    (false, "muted on its strip".to_string())
                } else if send <= crate::fx::OFF_DB {
                    (false, "its FX send is off".to_string())
                } else if state.device.is_mock() {
                    // No dots into effects that don't run (critique #72 A 3:
                    // the source map ran them while the mix map said none).
                    (false, "not in simulation".to_string())
                } else {
                    (true, format!("{} send", db_level(send)))
                };
                // AN1/2's send follows its faders: the knob's figure and the
                // one the unit gets, tied together (critique #72 A 2: "−13.0
                // dB send" beside a knob at −10.0).
                // In the send row's own words for what it follows; "would
                // get … on the unit" in the simulation, which runs none;
                // nothing while the strip is muted (critique #73 A 2).
                if matches!(cid, ChannelId::Input(0 | 1)) && !strip_muted && send > crate::fx::OFF_DB && (send - crate::fx::send_db(state)).abs() >= 0.05 {
                    let gets = if state.device.is_mock() { "on the unit the effects would get" } else { "the effects get" };
                    foot = Some(format!(
                        "FX send knob {}. {}, so {gets} {}.",
                        db_level(crate::fx::send_db(state)),
                        crate::fx::send_follow_words(state, cid),
                        db_level(send)
                    ));
                }
                lanes.push(Lane { label: fx_label(state).into(), note, live, more: false });
            }
            Flow {
                title: if plain_playback(state, cid) { "Where computer audio goes".to_string() } else { format!("Where {name} goes") },
                // Its code in the note, as in a mix's map; never "(AN1/2)"
                // beside "computer", which read as the mic pair (critique
                // #86 A P3-7).
                hub_note: if strip_muted {
                    "muted".into()
                } else if plain_playback(state, cid) {
                    name.clone()
                } else {
                    "its fader in each mix".into()
                },
                hub: if plain_playback(state, cid) { "computer audio".to_string() } else { name },
                lanes,
                fan_out: true,
                foot,
            }
        }
        FlowOf::Mix(p) => {
            let mix = crate::sidebar::mix_name(state, p);
            let out_muted = muted(state, ChannelId::Output(pair_ch(state, p)));
            // What plays, then what doesn't, dashed with why (critique #25:
            // "why can't I hear AN1 here?" went unanswered, and the caption
            // spoke of dashed lines never drawn), up to `MIX_LANES` in all,
            // the rest folded into one "+n more" lane.
            let mut playing = Vec::new();
            let mut silent = Vec::new();
            for cid in crate::app::mixer_strip_order(state).into_iter().filter(|c| !matches!(c, ChannelId::Output(_))) {
                let level = state.device.volume(cid, p).unwrap_or(0.0);
                let m = muted(state, cid);
                let code = source_name(state, cid);
                let plain = plain_playback(state, cid);
                let label = if plain { "computer audio".to_string() } else { code.clone() };
                if level > 0.0 {
                    playing.push(Lane {
                        label,
                        note: if m {
                            format!("{}, but muted", db(level))
                        } else if plain {
                            format!("{code} \u{b7} {}", db(level))
                        } else {
                            db(level)
                        },
                        live: !m && !out_muted,
                        more: false,
                    });
                } else {
                    let note = if plain { format!("{code} \u{b7} not here") } else { "not in this mix".to_string() };
                    silent.push(Lane { label, note, live: false, more: false });
                }
            }
            // Reverb & Echo with what plays when heard, else first of what
            // doesn't (it's the one a mix is most often asked about).
            if let Some(lane) = fx_return_lane(state, p, out_muted) {
                if lane.live {
                    playing.push(lane);
                } else {
                    silent.insert(0, lane);
                }
            }
            let room = MIX_LANES.saturating_sub(playing.len());
            let mut lanes = playing;
            if silent.len() > room {
                let shown = room.saturating_sub(1);
                let more = silent.len() - shown;
                lanes.extend(silent.into_iter().take(shown));
                lanes.push(Lane { label: format!("+{more} more, not in this mix"), note: String::new(), live: false, more: true });
            } else {
                lanes.extend(silent);
            }
            Flow {
                title: format!("What plays in {mix}"),
                hub: mix,
                hub_note: if out_muted {
                    "muted".into()
                } else if state.plain_names {
                    crate::sidebar::mix_role_short(state, p).into()
                } else {
                    "out to the jacks".into()
                },
                lanes,
                fan_out: false,
                foot: None,
            }
        }
    }
}

/// The grounds a map's lines and rims lie on: the card's surface, a box's
/// face and the hub's.
fn map_grounds() -> [Color; 3] {
    let ground = theme::surface();
    let over = |c: Color, a: f32| Color { r: c.r * a + ground.r * (1.0 - a), g: c.g * a + ground.g * (1.0 - a), b: c.b * a + ground.b * (1.0 - a), a: 1.0 };
    [ground, over(theme::text_primary(), 0.06), over(theme::accent(), 0.16)]
}

/// `c` moved toward the ink until it holds `need` on every ground of the
/// map (critique #73 B: the live paths 2.41-2.99, the dashes 1.58-1.98 and
/// the box rims 1.49-2.78), in fine steps so it stops near `need`.
fn map_ink_at(c: Color, need: f32) -> Color {
    let grounds = map_grounds();
    (0..=100)
        .map(|k| {
            let t = k as f32 / 100.0;
            let ink = theme::text_primary();
            Color { r: c.r + (ink.r - c.r) * t, g: c.g + (ink.g - c.g) * t, b: c.b + (ink.b - c.b) * t, a: 1.0 }
        })
        .find(|m| grounds.iter().all(|g| theme::contrast(*m, *g) >= need))
        .unwrap_or_else(theme::text_primary)
}

/// What plays: lines, dots and rims a step above the floor, so they lead
/// (critique #74 A 3: at one floor the dead routes weighed as much).
pub(crate) fn map_ink(c: Color) -> Color {
    map_ink_at(c, 4.5)
}

/// What doesn't play: near the 3:1 floor (`SHADE`, so a dash's soft edge
/// and a half-pixel side still read), legible and in the shade.
/// The shade's target: a box side that lands on a half pixel still reads
/// 3:1 (at 3.4 the light theme's measured 2.92).
const SHADE: f32 = 3.75;

pub(crate) fn map_ink_shade(c: Color) -> Color {
    // From the card's surface toward `c`, the first that reads: where `c`
    // is darker than it needs (the light theme's grey, 6.76), the shade
    // stops at the floor instead of outweighing the live coral (5.70).
    let grounds = map_grounds();
    let ground = theme::surface();
    (0..=100)
        .map(|k| {
            let t = k as f32 / 100.0;
            Color { r: ground.r + (c.r - ground.r) * t, g: ground.g + (c.g - ground.g) * t, b: ground.b + (c.b - ground.b) * t, a: 1.0 }
        })
        .find(|m| grounds.iter().all(|g| theme::contrast(*m, *g) >= SHADE))
        .unwrap_or_else(|| map_ink_at(c, SHADE))
}

/// The drawing.
struct FlowCanvas {
    flow: Flow,
    phase: f32,
    scale: f32,
}

const LANE_H: f32 = 46.0;
/// A mix's map shows this many lanes at most, what plays first.
const MIX_LANES: usize = 7;
/// A box's width: room for "Phones 3/4" over "not in this mix" (it was
/// 170; critique #25: with its panel beside it, the map covered its own
/// strip at 1600px).
const NODE_W: f32 = 140.0;
const NODE_H: f32 = 36.0;

/// A point on the cubic from `a` to `b` with horizontal handles, at `t`.
fn curve_at(a: Point, b: Point, t: f32) -> Point {
    let mx = (a.x + b.x) / 2.0;
    let (p0, p1, p2, p3) = (a, Point::new(mx, a.y), Point::new(mx, b.y), b);
    let u = 1.0 - t;
    Point::new(
        u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
        u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
    )
}

impl<M> canvas::Program<M> for FlowCanvas {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.scale;
        let n = self.flow.lanes.len().max(1);
        let (node_w, node_h, lane_h) = (NODE_W * s, NODE_H * s, LANE_H * s);
        let hub_x = if self.flow.fan_out { 0.0 } else { bounds.width - node_w };
        let lane_x = if self.flow.fan_out { bounds.width - node_w } else { 0.0 };
        let hub_y = (n as f32 * lane_h - node_h) / 2.0;
        let accent = theme::accent();
        let node = |frame: &mut Frame, x: f32, y: f32, title: &str, note: &str, live: bool, hub: bool| {
            let rect = Path::rounded_rectangle(Point::new(x, y), Size::new(node_w, node_h), (theme::RADIUS_SM * s).into());
            let fill = if hub { Color { a: 0.16, ..accent } } else { Color { a: 0.06, ..theme::text_primary() } };
            frame.fill(&rect, fill);
            let edge = if live || hub { map_ink(accent) } else { map_ink_shade(theme::muted()) };
            // 2 px, so no side splits over two pixels: at 1.5 the hub's side
            // read 2.99 on its tinted face, a shaded box's 2.79 on its own
            // (critique #74 B).
            frame.stroke(&rect, Stroke::default().with_color(edge).with_width(2.0 * s));
            frame.fill_text(Text {
                content: title.to_string(),
                // A descender ("computer audio") clear of the note under it
                // (critique #85 A P3-1: they shared a row).
                position: Point::new(x + 10.0 * s, y + 5.0 * s),
                color: if live || hub { theme::text_primary() } else { theme::text_sec() },
                size: (theme::TEXT_SM * s).into(),
                font: theme::SANS_SEMI,
                align_x: advanced_text::Alignment::Left,
                align_y: iced::alignment::Vertical::Top,
                ..Text::default()
            });
            frame.fill_text(Text {
                content: note.to_string(),
                position: Point::new(x + 10.0 * s, y + node_h - 5.0 * s),
                color: if live { theme::readable_on_floors(theme::signal()) } else { theme::muted() },
                // The type floor, 12px (critique #25: 11px).
                size: (theme::TEXT_SM * s).into(),
                align_x: advanced_text::Alignment::Left,
                align_y: iced::alignment::Vertical::Bottom,
                ..Text::default()
            });
        };
        // The paths first, under the nodes.
        for (i, lane) in self.flow.lanes.iter().enumerate() {
            if lane.more {
                continue;
            }
            let ly = i as f32 * lane_h + (lane_h - node_h) / 2.0;
            let (a, b) = if self.flow.fan_out {
                (Point::new(hub_x + node_w, hub_y + node_h / 2.0), Point::new(lane_x, ly + node_h / 2.0))
            } else {
                (Point::new(lane_x + node_w, ly + node_h / 2.0), Point::new(hub_x, hub_y + node_h / 2.0))
            };
            let mx = (a.x + b.x) / 2.0;
            let path = Path::new(|p| {
                p.move_to(a);
                p.bezier_curve_to(Point::new(mx, a.y), Point::new(mx, b.y), b);
            });
            if lane.live {
                frame.stroke(&path, Stroke::default().with_color(map_ink(accent)).with_width(2.0 * s));
                // Three dots running from the source to where it plays.
                for k in 0..3 {
                    let t = (self.phase + k as f32 / 3.0).fract();
                    frame.fill(&Path::circle(curve_at(a, b, t), 3.2 * s), map_ink(accent));
                }
            } else {
                // Dashed, and stopping short of where it would play with a
                // bar across its end: the sound stops there (critique #26:
                // run into the box, the dashes said it arrives).
                // Out from a source the lines bunch at its box, so they
                // stop near where they'd land, plain which box each aims
                // at; into a mix they bunch at the mix, so they stop
                // halfway, clear of the lines that arrive.
                let stop = if self.flow.fan_out { 0.8 } else { 0.5 };
                // Out from a source the dashes start a little way from its
                // box, where the lines bunch (critique #74 A 4: a grey knot
                // over the live paths' start).
                // 0.12 of the way: at 0.07 the lines still met in a 5x8 px
                // knot (critique #75 A 5).
                // Where the curves have parted: with flat handles they leave
                // the box together, and at 0.12 the first dashes still knotted
                // (critique #75 A 5, B).
                let from = if self.flow.fan_out { 0.28 } else { 0.0 };
                let short = Path::new(|p| {
                    p.move_to(curve_at(a, b, from));
                    for k in 1..=24 {
                        p.line_to(curve_at(a, b, from + (stop - from) * k as f32 / 24.0));
                    }
                });
                let dash = [5.0 * s, 5.0 * s];
                let ink = map_ink_shade(theme::muted());
                frame.stroke(
                    &short,
                    Stroke {
                        line_dash: LineDash { segments: &dash, offset: 0 },
                        ..Stroke::default().with_color(ink).with_width(2.0 * s)
                    },
                );
                let end = curve_at(a, b, stop);
                let before = curve_at(a, b, stop - 0.02);
                let (dx, dy) = (end.x - before.x, end.y - before.y);
                let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
                let (nx, ny) = (-dy / len * 4.0 * s, dx / len * 4.0 * s);
                frame.stroke(
                    &Path::line(Point::new(end.x - nx, end.y - ny), Point::new(end.x + nx, end.y + ny)),
                    // 2 px: a vertical tick on a half pixel read 2.98 at 1.5
                    // (critique #75 B).
                    Stroke::default().with_color(ink).with_width(2.0 * s),
                );
            }
        }
        for (i, lane) in self.flow.lanes.iter().enumerate() {
            let ly = i as f32 * lane_h + (lane_h - node_h) / 2.0;
            if lane.more {
                frame.fill_text(Text {
                    content: lane.label.clone(),
                    position: Point::new(lane_x + 10.0 * s, ly + node_h / 2.0),
                    color: theme::muted(),
                    size: (theme::TEXT_SM * s).into(),
                    align_x: advanced_text::Alignment::Left,
                    align_y: iced::alignment::Vertical::Center,
                    ..Text::default()
                });
                continue;
            }
            node(&mut frame, lane_x, ly, &lane.label, &lane.note, lane.live, false);
        }
        node(&mut frame, hub_x, hub_y, &self.flow.hub, &self.flow.hub_note, true, true);
        vec![frame.into_geometry()]
    }
}

/// Where the dots are now, 0..1 along a trip (held for a screenshot).
fn phase(state: &TuxMix) -> f32 {
    let at = state.flow_card.map_or_else(Instant::now, |(_, at)| at);
    crate::motion::since_held(at).as_secs_f32() / DOT_TRIP.as_secs_f32() % 1.0
}

/// What a strip's card shows: a source's paths out, an output's mix in
/// (an output strip's pair).
pub fn of_strip(state: &TuxMix, cid: ChannelId) -> FlowOf {
    match cid {
        ChannelId::Output(ch) => FlowOf::Mix(crate::mixops::output_pair(state, ch)),
        other => FlowOf::Source(other),
    }
}

/// Whether the card is open for `cid`'s Settings panel.
pub fn open_for(state: &TuxMix, cid: ChannelId) -> bool {
    state.flow_card.is_some_and(|(of, _)| of == of_strip(state, cid))
}

/// Whether the card is on screen: the window shown, the Mixer up, and the
/// Settings panel it hangs beside open. Its dots ask for frames only then
/// (the scout, 2026-11-12: closing the panel, changing view or hiding to
/// the tray left the card "open", redrawing 60 times a second for nothing).
pub fn drawn(state: &TuxMix) -> bool {
    !state.window_hidden
        && state.view == crate::app::View::Mixer
        && state
            .flyout_open
            .iter()
            .any(|&(cid, kind)| kind == crate::widgets::strip::FlyoutKind::Settings && open_for(state, cid))
}

/// The room between the card's two columns of boxes, for the paths.
const CURVE_W: f32 = 84.0;

/// The card's width at `scale`: its two columns of boxes and the paths
/// between, and its padding; no more (issue #23, the owner: "too much
/// space on the right that is empty").
pub fn popup_w(scale: f32) -> f32 {
    (2.0 * NODE_W + CURVE_W) * scale + 2.0 * theme::SPACE_MD * scale
}

/// The card beside a strip's open Settings panel (issue #23, the owner's
/// pick: "Where it goes" from a source's panel, "What plays here" from an
/// output's, a popup beside the row it opens from): the panel's floating
/// card, as tall as the panel beside it (both fill the height they're
/// given), as wide as its drawing. Its × or Esc closes it.
pub fn popup(state: &TuxMix, of: FlowOf) -> Element<'_, Message> {
    let scale = state.ui_scale;
    let mut flow = build(state, of);
    let foot = flow.foot.take();
    let n = flow.lanes.len().max(1) as f32;
    let head = row![
        text(flow.title.clone()).font(theme::SERIF).size(theme::TEXT_ROW * scale).color(theme::text_primary()),
        Space::new().width(Length::Fill),
        crate::widgets::strip::hint(
            button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
                .padding(0)
                .width(Length::Fixed(20.0 * scale))
                .height(Length::Fixed(20.0 * scale))
                .style(theme::icon_button(false))
                .on_press(Message::FlowClose),
            "Close (Esc)",
            scale,
        ),
    ]
    .align_y(Alignment::Center);
    let drawing = canvas::Canvas::new(FlowCanvas { flow, phase: if state.animations { phase(state) } else { 0.5 }, scale })
        .width(Length::Fill)
        .height(Length::Fixed(n * LANE_H * scale));
    // The drawing in the middle of the room the panel's height gives the
    // card, the caption at its foot (critique #25: the room collected under
    // the caption as an empty slab, half the card).
    // The send's line right under the drawing, with it in the middle of
    // the room (critique #73 A 3: at the card's foot, 230 px under the
    // Reverb box, it read as the whole map's caption).
    let mut middle = column![drawing].spacing(theme::SPACE_MD * scale);
    if let Some(foot) = foot {
        middle = middle.push(text(foot).size(theme::TEXT_SM * scale).color(theme::text_sec()));
    }
    let body = column![
        head,
        container(middle).height(Length::Fill).center_y(Length::Fill),
        text("Dots run along what plays; a dashed line says why it stops.").size(theme::TEXT_SM * scale).color(theme::muted()),
    ]
    .spacing(theme::SPACE_MD * scale);
    container(body)
        .padding(theme::SPACE_MD * scale)
        .width(Length::Fixed(popup_w(scale)))
        .height(Length::Fill)
        .style(theme::floating_card)
        .into()
}

#[cfg(test)]
mod tests {
    use super::{build, FlowOf};
    use crate::app::{new, update, Message};
    use tuxmix_core::ChannelId;

    #[test]
    fn the_maps_show_reverb_and_echo_by_the_one_model() {
        // Carried since critique #66: the flow map had no Reverb & Echo.
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        let fx = |f: &super::Flow| f.lanes.iter().find(|l| l.label.starts_with("Reverb") || l.label == "Echo").cloned();
        assert!(fx(&build(&state, FlowOf::Mix(main))).is_none(), "both off: no lane");
        assert!(fx(&build(&state, FlowOf::Source(ChannelId::Input(0)))).is_none());
        state.fx.reverb_on = true;
        let lane = fx(&build(&state, FlowOf::Mix(main))).expect("the lane");
        assert_eq!((lane.label.as_str(), lane.note.as_str(), lane.live), ("Reverb", "its FX is down", false));
        let lane = fx(&build(&state, FlowOf::Source(ChannelId::Input(0)))).expect("AN1's lane");
        assert_eq!((lane.note.as_str(), lane.live), ("its FX send is off", false));
        // A send up and the return: AN1 goes in; not in simulation.
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), main, 0.5));
        let _ = update(&mut state, Message::FxSend(-10.0));
        crate::fx::set_return(&mut state, main, crate::fx::db_vol(-12.0));
        crate::app::settle_glides(&mut state);
        // In the simulation no dots run into them (critique #72 A 3); the
        // knob and the level the effects get, tied in a line (A 2).
        let flow = build(&state, FlowOf::Source(ChannelId::Input(0)));
        let lane = fx(&flow).unwrap();
        assert_eq!((lane.note.as_str(), lane.live), ("not in simulation", false));
        let foot = flow.foot.expect("the knob and the level, tied");
        assert!(foot.contains("knob \u{2212}10.0 dB") && foot.contains("their loudest fader") && foot.contains("would get"), "{foot}");
        let lane = fx(&build(&state, FlowOf::Mix(main))).unwrap();
        assert_eq!((lane.note.as_str(), lane.live), ("not in simulation", false));
        // Mute FX: said so, never as playing.
        crate::fx::set_mute(&mut state, true);
        crate::app::settle_glides(&mut state);
        assert_eq!(fx(&build(&state, FlowOf::Mix(main))).unwrap().note, "held by Mute FX");
        // Every note fits its box (about 17 characters at 12 px in 140).
        for note in ["not in simulation", "output muted", "held by Mute FX", "its FX is down", "nothing sent yet", "AN1/2 is down", "driver can't play", "not running", "its FX send is off"] {
            assert!(note.chars().count() <= 19, "{note}");
        }
    }
}
