use crate::timeline::{ThreadData, LANE_HEIGHT, LANE_SPACING};
use crate::Message;
use iced::mouse;
use iced::widget::canvas::{self, Action, Geometry, Program};
use iced::{Color, Event, Point, Rectangle, Renderer, Size, Theme, Vector};

pub(crate) struct ThreadsProgram<'a> {
    pub(crate) threads: &'a [ThreadData],
    pub(crate) scroll_offset: Vector,
}

#[derive(Default)]
pub(crate) struct ThreadsState {
    hovered_thread: Option<u64>,
}

impl<'a> ThreadsProgram<'a> {
    fn thread_at(&self, position: Point) -> Option<u64> {
        let mut y_offset = 0.0;
        let content_y = position.y + self.scroll_offset.y;

        for thread in self.threads {
            let lane_total_height = if thread.is_collapsed {
                LANE_HEIGHT
            } else {
                (thread.max_depth + 1) as f32 * LANE_HEIGHT
            };

            if content_y >= y_offset && content_y < y_offset + lane_total_height {
                return Some(thread.thread_id);
            }

            y_offset += lane_total_height + LANE_SPACING;
        }

        None
    }
}

impl<'a> Program<Message> for ThreadsProgram<'a> {
    type State = ThreadsState;

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
            Color::from_rgb(0.98, 0.98, 0.98),
        );

        let mut y_offset = 0.0;
        for thread in self.threads {
            let lane_total_height = if thread.is_collapsed {
                LANE_HEIGHT
            } else {
                (thread.max_depth + 1) as f32 * LANE_HEIGHT
            };

            let y = y_offset - self.scroll_offset.y;
            let row_top = y;
            let is_hovered = state.hovered_thread == Some(thread.thread_id);
            if is_hovered {
                frame.fill_rectangle(
                    Point::new(0.0, row_top),
                    Size::new(bounds.width, LANE_HEIGHT + 2.0),
                    Color::from_rgb(0.94, 0.94, 0.94),
                );
            }

            frame.stroke(
                &canvas::Path::line(Point::new(0.0, row_top), Point::new(bounds.width, row_top)),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.9, 0.9, 0.9))
                    .with_width(1.0),
            );

            let icon = if thread.is_collapsed { "▶" } else { "▼" };
            let icon_box = Rectangle {
                x: 6.0,
                y: row_top + 3.0,
                width: 14.0,
                height: 14.0,
            };

            let icon_bg = if is_hovered {
                Color::from_rgb(0.8, 0.86, 0.95)
            } else {
                Color::from_rgb(0.92, 0.92, 0.92)
            };

            frame.fill_rectangle(icon_box.position(), icon_box.size(), icon_bg);

            frame.stroke(
                &canvas::Path::rectangle(icon_box.position(), icon_box.size()),
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.2))
                    .with_width(1.0),
            );

            frame.fill_text(canvas::Text {
                content: icon.to_string(),
                position: Point::new(icon_box.x + 3.0, icon_box.y - 1.0),
                color: Color::from_rgb(0.2, 0.2, 0.2),
                size: 12.0.into(),
                ..Default::default()
            });

            frame.fill_text(canvas::Text {
                content: format!("Thread {}", thread.thread_id),
                position: Point::new(26.0, row_top + 5.0),
                color: if is_hovered {
                    Color::from_rgb(0.1, 0.2, 0.35)
                } else {
                    Color::from_rgb(0.2, 0.2, 0.2)
                },
                size: 12.0.into(),
                ..Default::default()
            });

            y_offset += lane_total_height + LANE_SPACING;
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
        if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event {
            let hovered = cursor
                .position_in(bounds)
                .and_then(|position| self.thread_at(position));

            if hovered != state.hovered_thread {
                state.hovered_thread = hovered;
            }
        }

        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if let Some(position) = cursor.position_in(bounds) {
                if let Some(thread_id) = self.thread_at(position) {
                    return Some(Action::publish(Message::ToggleThreadCollapse(thread_id)));
                }
            }
        }

        None
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hovered_thread.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
