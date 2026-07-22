#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleDecision {
    Render,
    WaitForFrameCallback,
    WaitForBuffer,
    Idle,
}

#[derive(Debug, Default)]
pub struct FrameScheduler {
    dirty: bool,
    frame_callback_outstanding: bool,
}

impl FrameScheduler {
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn frame_committed(&mut self) {
        self.frame_callback_outstanding = true;
        self.dirty = false;
    }

    pub fn frame_callback_done(&mut self) {
        self.frame_callback_outstanding = false;
    }

    pub fn stop_scheduling(&mut self) {
        self.dirty = false;
    }

    pub fn decision(&self, configured: bool, free_buffer: bool) -> ScheduleDecision {
        if !configured || !self.dirty {
            return ScheduleDecision::Idle;
        }
        if self.frame_callback_outstanding {
            return ScheduleDecision::WaitForFrameCallback;
        }
        if !free_buffer {
            return ScheduleDecision::WaitForBuffer;
        }
        ScheduleDecision::Render
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn frame_callback_outstanding(&self) -> bool {
        self.frame_callback_outstanding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_state_is_idle_and_dirty_state_renders_once() {
        let mut scheduler = FrameScheduler::default();
        assert_eq!(scheduler.decision(true, true), ScheduleDecision::Idle);
        scheduler.mark_dirty();
        assert_eq!(scheduler.decision(true, true), ScheduleDecision::Render);
        scheduler.frame_committed();
        assert_eq!(scheduler.decision(true, true), ScheduleDecision::Idle);
        assert!(scheduler.frame_callback_outstanding());
    }

    #[test]
    fn dirty_updates_wait_for_callback_or_buffer() {
        let mut scheduler = FrameScheduler::default();
        scheduler.mark_dirty();
        assert_eq!(
            scheduler.decision(true, false),
            ScheduleDecision::WaitForBuffer
        );
        scheduler.frame_committed();
        scheduler.mark_dirty();
        assert_eq!(
            scheduler.decision(true, true),
            ScheduleDecision::WaitForFrameCallback
        );
        scheduler.frame_callback_done();
        assert_eq!(scheduler.decision(true, true), ScheduleDecision::Render);
    }
}
