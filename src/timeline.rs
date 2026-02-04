use crate::Message;
use iced::mouse::Cursor;
use iced::widget::canvas::{self, Action, Canvas, Geometry, Program};
use iced::widget::{column, container, row, scrollable, slider, text, Space};
use iced::{Color, Element, Length, Padding, Point, Rectangle, Renderer, Size};

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

pub fn view<'a>(
    timeline_data: &'a TimelineData,
    view_start_ns: u64,
    view_end_ns: u64,
    selected_event: &'a Option<TimelineEvent>,
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

    let mut thread_labels = column![].spacing(5);
    let mut total_height = 0.0;

    for thread in &timeline_data.threads {
        let lane_height = (thread.max_depth + 1) as f32 * 20.0;
        total_height += lane_height + 5.0;

        thread_labels = thread_labels.push(
            container(text(format!("Thread {}", thread.thread_id)))
                .height(Length::Fixed(lane_height))
                .padding(5),
        );
    }

    let timeline_canvas = Canvas::new(TimelineProgram {
        threads: &timeline_data.threads,
        view_start_ns,
        view_end_ns,
        selected_event,
    })
    .width(Length::Fill)
    .height(Length::Fixed(total_height));

    let main_view = scrollable(row![
        thread_labels.width(Length::Fixed(150.0)),
        timeline_canvas,
    ])
    .height(Length::Fill);

    let duration = view_end_ns.saturating_sub(view_start_ns);
    let max_scroll = timeline_data.max_ns.saturating_sub(duration);

    let scrollbar = if max_scroll > timeline_data.min_ns {
        container(slider(
            timeline_data.min_ns as f64..=max_scroll as f64,
            view_start_ns as f64,
            |val| Message::TimelinePanned(val as u64),
        ))
        .padding(Padding {
            top: 0.0,
            right: 10.0,
            bottom: 0.0,
            left: 150.0,
        }) // Align with timeline lanes
    } else {
        container(Space::new().height(0))
    };

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

    column![main_view, scrollbar, details_panel].into()
}

struct TimelineProgram<'a> {
    threads: &'a [ThreadData],
    view_start_ns: u64,
    view_end_ns: u64,
    selected_event: &'a Option<TimelineEvent>,
}

#[derive(Default)]
struct TimelineState {
    drag_start: Option<Point>,
}

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

        let total_ns = self.view_end_ns.saturating_sub(self.view_start_ns);
        if total_ns == 0 || self.threads.is_empty() {
            return vec![frame.into_geometry()];
        }

        let mut y_offset = 0.0;
        for thread in self.threads {
            let lane_height = (thread.max_depth + 1) as f32 * 20.0;

            let mut last_rects: Vec<Option<(f32, f32, Color)>> =
                vec![None; (thread.max_depth + 1) as usize];

            for event in &thread.events {
                let event_end_ns = event.start_ns + event.duration_ns;
                if event_end_ns < self.view_start_ns || event.start_ns > self.view_end_ns {
                    continue;
                }

                let x = ((event.start_ns as f64 - self.view_start_ns as f64) / total_ns as f64)
                    as f32
                    * bounds.width;
                let width = (event.duration_ns as f64 / total_ns as f64) as f32 * bounds.width;
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
                    let x = ((selected.start_ns as f64 - self.view_start_ns as f64)
                        / total_ns as f64) as f32
                        * bounds.width;
                    let width =
                        (selected.duration_ns as f64 / total_ns as f64) as f32 * bounds.width;
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

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    let total_ns = self.view_end_ns.saturating_sub(self.view_start_ns);
                    if total_ns == 0 {
                        return None;
                    }

                    let mut y_offset = 0.0;
                    for thread in self.threads {
                        let lane_height = (thread.max_depth + 1) as f32 * 20.0;

                        if position.y >= y_offset && position.y < y_offset + lane_height {
                            for event in &thread.events {
                                let event_end_ns = event.start_ns + event.duration_ns;
                                if event_end_ns < self.view_start_ns
                                    || event.start_ns > self.view_end_ns
                                {
                                    continue;
                                }

                                let x = ((event.start_ns as f64 - self.view_start_ns as f64)
                                    / total_ns as f64)
                                    as f32
                                    * bounds.width;
                                let width = (event.duration_ns as f64 / total_ns as f64) as f32
                                    * bounds.width;
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

                    // If we reach here, no event was hit
                }
                // Always start drag if left button is pressed, using global position
                if let iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                    iced::mouse::Button::Left,
                )) = event
                {
                    state.drag_start = cursor.position();
                }
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                state.drag_start = None;
            }
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                if let Some(start_pos) = state.drag_start {
                    let current_pos = *position;
                    let delta_x = current_pos.x - start_pos.x;
                    state.drag_start = Some(current_pos);
                    return Some(Action::publish(Message::TimelineDragPanned { delta_x }));
                }
            }
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    match delta {
                        iced::mouse::ScrollDelta::Lines { x, y }
                        | iced::mouse::ScrollDelta::Pixels { x, y } => {
                            if y.abs() > x.abs() {
                                // Vertical scroll -> Zoom
                                return Some(Action::publish(Message::TimelineZoomed {
                                    delta: *y,
                                    x: position.x,
                                    width: bounds.width,
                                }));
                            } else {
                                // Horizontal scroll -> Pan
                                return Some(Action::publish(Message::TimelineScrolled {
                                    delta_x: *x,
                                    width: bounds.width,
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
