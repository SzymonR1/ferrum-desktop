use std::time::Duration;

const RETRY_DELAYS_MS: [u64; 3] = [250, 500, 1_000];

/// A bounded delay schedule shared by background engine streams. Callers
/// decide when a stream is healthy enough to reset the budget.
#[derive(Default)]
pub(super) struct RetryBudget {
    next_attempt: usize,
}

impl RetryBudget {
    pub(super) fn next_delay(&mut self) -> Option<Duration> {
        let delay = RETRY_DELAYS_MS.get(self.next_attempt).copied()?;
        self.next_attempt += 1;
        Some(Duration::from_millis(delay))
    }
    pub(super) async fn wait_next(&mut self) -> bool {
        let Some(delay) = self.next_delay() else {
            return false;
        };
        tokio::time::sleep(delay).await;
        true
    }

    pub(super) fn reset(&mut self) {
        self.next_attempt = 0;
    }
}
