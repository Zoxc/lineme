use iced::widget::button;
use iced::Theme;

pub fn neutral_button_style(theme: &Theme, status: button::Status) -> button::Style {
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
}
