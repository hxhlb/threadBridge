use crate::workspace_status::{SessionActivitySource, SessionCurrentStatus, WorkspaceStatusPhase};

pub(crate) fn busy_text_message(
    snapshot: &SessionCurrentStatus,
    image_saved: bool,
) -> &'static str {
    if snapshot.phase == WorkspaceStatusPhase::TurnFinalizing {
        return match snapshot.activity_source {
            SessionActivitySource::Tui if image_saved => {
                "Image saved. The shared TUI session is already settling after an interrupt request. Analysis will stay pending until that turn fully stops."
            }
            SessionActivitySource::Tui => {
                "The shared TUI session is already settling after an interrupt request. Wait for it to stop before sending a new Telegram request."
            }
            SessionActivitySource::ManagedRuntime => {
                "This thread's current Codex session is already settling after an interrupt request. Wait for it to stop before sending a new Telegram request."
            }
        };
    }
    match snapshot.activity_source {
        SessionActivitySource::Tui if image_saved => {
            "Image saved. Analysis will stay pending until the shared TUI session finishes its current turn. Use /stop if you want to interrupt it."
        }
        SessionActivitySource::Tui => {
            "The shared TUI session is already running a turn. Wait for it to finish before sending a new Telegram request, or use /stop to interrupt it."
        }
        SessionActivitySource::ManagedRuntime => {
            "This thread's current Codex session is already handling another Telegram request. Wait for it to finish before sending a new one, or use /stop to interrupt it."
        }
    }
}

pub(crate) fn busy_command_message(snapshot: &SessionCurrentStatus) -> &'static str {
    if snapshot.phase == WorkspaceStatusPhase::TurnFinalizing {
        return match snapshot.activity_source {
            SessionActivitySource::Tui => {
                "The shared TUI session is already settling after an interrupt request. Wait for it to stop before changing this thread's session state."
            }
            SessionActivitySource::ManagedRuntime => {
                "This thread's current Codex session is already settling after an interrupt request. Wait for it to stop before changing session state."
            }
        };
    }
    match snapshot.activity_source {
        SessionActivitySource::Tui => {
            "The shared TUI session is already running a turn. Wait for it to finish before changing this thread's session state, or use /stop to interrupt it."
        }
        SessionActivitySource::ManagedRuntime => {
            "This thread's current Codex session is already handling another Telegram request. Wait for it to finish before changing session state, or use /stop to interrupt it."
        }
    }
}
