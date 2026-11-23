//! MIDI remote control (manual 24), the generic kind: control changes and
//! notes from any MIDI controller move Audra's faders, mutes, solos, the
//! Main and Phones levels and the Key Commands functions. A binding is
//! made by learning: pick a target in Audra settings, press Learn, move a
//! control on the device. (The manual's Mackie Control protocol, with its
//! fader banks and displays, is not built yet.)
//!
//! Audra is an ALSA sequencer client named "Audra" with one input port. It
//! connects itself to every MIDI controller present, and to each one
//! plugged in later (through the sequencer's announce port), so a
//! controller works without patching anything. The reader runs on its own
//! thread, as the ALSA sequencer read blocks, and hands plain events to
//! the app; it never touches the mixer itself.
//!
//! A binding resolves to the same message as the on-screen control, so the
//! lock, undo and the notices treat a MIDI move like a mouse move.

use std::ffi::CString;

use iced::futures::{SinkExt, Stream};
use iced::stream;
use serde::{Deserialize, Serialize};
use tuxmix_core::ChannelId;

use crate::app::{Message, TuxMix};
use crate::keycommands::KeyAction;

/// Where a MIDI event came from: a control change or a note, its MIDI
/// channel (0 to 15) and its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MidiSource {
    Cc { channel: u8, number: u8 },
    Note { channel: u8, number: u8 },
    /// Pitch bend: a 14-bit value (0 to 16383), what motor faders send.
    Bend { channel: u8 },
}

impl std::fmt::Display for MidiSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MidiSource::Cc { channel, number } => write!(f, "CC {number} (ch {})", u16::from(*channel) + 1),
            MidiSource::Note { channel, number } => write!(f, "note {number} (ch {})", u16::from(*channel) + 1),
            MidiSource::Bend { channel } => write!(f, "pitch bend (ch {})", u16::from(*channel) + 1),
        }
    }
}

/// One MIDI event: a control change's value or a note's velocity (0 to
/// 127, 0 for a note released), or a pitch bend's (0 to 16383), and the
/// sequencer client and port it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiEvent {
    pub source: MidiSource,
    pub value: u16,
    pub from: (i32, i32),
}

impl MidiEvent {
    /// An event from nowhere in particular (the tests').
    #[cfg(test)]
    pub fn new(source: MidiSource, value: u16) -> Self {
        Self { source, value, from: (0, 0) }
    }
}

/// A MIDI port Audra can send to, for picking a Mackie Control surface:
/// its name ("X-Touch: X-Touch MIDI 1"), and its sequencer address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiPort {
    pub name: String,
    pub client: i32,
    pub port: i32,
}

impl std::fmt::Display for MidiPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// What the reader thread reports.
#[derive(Debug, Clone)]
pub enum FromMidi {
    Event(MidiEvent),
    /// The MIDI ports there are now (at start, and when one comes or goes).
    Ports(Vec<MidiPort>),
}

/// What a binding moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidiTarget {
    /// A strip's fader, in the mix it edits.
    Fader(ChannelId),
    Mute(ChannelId),
    Solo(ChannelId),
    MainLevel,
    PhonesLevel,
    /// A Key Commands function (Dim, Talkback, ...).
    Key(KeyAction),
}

/// A learned binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub source: MidiSource,
    pub target: MidiTarget,
}

/// A target with its name, for the Settings list.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetChoice {
    pub target: MidiTarget,
    pub label: String,
}

impl std::fmt::Display for TargetChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Every target, named as the strips show them.
pub fn targets(state: &TuxMix) -> Vec<TargetChoice> {
    let mut list = vec![
        TargetChoice { target: MidiTarget::MainLevel, label: "Main level".into() },
        TargetChoice { target: MidiTarget::PhonesLevel, label: "Phones level".into() },
    ];
    for action in KeyAction::ALL.into_iter().skip(1) {
        list.push(TargetChoice { target: MidiTarget::Key(action), label: action.to_string() });
    }
    // Channels kept from remote control are not offered (manual 21.5.3).
    for cid in crate::app::mixer_strip_ids(state).into_iter().filter(|&c| !crate::app::remote_hidden(state, c)) {
        let name = crate::app::strip_display_name(state, cid);
        list.push(TargetChoice { target: MidiTarget::Fader(cid), label: format!("{name} fader") });
        list.push(TargetChoice { target: MidiTarget::Mute(cid), label: format!("{name} mute") });
        list.push(TargetChoice { target: MidiTarget::Solo(cid), label: format!("{name} solo") });
    }
    list
}

/// A target's name.
pub fn target_label(state: &TuxMix, target: MidiTarget) -> String {
    targets(state)
        .into_iter()
        .find(|c| c.target == target)
        .map_or_else(|| format!("{target:?}"), |c| c.label)
}

/// The message `event` sends through `binding`'s target, if any: a fader
/// follows a control change on the fader's own taper; a mute, solo or key
/// follows a button (a note pressed, or a control change crossing 64).
pub fn message_for(state: &TuxMix, target: MidiTarget, event: MidiEvent) -> Option<Message> {
    let level = || match event.source {
        MidiSource::Bend { .. } => crate::widgets::fader::t_to_vol(event.value as f32 / 16383.0),
        _ => crate::widgets::fader::t_to_vol(event.value as f32 / 127.0),
    };
    let pressed = match event.source {
        MidiSource::Note { .. } => event.value > 0,
        MidiSource::Cc { .. } => event.value >= 64,
        MidiSource::Bend { .. } => event.value >= 8192,
    };
    // A binding made before the channel was kept from remote control does
    // nothing now.
    if let MidiTarget::Fader(cid) | MidiTarget::Mute(cid) | MidiTarget::Solo(cid) = target {
        if crate::app::remote_hidden(state, cid) {
            return None;
        }
    }
    match target {
        MidiTarget::Fader(cid) => match event.source {
            MidiSource::Cc { .. } | MidiSource::Bend { .. } => {
                let out = crate::app::route_of(state, cid);
                Some(Message::VolumeChanged(cid, out, level()))
            }
            MidiSource::Note { .. } => None,
        },
        MidiTarget::MainLevel | MidiTarget::PhonesLevel => {
            if let MidiSource::Note { .. } = event.source {
                return None;
            }
            let assign = state.control_room.assign;
            let pair = match target {
                MidiTarget::MainLevel => Some(crate::control_room::monitor_pair(state)),
                _ => assign.phones_pairs().first().copied(),
            }?;
            let ch = crate::control_room::pair_channels(state, pair).first().copied()?;
            Some(Message::VolumeChanged(ChannelId::Output(ch), pair, level()))
        }
        MidiTarget::Mute(cid) => {
            // A note toggles; a control change sets.
            let on = match event.source {
                MidiSource::Note { .. } if pressed => !crate::app::channel_is_muted(state, cid),
                MidiSource::Note { .. } => return None,
                MidiSource::Cc { .. } | MidiSource::Bend { .. } => pressed,
            };
            Some(Message::Mute(cid, on))
        }
        MidiTarget::Solo(cid) => {
            let on = match event.source {
                MidiSource::Note { .. } if pressed => !crate::app::channel_is_soloed(state, cid),
                MidiSource::Note { .. } => return None,
                MidiSource::Cc { .. } | MidiSource::Bend { .. } => pressed,
            };
            Some(Message::Solo(cid, on))
        }
        MidiTarget::Key(action) => pressed.then(|| action.message(state)).flatten(),
    }
}

/// The reader, as an iced subscription stream: opens the sequencer, makes
/// the "Audra" port, connects the controllers, and sends every control
/// change and note to the app as `Message::Midi`.
pub fn worker() -> impl Stream<Item = Message> {
    stream::channel(256, async move |mut output| {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<FromMidi>(256);
        let started = std::thread::Builder::new()
            .name("audra-midi".into())
            .spawn(move || {
                if let Err(e) = read_loop(&tx) {
                    log::warn!("MIDI: {e}");
                }
            });
        if let Err(e) = started {
            log::warn!("MIDI: could not start the reader: {e}");
            return;
        }
        while let Some(item) = rx.recv().await {
            let message = match item {
                FromMidi::Event(event) => Message::Midi(event),
                FromMidi::Ports(ports) => Message::MidiPorts(ports),
            };
            if output.send(message).await.is_err() {
                break;
            }
        }
    })
}

/// Whether a port is a controller to listen to: someone else's readable
/// MIDI port, not the system's own ports and not the kernel's MIDI Through.
fn is_controller(port: &alsa::seq::PortInfo, own_client: i32) -> bool {
    use alsa::seq::{PortCap, PortType};
    let caps = port.get_capability();
    let kind = port.get_type();
    let client = port.get_client();
    client != own_client
        && client != 0
        && caps.contains(PortCap::READ | PortCap::SUBS_READ)
        && kind.contains(PortType::MIDI_GENERIC)
        && !port.get_name().unwrap_or("").starts_with("Midi Through")
}

/// Whether a port is somewhere Audra could send to (a surface's input):
/// someone else's writable MIDI port, not the system's or MIDI Through.
fn is_destination(port: &alsa::seq::PortInfo, own_client: i32) -> bool {
    use alsa::seq::{PortCap, PortType};
    let caps = port.get_capability();
    let client = port.get_client();
    client != own_client
        && client != 0
        && caps.contains(PortCap::WRITE | PortCap::SUBS_WRITE)
        && port.get_type().contains(PortType::MIDI_GENERIC)
        && !port.get_name().unwrap_or("").starts_with("Midi Through")
}

/// The ports Audra could send to now, named "client: port".
fn destinations(seq: &alsa::seq::Seq, own_client: i32) -> Vec<MidiPort> {
    let mut out = Vec::new();
    for client in alsa::seq::ClientIter::new(seq) {
        let client_name = client.get_name().unwrap_or("").to_string();
        for info in alsa::seq::PortIter::new(seq, client.get_client()) {
            if is_destination(&info, own_client) && !client_name.starts_with("Audra") {
                out.push(MidiPort {
                    name: format!("{client_name}: {}", info.get_name().unwrap_or("")),
                    client: info.get_client(),
                    port: info.get_port(),
                });
            }
        }
    }
    out
}

fn connect(seq: &alsa::seq::Seq, from: alsa::seq::Addr, to: alsa::seq::Addr) {
    if let Ok(sub) = alsa::seq::PortSubscribe::empty() {
        sub.set_sender(from);
        sub.set_dest(to);
        let _ = seq.subscribe_port(&sub);
    }
}

fn read_loop(tx: &tokio::sync::mpsc::Sender<FromMidi>) -> Result<(), String> {
    use alsa::seq::{Addr, EvCtrl, EvNote, EventType, PortCap, PortType, Seq};
    let seq = Seq::open(None, Some(alsa::Direction::Capture), false)
        .map_err(|e| format!("no ALSA sequencer ({e})"))?;
    let name = CString::new("Audra").expect("no NUL");
    let _ = seq.set_client_name(&name);
    let port_name = CString::new("Audra control").expect("no NUL");
    let port = seq
        .create_simple_port(
            &port_name,
            PortCap::WRITE | PortCap::SUBS_WRITE,
            PortType::MIDI_GENERIC | PortType::APPLICATION,
        )
        .map_err(|e| format!("could not make the Audra port ({e})"))?;
    let me = seq.client_id().map_err(|e| e.to_string())?;
    let own = Addr { client: me, port };

    // Hear about controllers plugged in later.
    connect(&seq, Addr { client: 0, port: 1 }, own);
    // Connect the ones already here.
    let mut found = Vec::new();
    for client in alsa::seq::ClientIter::new(&seq) {
        for info in alsa::seq::PortIter::new(&seq, client.get_client()) {
            if is_controller(&info, me) {
                found.push(Addr { client: info.get_client(), port: info.get_port() });
            }
        }
    }
    for addr in found {
        connect(&seq, addr, own);
    }
    log::info!("MIDI: listening as \"Audra\" (client {me})");
    let _ = tx.blocking_send(FromMidi::Ports(destinations(&seq, me)));

    let mut input = seq.input();
    loop {
        let event = input.event_input().map_err(|e| e.to_string())?;
        let src = event.get_source();
        let from = (src.client, src.port);
        let midi = match event.get_type() {
            EventType::Controller => event.get_data::<EvCtrl>().map(|c| MidiEvent {
                source: MidiSource::Cc { channel: c.channel & 0x0F, number: c.param.min(127) as u8 },
                value: c.value.clamp(0, 127) as u16,
                from,
            }),
            EventType::Noteon => event.get_data::<EvNote>().map(|n| MidiEvent {
                source: MidiSource::Note { channel: n.channel & 0x0F, number: n.note },
                value: n.velocity as u16,
                from,
            }),
            EventType::Noteoff => event.get_data::<EvNote>().map(|n| MidiEvent {
                source: MidiSource::Note { channel: n.channel & 0x0F, number: n.note },
                value: 0,
                from,
            }),
            // ALSA gives pitch bend as -8192..8191.
            EventType::Pitchbend => event.get_data::<EvCtrl>().map(|c| MidiEvent {
                source: MidiSource::Bend { channel: c.channel & 0x0F },
                value: c.value.saturating_add(8192).clamp(0, 16383) as u16,
                from,
            }),
            EventType::PortStart => {
                if let Some(addr) = event.get_data::<Addr>() {
                    if let Ok(info) = seq.get_any_port_info(addr) {
                        if is_controller(&info, me) {
                            connect(&seq, addr, own);
                        }
                    }
                }
                let _ = tx.blocking_send(FromMidi::Ports(destinations(&seq, me)));
                None
            }
            EventType::PortExit | EventType::ClientExit => {
                let _ = tx.blocking_send(FromMidi::Ports(destinations(&seq, me)));
                None
            }
            _ => None,
        };
        if let Some(midi) = midi {
            if tx.blocking_send(FromMidi::Event(midi)).is_err() {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{new, update};
    use tuxmix_core::RmeDevice;

    fn cc(number: u8, value: u8) -> MidiEvent {
        MidiEvent::new(MidiSource::Cc { channel: 0, number }, value as u16)
    }

    #[test]
    fn learning_binds_the_next_event_and_a_fader_follows_it() {
        let mut state = new(true, None, None);
        let an1 = ChannelId::Input(0);
        state.midi_enabled = true;
        let _ = update(&mut state, Message::MidiLearn(Some(MidiTarget::Fader(an1))));
        let _ = update(&mut state, Message::Midi(cc(7, 0)));
        assert!(state.midi_learn.is_none(), "learning ends with the binding");
        assert_eq!(state.midi_bindings, vec![Binding { source: MidiSource::Cc { channel: 0, number: 7 }, target: MidiTarget::Fader(an1) }]);
        // Full travel is the fader's top, zero is off. From off to the top
        // in one message it glides up, never in one write (OpenCode's job
        // 36), and lands there.
        let _ = update(&mut state, Message::Midi(cc(7, 127)));
        let first = state.device.volume(an1, state.sel_out).unwrap();
        assert!(first < 0.5, "not at once: {first}");
        crate::app::settle_glides(&mut state);
        let top = state.device.volume(an1, state.sel_out).unwrap();
        assert!(top >= 0.99, "the top of the fader (the device caps it as it caps a drag): {top}");
        let _ = update(&mut state, Message::Midi(cc(7, 0)));
        assert_eq!(state.device.volume(an1, state.sel_out).ok(), Some(0.0));
    }

    #[test]
    fn a_note_toggles_a_mute_and_the_lock_holds_it() {
        let mut state = new(true, None, None);
        let an1 = ChannelId::Input(0);
        state.midi_enabled = true;
        let note = MidiSource::Note { channel: 0, number: 36 };
        state.midi_bindings.push(Binding { source: note, target: MidiTarget::Mute(an1) });
        let _ = update(&mut state, Message::Midi(MidiEvent::new(note, 100)));
        assert!(state.device.inputs()[0].mute);
        let _ = update(&mut state, Message::Midi(MidiEvent::new(note, 0)));
        assert!(state.device.inputs()[0].mute, "the release does nothing");
        let _ = update(&mut state, Message::SetUiLock(true));
        let _ = update(&mut state, Message::Midi(MidiEvent::new(note, 100)));
        assert!(state.device.inputs()[0].mute, "locked: held like the button");
    }

    #[test]
    fn a_key_binding_runs_its_function_on_press() {
        let mut state = new(true, None, None);
        state.midi_enabled = true;
        state.midi_bindings.push(Binding { source: MidiSource::Cc { channel: 1, number: 64 }, target: MidiTarget::Key(KeyAction::Dim) });
        let _ = update(&mut state, Message::Midi(MidiEvent::new(MidiSource::Cc { channel: 1, number: 64 }, 127)));
        assert!(state.control_room.is_dimmed());
        assert_eq!(MidiSource::Cc { channel: 1, number: 64 }.to_string(), "CC 64 (ch 2)");
    }
}
