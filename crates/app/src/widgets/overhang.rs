//! A strip's Settings or EQ panel that hangs past its row (issue #25).
//!
//! In iced a layered group (`stack`) is only as big as its bottom layer,
//! and the rows of strips scroll sideways and clip what sticks out, so a
//! panel laid over its row ended at the strip's foot and scrolled, even
//! with the window empty below it. [`overhang`] draws the panel as an
//! overlay instead, the way iced's own tooltip does: as tall as its rows,
//! down to the page's foot, over the row below, and scrolling only when
//! the page is too short for it. It is never shorter than its row, so its
//! foot still meets the strips' feet.
//!
//! An overlay is drawn after everything else and escapes every clip, so
//! two things keep it in its place:
//! - [`page`] and [`lane`] wrap the mixer's page and each row of strips.
//!   Each one tells the panels inside it where it ends on screen
//!   (`CLIP`), and a panel draws only inside that, so a strip scrolled
//!   away never paints its panel over the sidebar, the top bar or the
//!   folded rows. (iced's own `viewport` can't say: a row that scrolls
//!   sideways crosses its own content space with the page's screen space,
//!   scrollable.rs 1370–1381, so its top edge is wrong once the page has
//!   scrolled down.)
//! - A card at the window's foot (the notice, the guide's card, with the
//!   sidebar folded) is wrapped in [`keep_clear`], and a panel above it
//!   stops short of it.
//! - While a dialog lies over the mixer (`float` false), the panel stays
//!   in its row as before, so it never draws over that dialog.

use std::cell::Cell;

use iced::advanced::layout::{Layout, Limits, Node};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, tree, Tree, Widget};
use iced::advanced::{Clipboard, Renderer as _, Shell};
use iced::{mouse, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

thread_local! {
    /// Where on screen the innermost [`page`]/[`lane`] around the panel
    /// being built ends, set while its `overlay` runs. Only the UI thread
    /// builds overlays; set and put back around the call, like
    /// `keyfocus::in_region`.
    static CLIP: Cell<Option<Rectangle>> = const { Cell::new(None) };
    /// Where a card at the window's foot sits ([`keep_clear`]): the
    /// notice or the guide's card while the sidebar is folded to its rail.
    /// Cleared by the page at the start of every overlay pass and set by
    /// the card later in it (the card's layer comes after the mixer's), so
    /// a panel reads it when it lays out, draws or is asked, never when
    /// it's built.
    static KEEP_CLEAR: Cell<Option<Rectangle>> = const { Cell::new(None) };
}

/// No clip from a host: as far as the window goes (the overlay's layout
/// stops at the window's edge anyway).
const UNBOUNDED: Rectangle = Rectangle { x: 0.0, y: 0.0, width: 100_000.0, height: 100_000.0 };

/// Which of its edges a host holds a panel inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axes {
    /// The page: every edge.
    Both,
    /// A row of strips: its left and right, so a panel scrolls out of
    /// sight with its strip; up and down it may hang past the row.
    Across,
    /// Not a host: a card the panels keep clear of ([`keep_clear`]).
    Card,
    /// Not a host: a row of strips whose hints and lists draw over a
    /// hanging panel ([`lift`]).
    Lift,
    /// Not a host: content whose height is written down ([`measure`]).
    Plain,
}

/// The clip inside a host whose own box on screen is `mine`, nested in
/// the clip `outer` of the hosts around it.
fn nest(outer: Option<Rectangle>, mine: Rectangle, axes: Axes) -> Rectangle {
    let outer = outer.unwrap_or(UNBOUNDED);
    let span = |a0: f32, a1: f32, b0: f32, b1: f32| {
        let lo = a0.max(b0);
        (lo, (a1.min(b1) - lo).max(0.0))
    };
    let (x, width) = span(outer.x, outer.x + outer.width, mine.x, mine.x + mine.width);
    let (y, height) = match axes {
        Axes::Both => span(outer.y, outer.y + outer.height, mine.y, mine.y + mine.height),
        Axes::Across | Axes::Card | Axes::Lift | Axes::Plain => (outer.y, outer.height),
    };
    Rectangle { x, y, width, height }
}

/// The tallest a panel whose top is at `top` may be: down to the clip's
/// foot (and the window's), less `margin`, but never shorter than its
/// row (`min_h`).
fn reach(top: f32, min_h: f32, clip: Rectangle, window_h: f32, margin: f32) -> f32 {
    let foot = (clip.y + clip.height).min(window_h) - margin;
    (foot - top).max(min_h)
}

/// `area` cut at the top of `card` where the card stands under the
/// panel's columns (`x0`..`x1`), so a panel stops above it.
fn cut_at(area: Rectangle, card: Option<Rectangle>, x0: f32, x1: f32) -> Rectangle {
    match card {
        Some(card) if x0 < card.x + card.width && card.x < x1 => {
            let foot = (area.y + area.height).min(card.y);
            Rectangle { height: (foot - area.y).max(0.0), ..area }
        }
        _ => area,
    }
}

/// A card at the window's foot that floating panels keep clear of: they
/// stop above it rather than covering it (`KEEP_CLEAR`).
pub fn keep_clear<'a, Message: 'a>(card: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Host { content: card.into(), axes: Axes::Card, measure: None, across: false, perch: None })
}

/// `content`, its height written into `into` at every layout: what it
/// takes when nothing squeezes it, inside a scrollable (the column's head,
/// so its card knows the room left under it).
pub fn measure<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    into: &'static std::thread::LocalKey<Cell<f32>>,
) -> Element<'a, Message> {
    Element::new(Host { content: content.into(), axes: Axes::Plain, measure: Some(into), across: false, perch: None })
}

/// `content`, its width written into `into` at every layout (the top bar's
/// pieces, so the device chip knows the room it really has).
pub fn measure_width<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    into: &'static std::thread::LocalKey<Cell<f32>>,
) -> Element<'a, Message> {
    Element::new(Host { content: content.into(), axes: Axes::Plain, measure: Some(into), across: true, perch: None })
}

/// A row of strips: its hints and its routing lists draw over a panel
/// hanging past another row. iced's renderer paints in layers, and the
/// panel's clipped rows open layers of their own; what another overlay
/// beside it then draws in the layer they share lands under the panel,
/// whatever their order (a routing list opened above a playback panel
/// showed only down to the panel's top). In a layer of its own, opened
/// after the panel's, it lies over it.
pub fn lift<'a, Message: 'a>(row: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Host { content: row.into(), axes: Axes::Lift, measure: None, across: false, perch: None })
}

/// The mixer's page: panels inside it draw only inside it.
pub fn page<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Host { content: content.into(), axes: Axes::Both, measure: None, across: false, perch: None })
}

/// A row of strips: panels inside it draw only between its left and right
/// edges, and hang past it up and down.
pub fn lane<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Host { content: content.into(), axes: Axes::Across, measure: None, across: false, perch: None })
}

/// A panel `width` wide in its row's slot. `float`: hang past the row as
/// an overlay; else lay it out in the slot, as tall as the row (the old
/// way, kept for while a dialog lies over the mixer). `hold`: a menu
/// elsewhere is open (a strip's routing list), which may lie over the
/// panel and gets the same press (iced hands every overlay the pointer):
/// the panel then blocks the pointer but passes no press to its rows, and
/// publishes `hold`'s message on one, in case the menu closed unsaid.
/// `margin`: the gap kept above the page's foot.
pub fn overhang<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message>>,
    width: f32,
    float: bool,
    hold: Option<Message>,
    margin: f32,
) -> Element<'a, Message> {
    Element::new(Overhang { content: content.into(), width, float, hold, margin })
}

/// See [`page`] and [`lane`]: everything goes straight to the content, its
/// tree included; only `overlay` sets `CLIP` around the content's own.
struct Host<'a, Message> {
    content: Element<'a, Message>,
    axes: Axes,
    measure: Option<&'static std::thread::LocalKey<Cell<f32>>>,
    /// Its width written down instead of its height (`measure_width`).
    across: bool,
    /// Its box on screen written down as the helper cat's perch `id`
    /// (`perch`).
    perch: Option<u8>,
}

thread_local! {
    /// The mixer's page on screen while its rows draw (`page`): a perch
    /// inside it is drawn at its unscrolled place, and is moved by the
    /// page's scroll from here (Opus's review: a scrolled page put the cat
    /// over a strip's name plate).
    static PAGE: Cell<Option<Rectangle>> = const { Cell::new(None) };
    /// Where the helper cat may rest, as last drawn: an empty stretch's box
    /// on screen by perch id, and when it was drawn (`perch`, `perches`).
    static PERCHES: std::cell::RefCell<Vec<(u8, Rectangle, std::time::Instant)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// `content`, an empty stretch the helper cat may rest on (the gap in the
/// top bar, a row heading's middle): its box on screen written down at
/// every draw, under perch `id` (`helper`).
pub fn perch<'a, Message: 'a>(content: impl Into<Element<'a, Message>>, id: u8) -> Element<'a, Message> {
    Element::new(Host { content: content.into(), axes: Axes::Plain, measure: None, across: false, perch: Some(id) })
}

/// The perches drawn within the last two seconds (an empty stretch that
/// went, a folded row's heading, drops out), by id.
pub fn perches() -> Vec<(u8, Rectangle)> {
    let now = std::time::Instant::now();
    PERCHES.with(|p| p.borrow().iter().filter(|(_, _, at)| now.saturating_duration_since(*at).as_secs_f32() < 2.0).map(|&(id, r, _)| (id, r)).collect())
}

impl<Message> Widget<Message, Theme, Renderer> for Host<'_, Message> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        let node = self.content.as_widget_mut().layout(tree, renderer, limits);
        if let Some(into) = self.measure {
            let size = node.size();
            into.with(|h| h.set(if self.across { size.width } else { size.height }));
        }
        node
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(id) = self.perch {
            let mut at = layout.bounds();
            // Inside the scrolling page: moved by its scroll (the viewport
            // here is the page's visible part in its own coordinates), and
            // none while scrolled out of sight.
            let visible = match PAGE.with(Cell::get) {
                Some(page) => {
                    at.x -= viewport.x - page.x;
                    at.y -= viewport.y - page.y;
                    at.y >= page.y && at.y + at.height <= page.y + page.height
                }
                None => true,
            };
            PERCHES.with(|p| {
                let mut p = p.borrow_mut();
                p.retain(|(i, _, _)| *i != id);
                if visible {
                    p.push((id, at, std::time::Instant::now()));
                }
            });
        }
        if matches!(self.axes, Axes::Both) {
            PAGE.with(|p| p.set(Some(layout.bounds())));
            self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
            PAGE.with(|p| p.set(None));
            return;
        }
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        // `translation` carries every scroll above this host, so this is
        // the host's own box on screen, whatever `viewport` says.
        let mine = layout.bounds() + translation;
        match self.axes {
            Axes::Lift => {
                return self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation).map(lifted);
            }
            Axes::Plain => return self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation),
            Axes::Card => {
                KEEP_CLEAR.with(|k| k.set(Some(k.get().map_or(mine, |was| was.union(&mine)))));
                return self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation);
            }
            // A new pass: no card yet (one that is up says so after this).
            Axes::Both => KEEP_CLEAR.with(|k| k.set(None)),
            Axes::Across => {}
        }
        let outer = CLIP.with(|c| c.replace(Some(nest(c.get(), mine, self.axes))));
        let built = self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation);
        CLIP.with(|c| c.set(outer));
        built
    }
}

/// See [`overhang`].
struct Overhang<'a, Message> {
    content: Element<'a, Message>,
    width: f32,
    float: bool,
    hold: Option<Message>,
    margin: f32,
}

/// The panel's own pointer state, as it last said it (`Hanging`): kept in
/// its tree, since the overlay is built afresh for every call.
type Said = Cell<mouse::Interaction>;

impl<Message: Clone> Widget<Message, Theme, Renderer> for Overhang<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Said>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Said::new(mouse::Interaction::None))
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        // As tall as the row (the stack's layer is the row's size), as it
        // was when the panel itself filled the slot.
        Size::new(Length::Fixed(self.width), Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        let size = limits.resolve(Length::Fixed(self.width), Length::Fill, Size::ZERO);
        if self.float {
            // The slot only: the panel itself is laid out by the overlay.
            // (Laid out here too, at the row's height, its text would be
            // laid out twice a frame at two sizes.)
            Node::new(size)
        } else {
            let child = self.content.as_widget_mut().layout(&mut tree.children[0], renderer, &Limits::new(size, size));
            Node::with_children(size, vec![child])
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let (false, Some(child)) = (self.float, layout.children().next()) {
            self.content.as_widget_mut().update(&mut tree.children[0], event, child, cursor, renderer, clipboard, shell, viewport);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        match (self.float, layout.children().next()) {
            (false, Some(child)) => self.content.as_widget().mouse_interaction(&tree.children[0], child, cursor, viewport, renderer),
            _ => mouse::Interaction::None,
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let (false, Some(child)) = (self.float, layout.children().next()) {
            self.content.as_widget().draw(&tree.children[0], renderer, theme, style, child, cursor, viewport);
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        // Floating, the overlay's `operate` reaches the panel (the
        // runtime operates the overlays after the base), so the keyboard's
        // scroll-into-view (`PANEL_SCROLL_ID`) still finds it.
        if let (false, Some(child)) = (self.float, layout.children().next()) {
            self.content.as_widget_mut().operate(&mut tree.children[0], child, renderer, operation);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        if !self.float {
            let child = layout.children().next()?;
            return self.content.as_widget_mut().overlay(&mut tree.children[0], child, renderer, viewport, translation);
        }
        let slot = layout.bounds();
        let Tree { state, children, .. } = tree;
        Some(overlay::Element::new(Box::new(Hanging {
            content: &mut self.content,
            tree: &mut children[0],
            said: state.downcast_ref::<Said>(),
            hold: self.hold.clone(),
            origin: slot.position() + translation,
            // Its own width, not the slot's: a row narrower than the panel
            // (with its map beside it, `side_by_side`) squeezes the slot,
            // and the overlay isn't bound by the row.
            width: self.width,
            min_h: slot.height,
            area: CLIP.with(|c| c.get()).unwrap_or(UNBOUNDED),
            margin: self.margin,
        })))
    }
}

/// The panel drawn over the page, from its slot's top left (`origin`, on
/// screen) down.
struct Hanging<'a, 'b, Message> {
    content: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    /// Its pointer state, for what opens inside it (`Relay`).
    said: &'b Said,
    /// See [`overhang`].
    hold: Option<Message>,
    origin: Point,
    width: f32,
    min_h: f32,
    /// Where its hosts end on screen (`CLIP`); `clip` cuts it at a card.
    area: Rectangle,
    margin: f32,
}

impl<Message: Clone> Hanging<'_, '_, Message> {
    /// Where it may draw: its hosts' `area`, stopped above a card at the
    /// window's foot under it (`KEEP_CLEAR`).
    fn clip(&self) -> Rectangle {
        cut_at(self.area, KEEP_CLEAR.with(|k| k.get()), self.origin.x, self.origin.x + self.width)
    }

    /// The pointer, where the panel shows: a part of it out of sight under
    /// the clip takes no hover or press.
    fn seen(&self, cursor: mouse::Cursor) -> mouse::Cursor {
        match cursor.position() {
            Some(p) if self.clip().contains(p) => cursor,
            _ => mouse::Cursor::Unavailable,
        }
    }

    /// What the panel says of the pointer, kept in `said` too. The runtime
    /// hands the strips below the pointer unless this says something:
    /// over the panel's own words or padding it says Idle, so a wheel or a
    /// drag there never reaches the fader underneath.
    fn interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &Renderer) -> mouse::Interaction {
        let cursor = self.seen(cursor);
        let said = match cursor.position() {
            None => mouse::Interaction::None,
            Some(p) => {
                let own = self.content.as_widget().mouse_interaction(self.tree, layout, cursor, &self.clip(), renderer);
                if own == mouse::Interaction::None && layout.bounds().contains(p) {
                    mouse::Interaction::Idle
                } else {
                    own
                }
            }
        };
        self.said.set(said);
        said
    }
}

/// Something that opens inside the panel: a hint, the preset list. With
/// one open, iced asks only it what the pointer is over (overlay/nested.rs
/// 276–285), and a hint says nothing, so the strips under the panel got
/// the pointer back: a wheel over the panel moved the fader below while a
/// hint showed. This answers for the panel as well (`said`), and for
/// itself wherever it's drawn.
struct Relay<'c, Message> {
    inner: overlay::Element<'c, Message, Theme, Renderer>,
    said: &'c Said,
}

impl<Message> overlay::Overlay<Message, Theme, Renderer> for Relay<'_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> Node {
        self.inner.as_overlay_mut().layout(renderer, bounds)
    }

    fn draw(&self, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        self.inner.as_overlay().draw(renderer, theme, style, layout, cursor);
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn widget::Operation) {
        self.inner.as_overlay_mut().operate(layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        self.inner.as_overlay_mut().update(event, layout, cursor, renderer, clipboard, shell);
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &Renderer) -> mouse::Interaction {
        let own = self.inner.as_overlay().mouse_interaction(layout, cursor, renderer);
        let over_it = cursor.position().is_some_and(|p| layout.bounds().contains(p));
        own.max(self.said.get()).max(if over_it { mouse::Interaction::Idle } else { mouse::Interaction::None })
    }

    fn overlay<'d>(&'d mut self, layout: Layout<'d>, renderer: &Renderer) -> Option<overlay::Element<'d, Message, Theme, Renderer>> {
        let said = self.said;
        self.inner.as_overlay_mut().overlay(layout, renderer).map(|inner| relay(inner, said))
    }

    fn index(&self) -> f32 {
        self.inner.as_overlay().index()
    }
}

/// See [`lift`]: an overlay drawn in a layer of its own.
struct Lifted<'c, Message> {
    inner: overlay::Element<'c, Message, Theme, Renderer>,
}

impl<Message> overlay::Overlay<Message, Theme, Renderer> for Lifted<'_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> Node {
        self.inner.as_overlay_mut().layout(renderer, bounds)
    }

    fn draw(&self, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        renderer.with_layer(UNBOUNDED, |renderer| self.inner.as_overlay().draw(renderer, theme, style, layout, cursor));
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn widget::Operation) {
        self.inner.as_overlay_mut().operate(layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        self.inner.as_overlay_mut().update(event, layout, cursor, renderer, clipboard, shell);
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &Renderer) -> mouse::Interaction {
        self.inner.as_overlay().mouse_interaction(layout, cursor, renderer)
    }

    fn overlay<'d>(&'d mut self, layout: Layout<'d>, renderer: &Renderer) -> Option<overlay::Element<'d, Message, Theme, Renderer>> {
        self.inner.as_overlay_mut().overlay(layout, renderer)
    }

    fn index(&self) -> f32 {
        self.inner.as_overlay().index()
    }
}

fn lifted<'c, Message: 'c>(inner: overlay::Element<'c, Message, Theme, Renderer>) -> overlay::Element<'c, Message, Theme, Renderer> {
    overlay::Element::new(Box::new(Lifted { inner }))
}

fn relay<'c, Message: 'c>(inner: overlay::Element<'c, Message, Theme, Renderer>, said: &'c Said) -> overlay::Element<'c, Message, Theme, Renderer> {
    overlay::Element::new(Box::new(Relay { inner, said }))
}

impl<Message: Clone> overlay::Overlay<Message, Theme, Renderer> for Hanging<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> Node {
        let max_h = reach(self.origin.y, self.min_h, self.clip(), bounds.height, self.margin);
        // First its own height: `Fill` taken as "as much as it needs"
        // (compressed limits), its rows' scroll included, up to `max_h`.
        let measure = Limits::with_compression(Size::ZERO, Size::new(self.width, max_h), Size::new(false, true));
        let natural = self.content.as_widget_mut().layout(self.tree, renderer, &measure).size().height;
        // Then at that height exactly, at least the row's, so the card
        // and its scroll fill it (`Fill` is the card's height in its row
        // too). Both passes give its rows the same width and an endless
        // height, so their text is laid out once.
        let h = natural.clamp(self.min_h, max_h);
        let exact = Size::new(self.width, h);
        self.content.as_widget_mut().layout(self.tree, renderer, &Limits::new(exact, exact)).move_to(self.origin)
    }

    fn draw(&self, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        let cursor = self.seen(cursor);
        let clip = self.clip();
        renderer.with_layer(clip, |renderer| {
            self.content.as_widget().draw(self.tree, renderer, theme, style, layout, cursor, &clip);
        });
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn widget::Operation) {
        self.content.as_widget_mut().operate(self.tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let mut seen = self.seen(cursor);
        if let Some(hold) = &self.hold {
            // Under an open menu: no hover, no press, no wheel for the
            // rows; a press here closes the menu, as a press outside one
            // does.
            let here = seen.position().is_some_and(|p| layout.bounds().contains(p));
            if here && matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) {
                shell.publish(hold.clone());
            }
            seen = mouse::Cursor::Unavailable;
        }
        let clip = self.clip();
        self.content.as_widget_mut().update(self.tree, event, layout, seen, renderer, clipboard, shell, &clip);
        // Said again after the event (a drag may have begun), for `Relay`.
        let _ = self.interaction(layout, cursor, renderer);
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &Renderer) -> mouse::Interaction {
        self.interaction(layout, cursor, renderer)
    }

    fn index(&self) -> f32 {
        // Under the other overlays (hints, a strip's routing list; 1.0),
        // as it was under them in its row: iced draws and updates
        // overlays side by side in this order (overlay/group.rs).
        0.5
    }

    fn overlay<'c>(&'c mut self, layout: Layout<'c>, renderer: &Renderer) -> Option<overlay::Element<'c, Message, Theme, Renderer>> {
        // The panel's own hints and menus, over it, each answering for the
        // panel too (`Relay`).
        let clip = self.clip();
        let said = self.said;
        self.content.as_widget_mut().overlay(self.tree, layout, renderer, &clip, Vector::ZERO).map(|inner| relay(inner, said))
    }
}

/// Two cards side by side, as tall as each other: as tall as the taller
/// needs, within what they're given (issue #23: the map beside a strip's
/// panel, "the heights don't line up"). iced's `row` can't measure cards
/// that fill their height (in a measuring pass it gives them none), so a
/// hanging panel with its map beside it came out only row-high; this
/// measures each on its own, then lays both out at the taller one's height.
pub fn side_by_side<'a, Message: 'a>(
    first: impl Into<Element<'a, Message>>,
    second: impl Into<Element<'a, Message>>,
    gap: f32,
) -> Element<'a, Message> {
    Element::new(SideBySide { cards: [first.into(), second.into()], gap })
}

struct SideBySide<'a, Message> {
    cards: [Element<'a, Message>; 2],
    gap: f32,
}

impl<Message> Widget<Message, Theme, Renderer> for SideBySide<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        self.cards.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.cards);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        let (min, max) = (limits.min(), limits.max());
        let gap = self.gap;
        // Each on its own, `Fill` taken as what it needs.
        let measure = Limits::with_compression(Size::ZERO, max, Size::new(false, true));
        let natural = self
            .cards
            .iter_mut()
            .zip(tree.children.iter_mut())
            .map(|(card, t)| card.as_widget_mut().layout(t, renderer, &measure).size())
            .fold(Size::ZERO, |a, b| Size::new(a.width + b.width, a.height.max(b.height)));
        // Measuring (compressed): the taller one's height; else what's given.
        let h = if limits.compression().height { natural.height.clamp(min.height, max.height) } else { max.height };
        let mut x = 0.0;
        let mut nodes = Vec::with_capacity(2);
        for (card, t) in self.cards.iter_mut().zip(tree.children.iter_mut()) {
            let exact = Limits::new(Size::new(0.0, h), Size::new(max.width, h));
            let node = card.as_widget_mut().layout(t, renderer, &exact).move_to(Point::new(x, 0.0));
            x += node.size().width + gap;
            nodes.push(node);
        }
        Node::with_children(Size::new((x - gap).max(0.0), h), nodes)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for ((card, t), l) in self.cards.iter_mut().zip(tree.children.iter_mut()).zip(layout.children()) {
            card.as_widget_mut().update(t, event, l, cursor, renderer, clipboard, shell, viewport);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.cards
            .iter()
            .zip(tree.children.iter())
            .zip(layout.children())
            .map(|((card, t), l)| card.as_widget().mouse_interaction(t, l, cursor, viewport, renderer))
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for ((card, t), l) in self.cards.iter().zip(tree.children.iter()).zip(layout.children()) {
            card.as_widget().draw(t, renderer, theme, style, l, cursor, viewport);
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        for ((card, t), l) in self.cards.iter_mut().zip(tree.children.iter_mut()).zip(layout.children()) {
            card.as_widget_mut().operate(t, l, renderer, operation);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(&mut self.cards, tree, layout, renderer, viewport, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rectangle {
        Rectangle { x, y, width: w, height: h }
    }

    #[test]
    fn a_page_clips_every_edge_and_a_lane_only_across() {
        let page = nest(None, rect(240.0, 80.0, 1300.0, 900.0), Axes::Both);
        assert_eq!(page, rect(240.0, 80.0, 1300.0, 900.0));
        // A row scrolled half under the page's top: up and down it keeps
        // the page's edges, across its own.
        let row = nest(Some(page), rect(300.0, -100.0, 800.0, 400.0), Axes::Across);
        assert_eq!(row, rect(300.0, 80.0, 800.0, 900.0));
        // A row wider than the page: the page's sides.
        let wide = nest(Some(page), rect(0.0, 200.0, 3000.0, 400.0), Axes::Across);
        assert_eq!(wide, rect(240.0, 80.0, 1300.0, 900.0));
        // A host wholly outside: nothing shows.
        let gone = nest(Some(page), rect(2000.0, 2000.0, 100.0, 100.0), Axes::Both);
        assert_eq!(gone.width, 0.0);
        assert_eq!(gone.height, 0.0);
    }

    #[test]
    fn a_panel_stops_above_a_card_only_where_it_stands_over_it() {
        let area = rect(0.0, 80.0, 1600.0, 900.0);
        let card = Some(rect(24.0, 860.0, 320.0, 40.0));
        // Over the card's columns: cut at its top.
        assert_eq!(cut_at(area, card, 140.0, 350.0), rect(0.0, 80.0, 1600.0, 780.0));
        // Beside it: the whole page.
        assert_eq!(cut_at(area, card, 400.0, 600.0), area);
        // No card: the whole page.
        assert_eq!(cut_at(area, None, 140.0, 350.0), area);
    }

    #[test]
    fn a_panel_reaches_the_page_foot_but_never_ends_above_its_row() {
        let page = rect(0.0, 80.0, 1600.0, 1500.0);
        // Room below: down to the foot, less the margin.
        assert_eq!(reach(300.0, 500.0, page, 1600.0, 12.0), 1580.0 - 12.0 - 300.0);
        // The window ends before the page does: the window's edge.
        assert_eq!(reach(300.0, 500.0, page, 1000.0, 12.0), 1000.0 - 12.0 - 300.0);
        // A row taller than what's left: the row's height (it scrolls,
        // as before).
        assert_eq!(reach(1200.0, 500.0, page, 1600.0, 12.0), 500.0);
    }
}
