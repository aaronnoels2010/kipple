// SPDX-License-Identifier: GPL-3.0-or-later
//! Navigation guard (ADR-0007): the webview may only ever show the app's own pages.
//! Web and mail links open in the user's default app instead; everything else is blocked.

use tauri::Url;

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    OpenExternally,
    Block,
}

/// How the app's own pages are served on this OS.
#[derive(Debug, Clone, Copy)]
pub enum Platform {
    /// WebView2 can't serve custom schemes: `http://tauri.localhost`.
    Windows,
    /// WKWebView and WebKitGTK: `tauri://localhost`.
    Other,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Other
        }
    }
}

/// Decides what to do with a navigation to `url`. `dev_url` is the Vite dev server, only
/// passed in development builds.
pub fn decide(url: &Url, dev_url: Option<&Url>, platform: Platform) -> Decision {
    if is_own_page(url, platform) || dev_url.is_some_and(|dev| dev.origin() == url.origin()) {
        return Decision::Allow;
    }
    match url.scheme() {
        "http" | "https" | "mailto" => Decision::OpenExternally,
        _ => Decision::Block,
    }
}

/// Exact scheme, host and (absent) port of the one origin Tauri serves on this OS. Anything
/// else, e.g. `http://tauri.localhost:8080`, would reach a real local server.
fn is_own_page(url: &Url, platform: Platform) -> bool {
    let (scheme, host) = match platform {
        Platform::Windows => ("http", "tauri.localhost"),
        Platform::Other => ("tauri", "localhost"),
    };
    let own = url.scheme() == scheme && url.host_str() == Some(host) && url.port().is_none();
    // WebKit loads about:blank for empty subframes; it carries no content of its own.
    own || url.as_str() == "about:blank"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(platform: Platform, url: &str) -> Decision {
        decide(&Url::parse(url).unwrap(), None, platform)
    }

    #[test]
    fn own_pages_are_allowed_per_platform() {
        assert_eq!(
            on(Platform::Other, "tauri://localhost/index.html"),
            Decision::Allow
        );
        assert_eq!(
            on(Platform::Windows, "http://tauri.localhost/index.html"),
            Decision::Allow
        );
        assert_eq!(on(Platform::Other, "about:blank"), Decision::Allow);
    }

    #[test]
    fn the_other_platforms_origin_is_not_ours() {
        // On macOS/Linux, http://tauri.localhost is just a loopback host, not the app.
        assert_eq!(
            on(Platform::Other, "http://tauri.localhost/"),
            Decision::OpenExternally
        );
        assert_eq!(on(Platform::Windows, "tauri://localhost/"), Decision::Block);
    }

    #[test]
    fn ports_make_it_a_different_origin() {
        assert_eq!(
            on(Platform::Windows, "http://tauri.localhost:8080/"),
            Decision::OpenExternally
        );
        assert_eq!(
            on(Platform::Other, "tauri://localhost:1234/"),
            Decision::Block
        );
    }

    #[test]
    fn look_alike_hosts_are_not_own_pages() {
        assert_eq!(
            on(Platform::Windows, "https://tauri.localhost.evil.example/"),
            Decision::OpenExternally
        );
        assert_eq!(
            on(Platform::Windows, "https://tauri.localhost/"),
            Decision::OpenExternally
        );
        assert_eq!(
            on(Platform::Other, "tauri://evil.example/"),
            Decision::Block
        );
        assert_eq!(
            on(Platform::Other, "tauri://user@localhost.evil/"),
            Decision::Block
        );
    }

    #[test]
    fn web_and_mail_links_open_externally() {
        for platform in [Platform::Windows, Platform::Other] {
            assert_eq!(
                on(platform, "https://github.com/aaronnoels2010/kipple"),
                Decision::OpenExternally
            );
            assert_eq!(
                on(platform, "mailto:someone@example.com"),
                Decision::OpenExternally
            );
        }
    }

    #[test]
    fn dangerous_schemes_are_blocked() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<script>1</script>",
            "ftp://example.com/x",
        ] {
            assert_eq!(on(Platform::Other, url), Decision::Block, "{url}");
            assert_eq!(on(Platform::Windows, url), Decision::Block, "{url}");
        }
    }

    #[test]
    fn dev_server_is_allowed_only_when_given() {
        let dev = Url::parse("http://localhost:1420").unwrap();
        let page = Url::parse("http://localhost:1420/src/main.tsx").unwrap();
        assert_eq!(decide(&page, Some(&dev), Platform::Other), Decision::Allow);
        assert_eq!(
            decide(&page, None, Platform::Other),
            Decision::OpenExternally
        );
        let other_port = Url::parse("http://localhost:9999/").unwrap();
        assert_eq!(
            decide(&other_port, Some(&dev), Platform::Other),
            Decision::OpenExternally
        );
    }
}
