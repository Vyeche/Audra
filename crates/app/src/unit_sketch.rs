//! "Where it plugs in" (the owner's pick, 2026-11-16, after critique #78:
//! the TotalMix codes read as codes to a non-engineer): a small sketch of
//! the Babyface's connections in a strip's Settings, the socket(s) the
//! strip is lit. Audra's own line drawing, grouped by kind (inputs and
//! outputs, mic XLR, instrument jack, optical, XLR out, both phones), not a
//! picture of the unit: it claims no positions, and says so.

use iced::advanced::text as advanced_text;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::widget::{column, text};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};
use crate::theme;

/// A connection on the unit, as the sketch draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Socket {
    /// AN1 and AN2: the mic or line inputs (XLR).
    Combo1,
    Combo2,
    /// IN3 and IN4: the instrument jacks.
    Inst3,
    Inst4,
    /// The optical input (ADAT, or S/PDIF on AS1/2).
    OpticalIn,
    /// Outputs 1/2: the XLR outs.
    XlrOut,
    /// The phones jacks (outputs 3/4): the unit has two, a large and a
    /// small one, both playing the same pair (critique #80 A 3).
    Phones,
    /// The optical output.
    OpticalOut,
}

/// The sockets channel `cid` uses: both of a linked pair. None for the
/// playback, which comes from the computer.
pub fn sockets_of(state: &TuxMix, cid: ChannelId) -> Vec<Socket> {
    let dev = &state.device;
    match cid {
        ChannelId::Input(i) => {
            let linked = dev.input_pair_linked(i / 2);
            let one = |c: usize| match c {
                0 => Some(Socket::Combo1),
                1 => Some(Socket::Combo2),
                2 => Some(Socket::Inst3),
                3 => Some(Socket::Inst4),
                _ => Some(Socket::OpticalIn),
            };
            let mut v: Vec<Socket> = if linked { [i - i % 2, i - i % 2 + 1].into_iter().filter_map(one).collect() } else { one(i).into_iter().collect() };
            v.dedup();
            v
        }
        ChannelId::Output(o) => {
            let pair = if dev.outputs_one_per_pair() { o } else { o / 2 };
            vec![match pair {
                0 => Socket::XlrOut,
                1 => Socket::Phones,
                _ => Socket::OpticalOut,
            }]
        }
        ChannelId::Playback(_) => Vec::new(),
    }
}

/// The sketch and its caption, for a strip's Settings; None for a channel
/// with no socket (the playback).
pub fn view(state: &TuxMix, cid: ChannelId, scale: f32) -> Option<Element<'_, Message>> {
    // With the plain words (critique #81 A 6: a fixed 126 px on every
    // Settings open; a TotalMix veteran turns both off with one switch).
    if !state.plain_names {
        return None;
    }
    let lit = sockets_of(state, cid);
    if lit.is_empty() {
        return None;
    }
    let canvas = canvas::Canvas::new(Sketch { lit, scale }).width(Length::Fill).height(Length::Fixed(SKETCH_H * scale));
    // A hint, so What's this covers it too (critique #80 A 9).
    Some(crate::widgets::strip::hint(
        column![
            canvas,
            text("Where it plugs in on your Babyface (a sketch, not to scale)").size(theme::TEXT_SM * scale).color(theme::text_sec()),
        ]
        .spacing(theme::SPACE_TIGHT * scale),
        "The lit socket is where this strip's cable goes",
        scale,
    ))
}

// Tall enough that the Out labels, at 12 px, keep clear of the frame
// (critique #80 A 4: 2-3 px at 92).
const SKETCH_H: f32 = 102.0;

struct Sketch {
    lit: Vec<Socket>,
    scale: f32,
}

impl<M> canvas::Program<M> for Sketch {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.scale;
        let (w, h) = (bounds.width, bounds.height);
        // The body: a rounded slab, quiet.
        let body = Path::rounded_rectangle(Point::new(1.0, 1.0), Size::new(w - 2.0, h - 2.0), (8.0 * s).into());
        frame.fill(&body, Color { a: 0.05, ..theme::text_primary() });
        frame.stroke(&body, Stroke::default().with_color(crate::flow::map_ink_shade(theme::muted())).with_width(1.5 * s));
        let lit_ink = crate::flow::map_ink(theme::accent());
        let off_ink = crate::flow::map_ink_shade(theme::muted());
        // The lit socket solid in the accent's map ink (critique #80 B: a
        // 25% fill read 1.43-1.69 on the panel; #81 B: 55% still 2.33 light,
        // 2.75 Nord, the pins 2.58): the ink reads 4.5 on the panel, and the
        // pins in the page's ground read on it as the ink does on the page.
        let lit_fill = lit_ink;
        let label = |frame: &mut Frame, x: f32, y: f32, words: &str, on: bool| {
            frame.fill_text(Text {
                content: words.to_string(),
                position: Point::new(x, y),
                color: if on { theme::readable_on_floors(theme::accent()) } else { theme::text_sec() },
                size: (theme::TEXT_SM * s).into(),
                font: if on { theme::SANS_SEMI } else { iced::Font::default() },
                align_x: advanced_text::Alignment::Center,
                align_y: iced::alignment::Vertical::Top,
                ..Text::default()
            });
        };
        let row_label = |frame: &mut Frame, y: f32, words: &str| {
            frame.fill_text(Text {
                content: words.to_string(),
                position: Point::new(10.0 * s, y),
                color: theme::text_sec(),
                size: (theme::TEXT_SM * s).into(),
                align_x: advanced_text::Alignment::Left,
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        };
        let ink = |on: bool| if on { lit_ink } else { off_ink };
        // Pins and cores: the page's ground on a lit fill, so they still show.
        let dot = |on: bool| if on { theme::surface() } else { off_ink };
        // A socket, by kind, lit or not.
        let draw = |frame: &mut Frame, sock: Socket, c: Point| {
            let on = self.lit.contains(&sock);
            let stroke = Stroke::default().with_color(ink(on)).with_width(if on { 2.5 * s } else { 1.5 * s });
            match sock {
                Socket::Combo1 | Socket::Combo2 | Socket::XlrOut => {
                    let r = if matches!(sock, Socket::XlrOut) { 7.0 } else { 9.0 } * s;
                    let disc = Path::circle(c, r);
                    if on {
                        frame.fill(&disc, lit_fill);
                    }
                    frame.stroke(&disc, stroke);
                    // Three pins, an XLR's.
                    for (dx, dy) in [(-0.4, -0.25), (0.4, -0.25), (0.0, 0.4)] {
                        frame.fill(&Path::circle(Point::new(c.x + dx * r, c.y + dy * r), 1.4 * s), dot(on));
                    }
                }
                Socket::Phones => {
                    // Two jacks, a large and a small one, side by side.
                    for (dx, r) in [(-8.0, 6.0), (7.0, 4.5)] {
                        let at = Point::new(c.x + dx * s, c.y);
                        let ring = Path::circle(at, r * s);
                        if on {
                            frame.fill(&ring, lit_fill);
                        }
                        frame.stroke(&ring, stroke);
                        frame.fill(&Path::circle(at, (r * 0.36) * s), dot(on));
                    }
                }
                Socket::Inst3 | Socket::Inst4 => {
                    let ring = Path::circle(c, 6.0 * s);
                    if on {
                        frame.fill(&ring, lit_fill);
                    }
                    frame.stroke(&ring, stroke);
                    frame.fill(&Path::circle(c, 2.2 * s), dot(on));
                }
                Socket::OpticalIn | Socket::OpticalOut => {
                    let port = Path::rounded_rectangle(Point::new(c.x - 7.0 * s, c.y - 5.5 * s), Size::new(14.0 * s, 11.0 * s), (2.0 * s).into());
                    if on {
                        frame.fill(&port, lit_fill);
                    }
                    frame.stroke(&port, stroke);
                    frame.fill(&Path::rectangle(Point::new(c.x - 3.0 * s, c.y - 1.5 * s), Size::new(6.0 * s, 3.0 * s)), dot(on));
                }
            }
        };
        let left = 30.0 * s;
        let span = w - left - 10.0 * s;
        let (in_y, out_y) = (h * 0.28, h * 0.68);
        row_label(&mut frame, in_y, "In");
        row_label(&mut frame, out_y, "Out");
        let ins = [(Socket::Combo1, "AN1"), (Socket::Combo2, "AN2"), (Socket::Inst3, "IN3"), (Socket::Inst4, "IN4"), (Socket::OpticalIn, "Optical")];
        // Spaced by kind (the combos, the jacks, then optical, whose word is
        // longest), so the labels never run together in a narrow panel.
        let in_at = [0.06, 0.27, 0.47, 0.65, 0.90];
        for (k, (sock, name)) in ins.iter().enumerate() {
            let x = left + span * in_at[k];
            draw(&mut frame, *sock, Point::new(x, in_y));
            label(&mut frame, x, in_y + 11.0 * s, name, self.lit.contains(sock));
        }
        let outs = [(Socket::XlrOut, "XLR 1/2"), (Socket::Phones, "Phones"), (Socket::OpticalOut, "Optical")];
        let out_at = [0.14, 0.50, 0.88];
        for (k, (sock, name)) in outs.iter().enumerate() {
            let x = left + span * out_at[k];
            draw(&mut frame, *sock, Point::new(x, out_y));
            label(&mut frame, x, out_y + 9.0 * s, name, self.lit.contains(sock));
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::{sockets_of, Socket};
    use tuxmix_core::ChannelId;

    #[test]
    fn each_channel_lights_its_own_sockets() {
        // The owner's pick, 2026-11-16: where it plugs in.
        let state = crate::app::new(true, None, None);
        let an1 = sockets_of(&state, ChannelId::Input(0));
        assert!(an1.contains(&Socket::Combo1));
        assert!(sockets_of(&state, ChannelId::Input(2)).iter().all(|s| matches!(s, Socket::Inst3 | Socket::Inst4)));
        assert_eq!(sockets_of(&state, ChannelId::Input(6)), vec![Socket::OpticalIn]);
        let one = state.device.outputs_one_per_pair();
        let out = |pair: usize| ChannelId::Output(if one { pair } else { pair * 2 });
        assert_eq!(sockets_of(&state, out(0)), vec![Socket::XlrOut]);
        assert_eq!(sockets_of(&state, out(1)), vec![Socket::Phones]);
        assert_eq!(sockets_of(&state, out(3)), vec![Socket::OpticalOut]);
        assert!(sockets_of(&state, ChannelId::Playback(0)).is_empty(), "the computer's, no socket");
    }
}
