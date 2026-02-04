use crate::Message;
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Action, Canvas, Geometry, Program};
use iced::widget::{column, container, scrollable, text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size};

#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub label: String,
    pub start_ns: u64,
    pub duration_ns: u64,
    pub depth: u32,
    pub thread_id: u64,
    pub color: Color,
}

#[derive(Debug, Clone)]
pub struct ThreadData {
    pub thread_id: u64,
    pub events: Vec<TimelineEvent>,
    pub max_depth: u32,
}

#[derive(Debug, Clone, Default)]
pub struct TimelineData {
    pub threads: Vec<ThreadData>,
    pub min_ns: u64,
    pub max_ns: u64,
}

pub fn color_from_label(label: &str) -> Color {
    let mut hash = 0u64;
    for c in label.chars() {
        hash = hash.wrapping_add(c as u64);
        hash = hash.wrapping_mul(0x517cc1b727220a95);
    }

    let r = ((hash >> 16) & 0xFF) as f32 / 255.0;
    let g = ((hash >> 8) & 0xFF) as f32 / 255.0;
    let b = (hash & 0xFF) as f32 / 255.0;

    Color::from_rgb(0.3 + r * 0.4, 0.3 + g * 0.4, 0.3 + b * 0.4)
}

pub const LABEL_WIDTH: f32 = 150.0;

pub fn timeline_id() -> iced::widget::Id {
    iced::widget::Id::new("timeline_scrollable")
}

pub fn view<'a>(
    timeline_data: &'a TimelineData,
    zoom_level: f32,
    selected_event: &'a Option<TimelineEvent>,
    scroll_offset: iced::Vector,
) -> Element<'a, Message> {
    let total_ns = timeline_data.max_ns - timeline_data.min_ns;
    if total_ns == 0 {
        return container(text("No events to display"))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
    }

    let mut total_height = 0.0;
    for thread in &timeline_data.threads {
        let lane_height = (thread.max_depth + 1) as f32 * 20.0;
        total_height += lane_height + 5.0;
    }

    let canvas_width = total_ns as f32 * zoom_level + LABEL_WIDTH;

    let timeline_canvas = Canvas::new(TimelineProgram {
        threads: &timeline_data.threads,
        min_ns: timeline_data.min_ns,
        zoom_level,
        selected_event,
        scroll_offset,
    })
    .width(Length::Fixed(canvas_width))
    .height(Length::Fixed(total_height));

    let main_view = scrollable(timeline_canvas)
        .id(timeline_id())
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        })
        .on_scroll(|viewport| Message::TimelineScroll {
            offset: iced::Vector::new(viewport.absolute_offset().x, viewport.absolute_offset().y),
        });

    let details_panel = if let Some(event) = selected_event {
        container(
            column![
                text(format!("Event: {}", event.label)).size(20),
                text(format!("Thread: {}", event.thread_id)),
                text(format!("Start: {} ns", event.start_ns)),
                text(format!("Duration: {} ns", event.duration_ns)),
            ]
            .spacing(5)
            .padding(10),
        )
        .width(Length::Fill)
        .height(Length::Fixed(120.0))
    } else {
        container(text("Select an event to see details"))
            .width(Length::Fill)
            .height(Length::Fixed(120.0))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
    };

    column![main_view, details_panel].into()
}

struct TimelineProgram<'a> {
    threads: &'a [ThreadData],
    min_ns: u64,
    zoom_level: f32,
    selected_event: &'a Option<TimelineEvent>,
    scroll_offset: iced::Vector,
}

#[derive(Default)]
struct TimelineState {}

impl<'a> Program<Message> for TimelineProgram<'a> {
    type State = TimelineState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        if self.threads.is_empty() {
            return vec![frame.into_geometry()];
        }

        let mut y_offset = 0.0;
        for thread in self.threads {
            let lane_height = (thread.max_depth + 1) as f32 * 20.0;

            let mut last_rects: Vec<Option<(f32, f32, Color)>> =
                vec![None; (thread.max_depth + 1) as usize];

            for event in &thread.events {
                let x = (event.start_ns.saturating_sub(self.min_ns) as f64 * self.zoom_level as f64)
                    as f32
                    + LABEL_WIDTH;
                let width = (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                let depth = event.depth as usize;
                let color = event.color;

                if let Some((cur_x, cur_w, cur_color)) = last_rects[depth] {
                    let end_x = cur_x + cur_w;
                    if color == cur_color && x <= end_x + 0.5 {
                        let new_end = (x + width).max(end_x);
                        last_rects[depth] = Some((cur_x, new_end - cur_x, cur_color));
                        continue;
                    } else {
                        // Draw previous
                        let y = y_offset + depth as f32 * 20.0;
                        frame.fill_rectangle(
                            Point::new(cur_x, y),
                            Size::new(cur_w.max(1.0), 18.0),
                            cur_color,
                        );
                    }
                }
                last_rects[depth] = Some((x, width, color));
            }

            // Draw remaining rects
            for (depth, rect) in last_rects.into_iter().enumerate() {
                if let Some((cur_x, cur_w, cur_color)) = rect {
                    let y = y_offset + depth as f32 * 20.0;
                    frame.fill_rectangle(
                        Point::new(cur_x, y),
                        Size::new(cur_w.max(1.0), 18.0),
                        cur_color,
                    );
                }
            }

            // Draw selected highlight if any
            if let Some(selected) = self.selected_event {
                if selected.thread_id == thread.thread_id {
                    let x = (selected.start_ns.saturating_sub(self.min_ns) as f64
                        * self.zoom_level as f64) as f32
                        + LABEL_WIDTH;
                    let width = (selected.duration_ns as f64 * self.zoom_level as f64) as f32;
                    let y = y_offset + selected.depth as f32 * 20.0;

                    frame.stroke(
                        &canvas::Path::rectangle(Point::new(x, y), Size::new(width.max(1.0), 18.0)),
                        canvas::Stroke::default()
                            .with_color(Color::from_rgb(1.0, 1.0, 1.0))
                            .with_width(2.0),
                    );
                }
            }

            y_offset += lane_height + 5.0;
        }

        // Draw thread labels area (sticky background)
        frame.fill_rectangle(
            Point::new(self.scroll_offset.x, 0.0),
            Size::new(LABEL_WIDTH, bounds.height),
            Color::from_rgb(0.1, 0.1, 0.1),
        );

        y_offset = 0.0;
        for thread in self.threads {
            let lane_height = (thread.max_depth + 1) as f32 * 20.0;

            frame.fill_text(canvas::Text {
                content: format!("Thread {}", thread.thread_id),
                position: Point::new(self.scroll_offset.x + 5.0, y_offset + 5.0),
                color: Color::WHITE,
                size: 12.0.into(),
                ..Default::default()
            });

            y_offset += lane_height + 5.0;
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    if position.x < LABEL_WIDTH + self.scroll_offset.x {
                        return None;
                    }

                    let mut y_offset = 0.0;
                    for thread in self.threads {
                        let lane_height = (thread.max_depth + 1) as f32 * 20.0;

                        if position.y >= y_offset && position.y < y_offset + lane_height {
                            for event in &thread.events {
                                let x = (event.start_ns.saturating_sub(self.min_ns) as f64
                                    * self.zoom_level as f64)
                                    as f32
                                    + LABEL_WIDTH;
                                let width =
                                    (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                                let y = y_offset + event.depth as f32 * 20.0;
                                let height = 18.0;

                                let rect = Rectangle {
                                    x,
                                    y,
                                    width: width.max(1.0),
                                    height,
                                };

                                if rect.contains(position) {
                                    return Some(Action::publish(Message::EventSelected(
                                        event.clone(),
                                    )));
                                }
                            }
                        }
                        y_offset += lane_height + 5.0;
                    }
                }
            }
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    match delta {
                        iced::mouse::ScrollDelta::Lines { x: _, y }
                        | iced::mouse::ScrollDelta::Pixels { x: _, y } => {
                            if y.abs() > 0.0 {
                                return Some(Action::publish(Message::TimelineZoomed {
                                    delta: *y,
                                    x: position.x - self.scroll_offset.x,
                                }));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        None
    }
}
