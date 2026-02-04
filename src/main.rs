mod timeline;

use iced::widget::{button, column, container, pick_list, row, scrollable, text, Space};
use iced::{Alignment, Element, Length, Task};
use iced_aw::{tab_bar, TabLabel};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use analyzeme::ProfilingData;
use timeline::*;

const ICON_FONT: iced::Font = iced::Font::with_name("Material Icons");
const SETTINGS_ICON: char = '\u{e8b8}';
const OPEN_ICON: char = '\u{e2c7}';
const FILE_ICON: char = '\u{e873}';
// Note: CLOSE_ICON (\u{e5cd}) is defined but currently not supported by iced_aw 0.13
// for the tab close button.

pub fn main() -> iced::Result {
    iced::application(Lineme::new, Lineme::update, Lineme::view)
        .title(Lineme::title)
        .font(include_bytes!("../assets/MaterialIcons-Regular.ttf"))
        .run()
}

#[derive(Debug, Clone)]
struct Stats {
    event_count: usize,
    cmd: String,
    pid: u32,
    timeline: TimelineData,
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

#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    OpenFile,
    FileSelected(PathBuf),
    FileLoaded(PathBuf, Stats),
    ErrorOccurred(String),
    ViewChanged(ViewType),
    CloseTab(usize),
    OpenSettings,
    EventSelected(TimelineEvent),
    TimelineZoomed { delta: f32, x: f32 },
    TimelineScroll { offset: iced::Vector },
    ToggleThreadCollapse(u64),
    None,
}

struct Lineme {
    active_tab: usize,
    files: Vec<FileData>,
    show_settings: bool,
    #[allow(dead_code)]
    settings: SettingsPage,
}

struct FileData {
    path: PathBuf,
    stats: Stats,
    view_type: ViewType,
    selected_event: Option<TimelineEvent>,
    zoom_level: f32,
    scroll_offset: iced::Vector,
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
                settings: SettingsPage { show_details: true },
            },
            initial_task,
        )
    }

    fn title(&self) -> String {
        if self.show_settings {
            return "Lineme - Settings".to_string();
        }
        if let Some(file) = self.files.get(self.active_tab) {
            file.path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Lineme".to_string())
        } else {
            "Lineme - measureme profdata viewer".to_string()
        }
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
                let total_ns = stats.timeline.max_ns - stats.timeline.min_ns;
                let zoom_level = 1000.0 / total_ns.max(1) as f32; // Default to 1000px wide
                self.files.push(FileData { 
                    path, 
                    stats,
                    view_type: ViewType::default(),
                    selected_event: None,
                    zoom_level,
                    scroll_offset: iced::Vector::default(),
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
            Message::CloseTab(index) => {
                if index < self.files.len() {
                    self.files.remove(index);
                    if self.active_tab >= self.files.len() && !self.files.is_empty() {
                        self.active_tab = self.files.len() - 1;
                    }
                }
            }
            Message::OpenSettings => {
                self.show_settings = true;
            }
            Message::EventSelected(event) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.selected_event = Some(event);
                }
            }
            Message::TimelineZoomed { delta, x } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let zoom_factor = if delta > 0.0 { 1.1 } else { 0.9 };
                    file.zoom_level *= zoom_factor;
                    
                    // Adjust scroll offset to keep x position stable
                    let x_on_canvas = x + file.scroll_offset.x;
                    file.scroll_offset.x = x_on_canvas * zoom_factor - x;
                }
            }
            Message::TimelineScroll { offset } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    file.scroll_offset = offset;
                }
            }
            Message::ToggleThreadCollapse(thread_id) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    if let Some(thread) = file.stats.timeline.threads.iter_mut().find(|t| t.thread_id == thread_id) {
                        thread.is_collapsed = !thread.is_collapsed;
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
            .icon_font(ICON_FONT);
        // Note: iced_aw 0.13 does not support customizing the close icon character.
        // It uses a fixed character that might not display correctly in some fonts.

        for (i, file) in self.files.iter().enumerate() {
            let label = file.path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Unknown".to_string());
            
            bar = bar.push(i, TabLabel::IconText(FILE_ICON, label));
        }
        
        if !self.files.is_empty() && !self.show_settings {
            bar = bar.set_active_tab(&self.active_tab);
        }

        let header = row![
            bar,
            Space::new().width(Length::Fill),
            button(text(SETTINGS_ICON).font(ICON_FONT)).on_press(Message::OpenSettings)
        ];

        let header = header
            .push(
                button(
                    row![text(OPEN_ICON).font(ICON_FONT), text("Open")]
                        .spacing(5)
                        .align_y(Alignment::Center),
                )
                .on_press(Message::OpenFile),
            )
            .spacing(10)
            .padding(5)
            .align_y(Alignment::Center);

        let content: Element<'_, Message> = if self.show_settings {
            self.settings_view()
        } else if let Some(file) = self.files.get(self.active_tab) {
            let inner_view = match file.view_type {
                ViewType::Stats => self.file_view(file),
                ViewType::Timeline => self.timeline_view(file),
            };

            let view_selector_bar = container(
                row![
                    text("View:"),
                    pick_list(
                        &ViewType::ALL[..],
                        Some(file.view_type),
                        Message::ViewChanged,
                    ),
                ]
                .spacing(10)
                .padding(5)
                .align_y(Alignment::Center)
            )
            .width(Length::Fill)
            .style(|theme: &iced::Theme| {
                let palette = theme.extended_palette();
                container::Style::default().background(palette.background.weak.color)
            });

            column![view_selector_bar, inner_view].into()
        } else {
            container(text("Open a file to start").size(20))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        };

        column![header, content].into()
    }

    fn file_view(&self, file: &FileData) -> Element<'_, Message> {
        let content = column![
            text(format!("File: {}", file.path.display())).size(20),
            text(format!("Command: {}", file.stats.cmd)),
            text(format!("PID: {}", file.stats.pid)),
            text(format!("Event count: {}", file.stats.event_count)),
            button(
                row![text(OPEN_ICON).font(ICON_FONT), text("Open another file")]
                    .spacing(10)
                    .align_y(Alignment::Center)
            )
            .on_press(Message::OpenFile),
        ]
        .spacing(10)
        .padding(20);

        scrollable(content).into()
    }

    fn timeline_view<'a>(&self, file: &'a FileData) -> Element<'a, Message> {
        timeline::view(
            &file.stats.timeline,
            file.zoom_level,
            &file.selected_event,
            file.scroll_offset,
        )
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let content = column![
            text("Settings").size(30),
            text("Welcome to Lineme Settings"),
            text(format!("Currently managing {} open files", self.files.len())),
            button(
                row![text(OPEN_ICON).font(ICON_FONT), text("Open file from here")]
                    .spacing(10)
                    .align_y(Alignment::Center)
            )
            .on_press(Message::OpenFile),
        ]
        .spacing(10)
        .padding(20);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .into()
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

    for lightweight_event in data.iter() {
        event_count += 1;
        let event = data.to_full_event(&lightweight_event);
        let thread_id = event.thread_id as u64;

        if let analyzeme::EventPayload::Timestamp(timestamp) = &event.payload {
            let (start_ns, end_ns) = match timestamp {
                analyzeme::Timestamp::Interval { start, end } => {
                    let s = start.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
                    let e = end.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
                    (s, e)
                }
                analyzeme::Timestamp::Instant(t) => {
                    let ns = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
                    (ns, ns)
                }
            };

            min_ns = min_ns.min(start_ns);
            max_ns = max_ns.max(end_ns);

            threads.entry(thread_id).or_default().push(TimelineEvent {
                label: event.label.to_string(),
                start_ns,
                duration_ns: end_ns - start_ns,
                depth: 0,
                thread_id,
                color: color_from_label(&event.label),
            });
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
        let max_depth = events.iter().map(|e| e.depth).max().unwrap_or(0);
        thread_data_vec.push(ThreadData {
            thread_id,
            events,
            max_depth,
            is_collapsed: false,
        });
    }
    
    thread_data_vec.sort_by_key(|t| t.thread_id);

    Ok(Stats {
        event_count,
        cmd: metadata.cmd.clone(),
        pid: metadata.process_id,
        timeline: TimelineData {
            threads: thread_data_vec,
            min_ns: if min_ns == u64::MAX { 0 } else { min_ns },
            max_ns,
        },
    })
}
