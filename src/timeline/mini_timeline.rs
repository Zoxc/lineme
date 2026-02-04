use crate::Message;
use iced::mouse;
use iced::widget::canvas::{self, Action, Geometry, Program};
use iced::{Color, Event, Point, Rectangle, Renderer, Size, Theme, Vector};

pub(crate) struct MiniTimelineProgram {
    pub(crate) min_ns: u64,
    pub(crate) max_ns: u64,
    pub(crate) zoom_level: f32,
    pub(crate) scroll_offset: Vector,
    pub(crate) viewport_width: f32,
}

#[derive(Default)]
pub(crate) struct MiniTimelineState {
    selection_start: Option<Point>,
    selection_end: Option<Point>,
    selecting: bool,
    dragging: bool,
}

impl MiniTimelineProgram {
    fn selection_bounds(&self, state: &MiniTimelineState, bounds: Rectangle) -> Option<Rectangle> {
        let (start, end) = match (state.selection_start, state.selection_end) {
            (Some(start), Some(end)) => (start, end),
            _ => return None,
        };

        if bounds.width <= 0.0 {
            return None;
        }

        let x_start = start.x.min(end.x).max(0.0).min(bounds.width);
        let x_end = start.x.max(end.x).max(0.0).min(bounds.width);
        let width = (x_end - x_start).max(0.0);

        Some(Rectangle {
            x: x_start,
            y: 0.0,
            width,
            height: bounds.height,
        })
    }
}

impl Program<Message> for MiniTimelineProgram {
    type State = MiniTimelineState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            Size::new(bounds.width, bounds.height),
            Color::from_rgb(0.97, 0.97, 0.97),
        );

        let total_ns = self.max_ns.saturating_sub(self.min_ns) as f64;
        if total_ns <= 0.0 || bounds.width <= 0.0 {
            return vec![frame.into_geometry()];
        }

        let ns_per_pixel = total_ns / bounds.width as f64;
        let pixel_interval = 120.0;
        let ns_interval = pixel_interval as f64 * ns_per_pixel;

        let log10 = ns_interval.log10().floor();
        let base = 10.0f64.powf(log10);
        let nice_interval = if ns_interval / base < 2.0 {
            base * 2.0
        } else if ns_interval / base < 5.0 {
            base * 5.0
        } else {
            base * 10.0
        };

        let mut relative_ns = 0.0;
        while relative_ns <= total_ns {
            let x = (relative_ns / total_ns * bounds.width as f64) as f32;

            // Draw a faint vertical guide line for this tick across the mini timeline.
            frame.stroke(
                &canvas::Path::line(Point::new(x, 0.0), Point::new(x, bounds.height)),
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.5, 0.5, 0.5, 0.3))
                    .with_width(1.0),
            );

            let time_str = if nice_interval >= 1_000_000_000.0 {
                format!("{:.2} s", relative_ns / 1_000_000_000.0)
            } else if nice_interval >= 1_000_000.0 {
                format!("{:.2} ms", relative_ns / 1_000_000.0)
            } else if nice_interval >= 1_000.0 {
                format!("{:.2} µs", relative_ns / 1_000.0)
            } else {
                format!("{:.0} ns", relative_ns)
            };

            frame.fill_text(canvas::Text {
                content: time_str,
                position: Point::new(x + 2.0, 4.0),
                color: Color::from_rgb(0.4, 0.4, 0.4),
                size: 10.0.into(),
                ..Default::default()
            });

            relative_ns += nice_interval;
        }

        let total_width = total_ns as f32 * self.zoom_level;
        if total_width > 0.0 {
            let viewport_width = if self.viewport_width > 0.0 {
                self.viewport_width
            } else {
                bounds.width
            };
            let view_start = (self.scroll_offset.x / total_width).clamp(0.0, 1.0);
            let view_width = (viewport_width / total_width).clamp(0.0, 1.0);
            let x = view_start * bounds.width;
            let width = (view_width * bounds.width).max(4.0);

            frame.fill_rectangle(
                Point::new(x, 1.0),
                Size::new(width, bounds.height - 2.0),
                Color::from_rgba(0.1, 0.3, 0.6, 0.15),
            );

            frame.stroke(
                &canvas::Path::rectangle(Point::new(x, 1.0), Size::new(width, bounds.height - 2.0)),
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.1, 0.3, 0.6, 0.5))
                    .with_width(1.0),
            );
        }

        if let Some(selection) = self.selection_bounds(state, bounds) {
            frame.fill_rectangle(
                selection.position(),
                selection.size(),
                Color::from_rgba(0.2, 0.4, 0.6, 0.2),
            );
            frame.stroke(
                &canvas::Path::rectangle(selection.position(), selection.size()),
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.2, 0.4, 0.6, 0.6))
                    .with_width(1.0),
            );
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    if bounds.width > 0.0 {
                        let fraction = (position.x / bounds.width).clamp(0.0, 1.0) as f64;
                        state.dragging = true;
                        state.selecting = false;
                        state.selection_start = None;
                        state.selection_end = None;
                        return Some(Action::publish(Message::MiniTimelineJump {
                            fraction,
                            viewport_width: self.viewport_width.max(bounds.width),
                        }));
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    state.selecting = true;
                    state.dragging = false;
                    state.selection_start = Some(position);
                    state.selection_end = Some(position);
                    return Some(Action::publish(Message::None));
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.dragging {
                    if let Some(position) = cursor.position_in(bounds) {
                        if bounds.width > 0.0 {
                            let fraction = (position.x / bounds.width).clamp(0.0, 1.0) as f64;
                            return Some(Action::publish(Message::MiniTimelineJump {
                                fraction,
                                viewport_width: self.viewport_width.max(bounds.width),
                            }));
                        }
                    }
                }
                if state.selecting {
                    if let Some(position) = cursor.position_in(bounds) {
                        state.selection_end = Some(position);
                        return Some(Action::publish(Message::None));
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.dragging = false;
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)) => {
                if state.selecting {
                    state.selecting = false;
                    if let Some(selection) = self.selection_bounds(state, bounds) {
                        if selection.width >= 4.0 && bounds.width > 0.0 {
                            let start_fraction = (selection.x / bounds.width).clamp(0.0, 1.0);
                            let end_fraction =
                                ((selection.x + selection.width) / bounds.width).clamp(0.0, 1.0);
                            state.selection_start = None;
                            state.selection_end = None;
                            return Some(Action::publish(Message::MiniTimelineZoomTo {
                                start_fraction,
                                end_fraction,
                                viewport_width: self.viewport_width.max(bounds.width),
                            }));
                        }
                    }
                    state.selection_start = None;
                    state.selection_end = None;
                }
            }
            _ => {}
        }
        None
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.selecting || cursor.position_in(bounds).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
