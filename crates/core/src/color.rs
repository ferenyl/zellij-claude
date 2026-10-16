use crate::hook_map::SessionState;

/// A terminal foreground color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// ANSI palette index; 0–15 follow the terminal theme.
    Ansi(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    /// Parses `#rrggbb`, an ANSI index `0`–`255`, or a basic color name.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if let Some(hex) = value.strip_prefix('#') {
            if hex.len() != 6 || !hex.is_ascii() {
                return None;
            }
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            return Some(Self::Rgb(byte(0)?, byte(2)?, byte(4)?));
        }
        if let Ok(index) = value.parse::<u8>() {
            return Some(Self::Ansi(index));
        }
        let names = [
            "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
        ];
        let lower = value.to_ascii_lowercase();
        let (bright, name) = match lower.strip_prefix("bright_") {
            Some(rest) => (true, rest),
            None => (false, lower.as_str()),
        };
        let index = names.iter().position(|n| *n == name)? as u8;
        Some(Self::Ansi(if bright { index + 8 } else { index }))
    }

    /// Default color per state, from the terminal's own palette.
    pub fn default_for(state: SessionState) -> Self {
        match state {
            SessionState::Working => Self::Ansi(3),
            SessionState::Waiting => Self::Ansi(1),
            SessionState::Idle => Self::Ansi(2),
        }
    }

    /// ANSI escape sequence setting this as foreground color.
    pub fn fg(self) -> String {
        match self {
            Self::Ansi(i) if i < 8 => format!("\u{1b}[{}m", 30 + i),
            Self::Ansi(i) if i < 16 => format!("\u{1b}[{}m", 90 + i - 8),
            Self::Ansi(i) => format!("\u{1b}[38;5;{i}m"),
            Self::Rgb(r, g, b) => format!("\u{1b}[38;2;{r};{g};{b}m"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_formats() {
        assert_eq!(Color::parse("#ff8000"), Some(Color::Rgb(255, 128, 0)));
        assert_eq!(Color::parse("208"), Some(Color::Ansi(208)));
        assert_eq!(Color::parse("Red"), Some(Color::Ansi(1)));
        assert_eq!(Color::parse("bright_blue"), Some(Color::Ansi(12)));
    }

    #[test]
    fn rejects_invalid_values() {
        for value in ["#fff", "#gg0000", "256", "orange", "", "#ffé000"] {
            assert_eq!(Color::parse(value), None, "{value}");
        }
    }

    #[test]
    fn escape_sequences() {
        assert_eq!(Color::Ansi(1).fg(), "\u{1b}[31m");
        assert_eq!(Color::Ansi(9).fg(), "\u{1b}[91m");
        assert_eq!(Color::Ansi(200).fg(), "\u{1b}[38;5;200m");
        assert_eq!(Color::Rgb(1, 2, 3).fg(), "\u{1b}[38;2;1;2;3m");
    }
}
