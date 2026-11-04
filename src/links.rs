//! 编辑器请求用系统默认浏览器打开的链接。
//!
//! 表格的 `services.open_url` 回调拿不到 egui 上下文,先把地址放进这里的队列;
//! 宿主每帧取出,和 PDF 的 `system.open_url` 请求一起交给 `ctx.open_url`
//! (eframe 的 `links` 功能在 macOS / Windows / Linux 上调用系统默认浏览器)。

use std::cell::RefCell;

thread_local! {
    static PENDING: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// 记下一个待打开的地址,下一帧由宿主打开。
pub fn queue(url: &str) {
    PENDING.with(|pending| pending.borrow_mut().push(url.to_owned()));
}

/// 取出所有待打开的地址。
pub fn take() -> Vec<String> {
    PENDING.with(|pending| std::mem::take(&mut *pending.borrow_mut()))
}

/// 宿主只把网页和邮件地址交给系统:http、https、mailto。
pub fn external(url: &str) -> Option<&str> {
    let url = url.trim();
    if url.is_empty() || url.chars().any(char::is_control) {
        return None;
    }
    let (scheme, rest) = url.split_once(':')?;
    let allowed = ["http", "https", "mailto"]
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s));
    (allowed && !rest.trim().is_empty()).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_and_mail_addresses_leave_the_app() {
        assert_eq!(
            external(" https://example.com/a "),
            Some("https://example.com/a")
        );
        assert_eq!(external("HTTP://example.com"), Some("HTTP://example.com"));
        assert_eq!(external("mailto:a@b.c"), Some("mailto:a@b.c"));
        for refused in [
            "",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https:",
            "Sheet1!A1",
            "https://a\nb",
        ] {
            assert_eq!(external(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn queued_addresses_are_taken_once() {
        queue("https://a.example");
        queue("https://b.example");
        assert_eq!(take(), ["https://a.example", "https://b.example"]);
        assert!(take().is_empty());
    }
}
