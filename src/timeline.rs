mod events;
mod header;
mod mini_timeline;
mod threads;
mod ticks;

use crate::scrollbar;
use crate::Message;
use events::EventsProgram;
use header::HeaderProgram;
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{layout, renderer, Clipboard, Layout, Shell};
use iced::keyboard;
use iced::mouse;
use iced::widget::canvas::Canvas;
use iced::widget::{button, column, container, row, text, Space};
use iced::{Color, Element, Event, Length, Point, Rectangle, Size, Theme};
use mini_timeline::MiniTimelineProgram;
use std::sync::Arc;
use threads::ThreadsProgram;

pub const LABEL_WIDTH: f64 = 150.0_f64;
pub const HEADER_HEIGHT: f64 = 30.0_f64;
pub const MINI_TIMELINE_HEIGHT: f64 = 40.0_f64;
pub const LANE_HEIGHT: f64 = 20.0_f64;
pub const LANE_SPACING: f64 = 5.0_f64;
pub const DRAG_THRESHOLD: f64 = 3.0_f64;
pub const EVENT_LEFT_PADDING: f64 = 2.0_f64;
pub const SCROLLBAR_THICKNESS: f32 = 18.0;
pub const SCROLLBAR_CORNER_GAP: f32 = 6.0;

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
    pub mipmaps: Vec<ThreadGroupMipMap>,
    pub max_depth: u32,
    pub is_collapsed: bool,
}

#[derive(Debug, Clone)]
pub struct ThreadGroupMipMap {
    pub max_duration_ns: u64,
    pub events: Vec<TimelineEvent>,
    pub events_by_start: Vec<usize>,
    pub events_by_end: Vec<usize>,
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

// Previously we used a small fixed palette and hashing. Instead we assign
// hues evenly around the color wheel based on the number of distinct event
// kinds. The actual mapping from kind -> color is built during data loading
// in `src/data.rs` and stored on each `TimelineEvent`.

/// Convert HSL to RGB Color. Hue is in degrees [0,360), s and l are [0,1].
pub fn color_from_hsl(h: f32, s: f32, l: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = (h / 60.0) % 6.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());

    let (r1, g1, b1) = if (0.0..1.0).contains(&h_prime) {
        (c, x, 0.0)
    } else if (1.0..2.0).contains(&h_prime) {
        (x, c, 0.0)
    } else if (2.0..3.0).contains(&h_prime) {
        (0.0, c, x)
    } else if (3.0..4.0).contains(&h_prime) {
        (0.0, x, c)
    } else if (4.0..5.0).contains(&h_prime) {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let m = l - c / 2.0;
    Color::from_rgb(
        (r1 + m).clamp(0.0, 1.0),
        (g1 + m).clamp(0.0, 1.0),
        (b1 + m).clamp(0.0, 1.0),
    )
}

pub fn timeline_id() -> iced::widget::Id {
    iced::widget::Id::new("timeline_scrollable")
}

pub fn total_timeline_height(thread_groups: &[ThreadGroup]) -> f64 {
    let mut total_height = 0.0_f64;
    for group in thread_groups {
        let lane_total_height = group_total_height(group);
        total_height += lane_total_height + LANE_SPACING;
    }
    total_height
}

pub fn total_ns(min_ns: u64, max_ns: u64) -> u64 {
    max_ns.saturating_sub(min_ns)
}

pub fn clamp_scroll_offset_ns(
    scroll_offset_ns: f64,
    total_ns: u64,
    viewport_width: f64,
    zoom_level: f64,
) -> f64 {
    let total_ns = total_ns as f64;
    let zoom_level = zoom_level.max(1e-9);
    let visible_ns = (viewport_width / zoom_level).max(0.0);
    let max_start_ns = (total_ns - visible_ns).max(0.0);
    scroll_offset_ns.clamp(0.0, max_start_ns)
}

pub fn ns_to_x(start_ns: u64, min_ns: u64, zoom_level: f64) -> f64 {
    (start_ns.saturating_sub(min_ns) as f64) * zoom_level
}

pub fn duration_to_width(duration_ns: u64, zoom_level: f64) -> f64 {
    duration_ns as f64 * zoom_level
}

pub fn viewport_ns_range(
    scroll_offset_ns: f64,
    viewport_width: f64,
    zoom_level: f64,
    min_ns: u64,
) -> (u64, u64) {
    let zoom_level = zoom_level.max(1e-9);
    let ns_min = scroll_offset_ns.max(0.0) as u64 + min_ns;
    let ns_max = (scroll_offset_ns + viewport_width / zoom_level).max(0.0) as u64 + min_ns;
    (ns_min, ns_max)
}

/// Return the total vertical height occupied by a thread group (all lanes),
/// respecting collapsed state.
pub fn group_total_height(group: &ThreadGroup) -> f64 {
    if group.is_collapsed {
        LANE_HEIGHT
    } else {
        (group.max_depth + 1) as f64 * LANE_HEIGHT
    }
}

pub fn build_thread_group_events(
    threads: &[Arc<ThreadData>],
) -> (Vec<TimelineEvent>, u32, Vec<ThreadGroupMipMap>) {
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
        let mipmaps = build_thread_group_mipmaps(&events);
        return (events, max_depth, mipmaps);
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
    let mipmaps = build_thread_group_mipmaps(&events);
    (events, max_depth, mipmaps)
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

fn duration_bucket(duration_ns: u64) -> u32 {
    let duration = duration_ns.max(1);
    63 - duration.leading_zeros() as u32
}

fn build_thread_group_mipmaps(events: &[TimelineEvent]) -> Vec<ThreadGroupMipMap> {
    if events.is_empty() {
        return Vec::new();
    }

    let mut buckets: Vec<Vec<TimelineEvent>> = Vec::new();
    for event in events {
        let bucket = duration_bucket(event.duration_ns) as usize;
        if buckets.len() <= bucket {
            buckets.resize_with(bucket + 1, Vec::new);
        }
        buckets[bucket].push(event.clone());
    }

    let mut mipmaps = Vec::new();
    for (bucket, events) in buckets.into_iter().enumerate() {
        if events.is_empty() {
            continue;
        }
        let (events_by_start, events_by_end) = build_event_indices(&events);
        let max_duration_ns = if bucket >= 63 {
            u64::MAX
        } else {
            (1u64 << (bucket as u32 + 1)).saturating_sub(1)
        };
        mipmaps.push(ThreadGroupMipMap {
            max_duration_ns,
            events,
            events_by_start,
            events_by_end,
        });
    }

    mipmaps
}

fn visible_event_indices_in(
    events: &[TimelineEvent],
    events_by_start: &[usize],
    events_by_end: &[usize],
    ns_min: u64,
    ns_max: u64,
) -> Vec<usize> {
    let start_upper = events_by_start.partition_point(|&index| events[index].start_ns <= ns_max);
    let end_lower = events_by_end.partition_point(|&index| event_end_ns(&events[index]) < ns_min);

    let start_candidates = start_upper;
    let end_candidates = events_by_end.len().saturating_sub(end_lower);
    let mut indices = Vec::with_capacity(start_candidates.min(end_candidates));

    if start_candidates <= end_candidates {
        for &index in events_by_start[..start_upper].iter() {
            if event_end_ns(&events[index]) >= ns_min {
                indices.push(index);
            }
        }
    } else {
        for &index in events_by_end[end_lower..].iter() {
            if events[index].start_ns <= ns_max {
                indices.push(index);
            }
        }
    }

    indices
}

fn mipmap_level_fits(level: &ThreadGroupMipMap, zoom_level: f64) -> bool {
    (level.max_duration_ns as f64) * zoom_level >= 1.0
}

fn mipmap_levels_for_zoom<'a>(
    group: &'a ThreadGroup,
    zoom_level: f64,
) -> impl Iterator<Item = &'a ThreadGroupMipMap> {
    group
        .mipmaps
        .iter()
        .filter(move |level| mipmap_level_fits(level, zoom_level))
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
    zoom_level: f64,
    selected_event: &'a Option<TimelineEvent>,
    _hovered_event: &'a Option<TimelineEvent>,
    scroll_offset_x: f64,
    scroll_offset_y: f64,
    viewport_width: f64,
    viewport_height: f64,
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

    let total_height = total_timeline_height(thread_groups) as f32;

    let scroll_offset_x_px = scroll_offset_x * zoom_level;
    let mini_timeline_canvas = Canvas::new(MiniTimelineProgram {
        min_ns: timeline_data.min_ns,
        max_ns: timeline_data.max_ns,
        zoom_level,
        scroll_offset_x,
        viewport_width,
    })
    .width(Length::Fill)
    .height(Length::Fixed(MINI_TIMELINE_HEIGHT as f32));

    let header_canvas = Canvas::new(HeaderProgram {
        min_ns: timeline_data.min_ns,
        max_ns: timeline_data.max_ns,
        zoom_level,
        scroll_offset_x: scroll_offset_x_px,
    })
    .width(Length::Fill)
    .height(Length::Fixed(HEADER_HEIGHT as f32));

    let threads_canvas = Canvas::new(ThreadsProgram {
        thread_groups,
        scroll_offset_y,
    })
    .width(Length::Fixed(LABEL_WIDTH as f32))
    .height(Length::Fill);

    let events_canvas = Canvas::new(EventsProgram {
        thread_groups,
        min_ns: timeline_data.min_ns,
        max_ns: timeline_data.max_ns,
        zoom_level,
        selected_event,
        scroll_offset_x,
        scroll_offset_y,
        viewport_width,
        viewport_height,
        color_mode,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    let events_view = container(ViewportCatcher::new(
        WheelCatcher::new(events_canvas, modifiers),
        |size| Message::TimelineViewportChanged {
            viewport_width: size.width,
            viewport_height: size.height,
        },
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .clip(true);

    let total_ns = timeline_data.max_ns.saturating_sub(timeline_data.min_ns) as f64;
    let visible_ns = if zoom_level > 0.0 {
        (viewport_width / zoom_level).max(1.0)
    } else {
        total_ns.max(1.0)
    };
    let max_start_ns_rel = (total_ns - visible_ns).max(0.0);
    let start_ns_rel = scroll_offset_x.clamp(0.0, total_ns.max(0.0));
    let min_start_ns = 0.0;
    let start_ns = start_ns_rel;
    let max_start_ns = max_start_ns_rel;
    let thumb_fraction = if total_ns > 0.0 {
        (visible_ns / total_ns).clamp(0.02, 1.0)
    } else {
        1.0
    };
    let horizontal_scrollbar = scrollbar::scrollbar(
        start_ns,
        min_start_ns..=max_start_ns.max(min_start_ns),
        |start_ns| Message::TimelineHorizontalScrolled { start_ns },
    )
    .thumb_fraction(thumb_fraction)
    .height(Length::Fixed(SCROLLBAR_THICKNESS))
    .width(Length::Fill);

    let total_height_f64 = total_height as f64;
    let visible_height = viewport_height.max(1.0);
    let max_scroll_y = (total_height_f64 - visible_height).max(0.0);
    let thumb_fraction_y = if total_height_f64 > 0.0 {
        (visible_height / total_height_f64).clamp(0.02, 1.0)
    } else {
        1.0
    };
    let vertical_scrollbar =
        scrollbar::vertical_scrollbar(scroll_offset_y, 0.0..=max_scroll_y, |scroll_y| {
            Message::TimelineVerticalScrolled { scroll_y }
        })
        .thumb_fraction(thumb_fraction_y);

    let events_column = column![
        row![
            events_view,
            column![
                container(vertical_scrollbar)
                    .width(Length::Fixed(SCROLLBAR_THICKNESS))
                    .height(Length::Fill)
                    .padding([4.0, 4.0]),
                Space::new().height(SCROLLBAR_CORNER_GAP)
            ]
            .width(Length::Fixed(SCROLLBAR_THICKNESS))
            .height(Length::Fill)
        ]
        .height(Length::Fill)
        .width(Length::Fill),
        row![
            container(horizontal_scrollbar)
                .width(Length::Fill)
                .padding([4.0, 6.0]),
            Space::new().width(SCROLLBAR_CORNER_GAP),
            container(Space::new().width(SCROLLBAR_THICKNESS)).padding([4.0, 4.0])
        ]
    ]
    .height(Length::Fill)
    .width(Length::Fill);

    // Mini timeline should span the full window width (including the label area).
    let main_view = column![
        // Full-width mini timeline on its own row.
        mini_timeline_canvas.height(Length::Fixed(MINI_TIMELINE_HEIGHT as f32)),
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
                            .style(crate::ui::neutral_button_style)
                            .on_press(Message::CollapseAllThreads),
                            // Expand button with short text
                            button(
                                row![text("+").size(18), text("Expand").size(12)]
                                    .spacing(4)
                                    .align_y(iced::Alignment::Center),
                            )
                            .padding(6)
                            .style(crate::ui::neutral_button_style)
                            .on_press(Message::ExpandAllThreads),
                        ]
                        .spacing(5)
                        .align_y(iced::Alignment::Center),
                    )
                    .width(Length::Fixed(LABEL_WIDTH as f32)),
                    header_canvas
                ]
                .height(Length::Fixed(HEADER_HEIGHT as f32)),
                row![threads_canvas, events_column].height(Length::Fill)
            ]
            .height(Length::Fill),
        )
    ]
    .height(Length::Fill);

    // Only use explicit selections (clicks) to populate the details panel.
    let display_event = selected_event.as_ref();

    // Only show the details panel when an event is selected or hovered.
    if let Some(event) = display_event {
        // Also compute float-precision viewport endpoints (not truncated to u64)
        let zoom_level = zoom_level.max(1e-9);
        let view_start_f = scroll_offset_x.max(0.0) + timeline_data.min_ns as f64;
        let view_end_f =
            (scroll_offset_x + viewport_width / zoom_level).max(0.0) + timeline_data.min_ns as f64;
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
            // Debug: show current visible view start/end with float precision
            row![
                text("View (ns):").width(Length::Fixed(80.0)).size(12),
                text(format!(
                    "{:.12} — {:.12}",
                    view_start_f - timeline_data.min_ns as f64,
                    view_end_f - timeline_data.min_ns as f64
                ))
                .size(12)
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
            // Details content grows to fit (no fixed height) so debug rows are visible
            details_col,
        ])
        .width(Length::Fill)
        // No fixed height: allow the details panel to size to its content
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

pub struct ViewportCatcher<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    on_resize: Box<dyn Fn(Size<f32>) -> Message + 'a>,
}

impl<'a, Message, Theme, Renderer> ViewportCatcher<'a, Message, Theme, Renderer> {
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        on_resize: impl Fn(Size<f32>) -> Message + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            on_resize: Box::new(on_resize),
        }
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ViewportCatcher<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(Option::<Size<f32>>::None)
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
        let new_size = Size::new(bounds.width, bounds.height);
        let last_size = tree.state.downcast_mut::<Option<Size<f32>>>();
        let changed = match *last_size {
            Some(size) => size != new_size,
            None => true,
        };
        if changed {
            *last_size = Some(new_size);
            shell.publish((self.on_resize)(new_size));
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

impl<'a, Message, Theme, Renderer> From<ViewportCatcher<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(catcher: ViewportCatcher<'a, Message, Theme, Renderer>) -> Self {
        Self::new(catcher)
    }
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
                        && delta_from_press.x.hypot(delta_from_press.y) > DRAG_THRESHOLD as f32
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
