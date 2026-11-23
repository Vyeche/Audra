//! A local control socket, for automation and testing (`--control`): a
//! program on this computer, run by the same user, can press Audra's keys,
//! read its state and take a picture of its window, one JSON line per
//! request and one per reply. The owner, 2026-11-07: "You can create a mcp
//! layer and hook into any control", so a Claude session can test Audra on
//! the unit without anyone at the mouse (COSMIC's screenshot portal wants a
//! person's click, and Wayland has no synthetic clicks without root).
//! `tools/mcp/audra_mcp.py` puts these commands up as MCP tools.
//!
//! Off unless Audra was started with `--control`. The socket lives in the
//! user's runtime folder (`$XDG_RUNTIME_DIR/audra-control.sock`, the
//! simulation's `audra-mock-control.sock`), readable
//! and writable by the user alone.
//!
//! Requests (`{"cmd": ...}`):
//! - `keys`: the Control Room keys, with whether each is lit and can be
//!   pressed.
//! - `press` `{"key": "MONO"}`: presses one, as a click would.
//! - `view` `{"to": "mixer" | "matrix" | "quick"}`: switches the view.
//! - `fold` `{"strip": 0}`: folds or unfolds input strip N, as its switch
//!   does (filming the fold's motion, 2026-11-15).
//! - `state`: the view, the mix being edited, the Control Room's switches,
//!   what the device says about Mono, the notice showing.
//! - `screenshot` `{"path": "/abs/file.png"}`: saves the window as it is.
//! - `clap`: one clap through Reverb and Echo (the page's "Hear a clap"),
//!   for checking the FX path; refused while both are off.
//! - `snapshot` `{"n": 1..8}`: recalls a stored snapshot, as a click on
//!   its key would.
//! - `fx` `{"unit": "reverb" | "echo", "on": true | false}`: switches an
//!   effect, as its key on the Reverb & Echo page would.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::futures::{SinkExt, Stream};
use iced::Task;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;
use tuxmix_core::RmeDevice;

use crate::app::{Message, TuxMix};

/// Where a request's answer goes: the connection that sent it waits on it.
#[derive(Clone)]
pub struct Reply(Arc<Mutex<Option<oneshot::Sender<String>>>>);

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reply")
    }
}

impl Reply {
    /// Answers the request (once; later answers are dropped).
    pub fn send(&self, value: Value) {
        if let Some(tx) = self.0.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = tx.send(value.to_string());
        }
    }
}

/// One request from the socket.
#[derive(Debug, Clone)]
pub struct Request {
    pub cmd: Value,
    pub reply: Reply,
}

/// The longest request line the socket takes; a longer one is refused and
/// its connection closed (issue #22: there was no limit).
const MAX_LINE: usize = 64 * 1024;
/// A connection that sends nothing for this long is closed.
const IDLE_FOR: Duration = Duration::from_secs(60);
/// How long a request waits for the app's answer.
const ANSWER_IN: Duration = Duration::from_secs(10);

/// The socket's path: the user's runtime folder, else a folder of the
/// user's own in the temp folder (`private_dir`). The simulation has its
/// own, so a `--mock --control` run never takes the real instance's socket
/// over (the harden review).
pub fn socket_path(mock: bool) -> PathBuf {
    runtime_dir().join(if mock { "audra-mock-control.sock" } else { "audra-control.sock" })
}

/// Where this user's sockets go: the runtime folder, or the user's own
/// folder in the temp folder (made private by `private_dir` before use).
pub(crate) fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("audra-{}", own_uid().unwrap_or(0))))
}

/// This process's user id, read from `/proc/self` (no `unsafe`).
pub(crate) fn own_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// The socket's folder, made private before the socket is: the runtime
/// folder already is (0700, the user's); the temp folder's fallback is
/// created 0700, and one that exists must be the user's and closed to
/// others. In the shared temp folder the socket used to be bound first and
/// closed after, open to others between (issue #22).
pub(crate) fn private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    if std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|v| Path::new(&v) == dir) {
        return Ok(());
    }
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
    if !meta.is_dir() || Some(meta.uid()) != own_uid() || meta.permissions().mode() & 0o077 != 0 {
        return Err(format!("{} is not a private folder of this user's", dir.display()));
    }
    Ok(())
}

/// The listener: each line on a connection becomes a `Message::Control`,
/// and its reply goes back on the same connection.
pub fn worker(mock: &bool) -> impl Stream<Item = Message> {
    let path = socket_path(*mock);
    iced::stream::channel(16, move |out: iced::futures::channel::mpsc::Sender<Message>| async move {
        // Another Audra already answers there: leave it be.
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            log::warn!("control socket {} is in use by another Audra", path.display());
            return;
        }
        if let Some(dir) = path.parent() {
            if let Err(e) = private_dir(dir) {
                log::warn!("control socket not started: {e}");
                return;
            }
        }
        let _ = std::fs::remove_file(&path);
        let listener = match tokio::net::UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                log::warn!("control socket {}: {e}", path.display());
                return;
            }
        };
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        log::info!("control socket listening on {}", path.display());
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let mut out = out.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                loop {
                    // One line, at most `MAX_LINE`, within `IDLE_FOR`.
                    let mut buf = Vec::new();
                    let got = tokio::time::timeout(IDLE_FOR, (&mut reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf)).await;
                    match got {
                        Ok(Ok(n)) if n > 0 => {}
                        _ => break,
                    }
                    if buf.len() > MAX_LINE && buf.last() != Some(&b'\n') {
                        let refused = json!({"ok": false, "error": format!("request longer than {MAX_LINE} bytes")});
                        let _ = write.write_all(format!("{refused}\n").as_bytes()).await;
                        break;
                    }
                    let line = String::from_utf8_lossy(&buf);
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let answer = match serde_json::from_str::<Value>(line) {
                        Ok(cmd) => {
                            let (tx, rx) = oneshot::channel();
                            let reply = Reply(Arc::new(Mutex::new(Some(tx))));
                            if out.send(Message::Control(Request { cmd, reply })).await.is_err() {
                                break;
                            }
                            match tokio::time::timeout(ANSWER_IN, rx).await {
                                Ok(Ok(answer)) => answer,
                                Ok(Err(_)) => json!({"ok": false, "error": "no answer"}).to_string(),
                                Err(_) => json!({"ok": false, "error": "no answer in time"}).to_string(),
                            }
                        }
                        Err(e) => json!({"ok": false, "error": format!("not JSON: {e}")}).to_string(),
                    };
                    if write.write_all(format!("{answer}\n").as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    })
}

/// The Control Room keys as JSON.
fn keys(state: &TuxMix) -> Value {
    let keys: Vec<Value> = crate::control_room::key_specs(state)
        .into_iter()
        .map(|k| json!({"key": k.label, "lit": k.active, "enabled": k.on_press.is_some(), "tip": k.tip}))
        .collect();
    json!(keys)
}

/// What a test needs to read back.
fn snapshot(state: &TuxMix) -> Value {
    let cr = &state.control_room;
    json!({
        "view": format!("{:?}", state.view),
        // The theme shown, and the desktop's light or dark while followed
        // (issue #21), for a test to read back.
        "theme": {
            "shown": crate::theme::themes()[state.theme_shown.min(crate::theme::themes().len() - 1)].name,
            "follow_desktop": state.theme_desktop,
            "desktop_dark": state.desktop_dark,
        },
        "editing_mix": crate::sidebar::mix_name(state, state.sel_out),
        "control_room": {
            "mono": cr.mono,
            "speaker_b": cr.speaker_b,
            "talkback": cr.talkback,
            "ext_in": cr.ext_in,
            "dimmed": cr.is_dimmed(),
            "main": crate::sidebar::mix_name(state, cr.assign.main),
        },
        "device": {
            "can_mono": state.device.can_mono(),
            "mono_out": state.device.mono_out(),
        },
        "notice": state.notice.as_ref().map(|(n, _)| n.clone()),
    })
}

/// Handles one request. A press or a view change goes through `update`,
/// as a click would; a screenshot waits for the window's next frame.
pub fn handle(state: &mut TuxMix, req: Request) -> Task<Message> {
    let cmd = req.cmd.get("cmd").and_then(Value::as_str).unwrap_or("");
    match cmd {
        "keys" => {
            req.reply.send(json!({"ok": true, "keys": keys(state)}));
            Task::none()
        }
        "state" => {
            req.reply.send(json!({"ok": true, "state": snapshot(state)}));
            Task::none()
        }
        "press" => {
            let want = req.cmd.get("key").and_then(Value::as_str).unwrap_or("").to_uppercase();
            let spec = crate::control_room::key_specs(state).into_iter().find(|k| k.label.eq_ignore_ascii_case(&want));
            match spec.map(|k| (k.label, k.on_press)) {
                Some((label, Some(message))) => {
                    let task = crate::app::update(state, message);
                    let lit = crate::control_room::key_specs(state).into_iter().find(|k| k.label == label).map(|k| k.active);
                    req.reply.send(json!({"ok": true, "key": label, "lit": lit, "state": snapshot(state)}));
                    task
                }
                Some((label, None)) => {
                    req.reply.send(json!({"ok": false, "error": format!("{label} can't be pressed now")}));
                    Task::none()
                }
                None => {
                    req.reply.send(json!({"ok": false, "error": format!("no key {want}"), "keys": keys(state)}));
                    Task::none()
                }
            }
        }
        // "Hear a clap" (the Reverb & Echo page's key): one clap through
        // the effects, for checking the FX path end to end.
        "clap" => {
            let running = state.fx_loop.is_some();
            let task = crate::app::update(state, Message::FxAudition);
            req.reply.send(json!({"ok": running, "error": (!running).then_some("Reverb and Echo are off")}));
            task
        }
        "snapshot" => {
            let n = req.cmd.get("n").and_then(Value::as_u64).filter(|n| (1..=8).contains(n));
            match n {
                Some(n) if state.stored_snapshots[n as usize - 1] => {
                    let task = crate::app::update(state, Message::SnapshotClicked(n as u8));
                    let name = crate::app::snapshot_title(state, Some(n as usize));
                    req.reply.send(json!({"ok": true, "snapshot": name, "reverb": state.fx.reverb_on, "echo": state.fx.echo_on, "state": snapshot(state)}));
                    task
                }
                Some(n) => {
                    req.reply.send(json!({"ok": false, "error": format!("snapshot {n} is empty")}));
                    Task::none()
                }
                None => {
                    req.reply.send(json!({"ok": false, "error": "n: 1 to 8"}));
                    Task::none()
                }
            }
        }
        "fx" => {
            let unit = match req.cmd.get("unit").and_then(Value::as_str).unwrap_or("") {
                "reverb" => Some(crate::fx::Unit::Reverb),
                "echo" => Some(crate::fx::Unit::Echo),
                _ => None,
            };
            match (unit, req.cmd.get("on").and_then(Value::as_bool)) {
                (Some(unit), Some(on)) => {
                    let task = crate::app::update(state, Message::FxOn(unit, on));
                    req.reply.send(json!({"ok": true, "reverb": state.fx.reverb_on, "echo": state.fx.echo_on}));
                    task
                }
                _ => {
                    req.reply.send(json!({"ok": false, "error": "unit: reverb or echo; on: true or false"}));
                    Task::none()
                }
            }
        }
        "view" => {
            let view = match req.cmd.get("to").and_then(Value::as_str).unwrap_or("") {
                "mixer" => Some(crate::app::View::Mixer),
                "matrix" => Some(crate::app::View::Matrix),
                "quick" => Some(crate::app::View::Quick),
                _ => None,
            };
            match view {
                Some(v) => {
                    let task = crate::app::update(state, Message::SetView(v));
                    req.reply.send(json!({"ok": true, "state": snapshot(state)}));
                    task
                }
                None => {
                    req.reply.send(json!({"ok": false, "error": "to: mixer, matrix or quick"}));
                    Task::none()
                }
            }
        }
        "fold" => {
            let i = req.cmd.get("strip").and_then(Value::as_u64).unwrap_or(0) as usize;
            let task = crate::app::update(state, Message::ToggleCollapse(tuxmix_core::ChannelId::Input(i)));
            req.reply.send(json!({"ok": true, "folded": state.collapsed.contains(&tuxmix_core::ChannelId::Input(i))}));
            task
        }
        "screenshot" => {
            let Some(path) = req.cmd.get("path").and_then(Value::as_str).map(PathBuf::from).filter(|p| p.is_absolute() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))) else {
                req.reply.send(json!({"ok": false, "error": "path: an absolute .png path"}));
                return Task::none();
            };
            let Some(id) = state.window_id.filter(|_| !state.window_hidden) else {
                req.reply.send(json!({"ok": false, "error": "the window is hidden"}));
                return Task::none();
            };
            let reply = req.reply.clone();
            iced::window::screenshot(id).map(move |shot| {
                let size = shot.size;
                let result = crate::save_png(&path, &shot.rgba, size.width, size.height);
                match result {
                    Ok(()) => reply.send(json!({"ok": true, "path": path, "width": size.width, "height": size.height})),
                    Err(e) => reply.send(json!({"ok": false, "error": e.to_string()})),
                }
                Message::Tick
            })
        }
        _ => {
            req.reply.send(json!({"ok": false, "error": "cmd: keys, press, view, state or screenshot"}));
            Task::none()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(state: &mut TuxMix, cmd: Value) -> Value {
        let (tx, mut rx) = oneshot::channel();
        let _ = handle(state, Request { cmd, reply: Reply(Arc::new(Mutex::new(Some(tx)))) });
        serde_json::from_str(&rx.try_recv().expect("an answer")).unwrap()
    }

    #[test]
    fn the_socket_lists_presses_and_reads_back_the_control_room() {
        let mut state = crate::app::new(true, None, None);
        let keys = ask(&mut state, json!({"cmd": "keys"}));
        assert!(keys["keys"].as_array().unwrap().iter().any(|k| k["key"] == "MONO" && k["enabled"] == true));
        let pressed = ask(&mut state, json!({"cmd": "press", "key": "mono"}));
        assert_eq!(pressed["lit"], true);
        assert_eq!(pressed["state"]["control_room"]["mono"], true);
        assert!(state.device.mono_out().is_some());
        let unknown = ask(&mut state, json!({"cmd": "press", "key": "NOPE"}));
        assert_eq!(unknown["ok"], false);
        let view = ask(&mut state, json!({"cmd": "view", "to": "matrix"}));
        assert_eq!(view["state"]["view"], "Matrix");
        assert_eq!(ask(&mut state, json!({"cmd": "bogus"}))["ok"], false);
        // The clap needs the effects running: the simulation has no loop.
        let clap = ask(&mut state, json!({"cmd": "clap"}));
        assert_eq!(clap["ok"], false);
        assert_eq!(clap["error"], "Reverb and Echo are off");
        // An effect switched; a snapshot recalled, empty slots refused.
        let fx = ask(&mut state, json!({"cmd": "fx", "unit": "echo", "on": true}));
        assert_eq!(fx["echo"], true);
        assert_eq!(ask(&mut state, json!({"cmd": "fx", "unit": "delay", "on": true}))["ok"], false);
        assert_eq!(ask(&mut state, json!({"cmd": "snapshot", "n": 9}))["ok"], false);
        assert_eq!(ask(&mut state, json!({"cmd": "snapshot", "n": 7}))["ok"], false);
    }
}
