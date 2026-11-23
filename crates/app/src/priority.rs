//! Audio threads ask for the priority PipeWire's own clients get, so a busy
//! computer can't starve them (the owner, 2026-11-13: a Voice take with
//! Fine-tune came out "a bit strange or choppy"; it had nine holes of exact
//! silence, each a whole 1024-sample PipeWire cycle, while a local model
//! used eight cores and Audra, started by `cargo run` through a shim that
//! runs builds at nice 19, ran every thread and child at nice 19, its
//! `pw-play` included).
//!
//! RealtimeKit (`rtkit-daemon`, on Pop!_OS and most desktops) raises a
//! thread of the user's own to a higher priority on request: nice -11 here,
//! as PipeWire's module-rt asks for its clients' data loops. Not real-time
//! scheduling, so a runaway thread can't lock the machine. A raised
//! thread's children don't keep it (rtkit sets reset-on-fork: the child
//! of a raised process ran at nice 0), so every `pw-*` helper is raised on
//! its own once started (`raise_child`).

/// Whether an audio thread was refused its priority: the Voice panel says
/// so (Principle 3, honest about limits), since a busy computer may then
/// break its sound up.
pub static REFUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Nice level asked for (PipeWire's `nice.level` default for clients).
const NICE: i32 = -11;

/// Asks rtkit to run the calling thread at nice -11. Best effort: false
/// (and logged) where rtkit or `busctl` isn't there or says no; the thread
/// runs on as it was.
pub fn raise_this_thread(what: &str) -> bool {
    if cfg!(test) {
        return false;
    }
    // SAFETY: getpid/gettid have no preconditions.
    let (pid, tid) = unsafe { (libc::getpid(), libc::gettid()) };
    let before = nice_of(tid);
    if before.is_some_and(|n| n <= NICE) {
        return true;
    }
    let ok = ask(pid, tid);
    let after = nice_of(tid);
    if ok {
        log::info!("{what}: priority raised (nice {before:?} -> {after:?})");
        REFUSED.store(false, std::sync::atomic::Ordering::Relaxed);
    } else {
        log::warn!("{what}: rtkit didn't raise its priority (nice {before:?}); it may break up while the computer is busy");
        REFUSED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    ok
}

/// A thread's nice level.
fn nice_of(tid: libc::pid_t) -> Option<i32> {
    // SAFETY: getpriority reads only; errno is cleared first, since -1 is
    // also a valid nice level.
    unsafe {
        *libc::__errno_location() = 0;
        let n = libc::getpriority(libc::PRIO_PROCESS, tid as libc::id_t);
        (*libc::__errno_location() == 0).then_some(n)
    }
}


/// Asks rtkit to run thread `tid` of process `pid` at nice -11.
fn ask(pid: libc::pid_t, tid: libc::pid_t) -> bool {
    std::process::Command::new("busctl")
        .args([
            "--system",
            "call",
            "org.freedesktop.RealtimeKit1",
            "/org/freedesktop/RealtimeKit1",
            "org.freedesktop.RealtimeKit1",
            "MakeThreadHighPriorityWithPID",
            "tti",
            &pid.to_string(),
            &tid.to_string(),
            "--",
            &NICE.to_string(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Raises every thread of a child Audra started from a thread it can't
/// raise (the virtual mic, `pw-loopback`, started on the app's own thread:
/// on 2026-11-13 it ran at nice 19 while Voice ran at -11). Its audio
/// thread comes up once it has joined PipeWire, so this looks twice, off
/// the caller's thread.
pub fn raise_child(pid: u32, what: &'static str) {
    if cfg!(test) {
        return;
    }
    let _ = std::thread::Builder::new().name("audra-priority".into()).spawn(move || {
        for wait in [400u64, 1600] {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else { return };
            for t in tasks.flatten() {
                let Some(tid) = t.file_name().to_str().and_then(|n| n.parse::<libc::pid_t>().ok()) else { continue };
                // Logged only: a helper's thread can end between the list
                // and the ask, and rtkit limits a burst; the Voice panel's
                // warning follows Voice's own thread (`raise_this_thread`).
                if nice_of(tid).is_some_and(|n| n > NICE) && !ask(pid as libc::pid_t, tid) {
                    log::warn!("{what}: rtkit didn't raise thread {tid}");
                }
            }
        }
    });
}
