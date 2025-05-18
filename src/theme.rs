use ratatui::style::Color;

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
        Theme {
            row_number: Color::Rgb(64, 64, 64),
            border: Color::Rgb(64, 64, 64),
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
}
