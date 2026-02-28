use iced::advanced::renderer::Renderer as _;
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{layout, renderer, Clipboard, Layout, Shell};
use iced::mouse;
use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector,
};

use iced::advanced::Overlay;
use iced::overlay;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TooltipStyle {
    pub background: Color,
    pub border: Border,
    pub shadow_color: Color,
    pub shadow_offset: Vector,
    pub padding: f32,
}

impl Default for TooltipStyle {
    fn default() -> Self {
        Self {
            background: Color::from_rgb(1.0, 1.0, 1.0),
            border: Border {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow_color: Color::from_rgba(0.0, 0.0, 0.0, 0.15),
            shadow_offset: Vector::new(2.0, 2.0),
            padding: 6.0,
        }
    }
}

/// A lightweight, message-driven tooltip overlay.
///
/// - `show` and `position` are controlled externally (e.g. by app state)
/// - The overlay is intentionally non-interactive (does not capture mouse events)
pub(crate) struct Tooltip<'a> {
    underlay: Element<'a, crate::Message>,
    overlay_content: Element<'a, crate::Message>,
    show: bool,
    position: Point,
    offset: Vector,
    style: TooltipStyle,
}

impl std::fmt::Debug for Tooltip<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tooltip")
            .field("show", &self.show)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl<'a> Tooltip<'a> {
    pub fn new(
        underlay: impl Into<Element<'a, crate::Message>>,
        overlay_content: impl Into<Element<'a, crate::Message>>,
    ) -> Self {
        Self {
            underlay: underlay.into(),
            overlay_content: overlay_content.into(),
            show: false,
            position: Point::ORIGIN,
            offset: Vector::new(10.0, 10.0),
            style: TooltipStyle::default(),
        }
    }

    #[must_use]
    pub fn show(mut self, show: bool) -> Self {
        self.show = show;
        self
    }

    #[must_use]
    pub fn position(mut self, position: Point) -> Self {
        self.position = position;
        self
    }

    // Keep these as private for now; we can expose them once we have a use.
}

impl<'a> Widget<crate::Message, Theme, Renderer> for Tooltip<'a> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.underlay), Tree::new(&self.overlay_content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.underlay, &self.overlay_content]);
    }

    fn size(&self) -> Size<Length> {
        self.underlay.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.underlay
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        self.underlay.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, crate::Message>,
        viewport: &Rectangle,
    ) {
        self.underlay.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.underlay.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.underlay
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, crate::Message, Theme, Renderer>> {
        if !self.show {
            return self.underlay.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            );
        }

        self.overlay_content
            .as_widget_mut()
            .diff(&mut tree.children[1]);

        Some(
            TooltipOverlay::new(
                self.position + translation,
                self.offset,
                self.style,
                &mut tree.children[1],
                &mut self.overlay_content,
            )
            .overlay(),
        )
    }
}

impl<'a> From<Tooltip<'a>> for Element<'a, crate::Message> {
    fn from(widget: Tooltip<'a>) -> Self {
        Element::new(widget)
    }
}

#[derive(Debug, Default)]
struct State;

struct TooltipOverlay<'a, 'b> {
    anchor: Point,
    offset: Vector,
    style: TooltipStyle,
    tree: &'b mut Tree,
    content: &'b mut Element<'a, crate::Message>,
}

impl<'a, 'b> TooltipOverlay<'a, 'b> {
    fn new(
        anchor: Point,
        offset: Vector,
        style: TooltipStyle,
        tree: &'b mut Tree,
        content: &'b mut Element<'a, crate::Message>,
    ) -> Self {
        Self {
            anchor,
            offset,
            style,
            tree,
            content,
        }
    }

    fn overlay(self) -> overlay::Element<'b, crate::Message, Theme, Renderer> {
        overlay::Element::new(Box::new(self))
    }
}

impl Overlay<crate::Message, Theme, Renderer> for TooltipOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds);

        let mut content = self
            .content
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);

        let padding = self.style.padding;
        let background_w = content.size().width + padding * 2.0;
        let background_h = content.size().height + padding * 2.0;

        let mut position = Point::new(self.anchor.x + self.offset.x, self.anchor.y + self.offset.y);

        if position.x + background_w > bounds.width {
            position.x = (self.anchor.x - background_w - self.offset.x).max(0.0);
        }
        if position.y + background_h > bounds.height {
            position.y = (self.anchor.y - background_h - self.offset.y).max(0.0);
        }

        content.move_to_mut(Point::new(position.x + padding, position.y + padding));

        layout::Node::with_children(bounds, vec![content])
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        let padding = self.style.padding;
        let Some(content_layout) = layout.children().next() else {
            return;
        };

        let content_bounds = content_layout.bounds();

        let background_bounds = Rectangle {
            x: content_bounds.x - padding,
            y: content_bounds.y - padding,
            width: content_bounds.width + padding * 2.0,
            height: content_bounds.height + padding * 2.0,
        };

        let shadow_bounds = Rectangle {
            x: background_bounds.x + self.style.shadow_offset.x,
            y: background_bounds.y + self.style.shadow_offset.y,
            width: background_bounds.width,
            height: background_bounds.height,
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds: shadow_bounds,
                border: Border {
                    radius: self.style.border.radius,
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                ..Default::default()
            },
            self.style.shadow_color,
        );

        renderer.fill_quad(
            renderer::Quad {
                bounds: background_bounds,
                border: self.style.border,
                ..Default::default()
            },
            self.style.background,
        );

        self.content.as_widget().draw(
            self.tree,
            renderer,
            _theme,
            _style,
            content_layout,
            mouse::Cursor::Unavailable,
            &layout.bounds(),
        );
    }

    fn update(
        &mut self,
        _event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        _shell: &mut Shell<'_, crate::Message>,
    ) {
        // Intentionally ignore events: tooltips are display-only and must not
        // interfere with underlying interactions.
    }

    fn mouse_interaction(
        &self,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        mouse::Interaction::None
    }

    fn index(&self) -> f32 {
        // Ensure the tooltip stays above other overlays.
        10_000.0
    }
}
