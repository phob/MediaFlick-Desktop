use std::time::{Duration, Instant};

use cef::{TerminationStatus, sys};

use crate::app::logger;

pub(super) const CRASH_WINDOW: Duration = Duration::from_secs(30);
pub(super) const MAX_AUTO_RELOADS: usize = 2;
pub(super) const REVEAL_DEADLINE_MS: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RendererTerminationKind {
    AbnormalTermination,
    ProcessWasKilled,
    ProcessCrashed,
    ProcessOom,
    LaunchFailed,
    IntegrityFailure,
    Unknown,
}

impl RendererTerminationKind {
    pub(super) fn from_cef(status: TerminationStatus) -> Self {
        match *status.as_ref() {
            sys::cef_termination_status_t::TS_ABNORMAL_TERMINATION => Self::AbnormalTermination,
            sys::cef_termination_status_t::TS_PROCESS_WAS_KILLED => Self::ProcessWasKilled,
            sys::cef_termination_status_t::TS_PROCESS_CRASHED => Self::ProcessCrashed,
            sys::cef_termination_status_t::TS_PROCESS_OOM => Self::ProcessOom,
            sys::cef_termination_status_t::TS_LAUNCH_FAILED => Self::LaunchFailed,
            sys::cef_termination_status_t::TS_INTEGRITY_FAILURE => Self::IntegrityFailure,
            _ => Self::Unknown,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::AbnormalTermination => "Renderer process exited abnormally",
            Self::ProcessWasKilled => "Renderer process was terminated",
            Self::ProcessCrashed => "Renderer process crashed",
            Self::ProcessOom => "Renderer process ran out of memory",
            Self::LaunchFailed => "Renderer process failed to launch",
            Self::IntegrityFailure => "Renderer process failed code integrity checks",
            Self::Unknown => "Renderer process terminated unexpectedly",
        }
    }

    const fn allows_auto_reload(self) -> bool {
        !matches!(self, Self::LaunchFailed | Self::IntegrityFailure)
    }
}

pub(super) fn format_renderer_error_text(
    kind: RendererTerminationKind,
    error_string: Option<&str>,
) -> String {
    match error_string
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(detail) => format!("{} ({})", kind.label(), logger::redact_url_secrets(detail)),
        None => kind.label().to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RendererCrashAction {
    Ignore,
    ReloadApp,
    ShowErrorPage,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RendererCrashDecision {
    pub(super) action: RendererCrashAction,
    pub(super) reveal_window: bool,
}

#[derive(Debug, Default)]
pub(super) struct RendererRecoveryState {
    recent_reloads: Vec<Instant>,
    error_page_attempted_at: Option<Instant>,
}

impl RendererRecoveryState {
    pub(super) fn on_terminated(
        &mut self,
        kind: RendererTerminationKind,
        exiting: bool,
        now: Instant,
    ) -> RendererCrashDecision {
        if exiting {
            return RendererCrashDecision {
                action: RendererCrashAction::Ignore,
                reveal_window: false,
            };
        }

        self.recent_reloads
            .retain(|timestamp| now.saturating_duration_since(*timestamp) < CRASH_WINDOW);
        if self
            .error_page_attempted_at
            .is_some_and(|attempted| now.saturating_duration_since(attempted) >= CRASH_WINDOW)
        {
            self.error_page_attempted_at = None;
        }

        let action = if self.error_page_attempted_at.is_some() {
            RendererCrashAction::Stop
        } else if kind.allows_auto_reload() && self.recent_reloads.len() < MAX_AUTO_RELOADS {
            self.recent_reloads.push(now);
            RendererCrashAction::ReloadApp
        } else {
            self.error_page_attempted_at = Some(now);
            RendererCrashAction::ShowErrorPage
        };

        RendererCrashDecision {
            action,
            reveal_window: true,
        }
    }

    pub(super) fn on_user_retry(&mut self) {
        self.recent_reloads.clear();
        self.error_page_attempted_at = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RevealAction {
    RevealNow,
    DeferUntilReady,
    Skip,
}

#[derive(Debug)]
pub(super) struct WindowRevealState {
    hidden: bool,
    reveal_requested: bool,
    revealed: bool,
    deadline_generation: Option<u64>,
    next_generation: u64,
}

impl WindowRevealState {
    pub(super) const fn new(hidden: bool) -> Self {
        Self {
            hidden,
            reveal_requested: false,
            revealed: false,
            deadline_generation: None,
            next_generation: 0,
        }
    }

    pub(super) fn arm_deadline(&mut self, exiting: bool) -> Option<u64> {
        if self.hidden || self.revealed || self.reveal_requested || exiting {
            return None;
        }
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        self.deadline_generation = Some(generation);
        Some(generation)
    }

    pub(super) fn request_reveal(&mut self, target_available: bool, exiting: bool) -> RevealAction {
        self.deadline_generation = None;
        if exiting || self.hidden || self.revealed {
            return RevealAction::Skip;
        }
        self.reveal_requested = true;
        if target_available {
            self.revealed = true;
            RevealAction::RevealNow
        } else {
            RevealAction::DeferUntilReady
        }
    }

    pub(super) fn on_target_ready(&mut self, exiting: bool) -> RevealAction {
        if exiting || self.hidden || self.revealed || !self.reveal_requested {
            return RevealAction::Skip;
        }
        self.revealed = true;
        self.deadline_generation = None;
        RevealAction::RevealNow
    }

    pub(super) fn on_deadline(
        &mut self,
        generation: u64,
        target_available: bool,
        exiting: bool,
    ) -> RevealAction {
        if self.deadline_generation != Some(generation) {
            return RevealAction::Skip;
        }
        self.request_reveal(target_available, exiting)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn exiting_app_ignores_renderer_termination_and_does_not_reveal() {
        let mut recovery = RendererRecoveryState::default();
        let now = Instant::now();
        for kind in [
            RendererTerminationKind::AbnormalTermination,
            RendererTerminationKind::ProcessWasKilled,
            RendererTerminationKind::ProcessCrashed,
            RendererTerminationKind::ProcessOom,
            RendererTerminationKind::LaunchFailed,
            RendererTerminationKind::IntegrityFailure,
            RendererTerminationKind::Unknown,
        ] {
            assert_eq!(
                recovery.on_terminated(kind, true, now),
                RendererCrashDecision {
                    action: RendererCrashAction::Ignore,
                    reveal_window: false,
                }
            );
        }
    }

    #[test]
    fn repeated_renderer_crashes_reload_bounded_times_then_show_error_page_then_stop() {
        let mut recovery = RendererRecoveryState::default();
        let t0 = Instant::now();

        for offset in 0..MAX_AUTO_RELOADS {
            assert_eq!(
                recovery.on_terminated(
                    RendererTerminationKind::ProcessCrashed,
                    false,
                    t0 + Duration::from_secs(offset as u64),
                ),
                RendererCrashDecision {
                    action: RendererCrashAction::ReloadApp,
                    reveal_window: true,
                }
            );
        }

        assert_eq!(
            recovery.on_terminated(
                RendererTerminationKind::ProcessOom,
                false,
                t0 + Duration::from_secs(5),
            ),
            RendererCrashDecision {
                action: RendererCrashAction::ShowErrorPage,
                reveal_window: true,
            }
        );

        assert_eq!(
            recovery.on_terminated(
                RendererTerminationKind::ProcessCrashed,
                false,
                t0 + Duration::from_secs(6),
            ),
            RendererCrashDecision {
                action: RendererCrashAction::Stop,
                reveal_window: true,
            }
        );
    }

    #[test]
    fn launch_and_integrity_failures_skip_auto_reload_and_never_loop_error_page() {
        let now = Instant::now();
        for kind in [
            RendererTerminationKind::LaunchFailed,
            RendererTerminationKind::IntegrityFailure,
        ] {
            let mut recovery = RendererRecoveryState::default();
            assert_eq!(
                recovery.on_terminated(kind, false, now),
                RendererCrashDecision {
                    action: RendererCrashAction::ShowErrorPage,
                    reveal_window: true,
                }
            );
            assert_eq!(
                recovery.on_terminated(kind, false, now + Duration::from_secs(1)),
                RendererCrashDecision {
                    action: RendererCrashAction::Stop,
                    reveal_window: true,
                }
            );
        }
    }

    #[test]
    fn rolling_window_evicts_expired_reloads_individually() {
        let mut recovery = RendererRecoveryState::default();
        let t0 = Instant::now();

        assert_eq!(
            recovery
                .on_terminated(RendererTerminationKind::ProcessCrashed, false, t0)
                .action,
            RendererCrashAction::ReloadApp
        );
        assert_eq!(
            recovery
                .on_terminated(
                    RendererTerminationKind::ProcessCrashed,
                    false,
                    t0 + Duration::from_secs(20),
                )
                .action,
            RendererCrashAction::ReloadApp
        );
        assert_eq!(
            recovery
                .on_terminated(
                    RendererTerminationKind::ProcessCrashed,
                    false,
                    t0 + Duration::from_secs(31),
                )
                .action,
            RendererCrashAction::ReloadApp
        );
        assert_eq!(
            recovery
                .on_terminated(
                    RendererTerminationKind::ProcessCrashed,
                    false,
                    t0 + Duration::from_secs(35),
                )
                .action,
            RendererCrashAction::ShowErrorPage
        );
    }

    #[test]
    fn crash_budget_resets_after_crash_window_or_user_retry() {
        let mut recovery = RendererRecoveryState::default();
        let t0 = Instant::now();
        for i in 0..MAX_AUTO_RELOADS {
            assert_eq!(
                recovery
                    .on_terminated(
                        RendererTerminationKind::ProcessWasKilled,
                        false,
                        t0 + Duration::from_secs(i as u64),
                    )
                    .action,
                RendererCrashAction::ReloadApp
            );
        }
        assert_eq!(
            recovery
                .on_terminated(
                    RendererTerminationKind::ProcessWasKilled,
                    false,
                    t0 + Duration::from_secs(5),
                )
                .action,
            RendererCrashAction::ShowErrorPage
        );

        assert_eq!(
            recovery.on_terminated(
                RendererTerminationKind::AbnormalTermination,
                false,
                t0 + Duration::from_secs(5) + CRASH_WINDOW,
            ),
            RendererCrashDecision {
                action: RendererCrashAction::ReloadApp,
                reveal_window: true,
            }
        );

        let t1 = t0 + Duration::from_secs(6) + CRASH_WINDOW;
        for _ in 0..MAX_AUTO_RELOADS {
            let _ = recovery.on_terminated(RendererTerminationKind::ProcessCrashed, false, t1);
        }
        assert_eq!(
            recovery
                .on_terminated(RendererTerminationKind::ProcessCrashed, false, t1)
                .action,
            RendererCrashAction::Stop
        );

        recovery.on_user_retry();
        assert_eq!(
            recovery.on_terminated(RendererTerminationKind::ProcessCrashed, false, t1),
            RendererCrashDecision {
                action: RendererCrashAction::ReloadApp,
                reveal_window: true,
            }
        );
    }

    #[test]
    fn termination_labels_are_non_empty_and_distinct_for_known_statuses() {
        let kinds = [
            RendererTerminationKind::AbnormalTermination,
            RendererTerminationKind::ProcessWasKilled,
            RendererTerminationKind::ProcessCrashed,
            RendererTerminationKind::ProcessOom,
            RendererTerminationKind::LaunchFailed,
            RendererTerminationKind::IntegrityFailure,
            RendererTerminationKind::Unknown,
        ];
        for kind in kinds {
            assert!(!kind.label().is_empty());
        }
        let labels: HashSet<&str> = kinds.iter().map(|kind| kind.label()).collect();
        assert_eq!(labels.len(), kinds.len());
        assert_eq!(
            RendererTerminationKind::from_cef(TerminationStatus::PROCESS_CRASHED),
            RendererTerminationKind::ProcessCrashed
        );
        assert_eq!(
            RendererTerminationKind::from_cef(TerminationStatus::PROCESS_OOM),
            RendererTerminationKind::ProcessOom
        );
        assert_eq!(
            RendererTerminationKind::from_cef(TerminationStatus::LAUNCH_FAILED),
            RendererTerminationKind::LaunchFailed
        );
        assert_eq!(
            format_renderer_error_text(RendererTerminationKind::ProcessCrashed, None),
            "Renderer process crashed"
        );
        assert_eq!(
            format_renderer_error_text(
                RendererTerminationKind::ProcessCrashed,
                Some("  SIGSEGV  ")
            ),
            "Renderer process crashed (SIGSEGV)"
        );
    }

    #[test]
    fn fast_window_ready_reveals_once_and_supersedes_deadline() {
        let mut reveal = WindowRevealState::new(false);
        let gen_id = reveal.arm_deadline(false).expect("deadline should arm");

        assert_eq!(reveal.on_target_ready(false), RevealAction::Skip);
        assert_eq!(reveal.request_reveal(true, false), RevealAction::RevealNow);
        assert_eq!(reveal.request_reveal(true, false), RevealAction::Skip);
        assert_eq!(reveal.on_deadline(gen_id, true, false), RevealAction::Skip);
    }

    #[test]
    fn reveal_deadline_reveals_once_when_window_ready_never_arrives() {
        let mut reveal = WindowRevealState::new(false);
        let gen_id = reveal.arm_deadline(false).expect("deadline should arm");

        assert_eq!(
            reveal.on_deadline(gen_id, true, false),
            RevealAction::RevealNow
        );
        assert_eq!(reveal.on_deadline(gen_id, true, false), RevealAction::Skip);
        assert_eq!(reveal.request_reveal(true, false), RevealAction::Skip);
    }

    #[test]
    fn reveal_deadline_before_window_creation_defers_until_window_registers() {
        let mut reveal = WindowRevealState::new(false);
        let gen_id = reveal.arm_deadline(false).expect("deadline should arm");

        assert_eq!(
            reveal.on_deadline(gen_id, false, false),
            RevealAction::DeferUntilReady
        );
        assert_eq!(reveal.on_target_ready(false), RevealAction::RevealNow);
        assert_eq!(reveal.on_target_ready(false), RevealAction::Skip);
        assert_eq!(reveal.request_reveal(true, false), RevealAction::Skip);
    }

    #[test]
    fn hidden_or_exiting_window_never_arms_or_reveals() {
        let mut hidden = WindowRevealState::new(true);
        assert_eq!(hidden.arm_deadline(false), None);
        assert_eq!(hidden.request_reveal(true, false), RevealAction::Skip);
        assert_eq!(hidden.on_target_ready(false), RevealAction::Skip);

        let mut exiting = WindowRevealState::new(false);
        let gen_id = exiting.arm_deadline(false).expect("deadline should arm");
        assert_eq!(exiting.on_deadline(gen_id, true, true), RevealAction::Skip);
        assert_eq!(exiting.request_reveal(true, true), RevealAction::Skip);
        assert_eq!(exiting.on_target_ready(true), RevealAction::Skip);
    }
}
