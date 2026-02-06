use crate::{Message, FILE_ICON, ICON_FONT};
use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Element, Length};

#[derive(Debug, Default)]
pub struct SettingsPage {
    last_action_message: Option<String>,
}

impl SettingsPage {
    pub fn new() -> Self {
        Self {
            last_action_message: None,
        }
    }

    pub fn set_last_action_message(&mut self, message: Option<String>) {
        self.last_action_message = message;
    }

    pub fn view(&self) -> Element<'_, Message> {
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
                button(
                    row![
                        text(FILE_ICON).font(ICON_FONT).size(16),
                        text("Register .mm_profdata").size(12)
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .on_press(Message::RegisterFileExtension),
                if let Some(msg) = &self.last_action_message {
                    Element::from(text(msg).size(12))
                } else {
                    Element::from(Space::new().width(Length::Fill))
                }
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            container(hints).padding(6).style(|_theme: &iced::Theme| {
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
