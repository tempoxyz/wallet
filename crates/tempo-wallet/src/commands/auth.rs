//! Shared authentication and browser utilities.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserLaunchStatus {
    Opened,
    Skipped,
    Failed,
}

/// Attempt to open a URL in the default browser.
/// Prints a fallback message if it fails.
fn try_open_browser_with(
    url: &str,
    no_browser: bool,
    opener: impl FnOnce(&str) -> Result<(), String>,
) -> BrowserLaunchStatus {
    if no_browser {
        return BrowserLaunchStatus::Skipped;
    }
    match opener(url) {
        Ok(_) => BrowserLaunchStatus::Opened,
        Err(err) => {
            eprintln!("Failed to open browser: {err}");
            eprintln!("Open this URL manually: {url}");
            BrowserLaunchStatus::Failed
        }
    }
}

pub(crate) fn try_open_browser(url: &str, no_browser: bool) -> BrowserLaunchStatus {
    try_open_browser_with(url, no_browser, |url| {
        webbrowser::open(url).map_err(|err| err.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::{try_open_browser_with, BrowserLaunchStatus};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn browser_launch_is_skipped_when_no_browser_is_true() {
        let status = try_open_browser_with(
            "https://wallet.tempo.xyz/cli-auth?code=ANMGE375",
            true,
            |_url| Err("should not be called".to_string()),
        );
        assert_eq!(status, BrowserLaunchStatus::Skipped);
    }

    #[test]
    fn browser_launch_attempts_open_when_no_browser_is_false() {
        let seen_url = Rc::new(RefCell::new(None::<String>));
        let captured_url = Rc::clone(&seen_url);
        let status = try_open_browser_with(
            "https://wallet.tempo.xyz/cli-auth?code=ANMGE375",
            false,
            |url| {
                *captured_url.borrow_mut() = Some(url.to_string());
                Err("synthetic open failure".to_string())
            },
        );
        assert_eq!(status, BrowserLaunchStatus::Failed);
        assert_eq!(
            seen_url.borrow().as_deref(),
            Some("https://wallet.tempo.xyz/cli-auth?code=ANMGE375")
        );
    }
}
