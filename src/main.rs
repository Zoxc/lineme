#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod data;
mod file;
mod scrollbar;
mod settings;
mod symbols;
mod timeline;
mod tooltip;
mod ui;
use crate::data::EventId;
use crate::file::{FileLoadState, FileTab};
use data::{ProfileData, format_panic_payload, load_profiling_data};
use iced::futures::channel::oneshot;
use iced::widget::{
    Space, button, checkbox, column, container, pick_list, row, scrollable, svg, text,
};
use iced::{Alignment, Element, Length, Task};
use settings::{SettingsMessage, SettingsPage};
use std::path::PathBuf;
use std::thread;
use std::time::Instant;
use timeline::{ColorMode, format_duration};

pub const ICON_FONT: iced::Font = iced::Font::with_name("Material Icons");
const SETTINGS_ICON: char = '\u{e8b8}';
const OPEN_ICON: char = '\u{e2c7}';
const FILE_ICON: char = '\u{e873}';
const RESET_ICON: char = '\u{e5d5}';
const CLOSE_ICON: char = '\u{e5cd}';
const BACK_ICON: char = '\u{e5c4}';
const FORWARD_ICON: char = '\u{e5c8}';
const CLOSE_ICON_SIZE: f32 = 16.0;
const CLOSE_BUTTON_SIZE: f32 = 20.0;
const TAB_HEIGHT: f32 = 28.0;

// Try to register the .mm_profdata extension to open with the current executable.
// On Windows this writes under HKCU\Software\Classes so admin rights aren't required.
fn register_file_extension_impl() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use winreg::RegKey;
        use winreg::enums::*;

        let exe = std::env::current_exe().map_err(|e| format!("current_exe failed: {}", e))?;
        let exe_str = exe
            .to_str()
            .ok_or_else(|| "Executable path contains invalid UTF-8".to_string())?
            .to_string();

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        // Set the extension association to our progid
        let (ext_key, _disp) = hkcu
            .create_subkey("Software\\Classes\\.mm_profdata")
            .map_err(|e| format!("registry create failed: {}", e))?;
        ext_key
            .set_value("", &"lineme.mm_profdata")
            .map_err(|e| format!("registry set failed: {}", e))?;

        // ProgID with friendly name
        let (progid_key, _disp) = hkcu
            .create_subkey("Software\\Classes\\lineme.mm_profdata")
            .map_err(|e| format!("registry create failed: {}", e))?;
        progid_key
            .set_value("", &"LineMe measureme profdata")
            .map_err(|e| format!("registry set failed: {}", e))?;

        // Default icon: the file icon embedded as the second icon resource in the exe
        let (icon_key, _disp) = hkcu
            .create_subkey("Software\\Classes\\lineme.mm_profdata\\DefaultIcon")
            .map_err(|e| format!("registry create failed: {}", e))?;
        icon_key
            .set_value("", &format!("\"{}\",1", exe_str.replace('/', "\\")))
            .map_err(|e| format!("registry set failed: {}", e))?;

        // command to open files
        let (cmd_key, _disp) = hkcu
            .create_subkey("Software\\Classes\\lineme.mm_profdata\\shell\\open\\command")
            .map_err(|e| format!("registry create failed: {}", e))?;
        let cmd = format!("\"{}\" \"%1\"", exe_str.replace('/', "\\"));
        cmd_key
            .set_value("", &cmd)
            .map_err(|e| format!("registry set failed: {}", e))?;

        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        register_file_extension_linux()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err("Registering file extensions is not supported on this platform".to_string())
    }
}

// On Linux this installs a shared-mime-info type and a .desktop entry under the
// user's XDG data directory, then sets lineme as the default handler. This works
// with Dolphin/KDE as well as other XDG-compliant file managers.
#[cfg(target_os = "linux")]
fn register_file_extension_linux() -> Result<(), String> {
    use std::process::Command;

    const MIME_TYPE: &str = "application/x-mm-profdata";
    const DESKTOP_FILE: &str = "lineme.desktop";

    let exe = std::env::current_exe().map_err(|e| format!("current_exe failed: {}", e))?;
    let exe_str = exe
        .to_str()
        .ok_or_else(|| "Executable path contains invalid UTF-8".to_string())?;

    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| "Could not determine XDG data directory".to_string())?;

    let mime_dir = data_home.join("mime");
    let mime_packages = mime_dir.join("packages");
    let apps_dir = data_home.join("applications");
    std::fs::create_dir_all(&mime_packages)
        .map_err(|e| format!("create {} failed: {}", mime_packages.display(), e))?;
    std::fs::create_dir_all(&apps_dir)
        .map_err(|e| format!("create {} failed: {}", apps_dir.display(), e))?;

    let mime_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="{MIME_TYPE}">
    <comment>measureme profiling data</comment>
    <icon name="application-x-mm-profdata"/>
    <glob pattern="*.mm_profdata"/>
  </mime-type>
</mime-info>
"#
    );
    let mime_path = mime_packages.join("lineme.xml");
    std::fs::write(&mime_path, mime_xml)
        .map_err(|e| format!("write {} failed: {}", mime_path.display(), e))?;

    // Quote the executable path per the Desktop Entry spec.
    let mut quoted_exe = String::from("\"");
    for c in exe_str.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted_exe.push('\\');
        }
        quoted_exe.push(c);
    }
    quoted_exe.push('"');

    let desktop = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=LineMe\n\
         Comment=measureme profile viewer\n\
         Icon=lineme\n\
         Exec={quoted_exe} %f\n\
         Terminal=false\n\
         NoDisplay=false\n\
         Categories=Development;Profiling;\n\
         MimeType={MIME_TYPE};\n"
    );
    let desktop_path = apps_dir.join(DESKTOP_FILE);
    std::fs::write(&desktop_path, desktop)
        .map_err(|e| format!("write {} failed: {}", desktop_path.display(), e))?;

    // Install app and file type icons into the hicolor theme.
    let icons_dir = data_home.join("icons/hicolor");
    // The 16x16 SVGs are pixel-aligned so they stay sharp at small sizes.
    let icons: [(&str, &str, &[u8]); 4] = [
        ("scalable/apps", "lineme.svg", include_bytes!("../assets/lineme-app.svg")),
        ("16x16/apps", "lineme.svg", include_bytes!("../assets/lineme-app-16.svg")),
        (
            "scalable/mimetypes",
            "application-x-mm-profdata.svg",
            include_bytes!("../assets/lineme-file.svg"),
        ),
        (
            "16x16/mimetypes",
            "application-x-mm-profdata.svg",
            include_bytes!("../assets/lineme-file-16.svg"),
        ),
    ];
    // Remove PNG icons installed by earlier versions, they would take precedence.
    for size in [16, 22, 24, 32, 48] {
        let dir = icons_dir.join(format!("{size}x{size}"));
        let _ = std::fs::remove_file(dir.join("apps/lineme.png"));
        let _ = std::fs::remove_file(dir.join("mimetypes/application-x-mm-profdata.png"));
    }
    for (subdir, name, data) in icons {
        let dir = icons_dir.join(subdir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("create {} failed: {}", dir.display(), e))?;
        let path = dir.join(name);
        std::fs::write(&path, data).map_err(|e| format!("write {} failed: {}", path.display(), e))?;
    }

    let run = |cmd: &str, args: &[&std::ffi::OsStr]| -> Result<(), String> {
        let status = Command::new(cmd)
            .args(args)
            .status()
            .map_err(|e| format!("{} failed: {}", cmd, e))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("{} exited with {}", cmd, status))
        }
    };

    run("update-mime-database", &[mime_dir.as_os_str()])?;
    // Refreshing the desktop database is best-effort; it is not installed everywhere.
    let _ = run("update-desktop-database", &[apps_dir.as_os_str()]);
    let _ = run(
        "gtk-update-icon-cache",
        &["-f".as_ref(), "-t".as_ref(), icons_dir.as_os_str()],
    );
    // Set lineme as the default handler. This is what `xdg-mime default` does, but
    // that needs `qtpaths` on KDE which isn't always installed.
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| "Could not determine XDG config directory".to_string())?;
    std::fs::create_dir_all(&config_home)
        .map_err(|e| format!("create {} failed: {}", config_home.display(), e))?;
    let mimeapps_path = config_home.join("mimeapps.list");
    let mimeapps = match std::fs::read_to_string(&mimeapps_path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("read {} failed: {}", mimeapps_path.display(), e)),
    };
    let mimeapps = set_ini_value(
        &mimeapps,
        "Default Applications",
        MIME_TYPE,
        DESKTOP_FILE,
    );
    std::fs::write(&mimeapps_path, mimeapps)
        .map_err(|e| format!("write {} failed: {}", mimeapps_path.display(), e))?;
    // KDE caches rendered icons; drop the cache so updated icons are used.
    let cache_home = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")));
    if let Some(cache_home) = cache_home {
        let _ = std::fs::remove_file(cache_home.join("icon-cache.kcache"));
    }
    // Rebuild the KDE service cache so Dolphin picks up the change immediately.
    if run("kbuildsycoca6", &[]).is_err() {
        let _ = run("kbuildsycoca5", &[]);
    }
    // Tell running KDE apps (Dolphin, KWin) to reload their icons.
    let _ = run(
        "dbus-send",
        &[
            "--session".as_ref(),
            "--type=signal".as_ref(),
            "/KIconLoader".as_ref(),
            "org.kde.KIconLoader.iconChanged".as_ref(),
            "int32:0".as_ref(),
        ],
    );
    let _ = run(
        "dbus-send",
        &[
            "--session".as_ref(),
            "--type=signal".as_ref(),
            "/KGlobalSettings".as_ref(),
            "org.kde.KGlobalSettings.notifyChange".as_ref(),
            // KGlobalSettings::IconChanged
            "int32:4".as_ref(),
            "int32:0".as_ref(),
        ],
    );

    Ok(())
}

fn window_settings() -> iced::window::Settings {
    // On Linux the window icon is only used for small title bar icons (the taskbar
    // uses the .desktop icon), so use the pixel-snapped 16x16 variant there.
    // Windows loads the icons from the exe resources instead, see `set_windows_icons`.
    #[cfg(target_os = "linux")]
    let icon = iced::window::icon::from_rgba(
        include_bytes!("../assets/lineme-app-16.rgba").to_vec(),
        16,
        16,
    )
    .ok();
    #[cfg(not(target_os = "linux"))]
    let icon = None;

    iced::window::Settings {
        icon,
        #[cfg(target_os = "linux")]
        platform_specific: iced::window::settings::PlatformSpecific {
            // Must match the .desktop file basename so the desktop picks up its icon.
            application_id: "lineme".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

// Sets `key=value` in `[section]` of an INI style file, replacing an existing entry
// for `key` and creating the section if needed. Other content is kept as is.
#[cfg(target_os = "linux")]
fn set_ini_value(contents: &str, section: &str, key: &str, value: &str) -> String {
    let header = format!("[{section}]");
    let entry = format!("{key}={value}");
    let mut lines: Vec<String> = contents.lines().map(str::to_string).collect();

    let Some(start) = lines.iter().position(|l| l.trim() == header) else {
        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(header);
        lines.push(entry);
        return lines.join("\n") + "\n";
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| start + 1 + i);

    let existing = lines[start + 1..end].iter().position(|l| {
        l.split_once('=')
            .is_some_and(|(k, _)| k.trim() == key)
    });
    match existing {
        Some(i) => lines[start + 1 + i] = entry,
        None => {
            // Insert after the last non-empty line of the section.
            let insert = lines[start + 1..end]
                .iter()
                .rposition(|l| !l.trim().is_empty())
                .map_or(start + 1, |i| start + 2 + i);
            lines.insert(insert, entry);
        }
    }
    lines.join("\n") + "\n"
}

// Sets the small (title bar) and big (taskbar, Alt+Tab) window icons from the ICO
// embedded in the exe, so Windows picks the best matching size from it.
#[cfg(target_os = "windows")]
fn set_windows_icons(window: &dyn iced::window::Window) {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LoadImageW, SM_CXICON, SM_CXSMICON,
        SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
    };

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;

    // Icon resource id 1 in assets/lineme.rc.
    let resource = 1usize as windows_sys::core::PCWSTR;
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        for (kind, cx, cy) in [
            (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
            (ICON_BIG, SM_CXICON, SM_CYICON),
        ] {
            let icon = LoadImageW(
                module,
                resource,
                IMAGE_ICON,
                GetSystemMetrics(cx),
                GetSystemMetrics(cy),
                0,
            );
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize);
            }
        }
    }
}

pub fn main() -> iced::Result {
    iced::application(Lineme::new, Lineme::update, Lineme::view)
        .title(Lineme::title)
        .font(include_bytes!("../assets/MaterialIcons-Regular.ttf"))
        .subscription(Lineme::subscription)
        .theme(Lineme::theme)
        .window(window_settings())
        // wgpu's GL backend clips the MSAA resolve to the last scissor rect,
        // which drops most canvas geometry.
        .antialiasing(false)
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
                radius: crate::ui::UI_RADIUS.into(),
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
                    radius: crate::ui::UI_RADIUS.into(),
                    width: 1.0,
                    color: iced::Color::from_rgb(0.72, 0.72, 0.72),
                },
            }
        }
    }
}

fn toolbar_button_style(theme: &iced::Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let base = button::Style {
        text_color: palette.background.base.text,
        border: crate::ui::rounded(),
        ..Default::default()
    };
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(palette.background.weak.color.into()),
            ..base
        },
        button::Status::Disabled => button::Style {
            text_color: palette.background.strong.color,
            ..base
        },
        _ => base,
    }
}

#[derive(Debug, Clone)]
enum Message {
    TabSelected(usize),
    OpenFile,
    FileSelected(PathBuf),
    FileLoaded(u64, Box<ProfileData>, u64),
    FileLoadFailed(u64, String),
    ViewChanged(ViewType),
    ColorModeChanged(ColorMode),
    CloseTab(usize),
    OpenSettings,
    EventSelected(EventId),
    EventDoubleClicked(EventId),
    EventHovered {
        event: Option<EventId>,
        position: Option<iced::Point>,
    },
    TimelineZoomed {
        delta: f32,
        x: f32,
    },
    /// Zoom the timeline to an explicit ns range (start/end are ns relative to file min)
    TimelineZoomTo {
        start_ns: f64,
        end_ns: f64,
    },
    TimelineViewportChanged {
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
    TimelineHorizontalScrolled {
        start_ns: f64,
    },
    TimelineVerticalScrolled {
        scroll_y: f64,
    },
    TimelinePanned {
        delta: iced::Vector,
    },
    ResetView,
    NavigateBack,
    NavigateForward,
    ToggleThreadCollapse(timeline::ThreadGroupKey),
    CollapseAllThreads,
    ExpandAllThreads,
    MergeThreadsToggled(bool),
    ModifiersChanged(iced::keyboard::Modifiers),

    None,
    Settings(SettingsMessage),
}

struct Lineme {
    active_tab: usize,
    files: Vec<FileTab>,
    show_settings: bool,
    modifiers: iced::keyboard::Modifiers,
    settings: SettingsPage,
    next_file_id: u64,
}

impl Lineme {
    fn new() -> (Self, Task<Message>) {
        let mut app = Lineme {
            active_tab: 0,
            files: Vec::new(),
            show_settings: false,
            modifiers: iced::keyboard::Modifiers::default(),
            settings: SettingsPage::new(),
            next_file_id: 0,
        };

        let initial_task = if let Some(path_str) = std::env::args().nth(1) {
            let path = PathBuf::from(path_str);
            app.start_loading_file(path)
        } else {
            Task::none()
        };

        #[cfg(target_os = "windows")]
        let initial_task = Task::batch([
            initial_task,
            iced::window::oldest()
                .and_then(|id| iced::window::run(id, set_windows_icons))
                .discard(),
        ]);

        (app, initial_task)
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        iced::event::listen_with(|event, _status, _id| match event {
            iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                Some(Message::FileSelected(path))
            }
            // Track modifier changes for mouse-wheel & pan behavior
            iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                Some(Message::ModifiersChanged(modifiers))
            }
            // Pressing Escape resets the current view (zoom/scroll)
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            }) => Some(Message::ResetView),
            // Alt + Left / Right navigates back / forward
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowLeft),
                modifiers,
                ..
            }) if modifiers.alt() => Some(Message::NavigateBack),
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowRight),
                modifiers,
                ..
            }) if modifiers.alt() => Some(Message::NavigateForward),
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Back)) => {
                Some(Message::NavigateBack)
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Forward,
            )) => Some(Message::NavigateForward),
            iced::Event::Keyboard(_) => None,
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
        use iced::theme::Palette;
        use iced::Color;

        iced::Theme::custom(
            "Grey".to_string(),
            Palette {
                background: Color::WHITE,
                text: Color::BLACK,
                primary: Color::from_rgb(0.45, 0.45, 0.45),
                success: Color::from_rgb(0.07, 0.40, 0.31),
                warning: Color::from_rgb(0.72, 0.49, 0.20),
                danger: Color::from_rgb(0.76, 0.26, 0.25),
            },
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabSelected(index) => {
                self.active_tab = index;
                self.show_settings = false;

                if let Some(file) = self.files.get_mut(self.active_tab)
                    && let FileLoadState::Ready(stats) = &mut file.load_state
                {
                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;
                }
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
            Message::FileLoaded(id, mut stats, duration_ns) => {
                if let Some(file) = self.files.iter_mut().find(|file| file.id == id) {
                    // transfer load-duration into FileData and store ready state.
                    stats.load_duration_ns = Some(duration_ns);
                    file.load_state = FileLoadState::Ready(stats);
                }
            }
            Message::FileLoadFailed(id, error) => {
                if let Some(file) = self.files.iter_mut().find(|file| file.id == id) {
                    file.load_state = FileLoadState::Error(error);
                }
            }
            Message::ViewChanged(view) => {
                if let Some(file) = self.files.get_mut(self.active_tab)
                    && let FileLoadState::Ready(stats) = &mut file.load_state
                {
                    stats.ui.view_type = view;
                }
            }
            Message::ColorModeChanged(color_mode) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats.ui.color_mode = color_mode,
                        _ => {
                            // Keep color_mode in the UI until file loads; nothing to do here
                        }
                    }
                }
            }
            Message::CloseTab(index) => {
                if index < self.files.len() {
                    self.files.remove(index);
                    // Keep the same file selected when closing a tab before it.
                    if index < self.active_tab {
                        self.active_tab -= 1;
                    } else if self.active_tab >= self.files.len() && !self.files.is_empty() {
                        self.active_tab = self.files.len() - 1;
                    }
                }

                if let Some(file) = self.files.get_mut(self.active_tab)
                    && let FileLoadState::Ready(stats) = &mut file.load_state
                {
                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;
                }
            }
            Message::OpenSettings => {
                // Toggle settings panel on/off
                self.show_settings = !self.show_settings;
            }
            Message::Settings(SettingsMessage::RegisterFileExtension) => {
                // Run registration off the UI thread and report result back
                return Task::perform(
                    async move {
                        let (tx, rx) = oneshot::channel();
                        std::thread::spawn(move || {
                            let res = register_file_extension_impl();
                            let _ = tx.send(res);
                        });

                        match rx.await {
                            Ok(r) => {
                                Message::Settings(SettingsMessage::RegisterFileExtensionResult(r))
                            }
                            Err(_) => {
                                Message::Settings(SettingsMessage::RegisterFileExtensionResult(
                                    Err("Registration task failed".to_string()),
                                ))
                            }
                        }
                    },
                    |m| m,
                );
            }
            Message::Settings(SettingsMessage::RegisterFileExtensionResult(res)) => {
                let msg = match res {
                    Ok(()) => "Registered .mm_profdata for current user".to_string(),
                    Err(e) => format!("Registration failed: {}", e),
                };
                self.settings.set_last_action_message(Some(msg));
                self.show_settings = true;
            }
            Message::EventSelected(event) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    match &mut file.load_state {
                        FileLoadState::Ready(stats) => {
                            let was_empty = stats.ui.selected_event.is_none();
                            stats.ui.selected_event = Some(event);
                            if was_empty {
                                return Task::none();
                            }
                        }
                        _ => {
                            // Selection applies only after load; store temporarily on the
                            // load thread if necessary. For now, discard selection if not
                            // yet loaded.
                        }
                    }
                }
            }
            Message::EventDoubleClicked(event) => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };

                    let event = match stats.data.events.get(event.index()) {
                        Some(event) => event,
                        None => return Task::none(),
                    };

                    let min_ns = stats.data.timeline.min_ns;
                    let max_ns = stats.data.timeline.max_ns;
                    let total_ns = crate::timeline::total_ns(min_ns, max_ns).max(1);
                    let viewport_width = stats.ui.viewport_width.max(1.0_f64);

                    stats.ui.push_history();

                    let event_rel_start = event.start_ns.saturating_sub(min_ns);
                    let event_rel_end = event_rel_start.saturating_add(event.duration_ns);

                    // Add padding of 20% of event duration (10% on each side)
                    let padding_ns = ((event.duration_ns as f64) * 0.2).round() as u64;
                    let half_pad = padding_ns / 2;

                    let start_ns = event_rel_start.saturating_sub(half_pad).min(total_ns);
                    let end_ns = event_rel_end.saturating_add(half_pad).min(total_ns);

                    // Zoom so the selected range fills the viewport.
                    let target_ns = (end_ns.saturating_sub(start_ns)).max(1) as f64;
                    stats.ui.zoom_level = viewport_width / target_ns;

                    stats.ui.scroll_offset_x = crate::timeline::clamp_scroll_offset_ns(
                        start_ns as f64,
                        total_ns,
                        viewport_width,
                        stats.ui.zoom_level,
                    );

                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;

                    return Task::none();
                }
            }
            Message::EventHovered { event, position } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    match &mut file.load_state {
                        FileLoadState::Ready(stats) => {
                            stats.ui.hovered_event = event;
                            stats.ui.hovered_event_position = position;
                        }
                        _ => {
                            // Hover before load ignored
                        }
                    }
                }
            }
            Message::TimelineZoomed { delta, x } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    let min_ns = stats.data.timeline.min_ns;
                    let max_ns = stats.data.timeline.max_ns;
                    let zoom_factor = if delta > 0.0 { 1.1_f64 } else { 0.9_f64 };

                    let old_zoom = stats.ui.zoom_level.max(1e-9);
                    let new_zoom = (old_zoom * zoom_factor).max(1e-9);

                    // Adjust scroll offset to keep x position stable (work in f64)
                    let x_on_canvas = x as f64 + stats.ui.scroll_offset_x * old_zoom;
                    let new_scroll_px = x_on_canvas * zoom_factor - x as f64;
                    stats.ui.zoom_level = new_zoom;
                    stats.ui.scroll_offset_x = new_scroll_px / new_zoom;

                    let total_ns = crate::timeline::total_ns(min_ns, max_ns);
                    let viewport_width = stats.ui.viewport_width.max(0.0_f64);
                    stats.ui.scroll_offset_x = crate::timeline::clamp_scroll_offset_ns(
                        stats.ui.scroll_offset_x,
                        total_ns,
                        viewport_width,
                        stats.ui.zoom_level,
                    );

                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;

                    return Task::none();
                }
            }
            Message::TimelineViewportChanged {
                viewport_width,
                viewport_height,
            } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let thread_groups = file.thread_groups();
                    let total_height = timeline::total_timeline_height(thread_groups);

                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };

                    let first_time = stats.ui.viewport_width == 0.0 && viewport_width > 0.0;
                    if viewport_width > 0.0 {
                        stats.ui.viewport_width = viewport_width as f64;
                    }
                    if viewport_height > 0.0 {
                        stats.ui.viewport_height = viewport_height as f64;
                    }

                    let has_user_view = stats.ui.zoom_level != 1.0
                        || stats.ui.scroll_offset_x != 0.0
                        || stats.ui.scroll_offset_y != 0.0;
                    let should_initial_fit = (first_time
                        || (viewport_width > 0.0 && !stats.ui.initial_fit_done))
                        && !has_user_view;

                    if should_initial_fit {
                        let min_ns = stats.data.timeline.min_ns;
                        let max_ns = stats.data.timeline.max_ns;
                        let total_ns = max_ns.saturating_sub(min_ns);
                        stats.ui.zoom_level =
                            (viewport_width - 2.0).max(1.0) as f64 / total_ns.max(1) as f64;
                        stats.ui.initial_fit_done = true;
                    } else if !stats.ui.initial_fit_done && viewport_width > 0.0 {
                        stats.ui.initial_fit_done = true;
                    }

                    let min_ns = stats.data.timeline.min_ns;
                    let max_ns = stats.data.timeline.max_ns;
                    let total_ns = crate::timeline::total_ns(min_ns, max_ns);
                    let viewport_width = stats.ui.viewport_width.max(0.0_f64);
                    stats.ui.scroll_offset_x = crate::timeline::clamp_scroll_offset_ns(
                        stats.ui.scroll_offset_x,
                        total_ns,
                        viewport_width,
                        stats.ui.zoom_level,
                    );

                    let viewport_height = stats.ui.viewport_height.max(1.0);
                    let max_scroll_y = (total_height - viewport_height).max(0.0);
                    stats.ui.scroll_offset_y = stats.ui.scroll_offset_y.clamp(0.0, max_scroll_y);

                    return Task::none();
                }
            }
            Message::TimelineZoomTo { start_ns, end_ns } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    let min_ns = stats.data.timeline.min_ns;
                    let max_ns = stats.data.timeline.max_ns;
                    let total_ns = crate::timeline::total_ns(min_ns, max_ns);
                    let provided_viewport_width = stats.ui.viewport_width.max(1.0);

                    // Clamp to timeline range (start_ns/end_ns are relative to min_ns)
                    let start_ns = start_ns.clamp(0.0, total_ns as f64);
                    let end_ns = end_ns.clamp(0.0, total_ns as f64);
                    let range_ns = (end_ns - start_ns).max(1.0);

                    if stats.ui.viewport_width == 0.0 {
                        stats.ui.viewport_width = provided_viewport_width;
                    }
                    let viewport_width = stats.ui.viewport_width.max(1.0);
                    stats.ui.zoom_level = viewport_width / (range_ns.max(1.0));
                    stats.ui.initial_fit_done = true;

                    stats.ui.scroll_offset_x = crate::timeline::clamp_scroll_offset_ns(
                        start_ns,
                        total_ns,
                        viewport_width,
                        stats.ui.zoom_level,
                    );

                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;

                    return Task::none();
                }
            }
            Message::MiniTimelineJump {
                fraction,
                viewport_width,
            } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    let min_ns = stats.data.timeline.min_ns;
                    let max_ns = stats.data.timeline.max_ns;
                    let provided_viewport_width = viewport_width.max(1.0) as f64;
                    if stats.ui.viewport_width == 0.0 {
                        stats.ui.viewport_width = provided_viewport_width;
                    }

                    let viewport_width = stats.ui.viewport_width.max(1.0);
                    if !stats.ui.initial_fit_done {
                        let total_ns = max_ns.saturating_sub(min_ns).max(1);
                        stats.ui.zoom_level = (viewport_width - 2.0).max(1.0) / total_ns as f64;
                        stats.ui.initial_fit_done = true;
                    }

                    if max_ns > min_ns {
                        let total_ns = max_ns.saturating_sub(min_ns);
                        let target_center_ns = fraction * total_ns as f64;
                        let visible_ns = viewport_width / stats.ui.zoom_level.max(1e-9);
                        let target_ns = target_center_ns - visible_ns / 2.0;
                        stats.ui.scroll_offset_x = crate::timeline::clamp_scroll_offset_ns(
                            target_ns,
                            total_ns,
                            viewport_width,
                            stats.ui.zoom_level,
                        );

                        stats.ui.hovered_event = None;
                        stats.ui.hovered_event_position = None;

                        return Task::none();
                    }
                }
            }
            Message::TimelineHorizontalScrolled { start_ns } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    let zoom_level = stats.ui.zoom_level.max(1e-9);
                    let viewport_width = stats.ui.viewport_width.max(1.0);
                    let visible_ns = (viewport_width / zoom_level).max(1.0);
                    let total_ns = stats
                        .data
                        .timeline
                        .max_ns
                        .saturating_sub(stats.data.timeline.min_ns);
                    let max_start_ns = (total_ns as f64 - visible_ns).max(0.0);
                    let clamped_start = start_ns.clamp(0.0, max_start_ns);
                    stats.ui.scroll_offset_x = clamped_start;

                    let viewport_width = stats.ui.viewport_width.max(0.0);
                    stats.ui.scroll_offset_x = crate::timeline::clamp_scroll_offset_ns(
                        stats.ui.scroll_offset_x,
                        total_ns,
                        viewport_width,
                        stats.ui.zoom_level,
                    );

                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;

                    return Task::none();
                }
            }
            Message::TimelineVerticalScrolled { scroll_y } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let thread_groups = file.thread_groups();
                    let total_height = timeline::total_timeline_height(thread_groups);

                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };

                    let viewport_height = stats.ui.viewport_height.max(1.0);
                    let max_scroll_y = (total_height - viewport_height).max(0.0);
                    stats.ui.scroll_offset_y = scroll_y.clamp(0.0, max_scroll_y);

                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;

                    return Task::none();
                }
            }
            Message::MiniTimelineZoomTo {
                start_fraction,
                end_fraction,
                viewport_width,
            } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    let min_ns = stats.data.timeline.min_ns;
                    let max_ns = stats.data.timeline.max_ns;
                    let total_ns = crate::timeline::total_ns(min_ns, max_ns);
                    let total_ns_f64 = total_ns.max(1) as f64;
                    let range_fraction = (end_fraction - start_fraction).max(0.0) as f64;
                    let target_ns = (range_fraction * total_ns_f64).max(1.0);
                    let provided_viewport_width = viewport_width.max(1.0) as f64;
                    if stats.ui.viewport_width == 0.0 {
                        stats.ui.viewport_width = provided_viewport_width;
                    }
                    let viewport_width = stats.ui.viewport_width.max(1.0);
                    stats.ui.zoom_level = viewport_width / target_ns;
                    stats.ui.initial_fit_done = true;
                    let target_ns = start_fraction as f64 * total_ns as f64;
                    stats.ui.scroll_offset_x = crate::timeline::clamp_scroll_offset_ns(
                        target_ns,
                        total_ns,
                        viewport_width,
                        stats.ui.zoom_level,
                    );

                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;

                    return Task::none();
                }
            }
            Message::TimelinePanned { delta } => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    // Get thread_groups and compute total height before taking a
                    // mutable borrow of file.load_state to avoid borrow conflicts.
                    let thread_groups = file.thread_groups();
                    let total_height = timeline::total_timeline_height(thread_groups);

                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    let min_ns = stats.data.timeline.min_ns;
                    let max_ns = stats.data.timeline.max_ns;
                    let total_ns = crate::timeline::total_ns(min_ns, max_ns);
                    let viewport_width = stats.ui.viewport_width.max(0.0_f64);

                    let viewport_height = stats.ui.viewport_height.max(0.0_f64);
                    let max_scroll_y = (total_height - viewport_height).max(0.0);

                    stats.ui.scroll_offset_x = crate::timeline::clamp_scroll_offset_ns(
                        stats.ui.scroll_offset_x - delta.x as f64 / stats.ui.zoom_level.max(1e-9),
                        total_ns,
                        viewport_width,
                        stats.ui.zoom_level,
                    );
                    stats.ui.scroll_offset_y =
                        (stats.ui.scroll_offset_y - delta.y as f64).clamp(0.0, max_scroll_y);

                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;

                    return Task::none();
                }
            }
            Message::NavigateBack | Message::NavigateForward => {
                if let Some(file) = self.files.get_mut(self.active_tab)
                    && let FileLoadState::Ready(stats) = &mut file.load_state
                {
                    if matches!(message, Message::NavigateBack) {
                        stats.ui.navigate_back();
                    } else {
                        stats.ui.navigate_forward();
                    }
                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;
                }
            }
            Message::ResetView => {
                if let Some(file) = self.files.get_mut(self.active_tab) {
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    let min_ns = stats.data.timeline.min_ns;
                    let max_ns = stats.data.timeline.max_ns;
                    let total_ns = max_ns.saturating_sub(min_ns);
                    let viewport_width = stats.ui.viewport_width.max(0.0_f64);
                    if viewport_width > 0.0 {
                        stats.ui.zoom_level =
                            (viewport_width - 2.0).max(1.0) / total_ns.max(1) as f64;
                    } else {
                        stats.ui.zoom_level = 1000.0 / total_ns.max(1) as f64;
                    }
                    stats.ui.scroll_offset_x = 0.0_f64;
                    stats.ui.scroll_offset_y = 0.0_f64;

                    stats.ui.hovered_event = None;
                    stats.ui.hovered_event_position = None;

                    return Task::none();
                }
            }
            Message::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
            }

            Message::ToggleThreadCollapse(thread_id) => {
                if let Some(file) = self.active_file_mut() {
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

                    let thread_groups = file.thread_groups();
                    let total_height = timeline::total_timeline_height(thread_groups);
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    Lineme::clamp_vertical_scroll_if_needed(
                        &mut stats.ui.scroll_offset_y,
                        total_height,
                        stats.ui.viewport_height,
                    );
                }
            }
            Message::CollapseAllThreads => {
                if let Some(file) = self.active_file_mut() {
                    let thread_groups_mut = match file.thread_groups_mut() {
                        Some(groups) => groups,
                        None => return Task::none(),
                    };
                    for group in thread_groups_mut.iter_mut() {
                        group.is_collapsed = true;
                    }

                    let thread_groups = file.thread_groups();
                    let total_height = timeline::total_timeline_height(thread_groups);
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    Lineme::clamp_vertical_scroll_if_needed(
                        &mut stats.ui.scroll_offset_y,
                        total_height,
                        stats.ui.viewport_height,
                    );
                }
            }
            Message::ExpandAllThreads => {
                if let Some(file) = self.active_file_mut() {
                    let thread_groups_mut = match file.thread_groups_mut() {
                        Some(groups) => groups,
                        None => return Task::none(),
                    };
                    for group in thread_groups_mut.iter_mut() {
                        group.is_collapsed = false;
                    }

                    let thread_groups = file.thread_groups();
                    let total_height = timeline::total_timeline_height(thread_groups);
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    Lineme::clamp_vertical_scroll_if_needed(
                        &mut stats.ui.scroll_offset_y,
                        total_height,
                        stats.ui.viewport_height,
                    );
                }
            }
            Message::MergeThreadsToggled(enabled) => {
                if let Some(file) = self.active_file_mut() {
                    // Update merge_threads on loaded FileData if present.
                    if let FileLoadState::Ready(stats) = &mut file.load_state {
                        stats.ui.merge_threads = enabled;
                    }

                    let thread_groups = file.thread_groups();
                    let total_height = timeline::total_timeline_height(thread_groups);
                    let stats = match &mut file.load_state {
                        FileLoadState::Ready(stats) => stats,
                        _ => return Task::none(),
                    };
                    Lineme::clamp_vertical_scroll_if_needed(
                        &mut stats.ui.scroll_offset_y,
                        total_height,
                        stats.ui.viewport_height,
                    );
                }
            }
            Message::None => {}
        }
        Task::none()
    }

    fn start_loading_file(&mut self, path: PathBuf) -> Task<Message> {
        let id = self.next_file_id;
        self.next_file_id = self.next_file_id.wrapping_add(1);

        self.files.push(FileTab {
            id,
            path: path.clone(),
            load_state: FileLoadState::Loading,
        });
        self.active_tab = self.files.len() - 1;
        self.show_settings = false;

        Task::perform(
            async move {
                let (tx, rx) = oneshot::channel();
                thread::spawn(move || {
                    let start = Instant::now();
                    let result = std::panic::catch_unwind(|| load_profiling_data(&path));
                    let outcome = match result {
                        Ok(result) => result,
                        Err(payload) => Err(format_panic_payload(payload)),
                    };
                    let duration_ns = start.elapsed().as_nanos() as u64;
                    let _ = tx.send((outcome, duration_ns));
                });

                match rx.await {
                    Ok((Ok(stats), duration)) => Message::FileLoaded(id, Box::new(stats), duration),
                    Ok((Err(error), _)) => Message::FileLoadFailed(id, error),
                    Err(_) => Message::FileLoadFailed(
                        id,
                        "Loading thread exited before sending results".to_string(),
                    ),
                }
            },
            |msg| msg,
        )
    }

    // Convenience accessor for the currently active file (mutable).
    fn active_file_mut(&mut self) -> Option<&mut FileTab> {
        self.files.get_mut(self.active_tab)
    }

    // Helper used after operations that can change the total vertical height of
    // the timeline (collapse/expand, merge threads, ...). If the current
    // vertical scroll is beyond the new total height, clamp it.
    fn clamp_vertical_scroll_if_needed(
        scroll_offset_y: &mut f64,
        total_height: f64,
        viewport_height: f64,
    ) -> bool {
        if !scroll_offset_y.is_finite() {
            *scroll_offset_y = 0.0;
            return true;
        }

        let total_height = total_height.max(0.0);
        let viewport_height = viewport_height.max(1.0);
        let max_scroll_y = (total_height - viewport_height).max(0.0);

        let clamped = scroll_offset_y.clamp(0.0, max_scroll_y);
        if clamped != *scroll_offset_y {
            *scroll_offset_y = clamped;
            true
        } else {
            false
        }
    }

    fn view(&self) -> Element<'_, Message> {
        // Background/text colors of a tab, flat for the active tab so it matches the
        // content below.
        fn tab_colors(
            palette: &iced::theme::palette::Extended,
            active: bool,
            hovered: bool,
        ) -> iced::theme::palette::Pair {
            if active {
                palette.background.base
            } else if hovered {
                palette.background.strong
            } else {
                palette.background.weak
            }
        }

        let file_icon = svg::Handle::from_memory(include_bytes!("../assets/lineme-file-16.svg"));
        let tabs = self.files.iter().enumerate().map(|(i, file)| {
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

            let active = !self.show_settings && i == self.active_tab;
            let close = button(
                text(CLOSE_ICON)
                    .font(ICON_FONT)
                    .size(CLOSE_ICON_SIZE)
                    .line_height(1.0)
                    .center(),
            )
            .width(Length::Fixed(CLOSE_BUTTON_SIZE))
            .height(Length::Fixed(CLOSE_BUTTON_SIZE))
            .padding(0)
            .style(move |theme: &iced::Theme, status: button::Status| {
                let palette = theme.extended_palette();
                // Hovered inactive tabs already use `strong`, so use the strongest
                // shade there to stand out.
                let hover = if active {
                    palette.background.strong
                } else {
                    palette.background.strongest
                };
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: hovered.then(|| hover.color.into()),
                    text_color: if hovered {
                        hover.text
                    } else {
                        tab_colors(palette, active, false).text
                    },
                    border: crate::ui::rounded(),
                    ..Default::default()
                }
            })
            .on_press(Message::CloseTab(i));

            button(
                row![
                    svg(file_icon.clone())
                        .width(Length::Fixed(16.0))
                        .height(Length::Fixed(16.0)),
                    text(label).size(12.0).wrapping(text::Wrapping::None),
                    close,
                ]
                .spacing(6)
                .height(Length::Fill)
                .align_y(Alignment::Center),
            )
            // A fixed tab height with the close button centered vertically, and the
            // right padding matching the space above and below it.
            .height(Length::Fixed(TAB_HEIGHT))
            .padding(iced::Padding {
                top: 0.0,
                right: (TAB_HEIGHT - CLOSE_BUTTON_SIZE) / 2.0,
                bottom: 0.0,
                left: 8.0,
            })
            .style(move |theme: &iced::Theme, status: button::Status| {
                let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
                let pair = tab_colors(theme.extended_palette(), active, hovered);
                button::Style {
                    background: Some(pair.color.into()),
                    text_color: pair.text,
                    border: crate::ui::rounded(),
                    ..Default::default()
                }
            })
            .on_press(Message::TabSelected(i))
            .into()
        });
        let bar = row(tabs.collect::<Vec<Element<'_, Message>>>()).align_y(Alignment::Center);

        // Move the Open button left of the Settings toggle and make the header
        // bar background/border a neutral grey.
        let header = container(
            row![
                // Tabs take the remaining width and scroll horizontally (without a
                // visible scrollbar) when they don't fit, so the buttons stay visible.
                scrollable(bar)
                    .direction(scrollable::Direction::Horizontal(
                        scrollable::Scrollbar::new().width(0).scroller_width(0),
                    ))
                    .width(Length::Fill),
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
                                border: crate::ui::rounded(),
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
            self.settings.view().map(Message::Settings)
        } else if let Some(file) = self.files.get(self.active_tab) {
            // Use view_type from FileData when available; fall back to default
            let current_view = file.stats().map(|s| s.ui.view_type).unwrap_or_default();
            let inner_view = match current_view {
                ViewType::Stats => self.file_view(file),
                ViewType::Timeline => self.timeline_view(file),
            };

            if matches!(file.load_state, FileLoadState::Ready(_)) {
                // Keep "View:" and its pick_list on the left, and push the rest
                // of the timeline-specific controls to the right.
                let left_controls = row![
                    text("View:").size(12),
                    pick_list(&ViewType::ALL[..], Some(current_view), Message::ViewChanged,)
                        .text_size(12)
                        .padding(3)
                        .style(neutral_pick_list_style),
                ]
                .spacing(5)
                .align_y(Alignment::Center);

                let right_controls: Element<'_, Message> = if current_view == ViewType::Timeline {
                    Element::from(
                        row![
                            text("Color by:").size(12),
                            // When file is loaded the pick_list reads color mode from Stats.
                            pick_list(
                                &ColorMode::ALL[..],
                                file.stats().map(|s| s.ui.color_mode),
                                Message::ColorModeChanged,
                            )
                            .text_size(12)
                            .padding(3)
                            .style(neutral_pick_list_style),
                            checkbox(file.stats().map(|s| s.ui.merge_threads).unwrap_or(false))
                                .label("Merge threads")
                                .size(14)
                                .text_size(12)
                                .on_toggle(Message::MergeThreadsToggled),
                            row![
                                button(text(BACK_ICON).font(ICON_FONT))
                                    .style(toolbar_button_style)
                                    .padding(3)
                                    .on_press_maybe(
                                        file.stats()
                                            .is_some_and(|s| !s.ui.back_history.is_empty())
                                            .then_some(Message::NavigateBack),
                                    ),
                                button(text(FORWARD_ICON).font(ICON_FONT))
                                    .style(toolbar_button_style)
                                    .padding(3)
                                    .on_press_maybe(
                                        file.stats()
                                            .is_some_and(|s| !s.ui.forward_history.is_empty())
                                            .then_some(Message::NavigateForward),
                                    ),
                            ]
                            .spacing(2),
                            button(
                                row![
                                    text(RESET_ICON).font(ICON_FONT),
                                    text("Reset View").size(12.0)
                                ]
                                .spacing(5)
                                .align_y(Alignment::Center),
                            )
                            .style(toolbar_button_style)
                            .padding(3)
                            .on_press(Message::ResetView),
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    )
                } else {
                    Element::from(Space::new().width(0))
                };

                let view_selector_bar = container(
                    row![
                        left_controls,
                        Space::new().width(Length::Fill),
                        right_controls
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

        let root = column![header, content].height(Length::Fill);

        // Tooltip overlay: show event details on hover.
        // This is message-driven and intentionally non-interactive so it does
        // not interfere with timeline mouse events.
        //
        // Always wrap in Tooltip so the widget tree structure stays stable
        // regardless of hover state.  Changing the root widget between Tooltip
        // and a bare Element causes iced to recreate the tree (tags differ),
        // which resets the canvas EventsState and creates an oscillation where
        // the hover is repeatedly rediscovered and cleared every frame.
        let tooltip_underlay: Element<'_, Message> = root.into();
        let (show_tooltip, tooltip_position, tooltip_content): (bool, iced::Point, Element<'_, Message>) =
            if let Some(file) = self.files.get(self.active_tab)
                && let FileLoadState::Ready(stats) = &file.load_state
                && let (Some(event_id), Some(position)) =
                    (stats.ui.hovered_event, stats.ui.hovered_event_position)
                && let Some(event) = stats.data.events.get(event_id.index())
            {
                let label = stats.data.symbols.resolve(event.label);
                let duration_str =
                    crate::timeline::format_duration(event.duration_ns);

                let content = row![
                    text(duration_str).size(12).style(|_t: &iced::Theme| text::Style {
                        color: Some(iced::Color::from_rgb(0.408, 0.322, 0.459)),
                        ..Default::default()
                    }),
                    text(label).size(12).style(|_t: &iced::Theme| text::Style {
                        color: Some(iced::Color::from_rgb(0.15, 0.15, 0.15)),
                        ..Default::default()
                    }),
                ]
                .spacing(8)
                .align_y(Alignment::Center);

                let tooltip_el: Element<'_, Message> = container(content)
                    .padding(0)
                    .style(|_theme: &iced::Theme| container::Style::default())
                    .into();
                (true, position, tooltip_el)
            } else {
                (false, iced::Point::ORIGIN, text("").into())
            };

        crate::tooltip::Tooltip::new(tooltip_underlay, tooltip_content)
            .show(show_tooltip)
            .position(tooltip_position)
            .into()
    }

    fn file_view<'a>(&self, file: &'a FileTab) -> Element<'a, Message> {
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
                        text(&stats.data.cmd).size(12)
                    ],
                    row![
                        text("PID:").width(Length::Fixed(120.0)).size(12),
                        text(format!("{}", stats.data.pid)).size(12)
                    ],
                    row![
                        text("Event count:").width(Length::Fixed(120.0)).size(12),
                        text(format!("{}", stats.data.event_count)).size(12)
                    ],
                    row![
                        text("Load time:").width(Length::Fixed(120.0)).size(12),
                        text(match stats.load_duration_ns {
                            Some(ns) => format_duration(ns),
                            None => "unknown".to_string(),
                        })
                        .size(12)
                    ],
                    row![
                        text("Total duration:").width(Length::Fixed(120.0)).size(12),
                        text(format_duration(
                            stats
                                .data
                                .timeline
                                .max_ns
                                .saturating_sub(stats.data.timeline.min_ns)
                        ))
                        .size(12)
                    ],
                ]
                .spacing(8)
                .padding(10)
            }
        };

        let inner = container(stats_col)
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

        let scroll = scrollable::Scrollable::new(inner)
            .width(Length::Fill)
            .height(Length::Fill);

        scroll.into()
    }

    fn timeline_view<'a>(&self, file: &'a FileTab) -> Element<'a, Message> {
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
            FileLoadState::Ready(stats) => timeline::view(timeline::TimelineViewArgs {
                timeline_data: &stats.data.timeline,
                events: &stats.data.events,
                thread_groups: file.thread_groups(),
                kinds: &stats.data.kinds,
                zoom_level: stats.ui.zoom_level,
                selected_event: &stats.ui.selected_event,
                scroll_offset_x: stats.ui.scroll_offset_x,
                scroll_offset_y: stats.ui.scroll_offset_y,
                viewport_width: stats.ui.viewport_width,
                viewport_height: stats.ui.viewport_height,
                color_mode: stats.ui.color_mode,
                symbols: &stats.data.symbols,
            }),
        }
    }
}
