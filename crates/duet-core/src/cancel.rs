//! Cooperative cancellation shared between the UI thread and background jobs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("operation cancelled")]
pub struct Cancelled;

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

    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_token_is_not_cancelled() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        assert_eq!(token.check(), Ok(()));
    }

    #[test]
    fn clones_share_cancellation() {
        let token = CancelToken::new();
        let worker = token.clone();
        token.cancel();
        assert!(worker.is_cancelled());
        assert_eq!(worker.check(), Err(Cancelled));
    }

    #[test]
    fn token_crosses_threads() {
        let token = CancelToken::new();
        let worker = token.clone();
        let handle = std::thread::spawn(move || {
            while !worker.is_cancelled() {
                std::thread::yield_now();
            }
            worker.check()
        });
        token.cancel();
        assert_eq!(handle.join().unwrap(), Err(Cancelled));
    }
}
