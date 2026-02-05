// Mini timeline receives explicit f64 scroll offsets from app state.
use crate::timeline::ticks::{format_time_label, nice_interval};
use crate::Message;
use iced::mouse;
use iced::widget::canvas::{self, Action, Geometry, Program};
use iced::{Color, Event, Point, Rectangle, Renderer, Size, Theme};

pub(crate) struct MiniTimelineProgram {
    pub(crate) min_ns: u64,
    pub(crate) max_ns: u64,
    pub(crate) zoom_level: f64,
    pub(crate) scroll_offset_x: f64,
    pub(crate) viewport_width: f64,
}

#[derive(Default)]
pub(crate) struct MiniTimelineState {
    selection_start: Option<Point>,
    selection_end: Option<Point>,
    selecting: bool,
    dragging: bool,
}

impl MiniTimelineProgram {
    fn fallback_viewport_width(&self, bounds: Rectangle) -> f32 {
        (bounds.width - super::LABEL_WIDTH as f32).max(0.0)
    }

    fn viewport_width_for_bounds(&self, bounds: Rectangle) -> f32 {
        if self.viewport_width > 0.0 {
            self.viewport_width as f32
        } else {
            self.fallback_viewport_width(bounds)
        }
    }

    fn selection_bounds(&self, state: &MiniTimelineState, bounds: Rectangle) -> Option<Rectangle> {
        let (start, end) = match (state.selection_start, state.selection_end) {
            (Some(start), Some(end)) => (start, end),
            _ => return None,
        };

        // Use the full width of the mini timeline
        let events_width = bounds.width;

        // Clamp start/end to the events area and return a rectangle.
        let raw_x_start = start.x.min(end.x);
        let raw_x_end = start.x.max(end.x);

        let x_start = raw_x_start.max(0.0).min(events_width);
        let x_end = raw_x_end.max(0.0).min(events_width);
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

        // Mini timeline background: use white for a clean look.
        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            Size::new(bounds.width, bounds.height),
            Color::WHITE,
        );

        let total_ns = crate::timeline::total_ns(self.min_ns, self.max_ns) as f64;
        if total_ns <= 0.0 || bounds.width <= 0.0 {
            return vec![frame.into_geometry()];
        }

        let ns_per_pixel = total_ns / bounds.width as f64;
        let pixel_interval = 120.0;
        let ns_interval = pixel_interval as f64 * ns_per_pixel;
        let nice_interval = nice_interval(ns_interval);

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

            let time_str = format_time_label(relative_ns, nice_interval);
            frame.fill_text(canvas::Text {
                content: time_str,
                position: Point::new(x + 2.0, 4.0),
                color: Color::from_rgb(0.4, 0.4, 0.4),
                size: 10.0.into(),
                ..Default::default()
            });

            relative_ns += nice_interval;
        }

        let total_width = (total_ns * self.zoom_level).ceil() as f32;
        if total_width > 0.0 {
            // Map the main timeline viewport into the full width of the mini timeline
            let events_width = bounds.width;

            let viewport_width = self.viewport_width_for_bounds(bounds) as f64;

            let view_start = (self.scroll_offset_x / total_width as f64).clamp(0.0, 1.0) as f32;
            let view_width = (viewport_width / total_width as f64).clamp(0.0, 1.0) as f32;

            let x = view_start * events_width;
            let width = (view_width * events_width).max(4.0);

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

        // Draw a 1px separator line under the mini timeline to visually separate
        // it from the header/content below.
        frame.stroke(
            &canvas::Path::line(
                Point::new(0.0, bounds.height - 0.5),
                Point::new(bounds.width, bounds.height - 0.5),
            ),
            canvas::Stroke::default()
                .with_color(Color::from_rgba(0.6, 0.6, 0.6, 1.0))
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
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    if bounds.width > 0.0 {
                        // Map click position into the full width of the mini timeline
                        let events_width = bounds.width;
                        let rel_x = position.x.clamp(0.0, events_width);
                        let fraction = (rel_x / events_width).clamp(0.0, 1.0) as f64;
                        let viewport_width = self.viewport_width_for_bounds(bounds).max(1.0);
                        state.dragging = true;
                        state.selecting = false;
                        state.selection_start = None;
                        state.selection_end = None;
                        return Some(Action::publish(Message::MiniTimelineJump {
                            fraction,
                            // Use the actual viewport width when available; otherwise fall back
                            // to the mini timeline's width. Using `max(events_width)` here
                            // previously forced the viewport width up to the mini timeline
                            // width which prevented panning/zooming to the true end.
                            viewport_width,
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
                        let events_width = bounds.width;
                        if events_width > 0.0 {
                            let rel_x = position.x.clamp(0.0, events_width);
                            let fraction = (rel_x / events_width).clamp(0.0, 1.0) as f64;
                            let viewport_width = self.viewport_width_for_bounds(bounds).max(1.0);
                            return Some(Action::publish(Message::MiniTimelineJump {
                                fraction,
                                viewport_width,
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
                        if selection.width >= 4.0 {
                            let events_width = bounds.width;
                            let start_fraction = (selection.x / events_width).clamp(0.0, 1.0);
                            let end_fraction =
                                ((selection.x + selection.width) / events_width).clamp(0.0, 1.0);
                            let viewport_width = self.viewport_width_for_bounds(bounds).max(1.0);
                            state.selection_start = None;
                            state.selection_end = None;
                            return Some(Action::publish(Message::MiniTimelineZoomTo {
                                start_fraction,
                                end_fraction,
                                viewport_width,
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
