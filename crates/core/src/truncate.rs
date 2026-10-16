use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const ELLIPSIS: char = '…';

/// Display width in terminal cells.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Cuts `s` to at most `max` cells without an ellipsis.
pub fn fit(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > max {
            break;
        }
        used += w;
        out.push(c);
    }
    out
}

/// Keeps the start of `s`, ending with `…` when cut.
pub fn truncate_end(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = fit(s, max - 1);
    out.push(ELLIPSIS);
    out
}

/// Keeps the end of `s`, starting with `…` when cut. Used for paths.
pub fn truncate_start(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let reversed: String = s.chars().rev().collect();
    let tail: String = fit(&reversed, max - 1).chars().rev().collect();
    format!("{ELLIPSIS}{tail}")
}

/// Pads `s` with spaces to exactly `cells` cells (cutting if longer).
pub fn pad(s: &str, cells: usize) -> String {
    let mut out = fit(s, cells);
    let w = width(&out);
    out.extend(std::iter::repeat_n(' ', cells - w));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_strings_are_unchanged() {
        assert_eq!(truncate_end("abc", 3), "abc");
        assert_eq!(truncate_start("abc", 5), "abc");
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate_end("backend", 4), "bac…");
        assert_eq!(truncate_start("~/code/api", 5), "…/api");
        assert_eq!(truncate_end("abc", 0), "");
        assert_eq!(truncate_start("abc", 1), "…");
    }

    #[test]
    fn respects_wide_characters() {
        assert_eq!(width("日本"), 4);
        assert_eq!(fit("日本語", 5), "日本");
        assert_eq!(truncate_end("日本語", 4), "日…");
        assert_eq!(pad("日", 3), "日 ");
    }
}
