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
    pub is_collapsed: bool,
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
pub const HEADER_HEIGHT: f32 = 30.0;
pub const LANE_HEIGHT: f32 = 20.0;
pub const LANE_SPACING: f32 = 5.0;

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

    let mut total_height = HEADER_HEIGHT;
    for thread in &timeline_data.threads {
        let lane_total_height = if thread.is_collapsed {
            LANE_HEIGHT
        } else {
            (thread.max_depth + 1) as f32 * LANE_HEIGHT
        };
        total_height += lane_total_height + LANE_SPACING;
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

        let total_ns = self
            .threads
            .first()
            .map(|_| {
                // We need the total range to draw the header markers
                // This is slightly inefficient as we don't have max_ns here easily
                // but we can assume the zoom_level * total_ns is the canvas_width - LABEL_WIDTH
                (bounds.width - LABEL_WIDTH) / self.zoom_level
            })
            .unwrap_or(0.0);

        // Draw horizontal lines and events
        let mut y_offset = HEADER_HEIGHT;
        for thread in self.threads {
            let lane_total_height = if thread.is_collapsed {
                LANE_HEIGHT
            } else {
                (thread.max_depth + 1) as f32 * LANE_HEIGHT
            };

            // Draw horizontal separator
            frame.stroke(
                &canvas::Path::line(
                    Point::new(self.scroll_offset.x, y_offset),
                    Point::new(bounds.width, y_offset),
                ),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.2, 0.2, 0.2))
                    .with_width(1.0),
            );

            let mut last_rects: Vec<Option<(f32, f32, Color)>> =
                vec![None; (thread.max_depth + 1) as usize];

            for event in &thread.events {
                if thread.is_collapsed && event.depth > 0 {
                    continue;
                }

                let width = (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                if width < 3.0 {
                    continue;
                }

                let x = (event.start_ns.saturating_sub(self.min_ns) as f64 * self.zoom_level as f64)
                    as f32
                    + LABEL_WIDTH;
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
                        let y = y_offset + depth as f32 * LANE_HEIGHT;
                        frame.fill_rectangle(
                            Point::new(cur_x, y + 1.0),
                            Size::new(cur_w.max(1.0), LANE_HEIGHT - 2.0),
                            cur_color,
                        );
                    }
                }
                last_rects[depth] = Some((x, width, color));
            }

            // Draw remaining rects
            for (depth, rect) in last_rects.into_iter().enumerate() {
                if let Some((cur_x, cur_w, cur_color)) = rect {
                    let y = y_offset + depth as f32 * LANE_HEIGHT;
                    frame.fill_rectangle(
                        Point::new(cur_x, y + 1.0),
                        Size::new(cur_w.max(1.0), LANE_HEIGHT - 2.0),
                        cur_color,
                    );
                }
            }

            // Draw selected highlight if any
            if let Some(selected) = self.selected_event {
                if selected.thread_id == thread.thread_id {
                    if !thread.is_collapsed || selected.depth == 0 {
                        let x = (selected.start_ns.saturating_sub(self.min_ns) as f64
                            * self.zoom_level as f64) as f32
                            + LABEL_WIDTH;
                        let width = (selected.duration_ns as f64 * self.zoom_level as f64) as f32;
                        let y = y_offset + selected.depth as f32 * LANE_HEIGHT;

                        frame.stroke(
                            &canvas::Path::rectangle(
                                Point::new(x, y + 1.0),
                                Size::new(width.max(1.0), LANE_HEIGHT - 2.0),
                            ),
                            canvas::Stroke::default()
                                .with_color(Color::WHITE)
                                .with_width(2.0),
                        );
                    }
                }
            }

            y_offset += lane_total_height + LANE_SPACING;
        }

        // Draw header background (sticky)
        frame.fill_rectangle(
            Point::new(self.scroll_offset.x, self.scroll_offset.y),
            Size::new(bounds.width, HEADER_HEIGHT),
            Color::from_rgb(0.15, 0.15, 0.15),
        );

        // Draw time markers in header
        if total_ns > 0.0 {
            let canvas_width = bounds.width;
            let ns_per_pixel = 1.0 / self.zoom_level as f64;

            // Choose a reasonable interval (e.g., every 100 pixels)
            let pixel_interval = 100.0;
            let ns_interval = pixel_interval as f64 * ns_per_pixel;

            // Round ns_interval to a nice power of 10 or similar
            let log10 = ns_interval.log10().floor();
            let base = 10.0f64.powf(log10);
            let nice_interval = if ns_interval / base < 2.0 {
                base * 2.0
            } else if ns_interval / base < 5.0 {
                base * 5.0
            } else {
                base * 10.0
            };

            let first_marker = (self.min_ns as f64 / nice_interval).floor() * nice_interval;
            let mut current_marker = first_marker;

            while ((current_marker - self.min_ns as f64) * self.zoom_level as f64)
                < canvas_width as f64
            {
                let x = ((current_marker - self.min_ns as f64) * self.zoom_level as f64) as f32
                    + LABEL_WIDTH;

                if x >= LABEL_WIDTH + self.scroll_offset.x {
                    // Draw vertical grid line
                    frame.stroke(
                        &canvas::Path::line(
                            Point::new(x, self.scroll_offset.y + HEADER_HEIGHT),
                            Point::new(x, bounds.height),
                        ),
                        canvas::Stroke::default()
                            .with_color(Color::from_rgb(0.18, 0.18, 0.18))
                            .with_width(1.0),
                    );

                    // Draw tick
                    frame.stroke(
                        &canvas::Path::line(
                            Point::new(x, self.scroll_offset.y + HEADER_HEIGHT - 5.0),
                            Point::new(x, self.scroll_offset.y + HEADER_HEIGHT),
                        ),
                        canvas::Stroke::default()
                            .with_color(Color::WHITE)
                            .with_width(1.0),
                    );

                    // Draw text
                    let time_str = if nice_interval >= 1_000_000_000.0 {
                        format!("{:.2} s", current_marker / 1_000_000_000.0)
                    } else if nice_interval >= 1_000_000.0 {
                        format!("{:.2} ms", current_marker / 1_000_000.0)
                    } else if nice_interval >= 1_000.0 {
                        format!("{:.2} µs", current_marker / 1_000.0)
                    } else {
                        format!("{} ns", current_marker)
                    };

                    frame.fill_text(canvas::Text {
                        content: time_str,
                        position: Point::new(x + 2.0, self.scroll_offset.y + 5.0),
                        color: Color::WHITE,
                        size: 10.0.into(),
                        ..Default::default()
                    });
                }
                current_marker += nice_interval;
            }
        }

        // Draw thread labels area (sticky background)
        frame.fill_rectangle(
            Point::new(self.scroll_offset.x, self.scroll_offset.y + HEADER_HEIGHT),
            Size::new(LABEL_WIDTH, bounds.height - HEADER_HEIGHT),
            Color::from_rgb(0.1, 0.1, 0.1),
        );

        y_offset = HEADER_HEIGHT;
        for thread in self.threads {
            let lane_total_height = if thread.is_collapsed {
                LANE_HEIGHT
            } else {
                (thread.max_depth + 1) as f32 * LANE_HEIGHT
            };

            let label_text = if thread.is_collapsed {
                format!("▶ Thread {}", thread.thread_id)
            } else {
                format!("▼ Thread {}", thread.thread_id)
            };

            frame.fill_text(canvas::Text {
                content: label_text,
                position: Point::new(self.scroll_offset.x + 5.0, y_offset + 5.0),
                color: Color::WHITE,
                size: 12.0.into(),
                ..Default::default()
            });

            y_offset += lane_total_height + LANE_SPACING;
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
                        // Check for thread label click
                        let mut y_offset = HEADER_HEIGHT;
                        for thread in self.threads {
                            let lane_total_height = if thread.is_collapsed {
                                LANE_HEIGHT
                            } else {
                                (thread.max_depth + 1) as f32 * LANE_HEIGHT
                            };

                            if position.y >= y_offset && position.y < y_offset + lane_total_height {
                                return Some(Action::publish(Message::ToggleThreadCollapse(
                                    thread.thread_id,
                                )));
                            }
                            y_offset += lane_total_height + LANE_SPACING;
                        }
                        return None;
                    }

                    let mut y_offset = HEADER_HEIGHT;
                    for thread in self.threads {
                        let lane_total_height = if thread.is_collapsed {
                            LANE_HEIGHT
                        } else {
                            (thread.max_depth + 1) as f32 * LANE_HEIGHT
                        };

                        if position.y >= y_offset && position.y < y_offset + lane_total_height {
                            for event in &thread.events {
                                if thread.is_collapsed && event.depth > 0 {
                                    continue;
                                }

                                let width =
                                    (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                                if width < 5.0 {
                                    continue;
                                }

                                let x = (event.start_ns.saturating_sub(self.min_ns) as f64
                                    * self.zoom_level as f64)
                                    as f32
                                    + LABEL_WIDTH;
                                let y = y_offset + event.depth as f32 * LANE_HEIGHT;
                                let height = LANE_HEIGHT - 2.0;

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
                        y_offset += lane_total_height + LANE_SPACING;
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
