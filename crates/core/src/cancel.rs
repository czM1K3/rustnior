use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{DecodeError, Result};

/// Cheap, clonable cancellation flag shared between the requester and a job.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// True if both handles refer to the same flag.
    pub fn same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Returns `Err(Cancelled)` if cancellation was requested.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(DecodeError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_state() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(b.check().is_ok());
        a.cancel();
        assert!(b.is_cancelled());
        assert!(matches!(b.check(), Err(DecodeError::Cancelled)));
    }
}
