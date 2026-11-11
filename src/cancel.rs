//! Stops a reading in the background once its result is no longer wanted:
//! a text file's parse checks the flag as it goes, and a workbook's reader
//! process is killed.

use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Default)]
pub struct Cancel(Arc<Inner>);

#[derive(Default)]
struct Inner {
    flag: AtomicBool,
    /// The workbook reader process, while it runs.
    child: Mutex<Option<Child>>,
}

impl Cancel {
    /// Stops the reading: the parse ends at its next check, and the reader
    /// process, if any, is killed now.
    pub fn cancel(&self) {
        self.0.flag.store(true, Ordering::Relaxed);
        let child = self
            .0
            .child
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.flag.load(Ordering::Relaxed)
    }

    pub fn flag(&self) -> &AtomicBool {
        &self.0.flag
    }

    /// Waits for `child` to end, unless the reading is cancelled first or
    /// `timeout` passes (it is then killed): its exit status, `None` once
    /// cancelled, or an error of kind `TimedOut`.
    pub fn wait(
        &self,
        child: Child,
        timeout: std::time::Duration,
    ) -> std::io::Result<Option<std::process::ExitStatus>> {
        let deadline = std::time::Instant::now() + timeout;
        {
            let mut slot = self.0.child.lock().unwrap_or_else(PoisonError::into_inner);
            // Checked under the lock: `cancel` sets the flag before taking
            // the lock, so the child is either killed here or found there.
            if self.is_cancelled() {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
            }
            *slot = Some(child);
        }
        loop {
            {
                let mut slot = self.0.child.lock().unwrap_or_else(PoisonError::into_inner);
                let Some(child) = slot.as_mut() else {
                    // Taken and killed by `cancel`.
                    return Ok(None);
                };
                match child.try_wait() {
                    Ok(Some(status)) => {
                        *slot = None;
                        return Ok(Some(status));
                    }
                    Ok(None) if std::time::Instant::now() < deadline => {}
                    Ok(None) => {
                        if let Some(mut child) = slot.take() {
                            let _ = child.kill();
                            let _ = child.wait();
                        }
                        return Err(std::io::ErrorKind::TimedOut.into());
                    }
                    Err(error) => {
                        if let Some(mut child) = slot.take() {
                            let _ = child.kill();
                            let _ = child.wait();
                        }
                        return Err(error);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: std::time::Duration = std::time::Duration::from_secs(60);

    #[test]
    fn cancelling_sets_the_flag_for_every_copy() {
        let cancel = Cancel::default();
        let copy = cancel.clone();
        assert!(!copy.is_cancelled());
        cancel.cancel();
        assert!(copy.is_cancelled());
        assert!(copy.flag().load(Ordering::Relaxed));
    }

    #[cfg(unix)]
    #[test]
    fn a_cancelled_child_is_killed() {
        let cancel = Cancel::default();
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("sleep");
        let waiter = cancel.clone();
        let started = std::time::Instant::now();
        let thread = std::thread::spawn(move || waiter.wait(child, LONG));
        std::thread::sleep(std::time::Duration::from_millis(50));
        cancel.cancel();
        assert!(matches!(thread.join().expect("waiter"), Ok(None)));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_ends_gives_its_status() {
        let child = std::process::Command::new("true").spawn().expect("true");
        let status = Cancel::default().wait(child, LONG).expect("wait");
        assert!(status.is_some_and(|s| s.success()));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_past_its_time_is_killed() {
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("sleep");
        let started = std::time::Instant::now();
        let waited = Cancel::default().wait(child, std::time::Duration::from_millis(50));
        assert_eq!(
            waited.map_err(|e| e.kind()),
            Err(std::io::ErrorKind::TimedOut)
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
