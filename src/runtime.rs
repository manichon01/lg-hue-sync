use crate::web::ControlCommand;
use crate::web::LiveSettings;
use std::time::Duration;
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineState {
    Running,
    Paused,
}

/// Shortest wait before an app whose capture failed is tried again after a video change.
const APP_RETRY_MIN: std::time::Duration = std::time::Duration::from_secs(3);
/// Retry an app capture at least this often while it stays in the foreground.
const APP_RETRY_FALLBACK: std::time::Duration = std::time::Duration::from_secs(30);

pub struct CaptureSourcePolicy {
    compatible: Option<bool>,
    display_off: bool,
    source_paused: bool,
    manually_stopped: bool,
    app_failed: bool,
    app_failed_at: Option<std::time::Instant>,
    app_failed_video: Option<Option<String>>,
    pattern_active: bool,
}

impl CaptureSourcePolicy {
    pub fn new(compatible: Option<bool>, requested: bool) -> Self {
        Self {
            compatible,
            display_off: false,
            source_paused: compatible == Some(false) && requested,
            manually_stopped: false,
            app_failed: false,
            app_failed_at: None,
            app_failed_video: None,
            pattern_active: false,
        }
    }

    /// A calibration pattern drives the lights without capture, so no source blocks it.
    pub fn observe_pattern(&mut self, active: bool) {
        self.pattern_active = active;
    }

    /// An app could not be captured (e.g. its video had not started): wait, then retry.
    pub fn app_capture_failed(&mut self) {
        self.app_failed = true;
        self.app_failed_at = Some(std::time::Instant::now());
        self.app_failed_video = None;
    }

    fn clear_app_failure(&mut self) {
        self.app_failed = false;
        self.app_failed_at = None;
        self.app_failed_video = None;
    }

    /// While an app is paused for a failed capture, retry once the TV's video status changes
    /// (playback started) or after a slow fallback interval, instead of waiting for HDMI.
    pub fn observe_video(&mut self, video: &Option<serde_json::Value>) {
        let Some(failed_at) = self.app_failed_at.filter(|_| self.app_failed) else {
            return;
        };
        let current = video.as_ref().map(|value| value.to_string());
        let elapsed = failed_at.elapsed();
        let changed = match &self.app_failed_video {
            None => {
                self.app_failed_video = Some(current);
                false
            }
            Some(previous) => *previous != current,
        };
        if (changed && elapsed >= APP_RETRY_MIN) || elapsed >= APP_RETRY_FALLBACK {
            self.clear_app_failure();
        }
    }

    pub fn observe_source(&mut self, source: crate::capture_source::CaptureSource) {
        if !matches!(source, crate::capture_source::CaptureSource::App | crate::capture_source::CaptureSource::Unknown) {
            self.clear_app_failure();
        }
        self.observe(source.compatible());
    }

    pub fn observe(&mut self, compatible: Option<bool>) {
        if compatible.is_some() {
            self.compatible = compatible;
        }
    }

    pub fn manual_request(&mut self, requested: Option<bool>) {
        if let Some(requested) = requested {
            self.manually_stopped = !requested;
            if requested {
                self.clear_app_failure();
            }
            if !requested {
                self.source_paused = false;
            } else if self.blocked() {
                self.source_paused = true;
            }
        }
    }

    pub fn observe_display(&mut self, off: bool) {
        self.display_off = off;
    }

    pub fn blocked(&self) -> bool {
        self.display_off
            || (!self.pattern_active && (self.app_failed || self.compatible == Some(false)))
    }
    pub fn manually_stopped(&self) -> bool {
        self.manually_stopped
    }
    pub fn source_paused(&self) -> bool {
        self.source_paused
    }
    pub fn pause_for_source(&mut self) {
        if !self.manually_stopped {
            self.source_paused = true;
        }
    }
    pub fn auto_resume(&self) -> bool {
        self.source_paused && !self.blocked() && !self.manually_stopped
    }
    pub fn resumed(&mut self) {
        self.source_paused = false;
    }
}

#[derive(Default)]
pub struct PendingCommands {
    pub desired_running: Option<bool>,
    pub manual_running: Option<bool>,
    pub restart: bool,
    pub reconfigure: bool,
    pub sync_bridge: bool,
    pub save_config: Vec<oneshot::Sender<Result<(), String>>>,
    pub apply_settings: Option<LiveSettings>,
}

impl PendingCommands {
    pub fn shutdown(&mut self, receiver: mpsc::Receiver<ControlCommand>) {
        // Drop queued and pending reply senders before waiting for HTTP handlers.
        drop(receiver);
        self.save_config.clear();
    }
    pub fn receive(&mut self, receiver: &mut mpsc::Receiver<ControlCommand>) {
        while let Ok(command) = receiver.try_recv() {
            match command {
                ControlCommand::Start => {
                    self.desired_running = Some(true);
                    self.manual_running = Some(true);
                }
                ControlCommand::Stop => {
                    self.desired_running = Some(false);
                    self.manual_running = Some(false);
                }
                ControlCommand::Restart => self.restart = true,
                ControlCommand::Reconfigure => self.reconfigure = true,
                ControlCommand::SyncBridge => self.sync_bridge = true,
                ControlCommand::SaveConfig(reply) => self.save_config.push(reply),
                ControlCommand::ApplySettings(settings) => self.apply_settings = Some(settings),
            }
        }
    }

    pub fn has_management_action(&self) -> bool {
        self.restart
            || self.reconfigure
            || self.sync_bridge
            || !self.save_config.is_empty()
            || self.apply_settings.is_some()
    }

    pub fn take_stop_if_running(&mut self, state: PipelineState) -> bool {
        state == PipelineState::Running && self.desired_running.take() == Some(false)
    }

    pub fn resume_fault_for_restart(&mut self, faulted: bool) {
        if faulted && self.restart && self.desired_running != Some(false) {
            self.desired_running = Some(true);
        }
    }

    pub fn capture_restart_cancelled(&self) -> bool {
        self.desired_running == Some(false) || self.reconfigure
    }
}

pub async fn wait_for_capture_retry(
    delay: Duration,
    shutdown: &tokio_util::sync::CancellationToken,
) -> bool {
    tokio::select! {
        biased;
        _ = shutdown.cancelled() => false,
        _ = tokio::time::sleep(delay) => true,
    }
}

pub struct RetryState {
    failures: u32,
    next_attempt: Instant,
}

impl RetryState {
    pub fn new() -> Self {
        Self {
            failures: 0,
            next_attempt: Instant::now(),
        }
    }

    pub fn ready(&self) -> bool {
        Instant::now() >= self.next_attempt
    }

    pub fn success(&mut self) {
        self.failures = 0;
        self.next_attempt = Instant::now();
    }

    pub fn failure(&mut self) -> Duration {
        self.failures = self.failures.saturating_add(1);
        let delay = Duration::from_millis(250 * (1u64 << self.failures.min(6)));
        self.next_attempt = Instant::now() + delay;
        delay
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn display_pause_resumes_without_overriding_manual_stop() {
        let mut policy = CaptureSourcePolicy::new(Some(true), true);
        policy.observe_display(true);
        assert!(policy.blocked());
        policy.pause_for_source();
        assert!(!policy.auto_resume());
        policy.observe_display(false);
        assert!(policy.auto_resume());
        policy.resumed();
        policy.manual_request(Some(false));
        policy.observe_display(true);
        policy.pause_for_source();
        policy.observe_display(false);
        assert!(!policy.auto_resume());
        policy.manual_request(Some(true));
        policy.observe(Some(false));
        assert!(policy.blocked());
    }

    use super::*;

    #[tokio::test]
    async fn shutdown_interrupts_capture_recovery_before_driver_recreation() {
        let shutdown = tokio_util::sync::CancellationToken::new();
        let cancel = shutdown.clone();
        let (may_recreate, ()) = tokio::join!(
            wait_for_capture_retry(Duration::from_secs(30), &shutdown),
            async move {
                tokio::task::yield_now().await;
                cancel.cancel();
            }
        );
        assert!(!may_recreate);
        assert!(!wait_for_capture_retry(Duration::ZERO, &shutdown).await);
        assert!(
            wait_for_capture_retry(Duration::ZERO, &tokio_util::sync::CancellationToken::new())
                .await
        );
    }

    #[tokio::test]
    async fn shutdown_releases_queued_sends_and_all_save_acknowledgements() {
        let (sender, mut receiver) = mpsc::channel(1);
        let (pending_reply, pending_result) = oneshot::channel();
        sender
            .send(ControlCommand::SaveConfig(pending_reply))
            .await
            .unwrap();
        let mut pending = PendingCommands::default();
        pending.receive(&mut receiver);
        let (queued_reply, queued_result) = oneshot::channel();
        sender
            .send(ControlCommand::SaveConfig(queued_reply))
            .await
            .unwrap();
        let blocked = sender.send(ControlCommand::Start);
        tokio::pin!(blocked);
        assert!(tokio::time::timeout(Duration::from_millis(5), &mut blocked)
            .await
            .is_err());
        pending.shutdown(receiver);
        assert!(blocked.await.is_err());
        assert!(pending_result.await.is_err());
        assert!(queued_result.await.is_err());
    }

    #[tokio::test]
    async fn latest_start_stop_command_wins_without_losing_other_actions() {
        let (sender, mut receiver) = mpsc::channel(4);
        sender.send(ControlCommand::Start).await.unwrap();
        let (reply, _result) = oneshot::channel();
        sender
            .send(ControlCommand::SaveConfig(reply))
            .await
            .unwrap();
        sender.send(ControlCommand::Stop).await.unwrap();

        let mut pending = PendingCommands::default();
        pending.receive(&mut receiver);

        assert_eq!(pending.desired_running, Some(false));
        assert_eq!(pending.save_config.len(), 1);
        assert!(pending.has_management_action());
    }

    #[tokio::test]
    async fn concurrent_save_requests_keep_each_acknowledgement() {
        let (sender, mut receiver) = mpsc::channel(2);
        let (first_reply, first_result) = oneshot::channel();
        let (second_reply, second_result) = oneshot::channel();
        sender
            .send(ControlCommand::SaveConfig(first_reply))
            .await
            .unwrap();
        sender
            .send(ControlCommand::SaveConfig(second_reply))
            .await
            .unwrap();

        let mut pending = PendingCommands::default();
        pending.receive(&mut receiver);
        assert_eq!(pending.save_config.len(), 2);
        for reply in std::mem::take(&mut pending.save_config) {
            reply.send(Ok(())).unwrap();
        }
        assert_eq!(first_result.await.unwrap(), Ok(()));
        assert_eq!(second_result.await.unwrap(), Ok(()));
    }

    #[test]
    fn start_and_stop_alone_do_not_interrupt_paused_management() {
        let mut pending = PendingCommands {
            desired_running: Some(false),
            ..PendingCommands::default()
        };
        assert!(!pending.has_management_action());
        pending.reconfigure = true;
        assert!(pending.has_management_action());
    }

    #[tokio::test]
    async fn fault_restart_resumes_without_overriding_an_explicit_stop() {
        let mut pending = PendingCommands {
            restart: true,
            ..PendingCommands::default()
        };
        pending.resume_fault_for_restart(false);
        assert_eq!(pending.desired_running, None);
        pending.resume_fault_for_restart(true);
        assert_eq!(pending.desired_running, Some(true));
        assert!(!pending.capture_restart_cancelled());
        let (sender, mut receiver) = mpsc::channel(1);
        sender.send(ControlCommand::Stop).await.unwrap();
        // Commands received during the restart delay must cancel capture recreation too.
        pending.receive(&mut receiver);
        assert!(pending.capture_restart_cancelled());
        pending.desired_running = Some(false);
        pending.resume_fault_for_restart(true);
        assert_eq!(pending.desired_running, Some(false));
        assert!(pending.capture_restart_cancelled());
        pending.desired_running = None;
        pending.reconfigure = true;
        assert!(pending.capture_restart_cancelled());
    }

    #[test]
    fn paused_start_request_survives_outer_command_check() {
        let mut pending = PendingCommands {
            desired_running: Some(true),
            ..PendingCommands::default()
        };
        assert!(!pending.take_stop_if_running(PipelineState::Paused));
        assert_eq!(pending.desired_running, Some(true));
    }

    #[test]
    fn retry_delay_is_bounded_and_resets() {
        let mut retry = RetryState::new();
        assert_eq!(retry.failure(), Duration::from_millis(500));
        for _ in 0..20 {
            retry.failure();
        }
        assert_eq!(retry.failure(), Duration::from_secs(16));
        retry.success();
        assert!(retry.ready());
        assert_eq!(retry.failure(), Duration::from_millis(500));
    }

    #[test]
    fn source_pause_retains_unknown_policy_and_resumes_hdmi_without_rearming_faults() {
        let mut policy = CaptureSourcePolicy::new(Some(false), true);
        assert!(policy.blocked());
        assert!(policy.source_paused());
        assert!(!policy.auto_resume());
        policy.observe(None);
        assert!(policy.blocked());
        policy.observe(Some(true));
        assert!(policy.auto_resume());
        policy.resumed();
        assert!(!policy.auto_resume());
        policy.observe(None);
        assert!(!policy.blocked());
        policy.observe(Some(false));
        assert!(!policy.auto_resume());
        policy.pause_for_source();
        policy.observe(Some(true));
        assert!(policy.auto_resume());
        policy.resumed();
        // A later capture fault or Hue/TV pause is not a source pause.
        assert!(!policy.auto_resume());
    }

    #[tokio::test]
    async fn manual_stop_and_queued_restart_cannot_reinitialize_an_unsupported_source() {
        let mut policy = CaptureSourcePolicy::new(Some(false), true);
        let (sender, mut receiver) = mpsc::channel(2);
        sender.send(ControlCommand::Restart).await.unwrap();
        sender.send(ControlCommand::Stop).await.unwrap();
        let mut pending = PendingCommands::default();
        pending.receive(&mut receiver);
        policy.manual_request(pending.manual_running.take());
        assert!(pending.restart);
        assert!(policy.blocked());
        assert!(policy.manually_stopped());
        assert!(!policy.source_paused());
        assert!(pending.capture_restart_cancelled());
        policy.observe(Some(true));
        assert!(!policy.auto_resume());
        policy.observe(Some(false));
        sender.send(ControlCommand::Start).await.unwrap();
        pending.receive(&mut receiver);
        policy.manual_request(pending.manual_running.take());
        assert!(policy.blocked());
        assert!(policy.source_paused());
        assert!(!policy.manually_stopped());
        assert!(!policy.auto_resume());
        policy.observe(Some(true));
        assert!(policy.auto_resume());
    }

    #[test]
    fn failed_app_capture_retries_on_video_change_or_after_the_fallback_interval() {
        use crate::capture_source::CaptureSource;
        let video = |value: i64| Some(serde_json::json!({ "width": value }));
        let mut policy = CaptureSourcePolicy::new(Some(true), true);
        policy.observe_source(CaptureSource::App);
        policy.app_capture_failed();
        assert!(policy.blocked());
        // Same video status: stays paused. A change right away is held back by the minimum wait.
        policy.observe_video(&video(0));
        policy.observe_video(&video(1920));
        assert!(policy.blocked());
        // A change after the minimum wait retries.
        policy.app_failed_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(4));
        policy.observe_video(&video(1920));
        assert!(!policy.blocked());
        // No change at all still retries after the fallback interval.
        policy.app_capture_failed();
        policy.observe_video(&video(1920));
        policy.app_failed_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(31));
        policy.observe_video(&video(1920));
        assert!(!policy.blocked());
        // Leaving the app clears the failure.
        policy.app_capture_failed();
        policy.observe_source(CaptureSource::Hdmi);
        assert!(!policy.blocked());
    }
}
