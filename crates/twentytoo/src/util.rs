//! Internal helpers shared by the view layer.

/// Escape a string for HTML text and attribute contexts.
///
/// Used by every safe-string-returning template function — the framework
/// rule (`05` §5.2) is that those functions escape internally while the
/// rest of the template is autoescaped by the environment.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    return out;
}

/// Format a finite number as money: `1234.5` → `$1,234.50`.
pub fn format_money(n: f64) -> String {
    if !n.is_finite() {
        return n.to_string();
    }
    let negative = n < 0.0;
    let cents = (n.abs() * 100.0).round() as i64;
    let whole = cents / 100;
    let frac = cents % 100;

    let digits = whole.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let prefix = if negative { "-$" } else { "$" };
    return format!("{prefix}{grouped}.{frac:02}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_escapes_five_characters() {
        assert_eq!(
            escape_html("<a href=\"x\">a & b 'c'</a>"),
            "&lt;a href=&quot;x&quot;&gt;a &amp; b &#39;c&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn escape_html_leaves_plain_text_alone() {
        assert_eq!(escape_html("plain text"), "plain text");
    }

    #[test]
    fn money_formats_with_grouping_and_cents() {
        assert_eq!(format_money(1234.5), "$1,234.50");
        assert_eq!(format_money(0.0), "$0.00");
        assert_eq!(format_money(-42.1), "-$42.10");
        assert_eq!(format_money(9999999.999), "$10,000,000.00");
    }
}
