use crate::Message;
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{layout, renderer, Clipboard, Layout, Shell};
use iced::keyboard;
use iced::mouse;
use iced::widget::canvas::Action;
use iced::widget::canvas::{self, Canvas, Geometry, Program};
use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

pub const LABEL_WIDTH: f32 = 150.0;
pub const HEADER_HEIGHT: f32 = 30.0;
pub const MINI_TIMELINE_HEIGHT: f32 = 40.0;
pub const LANE_HEIGHT: f32 = 20.0;
pub const LANE_SPACING: f32 = 5.0;

#[derive(Debug, Clone, PartialEq)]
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

    Color::from_rgb(0.6 + r * 0.3, 0.6 + g * 0.3, 0.6 + b * 0.3)
}

pub fn timeline_id() -> iced::widget::Id {
    iced::widget::Id::new("timeline_scrollable")
}

pub fn format_duration(ns: u64) -> String {
    if ns >= 1_000_000_000 {
        format!("{:.2} s", ns as f64 / 1_000_000_000.0)
    } else if ns >= 1_000_000 {
        format!("{:.2} ms", ns as f64 / 1_000_000.0)
    } else if ns >= 1_000 {
        format!("{:.2} µs", ns as f64 / 1_000.0)
    } else {
        format!("{} ns", ns)
    }
}

pub fn view<'a>(
    timeline_data: &'a TimelineData,
    zoom_level: f32,
    selected_event: &'a Option<TimelineEvent>,
    hovered_event: &'a Option<TimelineEvent>,
    scroll_offset: Vector,
    viewport_width: f32,
    modifiers: keyboard::Modifiers,
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
        let lane_total_height = if thread.is_collapsed {
            LANE_HEIGHT
        } else {
            (thread.max_depth + 1) as f32 * LANE_HEIGHT
        };
        total_height += lane_total_height + LANE_SPACING;
    }

    let events_width = total_ns as f32 * zoom_level;

    let mini_timeline_canvas = Canvas::new(MiniTimelineProgram {
        min_ns: timeline_data.min_ns,
        max_ns: timeline_data.max_ns,
        zoom_level,
        scroll_offset,
        viewport_width,
    })
    .width(Length::Fill)
    .height(Length::Fixed(MINI_TIMELINE_HEIGHT));

    let header_canvas = Canvas::new(HeaderProgram {
        min_ns: timeline_data.min_ns,
        max_ns: timeline_data.max_ns,
        zoom_level,
        scroll_offset,
    })
    .width(Length::Fill)
    .height(Length::Fixed(HEADER_HEIGHT));

    let threads_canvas = Canvas::new(ThreadsProgram {
        threads: &timeline_data.threads,
        scroll_offset,
    })
    .width(Length::Fixed(LABEL_WIDTH))
    .height(Length::Fill);

    let events_canvas = Canvas::new(EventsProgram {
        threads: &timeline_data.threads,
        min_ns: timeline_data.min_ns,
        zoom_level,
        selected_event,
    })
    .width(Length::Fixed(events_width))
    .height(Length::Fixed(total_height));

    let events_view = scrollable(WheelCatcher::new(events_canvas, modifiers))
        .id(timeline_id())
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        })
        .on_scroll(|viewport| Message::TimelineScroll {
            offset: Vector::new(viewport.absolute_offset().x, viewport.absolute_offset().y),
            viewport_width: viewport.bounds().width,
        });

    let main_view = column![
        row![
            Space::new().width(Length::Fixed(LABEL_WIDTH)),
            mini_timeline_canvas
        ]
        .height(Length::Fixed(MINI_TIMELINE_HEIGHT)),
        row![
            Space::new().width(Length::Fixed(LABEL_WIDTH)),
            header_canvas
        ]
        .height(Length::Fixed(HEADER_HEIGHT)),
        row![threads_canvas, events_view].height(Length::Fill)
    ]
    .height(Length::Fill);

    let display_event = selected_event.as_ref().or(hovered_event.as_ref());

    let details_panel = if let Some(event) = display_event {
        container(column![
            row![text("Summary").size(14), Space::new().width(Length::Fill),]
                .padding(5)
                .align_y(iced::Alignment::Center),
            container(Space::new().height(1.0))
                .width(Length::Fill)
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style::default().background(palette.background.strong.color)
                }),
            column![
                row![
                    text("Label:").width(Length::Fixed(80.0)).size(12),
                    text(&event.label).size(12),
                ],
                row![
                    text("Thread:").width(Length::Fixed(80.0)).size(12),
                    text(format!("{}", event.thread_id)).size(12),
                ],
                row![
                    text("Start:").width(Length::Fixed(80.0)).size(12),
                    text(format_duration(
                        event.start_ns.saturating_sub(timeline_data.min_ns)
                    ))
                    .size(12),
                ],
                row![
                    text("Duration:").width(Length::Fixed(80.0)).size(12),
                    text(format_duration(event.duration_ns)).size(12),
                ],
            ]
            .spacing(5)
            .padding(10),
        ])
        .width(Length::Fill)
        .height(Length::Fixed(150.0))
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style::default()
                .background(palette.background.base.color)
                .border(iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    ..Default::default()
                })
        })
    } else {
        container(
            text("Select or hover over an event to see details")
                .size(12)
                .color(Color::from_rgb(0.5, 0.5, 0.5)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(150.0))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style::default()
                .background(palette.background.base.color)
                .border(iced::Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    ..Default::default()
                })
        })
    };

    column![main_view, details_panel]
        .height(Length::Fill)
        .into()
}

struct EventsProgram<'a> {
    threads: &'a [ThreadData],
    min_ns: u64,
    zoom_level: f32,
    selected_event: &'a Option<TimelineEvent>,
}

#[derive(Default)]
struct EventsState {
    modifiers: keyboard::Modifiers,
    hovered_event: Option<TimelineEvent>,
}

impl<'a> EventsProgram<'a> {
    fn find_event_at(&self, position: Point) -> Option<TimelineEvent> {
        let position = position;
        let mut y_offset = 0.0;
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

                    let width = (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                    if width < 5.0 {
                        continue;
                    }

                    let x = (event.start_ns.saturating_sub(self.min_ns) as f64
                        * self.zoom_level as f64) as f32;
                    let y = y_offset + event.depth as f32 * LANE_HEIGHT;
                    let height = LANE_HEIGHT - 2.0;

                    let rect = Rectangle {
                        x,
                        y,
                        width: width.max(1.0),
                        height,
                    };

                    if rect.contains(position) {
                        return Some(event.clone());
                    }
                }
            }
            y_offset += lane_total_height + LANE_SPACING;
        }
        None
    }
}

impl<'a> Program<Message> for EventsProgram<'a> {
    type State = EventsState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        if self.threads.is_empty() {
            return vec![frame.into_geometry()];
        }

        let mut y_offset = 0.0;
        for thread in self.threads {
            let lane_total_height = if thread.is_collapsed {
                LANE_HEIGHT
            } else {
                (thread.max_depth + 1) as f32 * LANE_HEIGHT
            };

            frame.stroke(
                &canvas::Path::line(
                    Point::new(0.0, y_offset),
                    Point::new(bounds.width, y_offset),
                ),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.9, 0.9, 0.9))
                    .with_width(1.0),
            );

            let mut last_rects: Vec<Option<(f32, f32, Color, String)>> =
                vec![None; (thread.max_depth + 1) as usize];

            for event in &thread.events {
                if thread.is_collapsed && event.depth > 0 {
                    continue;
                }

                let width = (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                if width < 5.0 {
                    continue;
                }

                let x = (event.start_ns.saturating_sub(self.min_ns) as f64 * self.zoom_level as f64)
                    as f32;
                let depth = event.depth as usize;
                let color = event.color;
                let label = &event.label;

                if let Some((cur_x, cur_w, cur_color, cur_label)) = &mut last_rects[depth] {
                    let end_x = *cur_x + *cur_w;
                    if color == *cur_color && x <= end_x + 0.5 && label == cur_label {
                        let new_end = (x + width).max(end_x);
                        *cur_w = new_end - *cur_x;
                        continue;
                    } else {
                        let y = y_offset + depth as f32 * LANE_HEIGHT;
                        let rect = Rectangle {
                            x: *cur_x,
                            y: y + 1.0,
                            width: cur_w.max(1.0),
                            height: LANE_HEIGHT - 2.0,
                        };

                        frame.fill_rectangle(rect.position(), rect.size(), *cur_color);

                        frame.stroke(
                            &canvas::Path::rectangle(rect.position(), rect.size()),
                            canvas::Stroke::default()
                                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.2))
                                .with_width(1.0),
                        );

                        if rect.width > 20.0 {
                            let mut truncated_label = cur_label.clone();
                            if truncated_label.len() > (rect.width / 6.0) as usize {
                                truncated_label.truncate((rect.width / 6.0) as usize);
                            }
                            frame.fill_text(canvas::Text {
                                content: truncated_label,
                                position: Point::new(rect.x + 2.0, rect.y + 2.0),
                                color: Color::from_rgb(0.2, 0.2, 0.2),
                                size: 10.0.into(),
                                ..Default::default()
                            });
                        }
                    }
                }
                last_rects[depth] = Some((x, width, color, label.clone()));
            }

            for (depth, rect) in last_rects.into_iter().enumerate() {
                if let Some((cur_x, cur_w, cur_color, cur_label)) = rect {
                    let y = y_offset + depth as f32 * LANE_HEIGHT;
                    let rect = Rectangle {
                        x: cur_x,
                        y: y + 1.0,
                        width: cur_w.max(1.0),
                        height: LANE_HEIGHT - 2.0,
                    };

                    frame.fill_rectangle(rect.position(), rect.size(), cur_color);

                    frame.stroke(
                        &canvas::Path::rectangle(rect.position(), rect.size()),
                        canvas::Stroke::default()
                            .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.2))
                            .with_width(1.0),
                    );

                    if rect.width > 20.0 {
                        let mut truncated_label = cur_label;
                        if truncated_label.len() > (rect.width / 6.0) as usize {
                            truncated_label.truncate((rect.width / 6.0) as usize);
                        }
                        frame.fill_text(canvas::Text {
                            content: truncated_label,
                            position: Point::new(rect.x + 2.0, rect.y + 2.0),
                            color: Color::from_rgb(0.2, 0.2, 0.2),
                            size: 10.0.into(),
                            ..Default::default()
                        });
                    }
                }
            }

            if let Some(hovered) = &state.hovered_event {
                if hovered.thread_id == thread.thread_id {
                    if !thread.is_collapsed || hovered.depth == 0 {
                        let x = (hovered.start_ns.saturating_sub(self.min_ns) as f64
                            * self.zoom_level as f64) as f32;
                        let width = (hovered.duration_ns as f64 * self.zoom_level as f64) as f32;
                        let y = y_offset + hovered.depth as f32 * LANE_HEIGHT;

                        frame.stroke(
                            &canvas::Path::rectangle(
                                Point::new(x, y + 1.0),
                                Size::new(width.max(1.0), LANE_HEIGHT - 2.0),
                            ),
                            canvas::Stroke::default()
                                .with_color(Color::from_rgba(0.0, 0.0, 0.0, 0.3))
                                .with_width(1.0),
                        );
                    }
                }
            }

            if let Some(selected) = self.selected_event {
                if selected.thread_id == thread.thread_id {
                    if !thread.is_collapsed || selected.depth == 0 {
                        let x = (selected.start_ns.saturating_sub(self.min_ns) as f64
                            * self.zoom_level as f64) as f32;
                        let width = (selected.duration_ns as f64 * self.zoom_level as f64) as f32;
                        let y = y_offset + selected.depth as f32 * LANE_HEIGHT;

                        frame.stroke(
                            &canvas::Path::rectangle(
                                Point::new(x, y + 1.0),
                                Size::new(width.max(1.0), LANE_HEIGHT - 2.0),
                            ),
                            canvas::Stroke::default()
                                .with_color(Color::from_rgb(0.0, 0.4, 0.8))
                                .with_width(2.0),
                        );
                    }
                }
            }

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
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let new_hovered = cursor
                    .position_in(bounds)
                    .and_then(|p| self.find_event_at(p));

                if new_hovered != state.hovered_event {
                    state.hovered_event = new_hovered;
                    return Some(Action::publish(Message::EventHovered(
                        state.hovered_event.clone(),
                    )));
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    if let Some(event) = self.find_event_at(position) {
                        return Some(Action::publish(Message::EventSelected(event)));
                    }
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    if !state.modifiers.control() {
                        match delta {
                            mouse::ScrollDelta::Lines { x: _, y }
                            | mouse::ScrollDelta::Pixels { x: _, y } => {
                                if y.abs() > 0.0 {
                                    return Some(Action::publish(Message::TimelineZoomed {
                                        delta: *y,
                                        x: position.x,
                                    }));
                                }
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

struct HeaderProgram {
    min_ns: u64,
    max_ns: u64,
    zoom_level: f32,
    scroll_offset: Vector,
}

struct MiniTimelineProgram {
    min_ns: u64,
    max_ns: u64,
    zoom_level: f32,
    scroll_offset: Vector,
    viewport_width: f32,
}

#[derive(Default)]
struct MiniTimelineState {
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

            frame.stroke(
                &canvas::Path::line(
                    Point::new(x, bounds.height - 8.0),
                    Point::new(x, bounds.height),
                ),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.6, 0.6, 0.6))
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

impl Program<Message> for HeaderProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            Size::new(bounds.width, bounds.height),
            Color::from_rgb(0.95, 0.95, 0.95),
        );

        let total_ns = self.max_ns.saturating_sub(self.min_ns) as f64;
        if total_ns <= 0.0 {
            return vec![frame.into_geometry()];
        }

        let ns_per_pixel = 1.0 / self.zoom_level as f64;
        let pixel_interval = 100.0;
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
            let x = (relative_ns * self.zoom_level as f64) as f32 - self.scroll_offset.x;

            if x >= 0.0 && x <= bounds.width {
                frame.stroke(
                    &canvas::Path::line(
                        Point::new(x, bounds.height - 5.0),
                        Point::new(x, bounds.height),
                    ),
                    canvas::Stroke::default()
                        .with_color(Color::from_rgb(0.5, 0.5, 0.5))
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
                    position: Point::new(x + 2.0, 5.0),
                    color: Color::from_rgb(0.3, 0.3, 0.3),
                    size: 10.0.into(),
                    ..Default::default()
                });
            }
            relative_ns += nice_interval;
        }

        vec![frame.into_geometry()]
    }
}

struct ThreadsProgram<'a> {
    threads: &'a [ThreadData],
    scroll_offset: Vector,
}

#[derive(Default)]
struct ThreadsState {
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

pub struct WheelCatcher<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    modifiers: keyboard::Modifiers,
}

impl<'a, Message, Theme, Renderer> WheelCatcher<'a, Message, Theme, Renderer> {
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        modifiers: keyboard::Modifiers,
    ) -> Self {
        Self {
            content: content.into(),
            modifiers,
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for WheelCatcher<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
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
        self.content.as_widget().draw(
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
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            _viewport,
        );

        if let Event::Mouse(mouse::Event::WheelScrolled { .. }) = event {
            if !self.modifiers.control() {
                shell.capture_event();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
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
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
}

impl<'a, Message, Theme, Renderer> From<WheelCatcher<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(catcher: WheelCatcher<'a, Message, Theme, Renderer>) -> Self {
        Self::new(catcher)
    }
}
