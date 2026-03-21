use anyhow::Result;
use std::process::Command;

/// Send a native OS notification.
///
/// Uses `notify-send` on Linux or `osascript` on macOS.
/// Returns Ok(()) if the notification was sent successfully, Err if the command failed or is unavailable.
pub fn send_notification(title: &str, body: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let status = Command::new("notify-send").arg(title).arg(body).status();

        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => anyhow::bail!("notify-send exited with status: {}", s),
            Err(e) => anyhow::bail!("Failed to run notify-send: {}", e),
        }
    }

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            r#"display notification "{}" with title "{}""#,
            body.replace('"', "\\\""),
            title.replace('"', "\\\"")
        );

        let status = Command::new("osascript").arg("-e").arg(&script).status();

        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => anyhow::bail!("osascript exited with status: {}", s),
            Err(e) => anyhow::bail!("Failed to run osascript: {}", e),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        anyhow::bail!("OS notifications are not supported on this platform")
    }
}
