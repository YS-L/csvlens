use ratatui::style::Color;
use terminal_colorsaurus::{ColorScheme, QueryOptions, color_scheme};

pub struct Theme {
    pub row_number: Color,
    pub border: Color,
    pub selected_fg: Color,
    pub selected_bg: Color,
    pub found: Color,
    pub found_selected_bg: Color,
    pub status: Color,
    pub column_colors: [Color; 5],
}

impl Theme {
    pub fn default() -> Self {
        match color_scheme(QueryOptions::default()) {
            Ok(ColorScheme::Dark) => Theme::dark(),
            Ok(ColorScheme::Light) => Theme::light(),
            _ => Theme::dark(),
        }
    }

    pub fn dark() -> Self {
        let gutter = Color::Rgb(131, 148, 150);
        Theme {
            row_number: gutter,
            border: gutter,
            selected_fg: Color::Rgb(192, 192, 192),
            selected_bg: Color::Rgb(64, 64, 64),
            found: Color::Rgb(200, 0, 0),
            found_selected_bg: Color::LightYellow,
            status: Color::Rgb(128, 128, 128),
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
            selected_fg: Color::Rgb(192, 192, 192),
            selected_bg: Color::Rgb(64, 64, 64),
            found: Color::Rgb(200, 0, 0),
            found_selected_bg: Color::LightYellow,
            status: Color::Rgb(128, 128, 128),
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
