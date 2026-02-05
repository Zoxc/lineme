mod data;
mod timeline;
mod ui;
use data::{Stats, format_panic_payload, load_profiling_data};
use iced::futures::channel::oneshot;
use iced::widget::operation::scroll_to;
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Space, button, checkbox, column, container, pick_list, row, scrollable, text};
use iced::{Alignment, Element, Length, Task};
use iced_aw::{TabLabel, tab_bar};
use std::path::PathBuf;
use std::thread;
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
    FileLoaded(u64, Stats),
    FileLoadFailed(u64, String),
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
    next_file_id: u64,
}

#[derive(Debug, Clone)]
enum FileLoadState {
    Loading,
    Ready(Stats),
    Error(String),
}

struct FileData {
    id: u64,
    path: PathBuf,
    load_state: FileLoadState,
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
        let mut app = Lineme {
            active_tab: 0,
            files: Vec::new(),
            show_settings: false,
            modifiers: iced::keyboard::Modifiers::default(),
            settings: SettingsPage { show_details: true },
            next_file_id: 0,
        };

        let initial_task = if let Some(path_str) = std::env::args().nth(1) {
            let path = PathBuf::from(path_str);
            app.start_loading_file(path)
        } else {
            Task::none()
        };

        (app, initial_task)
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
                return self.start_loading_file(path);
            }
            Message::FileLoaded(id, stats) => {
                if let Some(file) = self.files.iter_mut().find(|file| file.id == id) {
                    file.load_state = FileLoadState::Ready(stats);
                    file.initial_fit_done = false;
                }
            }
            Message::FileLoadFailed(id, error) => {
                if let Some(file) = self.files.iter_mut().find(|file| file.id == id) {
                    file.load_state = FileLoadState::Error(error);
                }
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
                    let (min_ns, max_ns) = match file
                        .stats()
                        .map(|stats| (stats.timeline.min_ns, stats.timeline.max_ns))
                    {
                        Some(values) => values,
                        None => return Task::none(),
                    };
                    let total_ns = max_ns.saturating_sub(min_ns).max(1);
                    let viewport_width = file.viewport_width.max(1.0);

                    let event_rel_start = event.start_ns.saturating_sub(min_ns);
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
                    let (min_ns, max_ns) = match file
                        .stats()
                        .map(|stats| (stats.timeline.min_ns, stats.timeline.max_ns))
                    {
                        Some(values) => values,
                        None => return Task::none(),
                    };
                    let zoom_factor = if delta > 0.0 { 1.1 } else { 0.9 };
                    file.zoom_level *= zoom_factor;

                    // Adjust scroll offset to keep x position stable
                    let x_on_canvas = x + file.scroll_offset.x;
                    file.scroll_offset.x = x_on_canvas * zoom_factor - x;

                    let total_ns = max_ns.saturating_sub(min_ns);
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
                    let (min_ns, max_ns) = match file
                        .stats()
                        .map(|stats| (stats.timeline.min_ns, stats.timeline.max_ns))
                    {
                        Some(values) => values,
                        None => {
                            file.scroll_offset = offset;
                            if viewport_width > 0.0 {
                                file.viewport_width = viewport_width;
                            }
                            if viewport_height > 0.0 {
                                file.viewport_height = viewport_height;
                            }
                            return Task::none();
                        }
                    };
                    file.scroll_offset = offset;
                    let first_time = file.viewport_width == 0.0 && viewport_width > 0.0;
                    if viewport_width > 0.0 {
                        file.viewport_width = viewport_width;
                    }
                    if viewport_height > 0.0 {
                        file.viewport_height = viewport_height;
                    }

                    if first_time || (viewport_width > 0.0 && !file.initial_fit_done) {
                        let total_ns = max_ns.saturating_sub(min_ns);
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
                    let (min_ns, max_ns) = match file
                        .stats()
                        .map(|stats| (stats.timeline.min_ns, stats.timeline.max_ns))
                    {
                        Some(values) => values,
                        None => return Task::none(),
                    };
                    let total_ns = max_ns.saturating_sub(min_ns);
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
                    let (min_ns, max_ns) = match file
                        .stats()
                        .map(|stats| (stats.timeline.min_ns, stats.timeline.max_ns))
                    {
                        Some(values) => values,
                        None => return Task::none(),
                    };
                    let total_ns = max_ns.saturating_sub(min_ns);
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
                    let (min_ns, max_ns) = match file
                        .stats()
                        .map(|stats| (stats.timeline.min_ns, stats.timeline.max_ns))
                    {
                        Some(values) => values,
                        None => return Task::none(),
                    };
                    let total_ns = max_ns.saturating_sub(min_ns);
                    let total_width = (total_ns as f64 * file.zoom_level as f64).ceil() as f32;
                    let viewport_width = file.viewport_width.max(0.0);
                    let max_scroll_x = (total_width - viewport_width).max(0.0);

                    let total_height =
                        timeline::total_timeline_height(file.thread_groups().unwrap_or_default());
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
                    let (min_ns, max_ns) = match file
                        .stats()
                        .map(|stats| (stats.timeline.min_ns, stats.timeline.max_ns))
                    {
                        Some(values) => values,
                        None => return Task::none(),
                    };
                    let total_ns = max_ns.saturating_sub(min_ns);
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
                    {
                        let thread_groups_mut = match file.thread_groups_mut() {
                            Some(groups) => groups,
                            None => return Task::none(),
                        };
                        if let Some(group) = thread_groups_mut
                            .iter_mut()
                            .find(|group| timeline::thread_group_key(group) == thread_id)
                        {
                            group.is_collapsed = !group.is_collapsed;
                        }
                    }
                    let total_height = {
                        let thread_groups = file.thread_groups().unwrap_or_default();
                        timeline::total_timeline_height(thread_groups)
                    };
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
                    {
                        let thread_groups_mut = match file.thread_groups_mut() {
                            Some(groups) => groups,
                            None => return Task::none(),
                        };
                        for group in thread_groups_mut.iter_mut() {
                            group.is_collapsed = true;
                        }
                    }
                    let total_height = {
                        let thread_groups = file.thread_groups().unwrap_or_default();
                        timeline::total_timeline_height(thread_groups)
                    };
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
                    {
                        let thread_groups_mut = match file.thread_groups_mut() {
                            Some(groups) => groups,
                            None => return Task::none(),
                        };
                        for group in thread_groups_mut.iter_mut() {
                            group.is_collapsed = false;
                        }
                    }
                    let total_height = {
                        let thread_groups = file.thread_groups().unwrap_or_default();
                        timeline::total_timeline_height(thread_groups)
                    };
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
                    let total_height = {
                        let thread_groups = match file.thread_groups() {
                            Some(groups) => groups,
                            None => return Task::none(),
                        };
                        timeline::total_timeline_height(thread_groups)
                    };
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

    fn start_loading_file(&mut self, path: PathBuf) -> Task<Message> {
        let id = self.next_file_id;
        self.next_file_id = self.next_file_id.wrapping_add(1);

        self.files.push(FileData {
            id,
            path: path.clone(),
            load_state: FileLoadState::Loading,
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

        Task::perform(
            async move {
                let (tx, rx) = oneshot::channel();
                thread::spawn(move || {
                    let result = std::panic::catch_unwind(|| load_profiling_data(&path));
                    let outcome = match result {
                        Ok(result) => result,
                        Err(payload) => Err(format_panic_payload(payload)),
                    };
                    let _ = tx.send(outcome);
                });

                match rx.await {
                    Ok(Ok(stats)) => Message::FileLoaded(id, stats),
                    Ok(Err(error)) => Message::FileLoadFailed(id, error),
                    Err(_) => Message::FileLoadFailed(
                        id,
                        "Loading thread exited before sending results".to_string(),
                    ),
                }
            },
            |msg| msg,
        )
    }

    // Helper used after operations that can change the total vertical height of
    // the timeline (collapse/expand, merge threads, ...). If the current
    // vertical scroll is beyond the new total height, clamp it and return a
    // `scroll_to` task to update the UI. Otherwise return None.
    // Note: this helper was useful during refactor but borrowing conflicts made it
    // less ergonomic than local handling. Keeping it in case future refactors
    // reuse it; otherwise it can be removed.
    #[allow(dead_code)]
    fn clamp_vertical_scroll_and_scroll_to_if_needed(
        scroll_offset: &mut iced::Vector,
        thread_groups: &[timeline::ThreadGroup],
    ) -> Option<Task<Message>> {
        let total_height = timeline::total_timeline_height(thread_groups);
        if scroll_offset.y > total_height {
            scroll_offset.y = total_height;
            return Some(scroll_to(
                timeline_id(),
                AbsoluteOffset {
                    x: scroll_offset.x,
                    y: scroll_offset.y,
                },
            ));
        }
        None
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

            let label = match &file.load_state {
                FileLoadState::Loading => format!("{} (loading...)", label),
                FileLoadState::Error(_) => format!("{} (error)", label),
                FileLoadState::Ready(_) => label,
            };

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
                .style(crate::ui::neutral_button_style)
                .on_press(Message::OpenFile),
                // Settings button acts as a toggle. When active, show a highlighted background.
                button(text(SETTINGS_ICON).font(ICON_FONT).size(18))
                    .style(|theme: &iced::Theme, status: button::Status| {
                        // Capture current settings state to render active appearance.
                        let show_settings = self.show_settings;
                        if show_settings {
                            let palette = theme.extended_palette();
                            return button::Style {
                                background: Some(palette.background.strong.color.into()),
                                text_color: palette.background.weak.text,
                                ..Default::default()
                            };
                        }
                        crate::ui::neutral_button_style(theme, status)
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

            if matches!(file.load_state, FileLoadState::Ready(_)) {
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
                inner_view
            }
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
        let stats_col = match &file.load_state {
            FileLoadState::Loading => column![
                text("Loading profiling data...").size(14),
                text(format!("{}", file.path.display())).size(12),
            ]
            .spacing(8)
            .padding(10),
            FileLoadState::Error(error) => column![
                text("Failed to load profiling data").size(14),
                text(format!("{}", file.path.display())).size(12),
                text(error).size(12),
            ]
            .spacing(8)
            .padding(10),
            FileLoadState::Ready(stats) => {
                // Use the same compact label/value layout and theme-aware container used
                // elsewhere so the stats panel visually matches the rest of the app.
                column![
                    row![
                        text("File:").width(Length::Fixed(120.0)).size(12),
                        text(format!("{}", file.path.display())).size(12)
                    ],
                    row![
                        text("Command:").width(Length::Fixed(120.0)).size(12),
                        text(&stats.cmd).size(12)
                    ],
                    row![
                        text("PID:").width(Length::Fixed(120.0)).size(12),
                        text(format!("{}", stats.pid)).size(12)
                    ],
                    row![
                        text("Event count:").width(Length::Fixed(120.0)).size(12),
                        text(format!("{}", stats.event_count)).size(12)
                    ],
                    row![
                        text("Total duration:").width(Length::Fixed(120.0)).size(12),
                        text(format_duration(
                            stats.timeline.max_ns - stats.timeline.min_ns
                        ))
                        .size(12)
                    ],
                ]
                .spacing(8)
                .padding(10)
            }
        };

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
        match &file.load_state {
            FileLoadState::Loading => container(text("Processing file...").size(16))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into(),
            FileLoadState::Error(error) => container(
                column![
                    text("Unable to render timeline").size(16),
                    text(format!("{}", file.path.display())).size(12),
                    text(error).size(12),
                ]
                .spacing(6),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
            FileLoadState::Ready(_) => timeline::view(
                &file.stats().expect("stats ready").timeline,
                file.thread_groups().unwrap_or_default(),
                file.zoom_level,
                &file.selected_event,
                &file.hovered_event,
                file.scroll_offset,
                file.viewport_width,
                file.viewport_height,
                self.modifiers,
                file.color_mode,
            ),
        }
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
    fn stats(&self) -> Option<&Stats> {
        match &self.load_state {
            FileLoadState::Ready(stats) => Some(stats),
            _ => None,
        }
    }

    fn thread_groups(&self) -> Option<&[ThreadGroup]> {
        let stats = self.stats()?;
        if self.merge_threads {
            Some(&stats.merged_thread_groups)
        } else {
            Some(&stats.timeline.thread_groups)
        }
    }

    fn thread_groups_mut(&mut self) -> Option<&mut [ThreadGroup]> {
        let stats = match &mut self.load_state {
            FileLoadState::Ready(stats) => stats,
            _ => return None,
        };
        if self.merge_threads {
            Some(&mut stats.merged_thread_groups)
        } else {
            Some(&mut stats.timeline.thread_groups)
        }
    }
}
