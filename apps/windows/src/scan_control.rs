use std::future::Future;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ScanStatus {
    stopping_for_update: bool,
    active_scans: usize,
}

/// Coordinates CLI scans with the Windows updater and blocks new scans while
/// the updater is preparing to replace the bundled executable.
#[derive(Clone, Debug)]
pub struct ScanControl {
    state: watch::Sender<ScanStatus>,
}

impl Default for ScanControl {
    fn default() -> Self {
        let (state, _) = watch::channel(ScanStatus::default());
        Self { state }
    }
}

impl ScanControl {
    pub fn begin_scan(&self) -> Result<(ScanPermit, ScanCancellation), String> {
        let cancellation = self.state.subscribe();
        let mut started = false;
        self.state.send_modify(|current| {
            if !current.stopping_for_update {
                current.active_scans += 1;
                started = true;
            }
        });
        if !started {
            return Err("Scan annulé pour préparer la mise à jour.".to_string());
        }
        Ok((
            ScanPermit {
                state: self.state.clone(),
            },
            ScanCancellation {
                state: cancellation,
            },
        ))
    }

    pub fn is_stopping_for_update(&self) -> bool {
        self.state.borrow().stopping_for_update
    }

    pub async fn run<T, F>(&self, future: F) -> Result<T, String>
    where
        F: Future<Output = T>,
    {
        let (_permit, mut cancellation) = self.begin_scan()?;
        tokio::pin!(future);
        loop {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    return Err("Scan annulé pour préparer la mise à jour.".to_string());
                }
                result = &mut future => return Ok(result),
            }
        }
    }

    /// Blocks new scans, cancels existing scan futures, and returns once their
    /// futures have dropped. It does not wait for scans to finish normally.
    pub async fn stop_for_update(&self) {
        self.state
            .send_modify(|current| current.stopping_for_update = true);
        let mut state = self.state.subscribe();
        loop {
            let active = state.borrow().active_scans;
            if active == 0 {
                return;
            }
            if state.changed().await.is_err() {
                return;
            }
        }
    }

    pub fn resume_after_cancelled_update(&self) {
        self.state
            .send_modify(|current| current.stopping_for_update = false);
    }
}

pub struct ScanPermit {
    state: watch::Sender<ScanStatus>,
}

pub struct ScanCancellation {
    state: watch::Receiver<ScanStatus>,
}

impl ScanCancellation {
    pub async fn cancelled(&mut self) {
        loop {
            if self.state.borrow().stopping_for_update {
                return;
            }
            if self.state.changed().await.is_err() {
                return;
            }
        }
    }
}

impl Drop for ScanPermit {
    fn drop(&mut self) {
        self.state.send_modify(|current| {
            current.active_scans = current.active_scans.saturating_sub(1);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn update_stop_cancels_active_scan_without_waiting_for_its_work() {
        let control = ScanControl::default();
        let scan_control = control.clone();
        let scan =
            tokio::spawn(async move { scan_control.run(std::future::pending::<()>()).await });
        tokio::task::yield_now().await;

        tokio::time::timeout(Duration::from_millis(100), control.stop_for_update())
            .await
            .expect("update preparation should cancel, not finish, a scan");
        assert!(scan.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn no_scan_can_start_while_update_preparation_is_active() {
        let control = ScanControl::default();
        control.stop_for_update().await;

        assert!(control.run(async {}).await.is_err());
    }

    #[tokio::test]
    async fn cancelling_update_reopens_scan_admission() {
        let control = ScanControl::default();
        control.stop_for_update().await;
        control.resume_after_cancelled_update();

        assert_eq!(control.run(async { 42 }).await.unwrap(), 42);
    }
}
