mod timeline;

use analyzeme::ProfilingData;
use iced::widget::operation::scroll_to;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Space, button, checkbox, column, container, pick_list, row, scrollable, text};
use iced::{Alignment, Element, Length, Task};
use iced_aw::{TabLabel, tab_bar};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use timeline::{ColorMode, *};

pub const ICON_FONT: iced::Font = iced::Font::with_name("Material Icons");
const SETTINGS_ICON: char = '\u{e8b8}';
const OPEN_ICON: char = '\u{e2c7}';
const FILE_ICON: char = '\u{e873}';
const RESET_ICON: char = '\u{e5d5}';
// Use explicit plus/minus codepoints (visible in normal UI fonts)
pub const COLLAPSE_ICON: char = '\u{2212}'; // '−' minus sign
pub const EXPAND_ICON: char = '\u{002B}'; // '+' plus sign

pub fn main() -> iced::Result {
    iced::application(Lineme::new, Lineme::update, Lineme::view)
        .title(Lineme::title)
        .font(include_bytes!("../assets/MaterialIcons-Regular.ttf"))
        .subscription(Lineme::subscription)
        .theme(Lineme::theme)
        .run()
}

#[derive(Debug, Clone)]
struct Stats {
    event_count: usize,
    cmd: String,
    pid: u32,
    timeline: TimelineData,
    merged_thread_groups: Vec<ThreadGroup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ViewType {
    Stats,
    #[default]
    Timeline,
}

impl ViewType {
    const ALL: [ViewType; 2] = [ViewType::Stats, ViewType::Timeline];
}

impl std::fmt::Display for ViewType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ViewType::Stats => write!(f, "Stats"),
            ViewType::Timeline => write!(f, "Timeline"),
        }
    }
}

// Common pick_list style function to keep a neutral, greyish appearance and
// avoid the default blue focus highlight. This is used for the small selector
// pick lists in the header.
fn neutral_pick_list_style(
    theme: &iced::Theme,
    status: iced::widget::pick_list::Status,
) -> iced::widget::pick_list::Style {
    let palette = theme.extended_palette();
    let base_bg = palette.background.weak.color;
    let base_text = palette.background.weak.text;
    let border_grey = iced::Color::from_rgb(0.8, 0.8, 0.8);

    match status {
        iced::widget::pick_list::Status::Active => iced::widget::pick_list::Style {
            text_color: base_text,
            placeholder_color: palette.secondary.base.color,
            handle_color: base_text,
            background: base_bg.into(),
            border: iced::Border {
                radius: 3.0.into(),
                width: 1.0,
                color: border_grey,
            },
        },
        iced::widget::pick_list::Status::Hovered
        | iced::widget::pick_list::Status::Opened { .. } => {
            let hover_bg = iced::Color::from_rgb(0.97, 0.97, 0.97);
            iced::widget::pick_list::Style {
                text_color: base_text,
                placeholder_color: palette.secondary.base.color,
                handle_color: base_text,
                background: hover_bg.into(),
                border: iced::Border {
                    radius: 3.0.into(),
                    width: 1.0,
                    color: iced::Color::from_rgb(0.72, 0.72, 0.72),
                },
            }
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    OpenFile,
    FileSelected(PathBuf),
    FileLoaded(PathBuf, Stats),
    ErrorOccurred(String),
    ViewChanged(ViewType),
    ColorModeChanged(ColorMode),
    CloseTab(usize),
    OpenSettings,
    EventSelected(TimelineEvent),
    EventDoubleClicked(TimelineEvent),
    EventHovered(Option<TimelineEvent>),
    TimelineZoomed {
        delta: f32,
        x: f32,
    },
    TimelineScroll {
        offset: iced::Vector,
        viewport_width: f32,
        viewport_height: f32,
    },
    MiniTimelineJump {
        fraction: f64,
        viewport_width: f32,
    },
    MiniTimelineZoomTo {
        start_fraction: f32,
        end_fraction: f32,
        viewport_width: f32,
    },
    TimelinePanned {
        delta: iced::Vector,
    },
    ResetView,
    ToggleThreadCollapse(timeline::ThreadGroupKey),
    CollapseAllThreads,
    ExpandAllThreads,
    MergeThreadsToggled(bool),
    ModifiersChanged(iced::keyboard::Modifiers),
    None,
}

struct Lineme {
    active_tab: usize,
    files: Vec<FileData>,
    show_settings: bool,
    modifiers: iced::keyboard::Modifiers,
    #[allow(dead_code)]
    settings: SettingsPage,
}

struct FileData {
    path: PathBuf,
    stats: Stats,
    view_type: ViewType,
    color_mode: ColorMode,
    selected_event: Option<TimelineEvent>,
    hovered_event: Option<TimelineEvent>,
    merge_threads: bool,
    zoom_level: f32,
    scroll_offset: iced::Vector,
    viewport_width: f32,
    viewport_height: f32,
    initial_fit_done: bool,
}

struct SettingsPage {
    #[allow(dead_code)]
    show_details: bool,
}

impl Lineme {
    fn new() -> (Self, Task<Message>) {
        let mut initial_task = Task::none();

        if let Some(path_str) = std::env::args().nth(1) {
            let path = PathBuf::from(path_str);
            initial_task = Task::perform(
                async move {
                    match load_profiling_data(&path) {
                        Ok(stats) => Message::FileLoaded(path, stats),
                        Err(e) => Message::ErrorOccurred(e),
                    }
                },
                |msg| msg,
            );
        }

        (
            Lineme {
                active_tab: 0,
                files: Vec::new(),
                show_settings: false,
                modifiers: iced::keyboard::Modifiers::default(),
                settings: SettingsPage { show_details: true },
            },
            initial_task,
        )
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        iced::event::listen_with(|event, _status, _id| match event {
            iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                Some(Message::FileSelected(path))
            }
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                Some(Message::ModifiersChanged(modifiers))
            }
            _ => None,
        })
    }

    fn title(&self) -> String {
        let app_name = "LineMe";
        if self.show_settings {
            return format!("{} - Settings", app_name);
        }

        if let Some(file) = self.files.get(self.active_tab) {
            let file_name = file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| app_name.to_string());
            format!("{} - {}", file_name, app_name)
        } else {
            format!("{} - measureme profdata viewer", app_name)
        }
    }

    fn theme(&self) -> iced::Theme {
        iced::Theme::Light
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(index) => {
                self.active_tab = index;
                self.show_settings = false;
            }
            Message::OpenFile => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("measureme profdata", &["mm_profdata"])
                            .pick_file()
                            .await
                    },
                    |file_handle| {
                        if let Some(handle) = file_handle {
                            Message::FileSelected(handle.path().to_path_buf())
                        } else {
                            Message::None
                        }
                    },
                );
            }
            Message::FileSelected(path) => {
                return Task::perform(
                    async move {
                        match load_profiling_data(&path) {
                            Ok(stats) => Message::FileLoaded(path, stats),
                            Err(e) => Message::ErrorOccurred(e),
                        }
                    },
                    |msg| msg,
                );
            }
            Message::FileLoaded(path, stats) => {
                self.files.push(FileData {
                    path,
                    stats,
                    view_type: ViewType::default(),
                    color_mode: ColorMode::default(),
                    selected_event: None,
                    hovered_event: None,
                    merge_threads: false,
                    zoom_level: 1.0,
                    scroll_offset: iced::Vector::default(),
                    viewport_width: 0.0,
                    viewport_height: 0.0,
                    initial_fit_done: false,
                });
                self.active_tab = self.files.len() - 1;
                self.show_settings = false;
            }
            Message::ErrorOccurred(e) => {
                eprintln!("Error: {}", e);
            }
            Message::ViewChanged(view) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.view_type = view;
                }
            }
            Message::ColorModeChanged(color_mode) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.color_mode = color_mode;
                }
            }
            Message::CloseTab(index) => {
                if index < self.files.len() {
                    self.files.remove(index);
                    if self.active_tab >= self.files.len() && !self.files.is_empty() {
                        self.active_tab = self.files.len() - 1;
                    }
                }
            }
            Message::OpenSettings => {
                // Toggle settings panel on/off
                self.show_settings = !self.show_settings;
            }
            Message::EventSelected(event) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.selected_event = Some(event);
                }
            }
            Message::EventDoubleClicked(event) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let total_ns = file
                        .stats
                        .timeline
                        .max_ns
                        .saturating_sub(file.stats.timeline.min_ns)
                        .max(1);
                    let viewport_width = file.viewport_width.max(1.0);

                    let event_rel_start = event.start_ns.saturating_sub(file.stats.timeline.min_ns);
                    let event_rel_end = event_rel_start.saturating_add(event.duration_ns);

                    // Add padding of 20% of event duration (10% on each side)
                    let padding_ns = ((event.duration_ns as f32) * 0.2).round() as u64;
                    let half_pad = padding_ns / 2;

                    let start_ns = event_rel_start.saturating_sub(half_pad).min(total_ns);
                    let end_ns = (event_rel_end + half_pad).min(total_ns);

                    // Zoom so the selected range fills the viewport.
                    let target_ns = (end_ns.saturating_sub(start_ns)).max(1) as f64;
                    file.zoom_level = (viewport_width as f64 / target_ns) as f32;

                    let total_width = (total_ns as f64 * file.zoom_level as f64).ceil() as f32;
                    let target_x = (start_ns as f64 * file.zoom_level as f64) as f32;
                    file.scroll_offset.x =
                        target_x.clamp(0.0, (total_width - viewport_width).max(0.0));

                    return scroll_to(
                        timeline_id(),
                        AbsoluteOffset {
                            x: file.scroll_offset.x,
                            y: file.scroll_offset.y,
                        },
                    );
                }
            }
            Message::EventHovered(event) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.hovered_event = event;
                }
            }
            Message::TimelineZoomed { delta, x } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let zoom_factor = if delta > 0.0 { 1.1 } else { 0.9 };
                    file.zoom_level *= zoom_factor;

                    // Adjust scroll offset to keep x position stable
                    let x_on_canvas = x + file.scroll_offset.x;
                    file.scroll_offset.x = x_on_canvas * zoom_factor - x;

                    let total_ns = file.stats.timeline.max_ns - file.stats.timeline.min_ns;
                    let total_width = (total_ns as f64 * file.zoom_level as f64).ceil() as f32;
                    let viewport_width = file.viewport_width.max(0.0);
                    let max_scroll = (total_width - viewport_width).max(0.0);
                    file.scroll_offset.x = file.scroll_offset.x.clamp(0.0, max_scroll);
                    return scroll_to(
                        timeline_id(),
                        AbsoluteOffset {
                            x: file.scroll_offset.x,
                            y: file.scroll_offset.y,
                        },
                    );
                }
            }
            Message::TimelineScroll {
                offset,
                viewport_width,
                viewport_height,
            } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.scroll_offset = offset;
                    let first_time = file.viewport_width == 0.0 && viewport_width > 0.0;
                    if viewport_width > 0.0 {
                        file.viewport_width = viewport_width;
                    }
                    if viewport_height > 0.0 {
                        file.viewport_height = viewport_height;
                    }

                    if first_time || (viewport_width > 0.0 && !file.initial_fit_done) {
                        let total_ns = file.stats.timeline.max_ns - file.stats.timeline.min_ns;
                        file.zoom_level = (viewport_width - 2.0).max(1.0) / total_ns.max(1) as f32;
                        file.initial_fit_done = true;
                    }
                }
            }
            Message::MiniTimelineJump {
                fraction,
                viewport_width,
            } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let total_ns = file.stats.timeline.max_ns - file.stats.timeline.min_ns;
                    let total_width = (total_ns as f64 * file.zoom_level as f64).ceil() as f32;
                    if total_width > 0.0 {
                        let viewport_width = viewport_width.max(1.0);
                        let target_center = fraction as f32 * total_width;
                        let mut target_x = target_center - viewport_width / 2.0;
                        target_x = target_x.clamp(0.0, (total_width - viewport_width).max(0.0));
                        file.scroll_offset.x = target_x;
                        return scroll_to(
                            timeline_id(),
                            AbsoluteOffset {
                                x: file.scroll_offset.x,
                                y: file.scroll_offset.y,
                            },
                        );
                    }
                }
            }
            Message::MiniTimelineZoomTo {
                start_fraction,
                end_fraction,
                viewport_width,
            } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let total_ns = file.stats.timeline.max_ns - file.stats.timeline.min_ns;
                    let total_ns_f64 = total_ns.max(1) as f64;
                    let range_fraction = (end_fraction - start_fraction).max(0.0) as f64;
                    let target_ns = (range_fraction * total_ns_f64).max(1.0);
                    file.zoom_level = viewport_width / target_ns as f32;
                    let total_width = (total_ns as f64 * file.zoom_level as f64).ceil() as f32;
                    let target_x = start_fraction * total_width;
                    file.scroll_offset.x =
                        target_x.clamp(0.0, (total_width - viewport_width).max(0.0));
                    return scroll_to(
                        timeline_id(),
                        AbsoluteOffset {
                            x: file.scroll_offset.x,
                            y: file.scroll_offset.y,
                        },
                    );
                }
            }
            Message::TimelinePanned { delta } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let total_ns = file.stats.timeline.max_ns - file.stats.timeline.min_ns;
                    let total_width = (total_ns as f64 * file.zoom_level as f64).ceil() as f32;
                    let viewport_width = file.viewport_width.max(0.0);
                    let max_scroll_x = (total_width - viewport_width).max(0.0);

                    let total_height = timeline::total_timeline_height(file.thread_groups());
                    let viewport_height = file.viewport_height.max(0.0);
                    let max_scroll_y = (total_height - viewport_height).max(0.0);

                    file.scroll_offset.x =
                        (file.scroll_offset.x - delta.x).clamp(0.0, max_scroll_x);
                    file.scroll_offset.y =
                        (file.scroll_offset.y - delta.y).clamp(0.0, max_scroll_y);

                    return scroll_to(
                        timeline_id(),
                        AbsoluteOffset {
                            x: file.scroll_offset.x,
                            y: file.scroll_offset.y,
                        },
                    );
                }
            }
            Message::ResetView => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let total_ns = file.stats.timeline.max_ns - file.stats.timeline.min_ns;
                    let viewport_width = file.viewport_width.max(0.0);
                    if viewport_width > 0.0 {
                        file.zoom_level = (viewport_width - 2.0).max(1.0) / total_ns.max(1) as f32;
                    } else {
                        file.zoom_level = 1000.0 / total_ns.max(1) as f32;
                    }
                    file.scroll_offset = iced::Vector::default();
                    return scroll_to(timeline_id(), AbsoluteOffset { x: 0.0, y: 0.0 });
                }
            }
            Message::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
            }
            Message::ToggleThreadCollapse(thread_id) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    if let Some(group) = file
                        .thread_groups_mut()
                        .iter_mut()
                        .find(|group| timeline::thread_group_key(group) == thread_id)
                    {
                        group.is_collapsed = !group.is_collapsed;
                    }
                    let total_height = timeline::total_timeline_height(file.thread_groups());
                    if file.scroll_offset.y > total_height {
                        file.scroll_offset.y = total_height;
                        return scroll_to(
                            timeline_id(),
                            AbsoluteOffset {
                                x: file.scroll_offset.x,
                                y: file.scroll_offset.y,
                            },
                        );
                    }
                }
            }
            Message::CollapseAllThreads => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    for group in file.thread_groups_mut() {
                        group.is_collapsed = true;
                    }
                    let total_height = timeline::total_timeline_height(file.thread_groups());
                    if file.scroll_offset.y > total_height {
                        file.scroll_offset.y = total_height;
                        return scroll_to(
                            timeline_id(),
                            AbsoluteOffset {
                                x: file.scroll_offset.x,
                                y: file.scroll_offset.y,
                            },
                        );
                    }
                }
            }
            Message::ExpandAllThreads => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let thread_groups = file.thread_groups_mut();
                    for group in thread_groups {
                        group.is_collapsed = false;
                    }
                    let total_height =
                        timeline::total_timeline_height(file.thread_groups());
                    if file.scroll_offset.y > total_height {
                        file.scroll_offset.y = total_height;
                        return scroll_to(
                            timeline_id(),
                            AbsoluteOffset {
                                x: file.scroll_offset.x,
                                y: file.scroll_offset.y,
                            },
                        );
                    }
                }
            }
            Message::MergeThreadsToggled(enabled) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.merge_threads = enabled;
                    let total_height = timeline::total_timeline_height(file.thread_groups());
                    if file.scroll_offset.y > total_height {
                        file.scroll_offset.y = total_height;
                        return scroll_to(
                            timeline_id(),
                            AbsoluteOffset {
                                x: file.scroll_offset.x,
                                y: file.scroll_offset.y,
                            },
                        );
                    }
                }
            }
            Message::None => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let mut bar = tab_bar::TabBar::new(Message::TabSelected)
            .on_close(Message::CloseTab)
            .icon_font(ICON_FONT)
            .text_size(12.0)
            .spacing(0.0)
            .padding(8.0)
            .style(|theme: &iced::Theme, status: iced_aw::tab_bar::Status| {
                let palette = theme.extended_palette();
                let mut style = iced_aw::tab_bar::Style::default();

                match status {
                    iced_aw::tab_bar::Status::Active => {
                        // Use flat styling for active tab — remove the strong
                        // accent border by setting border width to 0 so there's
                        // no blue highlight.
                        style.background = Some(palette.background.base.color.into());
                        style.text_color = palette.background.base.text;
                        style.tab_label_background = palette.background.base.color.into();
                        style.tab_label_border_width = 0.0;
                    }
                    iced_aw::tab_bar::Status::Hovered => {
                        style.tab_label_background = palette.background.strong.color.into();
                        style.text_color = palette.background.strong.text;
                    }
                    _ => {
                        style.tab_label_background = palette.background.weak.color.into();
                        style.text_color = palette.background.weak.text;
                        style.tab_label_border_width = 0.0;
                    }
                }
                style
            });

        for (i, file) in self.files.iter().enumerate() {
            let label = file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Unknown".to_string());

            bar = bar.push(i, TabLabel::IconText(FILE_ICON, label));
        }

        if !self.files.is_empty() && !self.show_settings {
            bar = bar.set_active_tab(&self.active_tab);
        }

        // Move the Open button left of the Settings toggle and make the header
        // bar background/border a neutral grey.
        let header = container(
            row![
                // Make the tab bar only take the space it needs instead of
                // expanding to fill the header. Wrapping the `tab_bar` in a
                // `container` with `Length::Shrink` forces it to size to its
                // content.
                container(bar).width(Length::Shrink),
                Space::new().width(Length::Fill),
                // Use the same font size as thread labels for button text
                button(
                    row![text(OPEN_ICON).font(ICON_FONT), text("Open").size(12.0)]
                        .spacing(5)
                        .align_y(Alignment::Center),
                )
                .style(|theme: &iced::Theme, status: button::Status| {
                    let palette = theme.extended_palette();
                    let base = button::Style {
                        text_color: palette.background.weak.text,
                        ..Default::default()
                    };
                    match status {
                        button::Status::Hovered | button::Status::Pressed => button::Style {
                            background: Some(palette.background.strong.color.into()),
                            ..base
                        },
                        _ => base,
                    }
                })
                .on_press(Message::OpenFile),
                // Settings button acts as a toggle. When active, show a highlighted background.
                button(text(SETTINGS_ICON).font(ICON_FONT).size(18))
                    .style(|theme: &iced::Theme, status: button::Status| {
                        let palette = theme.extended_palette();
                        let base = button::Style {
                            text_color: palette.background.weak.text,
                            ..Default::default()
                        };
                        // Capture current settings state to render active appearance.
                        let show_settings = self.show_settings;
                        if show_settings {
                            return button::Style {
                                background: Some(palette.background.strong.color.into()),
                                ..base
                            };
                        }
                        match status {
                            button::Status::Hovered | button::Status::Pressed => button::Style {
                                background: Some(palette.background.strong.color.into()),
                                ..base
                            },
                            _ => base,
                        }
                    })
                    .on_press(Message::OpenSettings),
            ]
            .spacing(10)
            .padding(5)
            .align_y(Alignment::Center),
        )
        .style(|_theme: &iced::Theme| {
            // Use explicit greys for a consistent look regardless of theme.
            container::Style::default()
                .background(iced::Color::from_rgb(0.95, 0.95, 0.95))
                .border(iced::Border {
                    color: iced::Color::from_rgb(0.8, 0.8, 0.8),
                    width: 1.0,
                    ..Default::default()
                })
        });

        let content: Element<'_, Message> = if self.show_settings {
            self.settings_view()
        } else if let Some(file) = self.files.get(self.active_tab) {
            let inner_view = match file.view_type {
                ViewType::Stats => self.file_view(file),
                ViewType::Timeline => self.timeline_view(file),
            };

            let view_selector_bar = container(
                row![
                    text("View:").size(12),
                    pick_list(
                        &ViewType::ALL[..],
                        Some(file.view_type),
                        Message::ViewChanged
                    )
                    .text_size(12)
                    .padding(3)
                    .style(neutral_pick_list_style),
                    if file.view_type == ViewType::Timeline {
                        Element::from(
                            row![
                                text("Color by:").size(12),
                                pick_list(
                                    &ColorMode::ALL[..],
                                    Some(file.color_mode),
                                    Message::ColorModeChanged
                                )
                                .text_size(12)
                                .padding(3)
                                .style(neutral_pick_list_style),
                                checkbox(file.merge_threads)
                                    .label("Merge threads")
                                    .size(14)
                                    .text_size(12)
                                    .on_toggle(Message::MergeThreadsToggled),
                                button(
                                    row![
                                        text(RESET_ICON).font(ICON_FONT),
                                        text("Reset View").size(12.0)
                                    ]
                                    .spacing(5)
                                    .align_y(Alignment::Center),
                                )
                                .style(|theme: &iced::Theme, status: button::Status| {
                                    let palette = theme.extended_palette();
                                    let base = button::Style {
                                        text_color: palette.background.base.text,
                                        ..Default::default()
                                    };
                                    match status {
                                        button::Status::Hovered | button::Status::Pressed => {
                                            button::Style {
                                                background: Some(
                                                    palette.background.weak.color.into(),
                                                ),
                                                ..base
                                            }
                                        }
                                        _ => base,
                                    }
                                })
                                .padding(3)
                                .on_press(Message::ResetView),
                            ]
                            .spacing(10)
                            .align_y(Alignment::Center),
                        )
                    } else {
                        Element::from(Space::new().width(0))
                    },
                ]
                .spacing(10)
                .padding(5)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .style(|_theme: &iced::Theme| {
                // Make the selector container a neutral grey to match the header.
                container::Style::default()
                    .background(iced::Color::from_rgb(0.95, 0.95, 0.95))
                    .border(iced::Border {
                        color: iced::Color::from_rgb(0.8, 0.8, 0.8),
                        width: 1.0,
                        ..Default::default()
                    })
            });

            column![view_selector_bar, inner_view]
                .height(Length::Fill)
                .into()
        } else {
            container(text("Open a file to start").size(20))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        };

        column![header, content].height(Length::Fill).into()
    }

    fn file_view<'a>(&self, file: &'a FileData) -> Element<'a, Message> {
        // Use the same compact label/value layout and theme-aware container used
        // elsewhere so the stats panel visually matches the rest of the app.
        let stats_col = column![
            row![
                text("File:").width(Length::Fixed(120.0)).size(12),
                text(format!("{}", file.path.display())).size(12)
            ],
            row![
                text("Command:").width(Length::Fixed(120.0)).size(12),
                text(&file.stats.cmd).size(12)
            ],
            row![
                text("PID:").width(Length::Fixed(120.0)).size(12),
                text(format!("{}", file.stats.pid)).size(12)
            ],
            row![
                text("Event count:").width(Length::Fixed(120.0)).size(12),
                text(format!("{}", file.stats.event_count)).size(12)
            ],
            row![
                text("Total duration:").width(Length::Fixed(120.0)).size(12),
                text(format_duration(
                    file.stats.timeline.max_ns - file.stats.timeline.min_ns
                ))
                .size(12)
            ],
        ]
        .spacing(8)
        .padding(10);

        let content =
            container(stats_col)
                .width(Length::Fill)
                .padding(12)
                .style(|theme: &iced::Theme| {
                    // Match details panel style from timeline: use the theme's background
                    // base color and a subtle strong-color border so the panel reads as
                    // a cohesive block in the layout.
                    let palette = theme.extended_palette();
                    container::Style::default()
                        .background(palette.background.base.color)
                        .border(iced::Border {
                            color: palette.background.strong.color,
                            width: 1.0,
                            ..Default::default()
                        })
                });

        scrollable(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn timeline_view<'a>(&self, file: &'a FileData) -> Element<'a, Message> {
        timeline::view(
            &file.stats.timeline,
            file.thread_groups(),
            file.zoom_level,
            &file.selected_event,
            &file.hovered_event,
            file.scroll_offset,
            file.viewport_width,
            file.viewport_height,
            self.modifiers,
            file.color_mode,
        )
    }

    fn settings_view(&self) -> Element<'_, Message> {
        // Make the settings panel visually consistent with other panels by using
        // the same themed container and compact label/value rows.
        let hints = column![
            text("Hints").size(16),
            row![
                text("Left click:").width(Length::Fixed(160.0)).size(12),
                text("Select an event and show details").size(12)
            ],
            row![
                text("Double click:").width(Length::Fixed(160.0)).size(12),
                text("Zoom to the clicked event (with padding)").size(12)
            ],
            row![
                text("Left click + drag (events area):")
                    .width(Length::Fixed(160.0))
                    .size(12),
                text("Pan the timeline").size(12)
            ],
            row![
                text("Mouse wheel:").width(Length::Fixed(160.0)).size(12),
                text("Zoom horizontally centered on the cursor (hold Ctrl to bypass)").size(12)
            ],
            row![
                text("Shift + mouse wheel:")
                    .width(Length::Fixed(160.0))
                    .size(12),
                text("Pan horizontally").size(12)
            ],
            row![
                text("Mini timeline — left click:")
                    .width(Length::Fixed(160.0))
                    .size(12),
                text("Jump the main view to that position").size(12)
            ],
            row![
                text("Mini timeline — right click + drag:")
                    .width(Length::Fixed(160.0))
                    .size(12),
                text("Select a range to zoom the main view to").size(12)
            ],
            row![
                text("Thread label click:")
                    .width(Length::Fixed(160.0))
                    .size(12),
                text("Toggle collapse/expand for that thread").size(12)
            ],
            row![
                text("Collapse/Expand buttons:")
                    .width(Length::Fixed(160.0))
                    .size(12),
                text("Collapse or expand all threads").size(12)
            ],
            row![
                text("Scrollbars:").width(Length::Fixed(160.0)).size(12),
                text("Use scrollbars for precise horizontal/vertical navigation").size(12)
            ],
        ]
        .spacing(6)
        .padding(6);

        let settings_col = column![
            text("Settings").size(20),
            row![
                text("Open files:").width(Length::Fixed(120.0)).size(12),
                text(format!("{}", self.files.len())).size(12)
            ],
            text("Welcome to Lineme Settings").size(12),
            container(hints).padding(6).style(|_theme: &iced::Theme| {
                // subtle background to separate hints from other settings
                container::Style::default().background(iced::Color::from_rgb(0.99, 0.99, 0.99))
            }),
        ]
        .spacing(8)
        .padding(10);

        container(settings_col)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .style(|theme: &iced::Theme| {
                let palette = theme.extended_palette();
                container::Style::default()
                    .background(palette.background.base.color)
                    .border(iced::Border {
                        color: palette.background.strong.color,
                        width: 1.0,
                        ..Default::default()
                    })
            })
            .into()
    }
}

impl FileData {
    fn thread_groups(&self) -> &[ThreadGroup] {
        if self.merge_threads {
            &self.stats.merged_thread_groups
        } else {
            &self.stats.timeline.thread_groups
        }
    }

    fn thread_groups_mut(&mut self) -> &mut [ThreadGroup] {
        if self.merge_threads {
            &mut self.stats.merged_thread_groups
        } else {
            &mut self.stats.timeline.thread_groups
        }
    }
}

fn load_profiling_data(path: &Path) -> Result<Stats, String> {
    let stem = path.with_extension("");

    let data = ProfilingData::new(&stem)
        .map_err(|e| format!("Failed to load profiling data from {:?}: {}", stem, e))?;

    let metadata = data.metadata();

    let mut threads: HashMap<u64, Vec<TimelineEvent>> = HashMap::new();
    let mut min_ns = u64::MAX;
    let mut max_ns = 0;
    let mut event_count = 0;

    // Only include interval events in the timeline. Instant timestamps are
    // ignored because the timeline view requires a duration.
    for lightweight_event in data.iter() {
        let event = data.to_full_event(&lightweight_event);
        let thread_id = event.thread_id as u64;

        if let analyzeme::EventPayload::Timestamp(timestamp) = &event.payload {
            if let analyzeme::Timestamp::Interval { start, end } = timestamp {
                // Count only interval events
                event_count += 1;

                let start_ns = start
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64;
                let end_ns = end
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64;

                min_ns = min_ns.min(start_ns);
                max_ns = max_ns.max(end_ns);

                let event_kind = event.event_kind.to_string();
                let additional_data = event
                    .additional_data
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                let payload_integer = event.payload.integer();

                threads.entry(thread_id).or_default().push(TimelineEvent {
                    label: event.label.to_string(),
                    start_ns,
                    duration_ns: end_ns.saturating_sub(start_ns),
                    depth: 0,
                    thread_id,
                    event_kind,
                    additional_data,
                    payload_integer,
                    color: color_from_label(&event.label),
                    is_thread_root: false,
                });
            }
        }
    }

    for thread_events in threads.values_mut() {
        thread_events.sort_by_key(|e| e.start_ns);
        let mut stack: Vec<u64> = Vec::new();
        for event in thread_events.iter_mut() {
            let end_ns = event.start_ns + event.duration_ns;
            while let Some(&last_end) = stack.last() {
                if last_end <= event.start_ns {
                    stack.pop();
                } else {
                    break;
                }
            }
            event.depth = stack.len() as u32;
            stack.push(end_ns);
        }
    }

    let mut thread_data_vec = Vec::new();
    for (thread_id, events) in threads {
        thread_data_vec.push(Arc::new(ThreadData {
            thread_id,
            events,
        }));
    }

    thread_data_vec.sort_by_key(|t| t.thread_id);

    let mut thread_groups = Vec::new();
    for thread in &thread_data_vec {
        let threads = Arc::new(vec![thread.clone()]);
        let (events, max_depth) = timeline::build_thread_group_events(&threads);
        thread_groups.push(ThreadGroup {
            threads,
            events,
            max_depth,
            is_collapsed: false,
        });
    }

    let merged_thread_groups = build_merged_thread_groups(&thread_data_vec);

    Ok(Stats {
        event_count,
        cmd: metadata.cmd.clone(),
        pid: metadata.process_id,
        timeline: TimelineData {
            thread_groups,
            min_ns: if min_ns == u64::MAX { 0 } else { min_ns },
            max_ns,
        },
        merged_thread_groups,
    })
}

fn build_merged_thread_groups(threads: &[Arc<ThreadData>]) -> Vec<ThreadGroup> {
    if threads.is_empty() {
        return Vec::new();
    }

    let mut intervals: Vec<(usize, u64, u64, Arc<ThreadData>)> = threads
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, thread)| {
            let mut start = u64::MAX;
            let mut end = 0u64;
            for event in &thread.events {
                start = start.min(event.start_ns);
                end = end.max(event.start_ns.saturating_add(event.duration_ns));
            }
            if start == u64::MAX {
                start = 0;
            }
            (index, start, end, thread)
        })
        .collect();

    intervals.sort_by(|(_, a_start, a_end, _), (_, b_start, b_end, _)| {
        let a_len = a_end.saturating_sub(*a_start);
        let b_len = b_end.saturating_sub(*b_start);
        a_len.cmp(&b_len).then_with(|| a_start.cmp(b_start))
    });

    let mut groups: Vec<Vec<(usize, Arc<ThreadData>)>> = Vec::new();
    let mut group_ranges: Vec<(u64, u64)> = Vec::new();

    for (index, start, end, thread) in intervals {
        let mut placed = false;
        for (group, (group_start, group_end)) in
            groups.iter_mut().zip(group_ranges.iter_mut())
        {
            let overlaps = start < *group_end && end > *group_start;
            if !overlaps {
                *group_start = (*group_start).min(start);
                *group_end = (*group_end).max(end);
                group.push((index, thread.clone()));
                placed = true;
                break;
            }
        }

        if !placed {
            groups.push(vec![(index, thread)]);
            group_ranges.push((start, end));
        }
    }

    groups.sort_by_key(|group| {
        group
            .iter()
            .map(|(index, _)| *index)
            .min()
            .unwrap_or(usize::MAX)
    });

    let mut thread_groups = Vec::new();
    for group in groups {
        let mut group = group;
        group.sort_by_key(|(index, _)| *index);
        let threads = Arc::new(
            group
                .into_iter()
                .map(|(_, thread)| thread)
                .collect::<Vec<_>>(),
        );
        let (events, max_depth) = timeline::build_thread_group_events(&threads);
        thread_groups.push(ThreadGroup {
            threads,
            events,
            max_depth,
            is_collapsed: false,
        });
    }

    thread_groups
}
