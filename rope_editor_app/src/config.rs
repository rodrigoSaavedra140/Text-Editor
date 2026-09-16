use crate::style::Theme;

pub struct Config {
    pub tab_size: usize,
    pub line_numbers: bool,
    pub theme: Theme,
}

impl Config {
    pub fn default() -> Self {
        Config { tab_size: 4, line_numbers: true, theme: Theme::default() }
    }
}
