//! The product artwork, decoded from the same PNGs the packaging installs
//! into the icon theme: the tile (for the window to wear, see
//! `main::window_icon`) and the bare mark (for the in-app lockup in the
//! top bar and the About dialog). One place decodes the assets, so the
//! UI and the packaging can never drift apart on which file is the icon.
//!
//! Decoding uses the `png` crate directly and feeds iced's codec-free
//! image widget (`image-without-codecs`, seven extra crates). The full
//! `image` feature would pull ~79 crates of decoders this app already
//! answers with two files and `decode()` below.

use std::sync::OnceLock;

use iced::widget::image::Handle;

/// The Audra tile icon, rendered from `assets/audra.svg`
/// (`ffmpeg -width 256 -height 256 -i audra.svg -pix_fmt rgba`).
const PNG: &[u8] = include_bytes!("../assets/audra-256.png");
/// The mark alone: the "A" with its fader-cap crossbar on transparency,
/// rendered from `assets/audra-mark.svg` at 189x256.
const MARK_PNG: &[u8] = include_bytes!("../assets/audra-mark-256.png");
/// The wordmark: the mark as the "A" of "Audra", the rest drawn in the
/// same rounded stroke (`assets/audra-wordmark.svg`, rendered 472x128).
/// The header and the About card use it, so the name never shows the
/// mark beside a second, typeset "A".
const WORDMARK_PNG: &[u8] = include_bytes!("../assets/audra-wordmark.png");

/// The tray icon at the sizes panels ask for: the mark in ivory with a
/// dark edge (`assets/audra-tray.svg`), because the tile reduced to 22 px
/// is a smear on a dark panel.
const TRAY_PNGS: [&[u8]; 4] = [
    include_bytes!("../assets/audra-tray-16.png"),
    include_bytes!("../assets/audra-tray-22.png"),
    include_bytes!("../assets/audra-tray-32.png"),
    include_bytes!("../assets/audra-tray-48.png"),
];

/// Every tray size as RGBA8 plus dimensions; a size that cannot be decoded
/// is skipped.
pub fn tray_rgba() -> Vec<(u32, u32, Vec<u8>)> {
    TRAY_PNGS.iter().filter_map(|p| decode(p)).collect()
}

/// The icon as RGBA8 plus its dimensions, or `None` when the PNG cannot be
/// decoded. A missing icon is cosmetic, so callers draw nothing instead of
/// failing the launch.
pub fn rgba() -> Option<(u32, u32, Vec<u8>)> {
    decode(PNG)
}

/// The bare mark as a ready-to-draw handle plus its width/height ratio
/// (the bird is wider than tall, so callers size it by height and scale
/// the width), decoded once per process; later calls clone the handle,
/// not the pixels.
pub fn mark() -> Option<(Handle, f32)> {
    static MARK: OnceLock<Option<(Handle, f32)>> = OnceLock::new();
    MARK.get_or_init(|| {
        let (width, height, pixels) = decode(MARK_PNG)?;
        let handle = Handle::from_rgba(width, height, pixels);
        Some((handle, width as f32 / height as f32))
    })
    .clone()
}

/// The wordmark recoloured for a theme: its letters in `ink`, its "A" mark
/// in `mark`, each pixel keeping its own coverage (alpha), so the edges
/// stay smooth. The ivory letters vanished on a light theme. On Audra's
/// own palette this matches the drawn PNG. Cached per colour pair.
/// How many recoloured wordmarks `wordmark_in` keeps.
const WORDMARK_CACHE_MAX: usize = 6;

pub fn wordmark_in(ink: iced::Color, mark: iced::Color) -> Option<(Handle, f32)> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    type Raw = Option<(u32, u32, Vec<u8>)>;
    static RAW: OnceLock<Raw> = OnceLock::new();
    type Cache = Mutex<HashMap<[u8; 6], (Handle, f32)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let (width, height, pixels) = RAW.get_or_init(|| decode(WORDMARK_PNG)).as_ref()?;
    let to8 = |c: iced::Color| {
        let [r, g, b, _] = c.into_rgba8();
        [r, g, b]
    };
    let (ink8, mark8) = (to8(ink), to8(mark));
    let key = [ink8[0], ink8[1], ink8[2], mark8[0], mark8[1], mark8[2]];
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Some(hit);
    }
    let mut out = pixels.clone();
    for px in out.as_chunks_mut::<4>().0 {
        if px[3] == 0 {
            continue;
        }
        let (max, min) = (px[0].max(px[1]).max(px[2]), px[0].min(px[1]).min(px[2]));
        // The coral mark is saturated; the letters are near-neutral ivory.
        let saturated = max > 0 && (max - min) as f32 / max as f32 > 0.25;
        let [r, g, b] = if saturated { mark8 } else { ink8 };
        px[0] = r;
        px[1] = g;
        px[2] = b;
    }
    let made = (Handle::from_rgba(*width, *height, out), *width as f32 / *height as f32);
    if let Ok(mut c) = cache.lock() {
        // Bounded: a theme fade blends the palette on every frame, and
        // each blend is a new colour pair, so an open cache kept a 236 KiB
        // copy per frame for good (the audit, 2026-11-05). A handful
        // covers the themes in use; a fade just recolours as it goes.
        if c.len() >= WORDMARK_CACHE_MAX {
            c.clear();
        }
        c.insert(key, made.clone());
    }
    Some(made)
}

fn decode(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoder = png::Decoder::new(png);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some((info.width, info.height, buf))
}

#[cfg(test)]
mod tests {
    use super::{decode, mark, rgba, MARK_PNG, WORDMARK_PNG};

    /// The packaged tile PNG is the one asset the window cannot live
    /// without: a replacement in another format, or at another size,
    /// would make `window_icon()` return `None` and the window silently
    /// lose its icon.
    #[test]
    fn the_packaged_icon_decodes_as_256_rgba() {
        let (width, height, pixels) = rgba().expect("the packaged PNG must decode");
        assert_eq!((width, height), (256, 256));
        assert_eq!(pixels.len(), (width * height * 4) as usize);
    }

    #[test]
    fn the_tray_icon_has_all_four_sizes() {
        let sizes: Vec<u32> = super::tray_rgba().iter().map(|(w, _, _)| *w).collect();
        assert_eq!(sizes, vec![16, 22, 32, 48]);
    }

    /// The mark feeds the top bar and the About dialog: it must decode,
    /// be taller than wide (the "A", not the square tile), and be bright
    /// warm ink (rose, honey, ivory) on transparency. An empty or
    /// solid-black rasterization would paint a blank box or a black one
    /// into the lockup.
    #[test]
    fn the_packaged_mark_is_bright_ink_on_transparency() {
        let (width, height, pixels) = decode(MARK_PNG).expect("the mark must decode");
        assert!(
            height > width,
            "the mark is taller than wide: {width}x{height}"
        );
        let mut drawn = 0usize;
        for px in pixels.as_chunks::<4>().0 {
            // Only fully opaque pixels are checked: the antialiased rim is
            // allowed to blend, the ink itself is not.
            if px[3] > 230 {
                drawn += 1;
                assert!(
                    px[0].max(px[1]).max(px[2]) > 0xb0,
                    "drawn pixels must be the bright ink, got {:?}",
                    &px[..3]
                );
            }
        }
        assert!(
            drawn > 1000,
            "the mark must actually be drawn, got {drawn} pixels"
        );
    }

    /// `mark()` memoizes: the second call must hand back the same ratio
    /// without touching the PNG again.
    #[test]
    fn mark_is_decoded_once_and_keeps_a_sane_ratio() {
        let (_, first) = mark().expect("the mark must decode");
        let (_, second) = mark().expect("second call");
        assert_eq!(first, second);
        // 189x256: the "A" is about three quarters as wide as it is tall.
        assert!(first > 0.70 && first < 0.78, "unexpected ratio {first}");
    }

    /// The wordmark must decode, be a wide lockup (about 3.7:1), and
    /// actually draw something.
    #[test]
    fn the_wordmark_decodes_as_a_wide_lockup() {
        let (width, height, pixels) = decode(WORDMARK_PNG).expect("the wordmark must decode");
        let ratio = width as f32 / height as f32;
        assert!(ratio > 3.4 && ratio < 4.0, "unexpected ratio {ratio}");
        let drawn = pixels.as_chunks::<4>().0.iter().filter(|px| px[3] > 230).count();
        assert!(drawn > 2000, "the wordmark must be drawn, got {drawn} pixels");
    }
}
