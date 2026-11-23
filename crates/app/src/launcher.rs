//! Audra in the app launcher and the dock (the owner, 2026-11-06: "Audra is
//! missing an icon when lunched from root or everywhere"). The window says
//! it is `audra` (`main::APP_ID`); a Wayland dock finds the icon through
//! the desktop entry of that name and the icon it names, so without them
//! a copy run straight from its folder showed a generic gear. Audra writes
//! both into the user's data folder as it starts, pointing at whichever
//! copy is running (as "Start with the session" does), and keeps them
//! current; Audra settings → App turns it off and takes them away.

use std::path::{Path, PathBuf};

/// The line that marks an entry as Audra's own, so it is only ever
/// rewritten or removed by Audra, never one the user or a package made.
const MARK: &str = "X-Audra-Managed=true";

const ICON_SVG: &[u8] = include_bytes!("../assets/audra.svg");
const ICON_128: &[u8] = include_bytes!("../assets/audra-128.png");
const ICON_256: &[u8] = include_bytes!("../assets/audra-256.png");

/// `$XDG_DATA_HOME`, else `~/.local/share`.
fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
}

fn entry_path(data: &Path) -> PathBuf {
    data.join("applications").join("audra.desktop")
}

/// The icons, by where they go under `data`.
fn icons(data: &Path) -> [(PathBuf, &'static [u8]); 3] {
    let hicolor = data.join("icons").join("hicolor");
    [
        (hicolor.join("scalable").join("apps").join("audra.svg"), ICON_SVG),
        (hicolor.join("128x128").join("apps").join("audra.png"), ICON_128),
        (hicolor.join("256x256").join("apps").join("audra.png"), ICON_256),
    ]
}

/// `exe` quoted for an entry's `Exec` (the Desktop Entry spec: `"`, `` ` ``,
/// `$` and `\` escaped inside the quotes, the backslash doubled again for
/// the string type, and `%` as `%%`).
fn exec_quoted(exe: &str) -> String {
    let mut out = String::from("\"");
    for c in exe.chars() {
        match c {
            '"' | '`' | '$' => {
                out.push_str("\\\\");
                out.push(c);
            }
            '\\' => out.push_str("\\\\\\\\"),
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The launcher entry for the copy at `exe`: the project's own entry, with
/// its `Exec` on this copy and Audra's mark.
fn entry(exe: &str) -> String {
    let exec = exec_quoted(exe);
    format!(
        "[Desktop Entry]\nType=Application\nName=Audra\nGenericName=Audio Mixer\n\
         Comment=Hardware mixer for the RME Babyface Pro FS on Linux\n\
         Exec={exec}\nIcon=audra\nTerminal=false\nStartupWMClass=audra\n\
         Categories=AudioVideo;Audio;Mixer;\n\
         Keywords=RME;Babyface;mixer;monitoring;audio interface;\n{MARK}\n"
    )
}

/// Whether a package installed Audra for the whole system (its own entry),
/// and this is that copy: then the user's folder is left alone.
fn packaged(exe: &Path) -> bool {
    exe.starts_with("/usr")
        && ["/usr/share/applications/audra.desktop", "/usr/local/share/applications/audra.desktop"]
            .iter()
            .any(|p| Path::new(p).exists())
}

/// Writes `bytes` to `path` unless it already holds them.
fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<bool> {
    if std::fs::read(path).is_ok_and(|now| now == bytes) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)?;
    Ok(true)
}

/// As Audra starts: its entry and icons in place for the running copy. An
/// entry there that isn't Audra's own (the user's, say) is left as it is.
pub fn install() -> std::io::Result<()> {
    let Some(data) = data_home() else {
        return Ok(());
    };
    let exe = std::env::current_exe()?;
    // A packaged copy has its own entry; a build in a temporary folder or
    // a debug build is a passing one, which would leave the dock pointing
    // at a file that goes away (the reviewer).
    if packaged(&exe) || exe.starts_with(std::env::temp_dir()) || exe.components().any(|c| c.as_os_str() == "debug") {
        return Ok(());
    }
    let path = entry_path(&data);
    if std::fs::read_to_string(&path).is_ok_and(|e| !e.contains(MARK)) {
        return Ok(());
    }
    for (icon, bytes) in icons(&data) {
        write_if_changed(&icon, bytes)?;
    }
    if write_if_changed(&path, entry(&exe.to_string_lossy()).as_bytes())? {
        log::info!("Added Audra to the app launcher: {}", path.display());
    }
    Ok(())
}

/// Turned off in Audra settings: Audra's own entry and icons taken away
/// (one that isn't Audra's own stays).
pub fn remove() -> std::io::Result<()> {
    let Some(data) = data_home() else {
        return Ok(());
    };
    let path = entry_path(&data);
    if std::fs::read_to_string(&path).is_ok_and(|e| e.contains(MARK)) {
        std::fs::remove_file(&path)?;
        for (icon, _) in icons(&data) {
            let _ = std::fs::remove_file(icon);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_points_at_the_running_copy_and_names_the_window() {
        let e = entry("/home/me/my apps/audra");
        assert!(e.starts_with("[Desktop Entry]\n"));
        assert!(e.contains("Exec=\"/home/me/my apps/audra\"\n"));
        // The spec's escapes, inside the quotes.
        assert_eq!(exec_quoted("/a/b$c"), "\"/a/b\\\\$c\"");
        assert_eq!(exec_quoted("/100%/x"), "\"/100%%/x\"");
        assert!(e.contains("Icon=audra\n") && e.contains("StartupWMClass=audra\n"));
        assert!(e.contains(MARK));
    }

    #[test]
    fn the_icons_go_where_the_icon_theme_looks() {
        let all = icons(Path::new("/d"));
        assert_eq!(all[0].0, Path::new("/d/icons/hicolor/scalable/apps/audra.svg"));
        assert_eq!(all[1].0, Path::new("/d/icons/hicolor/128x128/apps/audra.png"));
        assert!(all.iter().all(|(_, bytes)| !bytes.is_empty()));
    }
}
