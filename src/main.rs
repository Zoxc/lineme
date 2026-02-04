use iced::widget::{button, column, container, scrollable, text};
use iced::{Element, Length, Task};
use iced_aw::{TabLabel, Tabs};
use std::path::{Path, PathBuf};
use analyzeme::ProfilingData;

pub fn main() -> iced::Result {
    iced::application(Lineme::new, Lineme::update, Lineme::view)
        .title(Lineme::title)
        .run()
}

#[derive(Debug, Clone)]
struct Stats {
    event_count: usize,
    cmd: String,
    pid: u32,
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    OpenFile,
    FileSelected(PathBuf),
    FileLoaded(PathBuf, Stats),
    ErrorOccurred(String),
    None,
}

struct Lineme {
    active_tab: usize,
    files: Vec<FileData>,
    #[allow(dead_code)]
    settings: SettingsPage,
}

struct FileData {
    path: PathBuf,
    stats: Stats,
}

struct SettingsPage {
    #[allow(dead_code)]
    show_details: bool,
}

impl Lineme {
    fn new() -> (Self, Task<Message>) {
        (
            Lineme {
                active_tab: 0,
                files: Vec::new(),
                settings: SettingsPage { show_details: true },
            },
            Task::none(),
        )
    }

    fn title(&self) -> String {
        String::from("Lineme - measureme profdata viewer")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(index) => {
                self.active_tab = index;
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
                self.files.push(FileData { path, stats });
                self.active_tab = self.files.len() - 1;
            }
            Message::ErrorOccurred(e) => {
                eprintln!("Error: {}", e);
            }
            Message::None => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let mut tabs = Tabs::new(Message::TabSelected);

        for (i, file) in self.files.iter().enumerate() {
            let label = file.path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Unknown".to_string());
            
            tabs = tabs.push(i, TabLabel::Text(label), self.file_view(file));
        }

        tabs = tabs.push(
            self.files.len(),
            TabLabel::Text("Settings".to_string()),
            self.settings_view(),
        );

        tabs.set_active_tab(&self.active_tab).into()
    }

    fn file_view(&self, file: &FileData) -> Element<'_, Message> {
        let content = column![
            text(format!("File: {}", file.path.display())).size(20),
            text(format!("Command: {}", file.stats.cmd)),
            text(format!("PID: {}", file.stats.pid)),
            text(format!("Event count: {}", file.stats.event_count)),
            button("Open another file").on_press(Message::OpenFile),
        ]
        .spacing(10)
        .padding(20);

        scrollable(content).into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let content = column![
            text("Settings").size(30),
            text("Welcome to Lineme Settings"),
            text(format!("Currently managing {} open files", self.files.len())),
            button("Open file from here").on_press(Message::OpenFile),
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

    let event_count = data.iter().count();
    let metadata = data.metadata();

    Ok(Stats {
        event_count,
        cmd: metadata.cmd.clone(),
        pid: metadata.process_id,
    })
}
