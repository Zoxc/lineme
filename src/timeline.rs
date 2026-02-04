mod header;
mod mini_timeline;
mod threads;

use crate::Message;
use header::HeaderProgram;
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{layout, renderer, Clipboard, Layout, Shell};
use iced::keyboard;
use iced::mouse;
use iced::widget::canvas::Action;
use iced::widget::canvas::{self, Canvas, Geometry, Program};
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};
use mini_timeline::MiniTimelineProgram;
use std::sync::Arc;
use threads::ThreadsProgram;

pub const LABEL_WIDTH: f32 = 150.0;
pub const HEADER_HEIGHT: f32 = 30.0;
pub const MINI_TIMELINE_HEIGHT: f32 = 40.0;
pub const LANE_HEIGHT: f32 = 20.0;
pub const LANE_SPACING: f32 = 5.0;
pub const DRAG_THRESHOLD: f32 = 3.0;
pub const EVENT_LEFT_PADDING: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    #[default]
    Kind,
    Event,
}

impl ColorMode {
    pub const ALL: [ColorMode; 2] = [ColorMode::Kind, ColorMode::Event];
}

impl std::fmt::Display for ColorMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorMode::Kind => write!(f, "Kind"),
            ColorMode::Event => write!(f, "Event"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEvent {
    pub label: String,
    pub start_ns: u64,
    pub duration_ns: u64,
    pub depth: u32,
    pub thread_id: u64,
    pub event_kind: String,
    pub additional_data: Vec<String>,
    pub payload_integer: Option<u64>,
    pub color: Color,
    pub is_thread_root: bool,
}

#[derive(Debug, Clone)]
pub struct ThreadData {
    pub thread_id: u64,
    pub events: Vec<TimelineEvent>,
}

pub type ThreadGroupId = Arc<Vec<Arc<ThreadData>>>;
pub type ThreadGroupKey = usize;

#[derive(Debug, Clone)]
pub struct ThreadGroup {
    pub threads: ThreadGroupId,
    pub events: Vec<TimelineEvent>,
    pub events_by_start: Vec<usize>,
    pub events_by_end: Vec<usize>,
    pub max_depth: u32,
    pub is_collapsed: bool,
}

pub fn thread_group_key(group: &ThreadGroup) -> ThreadGroupKey {
    Arc::as_ptr(&group.threads) as ThreadGroupKey
}

#[derive(Debug, Clone, Default)]
pub struct TimelineData {
    pub thread_groups: Vec<ThreadGroup>,
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

pub fn total_timeline_height(thread_groups: &[ThreadGroup]) -> f32 {
    let mut total_height = 0.0;
    for group in thread_groups {
        let lane_total_height = if group.is_collapsed {
            LANE_HEIGHT
        } else {
            (group.max_depth + 1) as f32 * LANE_HEIGHT
        };
        total_height += lane_total_height + LANE_SPACING;
    }
    total_height
}

pub fn build_thread_group_events(
    threads: &[Arc<ThreadData>],
) -> (Vec<TimelineEvent>, u32, Vec<usize>, Vec<usize>) {
    if threads.len() > 1 {
        let mut events = Vec::new();

        for thread in threads {
            let mut start_ns = u64::MAX;
            let mut end_ns = 0u64;

            for event in &thread.events {
                start_ns = start_ns.min(event.start_ns);
                end_ns = end_ns.max(event.start_ns.saturating_add(event.duration_ns));
            }

            if start_ns != u64::MAX {
                events.push(TimelineEvent {
                    label: format!("Thread {}", thread.thread_id),
                    start_ns,
                    duration_ns: end_ns.saturating_sub(start_ns),
                    depth: 0,
                    thread_id: thread.thread_id,
                    event_kind: "Thread".to_string(),
                    additional_data: Vec::new(),
                    payload_integer: None,
                    color: Color::from_rgb(0.85, 0.87, 0.9),
                    is_thread_root: true,
                });

                for event in &thread.events {
                    let mut event = event.clone();
                    event.depth = event.depth.saturating_add(1);
                    event.is_thread_root = false;
                    events.push(event);
                }
            }
        }

        events.sort_by_key(|event| (event.start_ns, event.thread_id, event.depth));
        let max_depth = events.iter().map(|event| event.depth).max().unwrap_or(0);
        let (events_by_start, events_by_end) = build_event_indices(&events);
        return (events, max_depth, events_by_start, events_by_end);
    }

    let mut events = Vec::new();
    for thread in threads {
        events.extend(thread.events.iter().cloned());
    }

    events.sort_by_key(|event| (event.start_ns, event.thread_id));

    let mut stack: Vec<u64> = Vec::new();
    for event in events.iter_mut() {
        let end_ns = event.start_ns + event.duration_ns;
        while let Some(&last_end) = stack.last() {
            if last_end <= event.start_ns {
                stack.pop();
            } else {
                break;
            }
        }
        event.depth = stack.len() as u32;
        event.is_thread_root = false;
        stack.push(end_ns);
    }

    let max_depth = events.iter().map(|event| event.depth).max().unwrap_or(0);
    let (events_by_start, events_by_end) = build_event_indices(&events);
    (events, max_depth, events_by_start, events_by_end)
}

fn event_end_ns(event: &TimelineEvent) -> u64 {
    event.start_ns.saturating_add(event.duration_ns)
}

fn build_event_indices(events: &[TimelineEvent]) -> (Vec<usize>, Vec<usize>) {
    let mut events_by_start: Vec<usize> = (0..events.len()).collect();
    events_by_start.sort_by_key(|&index| {
        let event = &events[index];
        (event.start_ns, event.thread_id, event.depth)
    });

    let mut events_by_end: Vec<usize> = (0..events.len()).collect();
    events_by_end.sort_by_key(|&index| {
        let event = &events[index];
        (event_end_ns(event), event.start_ns, event.thread_id)
    });

    (events_by_start, events_by_end)
}

fn visible_event_indices(group: &ThreadGroup, ns_min: u64, ns_max: u64) -> Vec<usize> {
    let events = &group.events;
    let start_upper = group
        .events_by_start
        .partition_point(|&index| events[index].start_ns <= ns_max);
    let end_lower = group
        .events_by_end
        .partition_point(|&index| event_end_ns(&events[index]) < ns_min);

    let start_candidates = start_upper;
    let end_candidates = group.events_by_end.len().saturating_sub(end_lower);
    let mut indices = Vec::with_capacity(start_candidates.min(end_candidates));

    if start_candidates <= end_candidates {
        for &index in group.events_by_start[..start_upper].iter() {
            if event_end_ns(&events[index]) >= ns_min {
                indices.push(index);
            }
        }
    } else {
        for &index in group.events_by_end[end_lower..].iter() {
            if events[index].start_ns <= ns_max {
                indices.push(index);
            }
        }
    }

    indices
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
    thread_groups: &'a [ThreadGroup],
    zoom_level: f32,
    selected_event: &'a Option<TimelineEvent>,
    _hovered_event: &'a Option<TimelineEvent>,
    scroll_offset: Vector,
    viewport_width: f32,
    viewport_height: f32,
    modifiers: keyboard::Modifiers,
    color_mode: ColorMode,
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

    let total_height = total_timeline_height(thread_groups);

    let events_width = (total_ns as f64 * zoom_level as f64).ceil() as f32;

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
        thread_groups,
        scroll_offset,
    })
    .width(Length::Fixed(LABEL_WIDTH))
    .height(Length::Fill);

    let events_canvas = Canvas::new(EventsProgram {
        thread_groups,
        min_ns: timeline_data.min_ns,
        max_ns: timeline_data.max_ns,
        zoom_level,
        selected_event,
        scroll_offset,
        viewport_width,
        viewport_height,
        color_mode,
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
            viewport_height: viewport.bounds().height,
        });

    // Mini timeline should span the full window width (including the label area).
    let main_view = column![
        // Full-width mini timeline on its own row.
        mini_timeline_canvas.height(Length::Fixed(MINI_TIMELINE_HEIGHT)),
        PanCatcher::new(
            column![
                // Header remains aligned with the events area (leaving space for labels).
                row![
                    // Left area above the thread labels: collapse/expand all buttons
                    container(
                        row![
                            // Collapse button with short text
                            button(
                                row![text("-").size(18), text("Collapse").size(12)]
                                    .spacing(4)
                                    .align_y(iced::Alignment::Center),
                            )
                            .padding(6)
                            .style(|theme: &Theme, status: button::Status| {
                                let palette = theme.extended_palette();
                                let base = button::Style {
                                    text_color: palette.background.weak.text,
                                    ..Default::default()
                                };
                                match status {
                                    button::Status::Hovered | button::Status::Pressed => {
                                        button::Style {
                                            background: Some(
                                                palette.background.strong.color.into(),
                                            ),
                                            ..base
                                        }
                                    }
                                    _ => base,
                                }
                            })
                            .on_press(Message::CollapseAllThreads),
                            // Expand button with short text
                            button(
                                row![text("+").size(18), text("Expand").size(12)]
                                    .spacing(4)
                                    .align_y(iced::Alignment::Center),
                            )
                            .padding(6)
                            .style(|theme: &Theme, status: button::Status| {
                                let palette = theme.extended_palette();
                                let base = button::Style {
                                    text_color: palette.background.weak.text,
                                    ..Default::default()
                                };
                                match status {
                                    button::Status::Hovered | button::Status::Pressed => {
                                        button::Style {
                                            background: Some(
                                                palette.background.strong.color.into(),
                                            ),
                                            ..base
                                        }
                                    }
                                    _ => base,
                                }
                            })
                            .on_press(Message::ExpandAllThreads),
                        ]
                        .spacing(5)
                        .align_y(iced::Alignment::Center),
                    )
                    .width(Length::Fixed(LABEL_WIDTH)),
                    header_canvas
                ]
                .height(Length::Fixed(HEADER_HEIGHT)),
                row![threads_canvas, events_view].height(Length::Fill)
            ]
            .height(Length::Fill),
        )
    ]
    .height(Length::Fill);

    // Only use explicit selections (clicks) to populate the details panel.
    let display_event = selected_event.as_ref();

    // Only show the details panel when an event is selected or hovered.
    if let Some(event) = display_event {
        // Build details column, including one row per additional_data item.
        let mut details_col = column![
            row![
                text("Label:").width(Length::Fixed(80.0)).size(12),
                text(&event.label).size(12)
            ],
            row![
                text("Kind:").width(Length::Fixed(80.0)).size(12),
                text(&event.event_kind).size(12)
            ],
            row![
                text("Thread:").width(Length::Fixed(80.0)).size(12),
                text(format!("{}", event.thread_id)).size(12)
            ],
            row![
                text("Start:").width(Length::Fixed(80.0)).size(12),
                text(format_duration(
                    event.start_ns.saturating_sub(timeline_data.min_ns)
                ))
                .size(12)
            ],
            row![
                text("Duration:").width(Length::Fixed(80.0)).size(12),
                text(format_duration(event.duration_ns)).size(12)
            ],
        ]
        .spacing(5)
        .padding(10);

        for item in &event.additional_data {
            details_col = details_col.push(row![
                text("Data:").width(Length::Fixed(80.0)).size(12),
                text(item).size(12),
            ]);
        }

        if let Some(v) = event.payload_integer {
            details_col = details_col.push(row![
                text("Value:").width(Length::Fixed(80.0)).size(12),
                text(format!("{}", v)).size(12),
            ]);
        }

        let details_panel = container(column![
            row![text("Details").size(14), Space::new().width(Length::Fill),]
                .padding(5)
                .align_y(iced::Alignment::Center),
            container(Space::new().height(1.0))
                .width(Length::Fill)
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style::default().background(palette.background.strong.color)
                }),
            details_col,
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
        });

        column![main_view, details_panel]
            .height(Length::Fill)
            .into()
    } else {
        // No details to show: return the main view only.
        main_view.height(Length::Fill).into()
    }
}

struct EventsProgram<'a> {
    thread_groups: &'a [ThreadGroup],
    min_ns: u64,
    max_ns: u64,
    zoom_level: f32,
    selected_event: &'a Option<TimelineEvent>,
    scroll_offset: Vector,
    viewport_width: f32,
    viewport_height: f32,
    color_mode: ColorMode,
}

#[derive(Default)]
struct EventsState {
    modifiers: keyboard::Modifiers,
    hovered_event: Option<TimelineEvent>,
    last_click: Option<(TimelineEvent, std::time::Instant)>,
    press_position: Option<Point>,
    pressed_event: Option<TimelineEvent>,
    dragging: bool,
}

impl<'a> EventsProgram<'a> {
    fn find_event_at(&self, position: Point) -> Option<TimelineEvent> {
        let position = position;
        let mut y_offset = 0.0;
        for group in self.thread_groups {
            let lane_total_height = if group.is_collapsed {
                LANE_HEIGHT
            } else {
                (group.max_depth + 1) as f32 * LANE_HEIGHT
            };

            if position.y >= y_offset && position.y < y_offset + lane_total_height {
                let ns_min = (self.scroll_offset.x as f64 / self.zoom_level as f64).max(0.0) as u64
                    + self.min_ns;
                let ns_max = ((self.scroll_offset.x + self.viewport_width) as f64
                    / self.zoom_level as f64)
                    .max(0.0) as u64
                    + self.min_ns;

                for index in visible_event_indices(group, ns_min, ns_max) {
                    let event = &group.events[index];
                    if group.is_collapsed && event.depth > 0 {
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

        if self.thread_groups.is_empty() {
            return vec![frame.into_geometry()];
        }

        // Draw vertical tick guide lines matching the header ticks.
        let total_ns = self.max_ns.saturating_sub(self.min_ns) as f64;
        let x_min = self.scroll_offset.x;
        let x_max = self.scroll_offset.x + self.viewport_width;
        let ns_min = (x_min as f64 / self.zoom_level as f64).max(0.0) as u64 + self.min_ns;
        let ns_max = (x_max as f64 / self.zoom_level as f64).max(0.0) as u64 + self.min_ns;

        if total_ns > 0.0 {
            // ns per pixel given current zoom: 1 / zoom_level
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

            let mut relative_ns = if self.viewport_width > 0.0 {
                (x_min as f64 / self.zoom_level as f64 / nice_interval).floor() * nice_interval
            } else {
                0.0
            };

            while relative_ns <= total_ns {
                let x = (relative_ns * self.zoom_level as f64) as f32;
                if self.viewport_width > 0.0 && x > x_max {
                    break;
                }

                // Draw faint vertical line across the events area.
                frame.stroke(
                    &canvas::Path::line(Point::new(x, 0.0), Point::new(x, bounds.height)),
                    canvas::Stroke::default()
                        .with_color(Color::from_rgba(0.5, 0.5, 0.5, 0.3))
                        .with_width(1.0),
                );

                relative_ns += nice_interval;
            }
        }

        let mut y_offset = 0.0;
        let y_min = self.scroll_offset.y;
        let y_max = self.scroll_offset.y + self.viewport_height;

        for group in self.thread_groups {
            let lane_total_height = if group.is_collapsed {
                LANE_HEIGHT
            } else {
                (group.max_depth + 1) as f32 * LANE_HEIGHT
            };

            // Skip drawing if thread is completely outside vertical viewport
            if self.viewport_height > 0.0
                && (y_offset + lane_total_height < y_min || y_offset > y_max)
            {
                y_offset += lane_total_height + LANE_SPACING;
                continue;
            }

            frame.stroke(
                &canvas::Path::line(
                    Point::new(0.0, y_offset),
                    Point::new(bounds.width, y_offset),
                ),
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.9, 0.9, 0.9))
                    .with_width(1.0),
            );

            let mut last_rects: Vec<Option<(f32, f32, Color, String, bool)>> =
                vec![None; (group.max_depth + 1) as usize];

            for index in visible_event_indices(group, ns_min, ns_max) {
                let event = &group.events[index];
                if group.is_collapsed && event.depth > 0 {
                    continue;
                }

                let width = (event.duration_ns as f64 * self.zoom_level as f64) as f32;
                if width < 5.0 {
                    continue;
                }

                let x = (event.start_ns.saturating_sub(self.min_ns) as f64 * self.zoom_level as f64)
                    as f32;

                // Skip drawing if event is completely outside horizontal viewport
                if self.viewport_width > 0.0 && (x + width < x_min || x > x_max) {
                    continue;
                }

                let depth = event.depth as usize;
                let color = if event.is_thread_root {
                    event.color
                } else {
                    match self.color_mode {
                        ColorMode::Kind => color_from_label(&event.event_kind),
                        ColorMode::Event => color_from_label(&event.label),
                    }
                };
                let label = &event.label;
                let is_thread_root = event.is_thread_root;

                if let Some((cur_x, cur_w, cur_color, cur_label, cur_is_root)) =
                    &mut last_rects[depth]
                {
                    let end_x = *cur_x + *cur_w;
                    if !is_thread_root
                        && color == *cur_color
                        && x <= end_x + 0.5
                        && label == cur_label
                    {
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

                        let border_color = if *cur_is_root {
                            Color::from_rgba(0.0, 0.0, 0.0, 0.35)
                        } else {
                            Color::from_rgba(0.0, 0.0, 0.0, 0.2)
                        };

                        frame.stroke(
                            &canvas::Path::rectangle(rect.position(), rect.size()),
                            canvas::Stroke::default()
                                .with_color(border_color)
                                .with_width(1.0),
                        );

                        if rect.width > 20.0 {
                            let mut truncated_label = cur_label.clone();
                            let avail_chars =
                                ((rect.width - 4.0 - EVENT_LEFT_PADDING).max(0.0) / 6.0) as usize;
                            if truncated_label.len() > avail_chars {
                                truncated_label.truncate(avail_chars);
                            }
                            frame.with_clip(
                                Rectangle {
                                    x: rect.x + 1.0,
                                    y: rect.y + 1.0,
                                    width: rect.width - 2.0,
                                    height: rect.height - 2.0,
                                },
                                |frame| {
                                    frame.fill_text(canvas::Text {
                                        content: truncated_label,
                                        position: Point::new(
                                            rect.x + 2.0 + EVENT_LEFT_PADDING,
                                            rect.y + 2.0,
                                        ),
                                        color: if *cur_is_root {
                                            Color::from_rgb(0.35, 0.35, 0.35)
                                        } else {
                                            Color::from_rgb(0.2, 0.2, 0.2)
                                        },
                                        size: 12.0.into(),
                                        ..Default::default()
                                    });
                                },
                            );
                        }
                    }
                }
                last_rects[depth] = Some((x, width, color, label.clone(), is_thread_root));
            }

            for (depth, rect) in last_rects.into_iter().enumerate() {
                if let Some((cur_x, cur_w, cur_color, cur_label, cur_is_root)) = rect {
                    let y = y_offset + depth as f32 * LANE_HEIGHT;
                    let rect = Rectangle {
                        x: cur_x,
                        y: y + 1.0,
                        width: cur_w.max(1.0),
                        height: LANE_HEIGHT - 2.0,
                    };

                    frame.fill_rectangle(rect.position(), rect.size(), cur_color);

                    let border_color = if cur_is_root {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.35)
                    } else {
                        Color::from_rgba(0.0, 0.0, 0.0, 0.2)
                    };

                    frame.stroke(
                        &canvas::Path::rectangle(rect.position(), rect.size()),
                        canvas::Stroke::default()
                            .with_color(border_color)
                            .with_width(1.0),
                    );

                    if rect.width > 20.0 {
                        let mut truncated_label = cur_label;
                        let avail_chars =
                            ((rect.width - 4.0 - EVENT_LEFT_PADDING).max(0.0) / 6.0) as usize;
                        if truncated_label.len() > avail_chars {
                            truncated_label.truncate(avail_chars);
                        }
                        frame.with_clip(
                            Rectangle {
                                x: rect.x + 1.0,
                                y: rect.y + 1.0,
                                width: rect.width - 2.0,
                                height: rect.height - 2.0,
                            },
                            |frame| {
                                frame.fill_text(canvas::Text {
                                    content: truncated_label,
                                    position: Point::new(
                                        rect.x + 2.0 + EVENT_LEFT_PADDING,
                                        rect.y + 2.0,
                                    ),
                                    color: if cur_is_root {
                                        Color::from_rgb(0.35, 0.35, 0.35)
                                    } else {
                                        Color::from_rgb(0.2, 0.2, 0.2)
                                    },
                                    size: 12.0.into(),
                                    ..Default::default()
                                });
                            },
                        );
                    }
                }
            }

            if let Some(hovered) = &state.hovered_event {
                if group_contains_thread(group, hovered.thread_id) {
                    if !group.is_collapsed || hovered.depth == 0 {
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
                if group_contains_thread(group, selected.thread_id) {
                    if !group.is_collapsed || selected.depth == 0 {
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
                if let (
                    Some(press_position),
                    Event::Mouse(mouse::Event::CursorMoved { position }),
                ) = (state.press_position, event)
                {
                    let delta = *position - press_position;
                    if !state.dragging && delta.x.hypot(delta.y) > DRAG_THRESHOLD {
                        state.dragging = true;
                    }
                }
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
                    state.press_position = cursor.position();
                    state.pressed_event = self.find_event_at(position);
                    state.dragging = false;
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.dragging {
                    if let (Some(pressed_event), Some(position)) =
                        (state.pressed_event.clone(), cursor.position_in(bounds))
                    {
                        if let Some(release_event) = self.find_event_at(position) {
                            let is_same_event = pressed_event.start_ns == release_event.start_ns
                                && pressed_event.duration_ns == release_event.duration_ns
                                && pressed_event.thread_id == release_event.thread_id;
                            if is_same_event {
                                let now = std::time::Instant::now();
                                if let Some((prev_event, prev_time)) = &state.last_click {
                                    let is_double = prev_event.start_ns == release_event.start_ns
                                        && prev_event.duration_ns == release_event.duration_ns
                                        && prev_event.thread_id == release_event.thread_id
                                        && now.duration_since(*prev_time)
                                            <= std::time::Duration::from_millis(400);
                                    if is_double {
                                        state.last_click = None;
                                        state.press_position = None;
                                        state.pressed_event = None;
                                        state.dragging = false;
                                        return Some(Action::publish(Message::EventDoubleClicked(
                                            release_event,
                                        )));
                                    }
                                }

                                state.last_click = Some((release_event.clone(), now));
                                state.press_position = None;
                                state.pressed_event = None;
                                state.dragging = false;
                                return Some(Action::publish(Message::EventSelected(
                                    release_event,
                                )));
                            }
                        }
                    }
                }
                state.press_position = None;
                state.pressed_event = None;
                state.dragging = false;
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    // Shift + wheel: pan horizontally
                    if state.modifiers.shift() {
                        match delta {
                            mouse::ScrollDelta::Lines { x: _, y }
                            | mouse::ScrollDelta::Pixels { x: _, y } => {
                                if y.abs() > 0.0 {
                                    // Map wheel "lines" to pixels for a comfortable pan speed
                                    let scroll_amount = (*y as f32) * 30.0;
                                    return Some(Action::publish(Message::TimelinePanned {
                                        delta: Vector::new(scroll_amount, 0.0),
                                    }));
                                }
                            }
                        }
                    // Control (or other) keys: default behavior handled elsewhere — we only
                    // intercept wheel when control is NOT held to provide zoom by wheel.
                    } else if !state.modifiers.control() {
                        match delta {
                            mouse::ScrollDelta::Lines { x: _, y }
                            | mouse::ScrollDelta::Pixels { x: _, y } => {
                                if y.abs() > 0.0 {
                                    let viewport_width = self.viewport_width.max(0.0);
                                    let cursor_x = (position.x - self.scroll_offset.x)
                                        .clamp(0.0, viewport_width);
                                    return Some(Action::publish(Message::TimelineZoomed {
                                        delta: *y,
                                        x: cursor_x,
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

fn group_contains_thread(group: &ThreadGroup, thread_id: u64) -> bool {
    group
        .threads
        .iter()
        .any(|thread| thread.thread_id == thread_id)
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

struct PanCatcher<'a, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
}

impl<'a, Theme, Renderer> PanCatcher<'a, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

#[derive(Default)]
struct PanState {
    press_position: Option<Point>,
    last_position: Option<Point>,
    dragging: bool,
}

impl<'a, Theme, Renderer> Widget<Message, Theme, Renderer> for PanCatcher<'a, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(PanState::default())
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
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<PanState>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if !shell.is_event_captured() {
                    if let Some(position) = cursor.position_over(bounds) {
                        state.press_position = Some(position);
                        state.last_position = Some(position);
                        state.dragging = false;
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.press_position = None;
                state.last_position = None;
                state.dragging = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(press_position) = state.press_position {
                    let delta_from_press = *position - press_position;
                    if !state.dragging
                        && delta_from_press.x.hypot(delta_from_press.y) > DRAG_THRESHOLD
                    {
                        state.dragging = true;
                        state.last_position = Some(*position);
                    }

                    if state.dragging {
                        if let Some(last_position) = state.last_position {
                            let delta = *position - last_position;
                            if delta.x != 0.0 || delta.y != 0.0 {
                                shell.publish(Message::TimelinePanned { delta });
                                shell.capture_event();
                            }
                        }
                        state.last_position = Some(*position);
                    }
                }
            }
            _ => {}
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
        let state = tree.state.downcast_ref::<PanState>();
        let interaction = self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        );

        if state.dragging {
            mouse::Interaction::Grabbing
        } else {
            interaction
        }
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

impl<'a, Theme, Renderer> From<PanCatcher<'a, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(catcher: PanCatcher<'a, Theme, Renderer>) -> Self {
        Self::new(catcher)
    }
}
