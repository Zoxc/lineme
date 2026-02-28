// Threads panel receives explicit scroll offsets from the app state (f64)
use crate::Message;
use crate::data::{ThreadGroup, thread_group_key};
use crate::timeline::{LANE_HEIGHT, LANE_SPACING, group_total_height};
use iced::mouse;
use iced::widget::canvas::{self, Action, Geometry, Program};
use iced::{Color, Event, Point, Rectangle, Renderer, Size, Theme};

pub(crate) struct ThreadsProgram<'a> {
    pub(crate) thread_groups: &'a [ThreadGroup],
    pub(crate) scroll_offset_y: f64,
}

#[derive(Default)]
pub(crate) struct ThreadsState {
    hovered_group: Option<usize>,
}

impl<'a> ThreadsProgram<'a> {
    fn group_at(&self, position: Point) -> Option<usize> {
        let mut y_offset: f64 = 0.0;
        let content_y = position.y as f64 + self.scroll_offset_y;

        for group in self.thread_groups {
            let lane_total_height = group_total_height(group);

            if content_y >= y_offset && content_y < y_offset + LANE_HEIGHT + 2.0 {
                return Some(thread_group_key(group));
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

        let mut y_offset: f64 = 0.0;
        for group in self.thread_groups {
            let lane_total_height = group_total_height(group);

            let y = (y_offset - self.scroll_offset_y) as f32;
            let row_top = y;
            let is_hovered = state.hovered_group == Some(thread_group_key(group));
            if is_hovered {
                frame.fill_rectangle(
                    Point::new(0.0, row_top),
                    Size::new(bounds.width, (LANE_HEIGHT + 2.0) as f32),
                    Color::from_rgb(0.95, 0.95, 0.95),
                );
            }

            frame.stroke(
                &canvas::Path::line(Point::new(0.0, row_top), Point::new(bounds.width, row_top)),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.92, 0.92, 0.92))
                    .with_width(1.0),
            );

            let icon = if group.is_collapsed { "▶" } else { "▼" };
            let icon_color = if is_hovered {
                Color::from_rgb(0.25, 0.25, 0.25)
            } else {
                Color::from_rgb(0.5, 0.5, 0.5)
            };

            frame.fill_text(canvas::Text {
                content: icon.to_string(),
                position: Point::new(8.0, row_top + 4.0),
                color: icon_color,
                size: 10.0.into(),
                ..Default::default()
            });

            frame.fill_text(canvas::Text {
                content: group_label(group),
                position: Point::new(22.0, row_top + 3.0),
                color: if is_hovered {
                    Color::from_rgb(0.1, 0.1, 0.1)
                } else {
                    Color::from_rgb(0.3, 0.3, 0.3)
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
                .and_then(|position| self.group_at(position));

            if state.hovered_group != hovered {
                state.hovered_group = hovered;
            }
        }

        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(position) = cursor.position_in(bounds)
            && let Some(group_id) = self.group_at(position)
        {
            return Some(Action::publish(Message::ToggleThreadCollapse(group_id)));
        }

        None
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hovered_group.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

fn group_label(group: &ThreadGroup) -> String {
    // For a single-thread group use the concise form "Thread <id>".
    if group.threads.len() == 1
        && let Some(thread) = group.threads.first()
    {
        return format!("Thread {}", thread.thread_id);
    }

    // For multi-thread groups display a concise "Merged" label.
    "Merged".to_string()
}
