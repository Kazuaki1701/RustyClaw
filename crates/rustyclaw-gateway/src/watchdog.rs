use std::time::Duration;
use tokio::time;

pub struct WatchdogService;

impl WatchdogService {
    /// Background task that notifies systemd watchdog every 30 seconds
    pub fn start() {
        tokio::spawn(async {
            // Check if running under systemd
            if std::env::var("NOTIFY_SOCKET").is_err() {
                tracing::debug!(
                    "WatchdogService: NOTIFY_SOCKET environment variable not set. Not running under systemd. Watchdog notifications skipped."
                );
                return;
            }

            tracing::info!(
                "WatchdogService: Systemd watchdog detected. Starting notification task every 30s..."
            );
            let mut interval = time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                if let Err(e) = sd_notify::notify(false, &[sd_notify::NotifyState::Watchdog]) {
                    tracing::error!(
                        "WatchdogService: Failed to send watchdog notification to systemd: {:#}",
                        e
                    );
                } else {
                    tracing::debug!("WatchdogService: Watchdog tick sent successfully.");
                }
            }
        });
    }
}
