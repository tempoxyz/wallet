//! Shared authentication and browser utilities.

/// Attempt to open a URL in the default browser.
/// Prints a fallback message if it fails.
pub(crate) fn try_open_browser(url: &str) {
    if let Err(e) = webbrowser::open(url) {
        eprintln!("Failed to open browser: {e}");
        eprintln!("Open this URL manually: {url}");
    }
}
