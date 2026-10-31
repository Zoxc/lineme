use iced::widget::button;
use iced::{Border, Theme};

/// Corner radius shared by buttons, tabs, pick lists and panels.
pub const UI_RADIUS: f32 = 5.0;

pub fn rounded() -> Border {
    Border {
        radius: UI_RADIUS.into(),
        ..Default::default()
    }
}

pub fn neutral_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let base = button::Style {
        text_color: palette.background.weak.text,
        border: rounded(),
        ..Default::default()
    };
    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(palette.background.strong.color.into()),
            ..base
        },
        _ => base,
    }
}
