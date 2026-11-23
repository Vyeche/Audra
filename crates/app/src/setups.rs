//! Ready-made setups (Learning pass 2, the owner's first pick from mockups,
//! 2026-11-07): at the top of Quick setup, "What are you doing?" and four
//! tiles. One press sets the unit up for the job through the same messages a
//! hand would send (so the glides, the 48V arm and the unit's rules apply),
//! as one undo step, and a card says what changed and why.
//!
//! - **Podcast or call:** the mic in the Phones at −3 dB, the computer in
//!   the Phones at −10 dB, the mic out of the speakers (it can't howl), 48V
//!   offered for a click on a mic input.
//! - **Headphone mix:** the Phones get the speakers' mix.
//! - **Record:** a spare output's mix of mic (−3 dB) and computer (−10 dB),
//!   its loopback on, so a recording app records that one mix.
//! - **Stream:** Podcast, plus Record's mix for a streaming app.

use std::time::Instant;

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};
use tuxmix_core::{ChannelId, ChannelType, RmeDevice};

use crate::app::{Message, TuxMix};
use crate::theme;

/// A ready-made setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setup {
    Podcast,
    Headphones,
    Record,
    Stream,
    /// The call in the speakers, the mic only to the call (the owner's yes
    /// after critique #106 A P3-15: someone who doesn't wear headphones).
    SpeakerCall,
}

/// The tiles, in order: the setup, its name and one line.
pub const TILES: [(Setup, &str, &str); 5] = [
    (Setup::Podcast, "Podcast or call", "Your mic and the computer in the Phones"),
    (Setup::Headphones, "Headphone mix", "The Phones play what the speakers play"),
    (Setup::Record, "Record", "A mix of mic and computer, to your recording app"),
    (Setup::Stream, "Stream", "Your mic and computer, to your streaming app"),
    (Setup::SpeakerCall, "Call on my speakers", "The call on your speakers, mic to the call only"),
];

/// A tile's line: with the plain words on, the Phones are "your headphones"
/// as on the card below and in step 3 (critique #84 A P2-2: one output had
/// four names on the page).
fn tile_line(setup: Setup, line: &'static str, plain: bool) -> &'static str {
    match (setup, plain) {
        (Setup::Podcast, true) => "Your mic and the computer in your headphones",
        (Setup::Headphones, true) => "Your headphones play what the speakers play",
        _ => line,
    }
}

/// The mic in its mixes, and the computer.
const MIC_DB: f32 = -3.0;
const COMPUTER_DB: f32 = -10.0;

fn lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The card shown after a setup: which, what changed, why, and the pair
/// "Show me" opens in the Mixer.
#[derive(Debug, Clone)]
pub struct Done {
    pub setup: Setup,
    pub what: String,
    pub why: String,
    pub show: usize,
}

/// An output pair's first channel.
fn pair_ch(state: &TuxMix, pair: usize) -> usize {
    if state.device.outputs_one_per_pair() { pair } else { pair * 2 }
}

/// The first Phones pair assigned.
fn phones(state: &TuxMix) -> Option<usize> {
    state.control_room.assign.phones.iter().flatten().next().copied()
}

/// An output pair nobody listens on (not Main, Main B or a Phones): where
/// Record's and Stream's mix goes.
///
/// The optical pairs before the analog ones (with no Phones assigned the
/// first spare was the headphone jack, cleared and fed the mic), and a pair
/// that mirrors nothing, is mirrored by nothing and has no loopback on yet
/// before one that does: clearing a mirror's source clears its targets too
/// (the safety audit of 2026-11-17).
fn spare(state: &TuxMix) -> Option<usize> {
    let a = &state.control_room.assign;
    let free = |p: &usize| *p != a.main && a.main_b != Some(*p) && !a.phones.contains(&Some(*p));
    let outs = state.device.outputs();
    let in_use = |p: usize| {
        state.mirrors.iter().any(|m| m.0 == p || m.1 == p) || outs.get(pair_ch(state, p)).is_some_and(|o| o.loopback)
    };
    // Lowest first: an optical pair untouched, an optical pair, then any.
    (0..state.device.output_pair_count()).filter(free).min_by_key(|&p| (p < 2, in_use(p), p))
}

/// One change a setup makes: a level (`app::setup_level`, which keeps the
/// strip's other glides) or a message as a hand would send it.
#[derive(Debug, Clone)]
enum Step {
    Level(ChannelId, usize, f32),
    Send(Message),
}

/// What setup `setup` changes, and its card; or why it can't.
/// What a press on `setup` will do, for its sound: `None` refused, else
/// whether it leaves 48V waiting for a click.
pub fn outcome(state: &TuxMix, setup: Setup) -> Option<bool> {
    plan(state, setup).ok().map(|(_, _, arm)| arm.is_some())
}

fn plan(state: &TuxMix, setup: Setup) -> Result<(Vec<Step>, Done, Option<usize>), String> {
    let mic = state.quick_channel;
    let mic_name = crate::app::strip_display_name(state, mic);
    let computer = ChannelId::Playback(0);
    // With the plain words on, the card speaks them (critique #83 A 2: its
    // "AN1/2 at −10 dB" was the computer, and read as the mic pair). The
    // spare output keeps its code: the recording app lists it by that.
    let plain = state.plain_names;
    let computer_name = if plain && crate::app::user_label(state, computer).is_none() {
        "computer audio".to_string()
    } else {
        crate::app::strip_display_name(state, computer)
    };
    let mix_words = |pair: usize| if plain { crate::sidebar::mix_role_short(state, pair).to_string() } else { crate::sidebar::mix_name(state, pair) };
    let main = state.control_room.assign.main;
    let main_name = mix_words(main);
    let need_phones = || phones(state).ok_or("Assign a Phones pair first (Audra settings \u{2192} Monitor).".to_string());
    // Record and Stream stand on an output's loopback, which only the
    // Babyface's RME mode has: without it `update` drops the switch, and
    // the card claimed a loopback that never went on (the safety audit of
    // 2026-11-17, item 9).
    let need_spare = || {
        if !state.device.protocol_features() {
            return Err("This needs the Babyface in its RME mode, not class compliant: a recording mix is sent back to the computer by the unit.".to_string());
        }
        spare(state).ok_or("No spare output for a recording mix: every pair is Main, Main B or a Phones.".to_string())
    };
    // A mic input without 48V on: offered for a click, never switched on.
    let arm = match mic {
        ChannelId::Input(i) => state
            .device
            .inputs()
            .get(i)
            .filter(|c| c.channel_type == ChannelType::Mic && !c.phantom)
            .map(|_| i),
        _ => None,
    };
    let podcast = |ph: usize| {
        vec![
            Step::Level(mic, main, 0.0),
            Step::Level(mic, ph, lin(MIC_DB)),
            Step::Level(computer, ph, lin(COMPUTER_DB)),
        ]
    };
    let record = |sp: usize| {
        vec![
            // The mic out of the speakers, as Podcast and Stream do
            // (critique #104 A P3-7: Record left it open there).
            Step::Level(mic, main, 0.0),
            Step::Send(Message::MixClear(ChannelId::Output(pair_ch(state, sp)))),
            Step::Level(mic, sp, lin(MIC_DB)),
            Step::Level(computer, sp, lin(COMPUTER_DB)),
            Step::Send(Message::LoopbackChanged(pair_ch(state, sp), true)),
        ]
    };
    let db = |v: f32| format!("\u{2212}{} dB", v.abs());
    // The mic by what it is first, its code beside it, then "the mic"
    // (critique #84 A P2-2: "Your mic" on the tile, "AN1" on its card).
    let is_mic = matches!(mic, ChannelId::Input(i) if state.device.inputs().get(i).is_some_and(|c| c.channel_type == ChannelType::Mic));
    let (mic_first, mic_again) = if plain && is_mic { (format!("your mic ({mic_name})"), "the mic".to_string()) } else { (mic_name.clone(), mic_name.clone()) };
    // "AN1 out of your speakers" read as the mic playing from them
    // (critique #84 A P2-1).
    let kept_out = if plain { format!("{} don't play {mic_again}.", capitalised(&main_name)) } else { format!("{mic_name} out of {main_name}.") };
    Ok(match setup {
        Setup::Podcast => {
            let ph = need_phones()?;
            let ph_name = mix_words(ph);
            (
                podcast(ph),
                Done {
                    setup,
                    what: format!(
                        "{} in {ph_name} at {}, {computer_name} at {}. {kept_out}",
                        capitalised(&mic_first),
                        db(MIC_DB),
                        db(COMPUTER_DB)
                    ),
                    why: "You hear yourself and the call together, and the speakers can't feed back into the mic.".into(),
                    show: ph,
                },
                arm,
            )
        }
        Setup::Headphones => {
            let ph = need_phones()?;
            let ph_name = mix_words(ph);
            (
                vec![
                    Step::Send(Message::MixCopy(ChannelId::Output(pair_ch(state, main)))),
                    Step::Send(Message::MixPaste(ChannelId::Output(pair_ch(state, ph)))),
                ],
                Done {
                    setup,
                    what: if plain { format!("{} now play what {main_name} play.", capitalised(&ph_name)) } else { format!("{ph_name} now has {main_name}'s mix.") },
                    why: "What the speakers play, in your headphones: to check a mix, or to listen without the room.".into(),
                    show: ph,
                },
                None,
            )
        }
        Setup::Record => {
            let sp = need_spare()?;
            let sp_name = crate::sidebar::mix_name(state, sp);
            // The second mic's line only while it is out of this mix, naming
            // its fader (critique #99 A P3-9: "raise its fader" read as the
            // output's own, and it said "isn't in this mix" always).
            let second = match mic {
                ChannelId::Input(i) => {
                    let other = i ^ 1;
                    let other_mic = state.device.inputs().get(other).is_some_and(|c| c.channel_type == ChannelType::Mic);
                    let out = state.device.volume(ChannelId::Input(other), sp).unwrap_or(0.0) <= 0.0;
                    if other_mic && out {
                        let o = crate::app::short_label(&crate::app::strip_display_name(state, ChannelId::Input(other))).to_string();
                        format!(" A second mic? Pick {sp_name} as the mix in the Mixer and raise {o}.")
                    } else {
                        String::new()
                    }
                }
                _ => String::new(),
            };
            (
                record(sp),
                Done {
                    setup,
                    // The code stays (the recording app lists it); "loopback"
                    // and "replaces input" go (critique #84 A P3-2).
                    what: if plain {
                        format!(
                            "A recording mix on {sp_name}: {mic_first} at {}, {computer_name} at {}. Your recording app hears it as input {sp_name}. {kept_out}",
                            db(MIC_DB),
                            db(COMPUTER_DB)
                        )
                    } else {
                        format!(
                            "{sp_name}'s mix: {mic_name} at {}, {computer_name} at {}; its loopback is on, so it replaces input {sp_name} in recordings. {kept_out}",
                            db(MIC_DB),
                            db(COMPUTER_DB)
                        )
                    },
                    // And the way without one (critique #94 A P2-2: the card
                    // assumed a recording app and never named Rec).
                    // Two ways said apart (critique #100 A P3-11: one dense paragraph
                    // mixed the recording mix and Rec's file).
                    why: format!("In your recording app: record from {sp_name} (your voice and the computer; monitoring off there) \u{00b7} just your voice, dry: the Babyface\u{2019}s {mic_name} input \u{00b7} no app: Rec at the top saves a file.{second}"),
                    show: sp,
                },
                arm,
            )
        }
        Setup::SpeakerCall => {
            // The computer (the call) straight to the speakers; the mic kept
            // out of them, so it can't howl. The call app takes the mic from
            // the Babyface's input, not from a mix.
            (
                // And step 3 listens on the speakers, as this person does.
                // And the Mixer edits the speakers' mix (critique #109 A P3-30).
                vec![Step::Level(mic, main, 0.0), Step::Level(computer, main, 1.0), Step::Send(Message::QuickListen(main)), Step::Send(Message::SelectOutput(main))],
                Done {
                    setup,
                    what: format!("{} play {computer_name}, the call included. {kept_out}", capitalised(&main_name)),
                    why: "In your call app, pick the Babyface as the mic and as the speaker (step 4 can swap the mic for \u{201c}Audra\u{a0}processed\u{a0}mic\u{201d}, which sounds better). Others hear an echo? Turn your speakers down a little, or use headphones.".into(),
                    show: main,
                },
                arm,
            )
        }
        Setup::Stream => {
            let ph = need_phones()?;
            let sp = need_spare()?;
            let ph_name = mix_words(ph);
            let sp_name = crate::sidebar::mix_name(state, sp);
            // With Voice on, the stream gets the shaped voice ("Audra
            // processed mic") and AS1/2 carries the computer alone, so the
            // voice never goes out twice and Cough reaches the stream (the
            // owner's pick after critique #104 A P2-1/P2-2).
            let voice = matches!(mic, ChannelId::Input(_)) && crate::dynamics_panel::power_of(state, mic);
            let mut msgs = podcast(ph);
            let mut rec = record(sp);
            if voice {
                rec.retain(|s| !matches!(s, Step::Level(c, p, _) if *c == mic && *p == sp));
            }
            msgs.extend(rec);
            if voice {
                let label = crate::dynamics_loop::SOURCE_LABEL;
                return Ok((
                    msgs,
                    Done {
                        setup,
                        what: format!("You hear {mic_first} and {computer_name} in {ph_name}. Your streaming app gets {computer_name} as input {sp_name}, and your voice shaped by Voice as \u{201c}{label}\u{201d}. {kept_out}"),
                        why: format!("In your streaming app add two inputs: \u{201c}{label}\u{201d} (your voice) and {sp_name} (the computer) \u{00b7} turn off monitoring \u{00b7} mute Desktop Audio. Cough silences the stream too."),
                        show: sp,
                    },
                    arm,
                ));
            }
            (
                msgs,
                Done {
                    setup,
                    what: if plain {
                        format!("You hear {mic_first} and {computer_name} in {ph_name}. Your streaming app gets its own copy of that mix as input {sp_name}. {kept_out}")
                    } else {
                        format!("You hear {mic_name} and {computer_name} in {ph_name}; {sp_name}'s mix of both, its loopback on, goes to the computer. {kept_out}")
                    },
                    // With its reason (critique #88 A P3-7); the card's first
                    // line already says where you listen.
                    // Desktop Audio and Voice's own input said (critique #104 A P3-6, P2-1:
                    // the computer twice, the voice twice ~25 ms apart).
                    // A short list, not four sentences (critique #106 A P3-18).
                    why: format!("In your streaming app: pick {sp_name} (your voice and the computer) \u{00b7} turn off monitoring \u{00b7} mute Desktop Audio \u{00b7} skip \u{201c}Audra\u{a0}processed\u{a0}mic\u{201d} (your voice would go out twice)."),
                    show: sp,
                },
                arm,
            )
        }
    })
}

/// Applies `setup` as one undo step (the message itself opened it), and
/// leaves its card; or says why it can't.
pub fn apply(state: &mut TuxMix, setup: Setup) -> iced::Task<Message> {
    let (steps, done, arm) = match plan(state, setup) {
        Ok(p) => p,
        Err(why) => {
            // Nothing changed: the step this press opened goes, so Undo
            // doesn't spend a press on it.
            crate::app::drop_opened_step(state);
            let at = Instant::now();
            state.notice = Some((why, at));
            state.notice_sticky_at = Some(at);
            return iced::Task::none();
        }
    };
    let mut tasks: Vec<iced::Task<Message>> = Vec::new();
    // One undo step for the whole setup, the one this press opened: its
    // parts open none (a truncate after them failed on a full stack, where
    // each part's step pushed an older one out; the safety audit of
    // 2026-11-17).
    let batch = state.undo_batch;
    state.undo_batch = true;
    // Headphone mix copies Main's mix to paste it: the user's own copied
    // mix is theirs to paste after (the same audit).
    let clip = state.mix_clip.clone();
    // The setup made its one sound as it was pressed; its steps make none,
    // however long the unit takes to write them (OpenCode's job 34: the
    // 35 ms gap alone could let a step's click out after it).
    state.quiet_steps = true;
    for step in steps {
        match step {
            Step::Level(cid, out, v) => crate::app::setup_level(state, cid, out, v),
            Step::Send(m) => tasks.push(crate::app::update(state, m)),
        }
    }
    state.quiet_steps = false;
    state.undo_batch = batch;
    state.mix_clip = clip;
    if let Some(i) = arm {
        state.phantom_armed = Some((i, Instant::now()));
        state.phantom_arm_for = crate::app::PRESET_ARM_FOR;
    }
    state.setup_done = Some(done);
    // After its own steps, whose changes cleared it (`app::update`'s hook).
    state.setup_fresh = true;
    state.setup_arm = arm;
    state.setup_steps = 1;
    iced::Task::batch(tasks)
}

/// My setup's side column (its Save key, or the Replace ask) at scale 1:
/// the ask's line, the longer of the two, with room to spare.
const MY_SETUP_SIDE_W: f32 = 206.0;

/// `words` with its first letter a capital.
fn capitalised(words: &str) -> String {
    let mut c = words.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The tiles and, after a setup, its card: for the top of Quick setup.
pub fn view(state: &TuxMix) -> Element<'_, Message> {
    let scale = state.ui_scale;
    // Lit while the mix is still what the setup made, as My setup and a
    // snapshot are (critique #82 A 1).
    let lit = state.setup_done.as_ref().filter(|_| state.setup_fresh).map(|d| d.setup);
    let plain = state.plain_names;
    let tile = |&(setup, title, line): &(Setup, &'static str, &'static str)| -> Element<'static, Message> {
        let on = lit == Some(setup);
        let key = button(
            column![
                text(title).size(theme::TEXT_MD * scale).font(theme::SANS_SEMI),
                // Lit, in the key's own ink, as its title is (critique #90 B:
                // `on_active` read 4.19 on Solarized's lit tile).
                text(tile_line(setup, line, plain)).size(theme::TEXT_SM * scale).color_maybe((!on).then(theme::text_sec)),
            ]
            .spacing(theme::SPACE_TIGHT * scale),
        )
        .padding([theme::SPACE_MD * scale, theme::SPACE_LG * scale])
        .width(Length::Fill)
        // Rims that read wherever the cards land (critique #57 B: top
        // 1.62-2.04 on Daylight, off the pixel grid).
        .style(theme::split_safe_key(theme::lamp_button(theme::accent(), theme::Lamp { active: on, ..theme::Lamp::default() }), on))
        .on_press(Message::Setup(setup));
        crate::keyfocus::reach_press(title, key.into(), Some(Message::Setup(setup)), theme::RADIUS_SM * scale)
    };
    let mut col = column![
        text("What are you doing?").font(theme::SERIF).size(theme::TEXT_XL * scale).color(theme::text_primary()),
        text("One press sets the unit up for it. Undo on its card takes it back.").size(theme::TEXT_SM * scale).color(theme::text_sec()),
        row![tile(&TILES[0]), tile(&TILES[1])].spacing(theme::SPACE_MD * scale),
        row![tile(&TILES[2]), tile(&TILES[3])].spacing(theme::SPACE_MD * scale),
        row![tile(&TILES[4]), iced::widget::Space::new().width(Length::Fill)].spacing(theme::SPACE_MD * scale),
        my_setup_row(state),
    ]
    .spacing(theme::SPACE_MD * scale);
    if let Some(done) = &state.setup_done {
        let name = TILES.iter().find(|t| t.0 == done.setup).map_or("", |t| t.1);
        let small = |label: &'static str, message: Message| -> Element<'static, Message> {
            crate::keyfocus::reach_press(
                label,
                button(text(label).size(theme::TEXT_SM * scale))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                    // Rims that read on the card (critique #84 B: 2.34-2.87 at
                    // the capsule's ends and foot).
                    .style(theme::page_key(11.0 * scale))
                    .on_press(message.clone())
                    .into(),
                Some(message),
                theme::RADIUS_SM * scale,
            )
        };
        col = col.push(
            container(
                column![
                    text(if state.setup_fresh { format!("{name} is set") } else { format!("{name} was set; the mix has changed since") })
                        .size(theme::TEXT_MD * scale)
                        .font(theme::SANS_SEMI)
                        .color(theme::text_primary()),
                    // The 48V line only until its click (critique #84 A P3-1: the
                    // card went on saying so after it), and with no click
                    // count: after the key's wait lapses it takes two
                    // (critique #85 A P2-2).
                    // Changed since: what it set, said as past (critique
                    // #92 A P3-7: the title said the mix had moved on and
                    // the line stated the old mix as fact).
                    text(if state.setup_arm.is_some() && state.setup_fresh {
                        format!("{} 48V is still off: step 1 says how to tell if your mic needs it.", done.what)
                    } else if state.setup_fresh {
                        done.what.clone()
                    } else {
                        format!("It set: {}", done.what)
                    })
                        .size(theme::TEXT_SM * scale)
                        .color(theme::text_sec()),
                    // Record's and Stream's line is the next thing to do, not a
                    // reason (critique #93 A P3-7).
                    text(format!("{}: {}", if matches!(done.setup, Setup::Record | Setup::Stream | Setup::SpeakerCall) { "Next" } else { "Why" }, done.why))
                        .size(theme::TEXT_SM * scale)
                        .color(theme::text_sec()),
                    // A call needs its "Next" too (critique #94 A P3-1).
                    if matches!(done.setup, Setup::Podcast | Setup::Headphones) {
                        Element::from(
                            // Headphone mix copies the speakers; the way to a
                            // mix of their own (critique #95 A P2-4).
                            text(if done.setup == Setup::Podcast {
                                "Next: in your call app, pick the Babyface as the mic and as the speaker. Step 4 can swap the mic for \u{201c}Audra\u{a0}processed\u{a0}mic\u{201d}, which sounds better."
                            } else {
                                "Next: want something different in the headphones? In the Mixer, click the Phones strip: the faders then set the headphones only."
                            })
                                .size(theme::TEXT_SM * scale)
                                .color(theme::text_sec()),
                        )
                    } else {
                        iced::widget::Space::new().height(Length::Fixed(0.0)).into()
                    },
                    // Undo only while it would take back the setup itself.
                    if state.setup_fresh {
                        row![small("Undo", Message::SetupUndo), small("Show me in the Mixer", Message::SetupShow(done.show))]
                    } else {
                        row![small("Show me in the Mixer", Message::SetupShow(done.show))]
                    }
                    .spacing(theme::SPACE_SM * scale),
                ]
                .spacing(theme::SPACE_SM * scale),
            )
            .padding([theme::SPACE_MD * scale, theme::SPACE_LG * scale])
            .width(Length::Fill)
            .style(theme::trick_card),
        );
    }
    col.into()
}

/// The fifth card, "My setup" (the owner's pick, 2026-11-16): once saved,
/// one press brings the mix back as a snapshot comes back (levels glide;
/// Undo takes it back); beside it, the key that saves the mix you have now.
fn my_setup_row(state: &TuxMix) -> Element<'_, Message> {
    let scale = state.ui_scale;
    let small = |id: &'static str, label: &'static str, tip: &'static str, message: Message| -> Element<'static, Message> {
        crate::keyfocus::reach_press(
            id,
            crate::widgets::strip::hint(
                button(text(label).size(theme::TEXT_SM * scale))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                    .style(theme::page_key(11.0 * scale))
                    .on_press(message.clone()),
                tip,
                scale,
            ),
            Some(message),
            theme::RADIUS_SM * scale,
        )
    };
    // Before one is kept: a quiet key under the four cards, not a fifth
    // card (critique #80 A 8: it looked like them, under their "One press
    // sets the unit up", but saved instead; and it cost 54 px of the page).
    if !state.my_setup_stored {
        return row![
            small(
                "my-setup-keep",
                "Keep this mix as My setup",
                "Saves the mix you have now (levels, mutes, routing) as a fifth card here: one press brings it back",
                Message::MySetupSave,
            ),
            text("Your own mix, one press away").size(theme::TEXT_SM * scale).color(theme::text_sec()),
        ]
        .spacing(theme::SPACE_MD * scale)
        .align_y(iced::Alignment::Center)
        .into();
    }
    let lit = state.my_setup_lit;
    let card = button(
        column![
            text("My setup").size(theme::TEXT_MD * scale).font(theme::SANS_SEMI),
            text("Your own mix: one press brings it back").size(theme::TEXT_SM * scale).color_maybe((!lit).then(theme::text_sec)),
        ]
        .spacing(theme::SPACE_TIGHT * scale),
    )
    .padding([theme::SPACE_MD * scale, theme::SPACE_LG * scale])
    .width(Length::Fill)
    .style(theme::split_safe_key(theme::lamp_button(theme::accent(), theme::Lamp { active: lit, ..theme::Lamp::default() }), lit))
    .on_press(Message::MySetupRecall);
    let card = crate::keyfocus::reach_press("My setup", card.into(), Some(Message::MySetupRecall), theme::RADIUS_SM * scale);
    // Replacing it asks first, as Store does (critique #80 A 2: one press
    // overwrote it, and "Replace with the mix now" read either way).
    let side: Element<'_, Message> = if state.my_setup_replacing {
        column![
            text("Replace My setup with this mix?").size(theme::TEXT_SM * scale).color(theme::text_primary()),
            row![
                small("my-setup-replace", "Replace", "Your saved setup goes; the mix you have now takes its place", Message::MySetupSave),
                small("my-setup-cancel", "Cancel", "Keeps your saved setup as it is", Message::MySetupAsk(false)),
            ]
            .spacing(theme::SPACE_SM * scale),
        ]
        .spacing(theme::SPACE_TIGHT * scale)
        .into()
    } else {
        small("my-setup-ask", "Keep this mix instead\u{2026}", "Asks first, then keeps the mix you have now in its place", Message::MySetupAsk(true))
    };
    // The side keeps one width in both states, so the card doesn't narrow
    // when the ask opens (critique #83 A P3-5: 864 → 851).
    let side = container(side).width(Length::Fixed(MY_SETUP_SIDE_W * scale)).align_x(iced::alignment::Horizontal::Right);
    row![card, side].spacing(theme::SPACE_MD * scale).align_y(iced::Alignment::Center).into()
}
