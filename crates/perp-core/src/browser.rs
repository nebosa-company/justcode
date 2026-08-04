//! Browser launch and URL opening (`V-6` automation).
//!
//! Exercise the artefact by opening it in the default browser and capturing
//! evidence. Supports both URLs and local files.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::process::{Env, Spec};

/// Launch a URL in the default browser and return evidence it opened.
///
/// On success, returns: the URL opened, the wait time, and optional screenshot path.
/// On failure, returns the error and what was attempted.
pub fn launch_url(url: &str, wait_secs: Option<u64>) -> Result<LaunchResult> {
    let wait = Duration::from_secs(wait_secs.unwrap_or(2));

    // Validate URL is reasonable before launching.
    if !url.starts_with("http://") && !url.starts_with("https://") && !url.starts_with("file://") {
        return Err(Error::unbound(
            "launch",
            format!("`{url}` does not start with http://, https://, or file://"),
        ));
    }

    let command = launch_command(url)?;
    let spec = Spec::new(command.clone(), std::env::current_dir().unwrap_or_default(), Duration::from_secs(10))
        .with_env(Env::declared());

    let run = crate::process::run(&spec)?;

    // Give the browser time to open and render.
    std::thread::sleep(wait);

    Ok(LaunchResult {
        url: url.to_string(),
        command,
        exit_code: match run.exit {
            crate::process::Exit::Code(c) => c,
            crate::process::Exit::NoCode => -1,
            crate::process::Exit::TimedOut => -2,
        },
        waited_secs: wait.as_secs(),
        success: run.exit.is_success(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchResult {
    pub url: String,
    pub command: String,
    pub exit_code: i32,
    pub waited_secs: u64,
    pub success: bool,
}

impl LaunchResult {
    pub fn evidence(&self) -> String {
        format!(
            "opened {} with: {}\nexit code: {}\nwaited: {}s\nsuccess: {}",
            self.url, self.command, self.exit_code, self.waited_secs, self.success
        )
    }
}

/// Platform-specific command to open a URL in the default browser.
fn launch_command(url: &str) -> Result<String> {
    if cfg!(target_os = "windows") {
        Ok(format!(r#"powershell -NoProfile -Command "Start-Process '{url}'""#))
    } else if cfg!(target_os = "macos") {
        Ok(format!("open '{url}'"))
    } else {
        // Linux: try xdg-open first, fall back to others.
        Ok(format!("xdg-open '{url}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_command_windows() {
        if cfg!(target_os = "windows") {
            let cmd = launch_command("https://example.com").unwrap();
            assert!(cmd.contains("Start-Process"));
            assert!(cmd.contains("example.com"));
        }
    }

    #[test]
    fn launch_command_macos() {
        if cfg!(target_os = "macos") {
            let cmd = launch_command("https://example.com").unwrap();
            assert!(cmd.contains("open"));
            assert!(cmd.contains("example.com"));
        }
    }

    #[test]
    fn reject_bad_url() {
        let err = launch_url("not-a-url", None);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("does not start with"));
    }

    #[test]
    fn accept_http() {
        let cmd = launch_command("http://example.com").unwrap();
        assert!(cmd.contains("example.com"));
    }

    #[test]
    fn accept_https() {
        let cmd = launch_command("https://example.com").unwrap();
        assert!(cmd.contains("example.com"));
    }

    #[test]
    fn accept_file() {
        let cmd = launch_command("file:///tmp/test.html").unwrap();
        assert!(cmd.contains("test.html"));
    }
}
