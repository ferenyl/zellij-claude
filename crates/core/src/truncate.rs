use unicode_width::UnicodeWidthStr;

const ELLIPSIS: char = '…';

/// Display width in terminal cells.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Cuts `s` to at most `max` cells without an ellipsis. Measures with
/// `width` on the whole string: `str::width` is not the sum of `char::width`
/// (e.g. U+2764 + VS16 is 1 cell by char, 2 by str).
pub fn fit(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        out.push(c);
        if width(&out) > max {
            out.pop();
            break;
        }
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
    let mut tail: String = fit(&reversed, max - 1).chars().rev().collect();
    while width(&tail) > max - 1 {
        tail.remove(0);
    }
    format!("{ELLIPSIS}{tail}")
}

/// Pads `s` with spaces to exactly `cells` cells (cutting if longer).
pub fn pad(s: &str, cells: usize) -> String {
    let mut out = fit(s, cells);
    let w = width(&out);
    out.extend(std::iter::repeat_n(' ', cells.saturating_sub(w)));
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

    #[test]
    fn never_exceeds_max_with_variation_selectors() {
        let samples = [
            "\u{2764}\u{FE0F}",
            "ab\u{2764}\u{FE0F}",
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
            "x\u{2699}\u{FE0F}y",
        ];
        for s in samples {
            for max in 0..8 {
                assert!(width(&fit(s, max)) <= max, "fit {s:?} {max}");
                assert!(width(&truncate_end(s, max)) <= max, "end {s:?} {max}");
                assert!(width(&truncate_start(s, max)) <= max, "start {s:?} {max}");
                assert_eq!(width(&pad(s, max)), max, "pad {s:?} {max}");
            }
        }
    }
}
