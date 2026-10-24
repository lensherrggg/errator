use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Small cancellation primitive shared by the CLI, the TUI worker and future
/// GPU inference backends. It is intentionally lock-free and cheap to poll.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Returns true when the token was cancelled. A disabled token is useful
    /// for `--no-cancel` without threading an Option through every layer.
    pub fn cancelled(&self, enabled: bool) -> bool {
        enabled && self.is_cancelled()
    }
}
