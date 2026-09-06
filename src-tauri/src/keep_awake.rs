use parking_lot::Mutex;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn};

/// Prevents idle sleep and lid-close sleep while Cloud is on.
///
/// A sleeping Mac cannot serve files. This holds `caffeinate` and, with one
/// administrator prompt, `pmset disablesleep` so the lid can close on battery.
pub struct KeepAwake {
    child: Mutex<Option<Child>>,
    we_disabled_sleep: AtomicBool,
}

impl KeepAwake {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            we_disabled_sleep: AtomicBool::new(false),
        }
    }

    pub fn start(&self) {
        self.stop_caffeinate();
        match Command::new("/usr/bin/caffeinate")
            .args(["-i", "-m", "-s"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => {
                *self.child.lock() = Some(child);
                info!("Keeping this Mac awake while cloud access is on");
            }
            Err(error) => warn!("Could not start caffeinate: {error}"),
        }
        match apply_sleep_disabled(true) {
            Ok(true) => {
                self.we_disabled_sleep.store(true, Ordering::SeqCst);
                info!("Lid-close sleep disabled while cloud access is on");
            }
            Ok(false) => {}
            Err(error) => warn!("{error}"),
        }
    }

    pub fn stop(&self) {
        self.stop_caffeinate();
        if !self.we_disabled_sleep.swap(false, Ordering::SeqCst) && !sleep_is_disabled() {
            return;
        }
        if let Err(error) = apply_sleep_disabled(false) {
            warn!("{error}");
        }
    }

    fn stop_caffeinate(&self) {
        let mut slot = self.child.lock();
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        self.stop_caffeinate();
        if self.we_disabled_sleep.load(Ordering::SeqCst) {
            let _ = run_pmset(false, false);
        }
    }
}

fn sleep_is_disabled() -> bool {
    let Ok(output) = Command::new("/usr/bin/pmset").arg("-g").output() else {
        return false;
    };
    parse_sleep_disabled(&String::from_utf8_lossy(&output.stdout)).unwrap_or(false)
}

fn parse_sleep_disabled(text: &str) -> Option<bool> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("SleepDisabled") {
            return Some(rest.trim() != "0");
        }
    }
    None
}

/// Returns Ok(true) when lid-close sleep is disabled afterward.
fn apply_sleep_disabled(disabled: bool) -> Result<bool, String> {
    if sleep_is_disabled() == disabled {
        return Ok(disabled);
    }
    if run_pmset(disabled, false).is_ok() && sleep_is_disabled() == disabled {
        return Ok(disabled);
    }
    run_pmset(disabled, true)?;
    if sleep_is_disabled() == disabled {
        Ok(disabled)
    } else {
        Err("Could not change lid-close sleep. Approve the macOS password prompt when Cloud is enabled.".into())
    }
}

fn run_pmset(disabled: bool, privileged: bool) -> Result<(), String> {
    let flag = if disabled { "1" } else { "0" };
    let status = if privileged {
        let script = format!(
            r#"do shell script "pmset -a disablesleep {flag}" with administrator privileges"#
        );
        Command::new("/usr/bin/osascript")
            .args(["-e", &script])
            .status()
            .map_err(|e| format!("Could not ask for administrator access: {e}"))?
    } else {
        Command::new("/usr/bin/pmset")
            .args(["-a", "disablesleep", flag])
            .status()
            .map_err(|e| format!("Could not update sleep settings: {e}"))?
    };
    if status.success() {
        Ok(())
    } else {
        Err("Could not update sleep settings.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_sleep_disabled;

    #[test]
    fn reads_sleep_disabled_flag() {
        assert_eq!(
            parse_sleep_disabled(" hibernatemode        3\n SleepDisabled         1\n"),
            Some(true)
        );
        assert_eq!(parse_sleep_disabled(" SleepDisabled         0"), Some(false));
        assert_eq!(parse_sleep_disabled(" standbydelay 10800"), None);
    }
}
