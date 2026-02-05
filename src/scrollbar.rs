use iced::mouse;
use iced::widget::canvas::{self, Action, Canvas, Geometry, Program};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Theme, Vector};
use std::sync::Arc;

const DEFAULT_HEIGHT: f32 = 18.0;
const TRACK_HEIGHT: f32 = 6.0;
const TRACK_PADDING: f32 = 6.0;
const MIN_THUMB_WIDTH: f32 = 24.0;

pub fn scrollbar<'a, Message>(
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    on_change: impl Fn(f64) -> Message + 'a,
) -> Scrollbar<'a, Message> {
    Scrollbar::new(value, range, on_change)
}

pub struct Scrollbar<'a, Message> {
    value: f64,
    min: f64,
    max: f64,
    thumb_fraction: f64,
    width: Length,
    height: Length,
    on_change: Arc<dyn Fn(f64) -> Message + 'a>,
}

impl<'a, Message> Scrollbar<'a, Message> {
    pub fn new(
        value: f64,
        range: std::ops::RangeInclusive<f64>,
        on_change: impl Fn(f64) -> Message + 'a,
    ) -> Self {
        let (min, max) = (*range.start(), *range.end());
        let (min, max) = if min <= max { (min, max) } else { (max, min) };
        let value = value.clamp(min, max);
        Self {
            value,
            min,
            max,
            thumb_fraction: 0.2,
            width: Length::Fill,
            height: Length::Fixed(DEFAULT_HEIGHT),
            on_change: Arc::new(on_change),
        }
    }

    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    pub fn thumb_fraction(mut self, fraction: f64) -> Self {
        self.thumb_fraction = fraction.clamp(0.02, 1.0);
        self
    }
}

impl<'a, Message> From<Scrollbar<'a, Message>> for Element<'a, Message>
where
    Message: 'a,
{
    fn from(scrollbar: Scrollbar<'a, Message>) -> Self {
        let Scrollbar {
            value,
            min,
            max,
            thumb_fraction,
            width,
            height,
            on_change,
        } = scrollbar;
        let program = ScrollbarProgram {
            value,
            min,
            max,
            thumb_fraction,
            on_change,
        };
        Canvas::new(program).width(width).height(height).into()
    }
}

#[derive(Default)]
struct ScrollbarState {
    dragging: bool,
    drag_offset_x: f64,
    last_position: Option<Point>,
}

struct ScrollbarProgram<'a, Message> {
    value: f64,
    min: f64,
    max: f64,
    thumb_fraction: f64,
    on_change: Arc<dyn Fn(f64) -> Message + 'a>,
}

impl<'a, Message> ScrollbarProgram<'a, Message> {
    fn value_range(&self) -> f64 {
        (self.max - self.min).max(0.0)
    }

    fn fraction_from_value(&self) -> f64 {
        let range = self.value_range();
        if range == 0.0 {
            0.0
        } else {
            ((self.value - self.min) / range).clamp(0.0, 1.0)
        }
    }

    fn value_from_fraction(&self, fraction: f64) -> f64 {
        let range = self.value_range();
        if range == 0.0 {
            self.min
        } else {
            (self.min + range * fraction.clamp(0.0, 1.0)).clamp(self.min, self.max)
        }
    }

    fn thumb_width(&self, bounds: Rectangle) -> f64 {
        let track_width = (bounds.width - TRACK_PADDING * 2.0).max(1.0) as f64;
        let target = track_width * self.thumb_fraction.clamp(0.02, 1.0);
        target.max(MIN_THUMB_WIDTH as f64).min(track_width)
    }

    fn thumb_bounds(&self, bounds: Rectangle) -> Rectangle {
        let track_width = (bounds.width - TRACK_PADDING * 2.0).max(1.0) as f64;
        let thumb_width = self.thumb_width(bounds);
        let available = (track_width - thumb_width).max(0.0);
        let fraction = self.fraction_from_value();
        let x = TRACK_PADDING as f64 + available * fraction;
        let y = (bounds.height - TRACK_HEIGHT) * 0.5;

        Rectangle {
            x: x as f32,
            y,
            width: thumb_width as f32,
            height: TRACK_HEIGHT,
        }
    }

    fn value_from_local_x(&self, bounds: Rectangle, local_x: f64) -> f64 {
        let track_width = (bounds.width - TRACK_PADDING * 2.0).max(1.0) as f64;
        let thumb_width = self.thumb_width(bounds);
        let available = (track_width - thumb_width).max(0.0);
        let x = (local_x - TRACK_PADDING as f64).clamp(0.0, available);
        let fraction = if available == 0.0 { 0.0 } else { x / available };
        self.value_from_fraction(fraction)
    }

    fn clamp_local_position(&self, bounds: Rectangle, position: Point) -> Point {
        Point::new(
            position.x.clamp(bounds.x, bounds.x + bounds.width),
            position.y.clamp(bounds.y, bounds.y + bounds.height),
        )
    }
}

impl<'a, Message> Program<Message> for ScrollbarProgram<'a, Message> {
    type State = ScrollbarState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        let track_rect = Rectangle {
            x: TRACK_PADDING,
            y: (bounds.height - TRACK_HEIGHT) * 0.5,
            width: (bounds.width - TRACK_PADDING * 2.0).max(1.0),
            height: TRACK_HEIGHT,
        };

        frame.fill_rectangle(
            track_rect.position(),
            track_rect.size(),
            iced::Color::from_rgb(0.92, 0.92, 0.92),
        );

        let thumb = self.thumb_bounds(bounds);
        frame.fill_rectangle(
            thumb.position(),
            thumb.size(),
            iced::Color::from_rgb(0.75, 0.75, 0.78),
        );

        frame.stroke(
            &canvas::Path::rectangle(thumb.position(), thumb.size()),
            canvas::Stroke::default()
                .with_color(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.2))
                .with_width(1.0),
        );

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if let Some(position) = cursor.position() {
            state.last_position = Some(position);
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position()?;
                let local = position - Vector::new(bounds.x, bounds.y);
                let thumb = self.thumb_bounds(bounds);

                if thumb.contains(local) {
                    state.dragging = true;
                    state.drag_offset_x = (local.x - thumb.x).max(0.0) as f64;
                    return Some(Action::capture());
                }

                if bounds.contains(position) {
                    state.dragging = true;
                    state.drag_offset_x = self.thumb_width(bounds) * 0.5;
                    let value =
                        self.value_from_local_x(bounds, local.x as f64 - state.drag_offset_x);
                    return Some(Action::publish((self.on_change)(value)).and_capture());
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.dragging = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if state.dragging {
                    let clamped = self.clamp_local_position(bounds, *position);
                    let local = clamped - Vector::new(bounds.x, bounds.y);
                    let value =
                        self.value_from_local_x(bounds, local.x as f64 - state.drag_offset_x);
                    return Some(Action::publish((self.on_change)(value)).and_capture());
                }
            }
            _ => {}
        }

        if state.dragging {
            if let Some(position) = state.last_position {
                let clamped = self.clamp_local_position(bounds, position);
                let local = clamped - Vector::new(bounds.x, bounds.y);
                let value = self.value_from_local_x(bounds, local.x as f64 - state.drag_offset_x);
                return Some(Action::publish((self.on_change)(value)).and_capture());
            }
        }

        None
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging {
            return mouse::Interaction::Grabbing;
        }

        if let Some(position) = cursor.position() {
            let local = position - Vector::new(bounds.x, bounds.y);
            let thumb = self.thumb_bounds(bounds);
            if thumb.contains(local) {
                return mouse::Interaction::Grab;
            }
        }

        mouse::Interaction::default()
    }
}
