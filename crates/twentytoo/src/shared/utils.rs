//! Internal helpers shared by the view layer.

use axum::http::HeaderMap;
use axum::http::Uri;
use axum::http::header::COOKIE;
use axum::http::uri::Scheme;

/// Escape a string for HTML text and attribute contexts.
///
/// Used by every safe-string-returning template function — the framework
/// rule (`00` §8.3) is that those functions escape internally while the
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

/// The value of the `name` cookie from the request's `Cookie` header, if
/// present. Cookies are hand-rolled (no cookie crate): the header is split
/// on `;` and each pair is trimmed before matching.
pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = headers.get(COOKIE)?.to_str().ok()?;
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix(name)
            && value.starts_with('=')
        {
            return Some(value[1..].trim().to_string());
        }
    }
    return None;
}

/// Serialize a `Set-Cookie` header: session-scoped (`Max-Age`, not
/// `Expires`), `HttpOnly`, `SameSite=Lax`, `Path=/`, and `Secure` when the
/// request arrived over HTTPS.
pub fn set_cookie(name: &str, value: &str, max_age_secs: u64, secure: bool) -> String {
    let mut out = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}");
    if secure {
        out.push_str("; Secure");
    }
    return out;
}

/// Whether the request arrived over HTTPS: the `X-Forwarded-Proto` header
/// (reverse-proxy convention) or the request URI's own scheme.
pub fn is_secure_request(headers: &HeaderMap, uri: &Uri) -> bool {
    if let Some(proto) = headers.get("x-forwarded-proto")
        && proto
            .to_str()
            .is_ok_and(|v| return v.eq_ignore_ascii_case("https"))
    {
        return true;
    }
    return uri.scheme() == Some(&Scheme::HTTPS);
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

    #[test]
    fn read_cookie_finds_named_value() {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, "a=1; twentytoo_session=abc; b=2".parse().unwrap());
        assert_eq!(
            read_cookie(&headers, "twentytoo_session"),
            Some("abc".to_string())
        );
        assert_eq!(read_cookie(&headers, "missing"), None);
    }

    #[test]
    fn read_cookie_does_not_match_longer_names() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            "twentytoo_session=abc; twentytoo_session_step=xyz"
                .parse()
                .unwrap(),
        );
        // The `=`-after-name guard keeps prefix names from matching.
        assert_eq!(
            read_cookie(&headers, "twentytoo_session"),
            Some("abc".to_string())
        );
        assert_eq!(
            read_cookie(&headers, "twentytoo_session_step"),
            Some("xyz".to_string())
        );
    }

    #[test]
    fn set_cookie_composes_attributes() {
        assert_eq!(
            set_cookie("twentytoo_session", "tok", 600, false),
            "twentytoo_session=tok; Path=/; HttpOnly; SameSite=Lax; Max-Age=600"
        );
        assert_eq!(
            set_cookie("twentytoo_session", "tok", 0, true),
            "twentytoo_session=tok; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Secure"
        );
    }

    #[test]
    fn secure_request_detects_header_and_scheme() {
        let mut via_header = HeaderMap::new();
        via_header.insert("x-forwarded-proto", "https".parse().unwrap());
        assert!(is_secure_request(
            &via_header,
            &Uri::from_static("http://localhost/login")
        ));

        let plain_headers = HeaderMap::new();
        assert!(is_secure_request(
            &plain_headers,
            &Uri::from_static("https://localhost/login")
        ));
        assert!(!is_secure_request(
            &plain_headers,
            &Uri::from_static("http://localhost/login")
        ));
    }
}
