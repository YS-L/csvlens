use ratatui::style::Color;
use terminal_colorsaurus::{QueryOptions, ThemeMode, theme_mode};

pub struct Theme {
    pub row_number: Color,
    pub border: Color,
    pub selected_foreground: Color,
    pub selected_background: Color,
    pub marked_foreground: Color,
    pub marked_background: Color,
    pub found: Color,
    pub found_selected_background: Color,
    pub status: Color,
    pub column_colors: [Color; 5],
}

impl Theme {
    pub fn default() -> Self {
        match theme_mode(QueryOptions::default()) {
            Ok(ThemeMode::Dark) => Theme::dark(),
            Ok(ThemeMode::Light) => Theme::light(),
            _ => Theme::dark(),
        }
    }

    pub fn dark() -> Self {
        let gutter = Color::Rgb(131, 148, 150);
        Theme {
            row_number: gutter,
            border: gutter,
            selected_foreground: Color::Rgb(192, 192, 192),
            selected_background: Color::Rgb(62, 61, 50),
            marked_foreground: Color::Rgb(220, 230, 255),
            marked_background: Color::Rgb(40, 50, 80),
            found: Color::Rgb(200, 0, 0),
            found_selected_background: Color::LightYellow,
            status: gutter,
            column_colors: [
                Color::Rgb(253, 151, 31),
                Color::Rgb(102, 217, 239),
                Color::Rgb(190, 132, 255),
                Color::Rgb(249, 38, 114),
                Color::Rgb(230, 219, 116),
            ],
        }
    }

    pub fn light() -> Self {
        let gutter = Color::Rgb(131, 148, 150);
        Theme {
            row_number: gutter,
            border: gutter,
            selected_foreground: Color::Rgb(73, 72, 62),
            selected_background: Color::Rgb(230, 227, 196),
            marked_foreground: Color::Rgb(0, 40, 80),
            marked_background: Color::Rgb(220, 235, 255),
            found: Color::Rgb(200, 0, 0),
            found_selected_background: Color::LightYellow,
            status: gutter,
            column_colors: [
                Color::Rgb(207, 112, 0),
                Color::Rgb(0, 137, 179),
                Color::Rgb(104, 77, 153),
                Color::Rgb(249, 0, 90),
                Color::Rgb(153, 143, 47),
            ],
        }
    }
}
