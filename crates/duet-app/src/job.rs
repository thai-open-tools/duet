//! One background job per view: starting a new one cancels the old one, and results carry
//! the generation they were started with so stale ones are dropped (Review Focus 4).

use duet_core::cancel::CancelToken;

#[derive(Debug, Default)]
pub struct JobSlot {
    generation: u64,
    cancel: Option<CancelToken>,
}

impl JobSlot {
    pub fn start(&mut self) -> (u64, CancelToken) {
        self.cancel();
        self.generation += 1;
        let token = CancelToken::new();
        self.cancel = Some(token.clone());
        (self.generation, token)
    }

    /// True only for the newest job, and only until it finishes or is cancelled.
    pub fn is_current(&self, generation: u64) -> bool {
        generation == self.generation && self.cancel.is_some()
    }

    pub fn finish(&mut self, generation: u64) {
        if generation == self.generation {
            self.cancel = None;
        }
    }

    pub fn cancel(&mut self) {
        if let Some(token) = self.cancel.take() {
            token.cancel();
        }
    }

    /// Cancels the token of the running job but leaves it current: its result still arrives
    /// and is applied. For a pass whose cancel means "stop looking", not "throw it all away".
    pub fn stop_pass(&self) {
        if let Some(token) = &self.cancel {
            token.cancel();
        }
    }

    pub fn is_running(&self) -> bool {
        self.cancel.is_some()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// A closed tab drops its view and slot; the background job stops with it.
impl Drop for JobSlot {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_a_job_cancels_and_supersedes_the_previous_one() {
        let mut slot = JobSlot::default();
        let (first, first_token) = slot.start();
        let (second, second_token) = slot.start();
        assert!(first_token.is_cancelled());
        assert!(!second_token.is_cancelled());
        assert!(!slot.is_current(first));
        assert!(slot.is_current(second));
        slot.finish(first);
        assert!(slot.is_running(), "finishing a stale job changes nothing");
        slot.finish(second);
        assert!(!slot.is_current(second) && !slot.is_running());
    }

    #[test]
    fn dropping_the_slot_cancels_its_job() {
        // A closed tab drops its view and slot; the background compare must stop with it.
        let mut slot = JobSlot::default();
        let (_, token) = slot.start();
        drop(slot);
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_stops_the_running_job() {
        let mut slot = JobSlot::default();
        let (generation, token) = slot.start();
        slot.cancel();
        assert!(token.is_cancelled());
        assert!(!slot.is_current(generation));
    }
}
