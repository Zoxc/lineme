use iced::mouse;
use iced::widget::canvas::{self, Action, Canvas, Geometry, Program};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Theme, Vector};
use std::sync::Arc;

const DEFAULT_THICKNESS: f32 = 18.0;
const TRACK_THICKNESS: f32 = 6.0;
const TRACK_PADDING: f32 = 6.0;
const MIN_THUMB_LENGTH: f32 = 24.0;

pub fn scrollbar<'a, Message>(
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    on_change: impl Fn(f64) -> Message + 'a,
) -> Scrollbar<'a, Message> {
    Scrollbar::new(value, range, on_change)
}

pub fn vertical_scrollbar<'a, Message>(
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    on_change: impl Fn(f64) -> Message + 'a,
) -> Scrollbar<'a, Message> {
    Scrollbar::new(value, range, on_change)
        .orientation(Orientation::Vertical)
        .width(Length::Fixed(DEFAULT_THICKNESS))
        .height(Length::Fill)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

pub struct Scrollbar<'a, Message> {
    value: f64,
    min: f64,
    max: f64,
    thumb_fraction: f64,
    width: Length,
    height: Length,
    orientation: Orientation,
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
            height: Length::Fixed(DEFAULT_THICKNESS),
            orientation: Orientation::Horizontal,
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

    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
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
            orientation,
            on_change,
        } = scrollbar;
        let program = ScrollbarProgram {
            value,
            min,
            max,
            thumb_fraction,
            orientation,
            on_change,
        };
        Canvas::new(program).width(width).height(height).into()
    }
}

#[derive(Default)]
struct ScrollbarState {
    dragging: bool,
    drag_offset: f64,
    last_position: Option<Point>,
}

struct ScrollbarProgram<'a, Message> {
    value: f64,
    min: f64,
    max: f64,
    thumb_fraction: f64,
    orientation: Orientation,
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

    fn track_length(&self, bounds: Rectangle) -> f64 {
        match self.orientation {
            Orientation::Horizontal => (bounds.width - TRACK_PADDING * 2.0).max(1.0) as f64,
            Orientation::Vertical => (bounds.height - TRACK_PADDING * 2.0).max(1.0) as f64,
        }
    }

    fn thumb_length(&self, bounds: Rectangle) -> f64 {
        let track_length = self.track_length(bounds);
        let target = track_length * self.thumb_fraction.clamp(0.02, 1.0);
        target.max(MIN_THUMB_LENGTH as f64).min(track_length)
    }

    fn thumb_bounds(&self, bounds: Rectangle) -> Rectangle {
        let track_length = self.track_length(bounds);
        let thumb_length = self.thumb_length(bounds);
        let available = (track_length - thumb_length).max(0.0);
        let fraction = self.fraction_from_value();
        let offset = TRACK_PADDING as f64 + available * fraction;
        match self.orientation {
            Orientation::Horizontal => {
                let y = (bounds.height - TRACK_THICKNESS) * 0.5;
                Rectangle {
                    x: offset as f32,
                    y,
                    width: thumb_length as f32,
                    height: TRACK_THICKNESS,
                }
            }
            Orientation::Vertical => {
                let x = (bounds.width - TRACK_THICKNESS) * 0.5;
                Rectangle {
                    x,
                    y: offset as f32,
                    width: TRACK_THICKNESS,
                    height: thumb_length as f32,
                }
            }
        }
    }

    fn value_from_local_axis(&self, bounds: Rectangle, local_axis: f64) -> f64 {
        let track_length = self.track_length(bounds);
        let thumb_length = self.thumb_length(bounds);
        let available = (track_length - thumb_length).max(0.0);
        let axis = (local_axis - TRACK_PADDING as f64).clamp(0.0, available);
        let fraction = if available == 0.0 {
            0.0
        } else {
            axis / available
        };
        self.value_from_fraction(fraction)
    }

    fn local_axis(&self, local: Point) -> f64 {
        match self.orientation {
            Orientation::Horizontal => local.x as f64,
            Orientation::Vertical => local.y as f64,
        }
    }

    fn thumb_axis_start(&self, thumb: Rectangle) -> f64 {
        match self.orientation {
            Orientation::Horizontal => thumb.x as f64,
            Orientation::Vertical => thumb.y as f64,
        }
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
            x: if self.orientation == Orientation::Horizontal {
                TRACK_PADDING
            } else {
                (bounds.width - TRACK_THICKNESS) * 0.5
            },
            y: if self.orientation == Orientation::Horizontal {
                (bounds.height - TRACK_THICKNESS) * 0.5
            } else {
                TRACK_PADDING
            },
            width: if self.orientation == Orientation::Horizontal {
                (bounds.width - TRACK_PADDING * 2.0).max(1.0)
            } else {
                TRACK_THICKNESS
            },
            height: if self.orientation == Orientation::Horizontal {
                TRACK_THICKNESS
            } else {
                (bounds.height - TRACK_PADDING * 2.0).max(1.0)
            },
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
                    state.drag_offset =
                        (self.local_axis(local) - self.thumb_axis_start(thumb)).max(0.0);
                    return Some(Action::capture());
                }

                if bounds.contains(position) {
                    state.dragging = true;
                    state.drag_offset = self.thumb_length(bounds) * 0.5;
                    let value = self
                        .value_from_local_axis(bounds, self.local_axis(local) - state.drag_offset);
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
                    let value = self
                        .value_from_local_axis(bounds, self.local_axis(local) - state.drag_offset);
                    return Some(Action::publish((self.on_change)(value)).and_capture());
                }
            }
            _ => {}
        }

        if state.dragging {
            if let Some(position) = state.last_position {
                let clamped = self.clamp_local_position(bounds, position);
                let local = clamped - Vector::new(bounds.x, bounds.y);
                let value =
                    self.value_from_local_axis(bounds, self.local_axis(local) - state.drag_offset);
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
