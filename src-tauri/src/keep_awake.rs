use parking_lot::Mutex;
use std::process::{Child, Command, Stdio};
use tracing::{info, warn};

/// Process-scoped `caffeinate` flags used while Cloud is on.
///
/// None of these require administrator privileges:
/// - `-i` prevent idle sleep
/// - `-m` prevent disk idle sleep (keeps the disk available for serving files)
/// - `-s` prevent system sleep while on AC power
///
/// `-d` (display) is omitted so the screen can still sleep. These assertions
/// do not prevent closed-lid sleep on battery. That needs a one-time OS Energy
/// setting, not an in-app `pmset disablesleep` prompt.
const CAFFEINATE_ARGS: &[&str] = &["-i", "-m", "-s"];

/// Prevents idle sleep while Cloud is on, without an administrator prompt.
///
/// A sleeping Mac cannot serve the Cloudflare tunnel. This holds a
/// process-scoped `caffeinate -ims` child for the Cloud session. Start, stop,
/// and drop all kill that child. Closed-lid sleep on battery is out of scope.
pub struct KeepAwake {
    child: Mutex<Option<Child>>,
}

impl KeepAwake {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
        }
    }

    pub fn start(&self) {
        self.stop_caffeinate();
        match Command::new("/usr/bin/caffeinate")
            .args(CAFFEINATE_ARGS)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => {
                *self.child.lock() = Some(child);
                info!("Keeping this Mac awake while cloud access is on (caffeinate -ims)");
            }
            Err(error) => warn!("Could not start caffeinate: {error}"),
        }
    }

    pub fn stop(&self) {
        self.stop_caffeinate();
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
    }
}

#[cfg(test)]
mod tests {
    use super::{KeepAwake, CAFFEINATE_ARGS};

    #[test]
    fn caffeinate_flags_are_unprivileged() {
        assert_eq!(CAFFEINATE_ARGS, &["-i", "-m", "-s"]);
        for flag in CAFFEINATE_ARGS {
            assert!(
                matches!(*flag, "-i" | "-m" | "-s"),
                "unexpected caffeinate flag: {flag}"
            );
        }
    }

    #[test]
    fn start_stop_and_drop_clean_up() {
        let keep = KeepAwake::new();
        keep.start();
        keep.stop();
        keep.start();
        drop(keep);
    }
}
