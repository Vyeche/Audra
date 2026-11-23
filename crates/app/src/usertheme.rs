//! Themes people make themselves (PRODUCT.md: "people must be able to
//! create their own themes"): JSON files in `~/.config/audra/themes/`,
//! offered in the View tab beside the built-in ones.
//!
//! A theme file names a built-in theme to start from and the colours it
//! changes, so a small change is a small file:
//!
//! ```json
//! { "name": "Studio B", "based_on": "nord", "colors": { "accent": "#ff9e64" } }
//! ```
//!
//! "Save as your own" writes the theme showing now with every colour, as a
//! starting point. A loaded theme is checked with the same contrast rules
//! as the built-in ones (`theme::contrast_issues`); one that falls short
//! still loads, and its tile says where.

use std::path::{Path, PathBuf};

use iced::Color;
use serde::{Deserialize, Serialize};

use crate::theme::{self, Palette, ThemeEntry};

/// One theme file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeFile {
    pub name: String,
    /// A built-in theme's id ("audra", "light", "nord", …); Audra's when
    /// missing or unknown.
    #[serde(default)]
    pub based_on: Option<String>,
    /// Colour role → "#rrggbb"; `floors` takes three.
    #[serde(default)]
    pub colors: serde_json::Map<String, serde_json::Value>,
}

/// The folder the themes live in, beside the preferences. None in tests,
/// which must never read or write the user's own.
pub fn dir() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::prefs::path().and_then(|p| p.parent().map(|d| d.join("themes")))
}

macro_rules! roles {
    ($($role:ident),* $(,)?) => {
        /// Every single-colour role, by its name in a theme file.
        pub const ROLES: &[&str] = &[$(stringify!($role)),*];

        fn get(p: &Palette, role: &str) -> Option<Color> {
            match role {
                $(stringify!($role) => Some(p.$role),)*
                _ => None,
            }
        }

        fn set(p: &mut Palette, role: &str, c: Color) -> bool {
            match role {
                $(stringify!($role) => {
                    p.$role = c;
                    true
                })*
                _ => false,
            }
        }
    };
}

roles!(
    bg_deep, surface, surface_hi, border, control_edge, text_primary, text_sec, accent, accent_dim, fader,
    on_active, signal, attention, clip, mute, phantom, top_bar, counter, muted, lamp, on_placard_sec,
);

fn hex(c: Color) -> String {
    let [r, g, b, _] = c.into_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some(Color::from_rgb8(byte(0)?, byte(2)?, byte(4)?))
}

/// The palette a file describes, and what in it could not be read.
pub fn palette_of(file: &ThemeFile) -> (Palette, Vec<String>) {
    let base = file
        .based_on
        .as_deref()
        .and_then(|id| theme::THEMES.iter().find(|t| t.id == id))
        .unwrap_or(&theme::THEMES[0]);
    let mut p = *base.palette;
    let mut problems = Vec::new();
    for (role, value) in &file.colors {
        if role == "floors" {
            let floors: Option<Vec<Color>> = value
                .as_array()
                .filter(|a| a.len() == 3)
                .and_then(|a| a.iter().map(|v| v.as_str().and_then(parse_hex)).collect());
            match floors {
                Some(f) => p.floors = [f[0], f[1], f[2]],
                None => problems.push("floors: three \"#rrggbb\" colours".to_string()),
            }
            continue;
        }
        match value.as_str().and_then(parse_hex) {
            Some(c) if set(&mut p, role, c) => {}
            Some(_) => problems.push(format!("unknown colour \"{role}\"")),
            None => problems.push(format!("{role}: not a \"#rrggbb\" colour")),
        }
    }
    (p, problems)
}

/// A file with every colour of `palette`, starting from `based_on`.
pub fn file_of(name: &str, based_on: &str, palette: &Palette) -> ThemeFile {
    let mut colors = serde_json::Map::new();
    for role in ROLES {
        if let Some(c) = get(palette, role) {
            colors.insert(role.to_string(), hex(c).into());
        }
    }
    colors.insert(
        "floors".into(),
        serde_json::Value::Array(palette.floors.iter().map(|c| hex(*c).into()).collect()),
    );
    ThemeFile { name: name.to_string(), based_on: Some(based_on.to_string()), colors }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The themes in `dir`, in file-name order, and a line per file that could
/// not be read (or a colour in it). Ids are "file:" and the file's stem, so
/// the preferences find the same theme next time.
pub fn load_dir(dir: &Path) -> (Vec<ThemeEntry>, Vec<String>) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    for path in paths {
        let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let file: ThemeFile = match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
        {
            Ok(f) => f,
            Err(e) => {
                problems.push(format!("{stem}.json: {e}"));
                continue;
            }
        };
        let (palette, bad) = palette_of(&file);
        problems.extend(bad.into_iter().map(|b| format!("{stem}.json: {b}")));
        let issues: Vec<&'static str> = theme::contrast_issues(&palette)
            .into_iter()
            .map(|i| leak(format!("Hard to read: {i}")))
            .collect();
        let name = if file.name.trim().is_empty() { stem.clone() } else { file.name.trim().to_string() };
        entries.push(ThemeEntry {
            id: leak(format!("file:{stem}")),
            name: leak(name),
            blurb: leak(format!("Your theme, from {stem}.json")),
            palette: Box::leak(Box::new(palette)),
            custom: true,
            issues: Box::leak(issues.into_boxed_slice()),
        });
    }
    (entries, problems)
}

/// Writes `file` into `dir` as "<name>.json", never over another file
/// ("<name> 2.json" then), and returns where.
pub fn save(dir: &Path, file: &ThemeFile) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let base: String = file
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect();
    let base = base.trim();
    let base = if base.is_empty() { "My theme" } else { base };
    let mut path = dir.join(format!("{base}.json"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base} {n}.json"));
        n += 1;
    }
    let json = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("audra-themes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_small_file_changes_a_built_in_theme() {
        let dir = scratch("small");
        std::fs::write(
            dir.join("studio.json"),
            r##"{ "name": "Studio B", "based_on": "nord", "colors": { "accent": "#ff9e64" } }"##,
        )
        .unwrap();
        let (entries, problems) = load_dir(&dir);
        assert!(problems.is_empty(), "{problems:?}");
        let e = &entries[0];
        assert_eq!((e.id, e.name, e.custom), ("file:studio", "Studio B", true));
        let nord = theme::THEMES[theme::theme_index("nord")].palette;
        assert_eq!(e.palette.accent, Color::from_rgb8(0xff, 0x9e, 0x64));
        assert_eq!(e.palette.surface, nord.surface, "the rest is Nord's");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_cannot_be_read_is_reported_and_a_poor_theme_is_flagged() {
        let dir = scratch("bad");
        std::fs::write(dir.join("broken.json"), "{ not json").unwrap();
        std::fs::write(
            dir.join("dim.json"),
            r##"{ "name": "Dim", "colors": { "text_sec": "#2a2129", "glow": "#ffffff", "accent": "red" } }"##,
        )
        .unwrap();
        let (entries, problems) = load_dir(&dir);
        assert_eq!(entries.len(), 1, "the broken file is skipped");
        assert!(problems.iter().any(|p| p.starts_with("broken.json")));
        assert!(problems.iter().any(|p| p.contains("unknown colour \"glow\"")));
        assert!(problems.iter().any(|p| p.contains("accent: not")));
        assert!(entries[0].issues.iter().any(|i| i.starts_with("Hard to read: secondary text")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_saved_theme_loads_back_the_same_and_never_overwrites() {
        let dir = scratch("save");
        let mono = theme::THEMES[theme::theme_index("monokai")].palette;
        let file = file_of("My Monokai", "monokai", mono);
        let first = save(&dir, &file).unwrap();
        let second = save(&dir, &file).unwrap();
        assert_ne!(first, second);
        assert!(second.ends_with("My Monokai 2.json"));
        let (entries, problems) = load_dir(&dir);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(*entries[0].palette, *mono);
        assert!(entries[0].issues.is_empty(), "Monokai reads well");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registered_themes_are_offered_and_found_by_id() {
        let dir = scratch("reg");
        std::fs::write(dir.join("mine.json"), r##"{ "name": "Mine" }"##).unwrap();
        let (entries, _) = load_dir(&dir);
        theme::register(entries);
        assert_eq!(theme::themes().len(), theme::THEMES.len() + 1);
        assert_eq!(theme::theme_index("file:mine"), theme::THEMES.len());
        theme::register(Vec::new());
        assert_eq!(theme::theme_index("file:mine"), 0, "gone: Audra's");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
