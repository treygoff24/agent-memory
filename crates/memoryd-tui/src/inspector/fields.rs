use crate::theme_glue::ThemeStyles;
use ratatui::text::{Line, Span};

pub fn kv<'a>(label: &'a str, value: &'a str, styles: &ThemeStyles) -> Line<'a> {
    Line::from(vec![Span::styled(format!("{label}: "), styles.muted), Span::styled(value.to_string(), styles.base)])
}
