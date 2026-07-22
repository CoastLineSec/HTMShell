#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerState {
    New,
    WaitingForInitialCommit,
    WaitingForConfigure,
    Configured,
    Closed,
    OutputLost,
}

#[derive(Debug)]
pub(crate) struct LayerLifecycle {
    state: LayerState,
    configured_size: Option<(u32, u32)>,
    latest_serial: Option<u32>,
}

impl Default for LayerLifecycle {
    fn default() -> Self {
        Self {
            state: LayerState::New,
            configured_size: None,
            latest_serial: None,
        }
    }
}

impl LayerLifecycle {
    pub(crate) fn assign_role(&mut self) -> Result<(), &'static str> {
        if self.state != LayerState::New {
            return Err("layer role has already been assigned");
        }
        self.state = LayerState::WaitingForInitialCommit;
        Ok(())
    }

    pub(crate) fn initial_bufferless_commit(&mut self) -> Result<(), &'static str> {
        if self.state != LayerState::WaitingForInitialCommit {
            return Err("initial bufferless commit is out of order");
        }
        self.state = LayerState::WaitingForConfigure;
        Ok(())
    }

    pub(crate) fn configure(
        &mut self,
        serial: u32,
        width: u32,
        height: u32,
    ) -> Result<(), &'static str> {
        if !matches!(
            self.state,
            LayerState::WaitingForConfigure | LayerState::Configured
        ) {
            return Err("configure is invalid in the current layer state");
        }
        if width == 0 || height == 0 {
            return Err("configure dimensions must be nonzero for the full-output profile");
        }
        self.latest_serial = Some(serial);
        self.configured_size = Some((width, height));
        Ok(())
    }

    pub(crate) fn acknowledge(&mut self, serial: u32) -> Result<(), &'static str> {
        if self.latest_serial != Some(serial) {
            return Err("configure acknowledgement does not match the latest serial");
        }
        self.latest_serial = None;
        self.state = LayerState::Configured;
        Ok(())
    }

    pub(crate) fn can_attach_buffer(&self) -> bool {
        self.state == LayerState::Configured && self.latest_serial.is_none()
    }

    pub(crate) fn close(&mut self) {
        self.state = LayerState::Closed;
        self.latest_serial = None;
        self.configured_size = None;
    }

    pub(crate) fn output_lost(&mut self) {
        self.state = LayerState::OutputLost;
        self.latest_serial = None;
        self.configured_size = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_lifecycle_requires_bufferless_commit_configure_and_ack() {
        let mut lifecycle = LayerLifecycle::default();
        lifecycle.assign_role().unwrap();
        assert!(!lifecycle.can_attach_buffer());
        lifecycle.initial_bufferless_commit().unwrap();
        assert!(!lifecycle.can_attach_buffer());
        lifecycle.configure(7, 1280, 720).unwrap();
        assert!(!lifecycle.can_attach_buffer());
        lifecycle.acknowledge(7).unwrap();
        assert!(lifecycle.can_attach_buffer());
    }

    #[test]
    fn stale_ack_and_buffer_before_configure_are_rejected() {
        let mut lifecycle = LayerLifecycle::default();
        lifecycle.assign_role().unwrap();
        lifecycle.initial_bufferless_commit().unwrap();
        assert!(!lifecycle.can_attach_buffer());
        lifecycle.configure(9, 800, 600).unwrap();
        assert!(lifecycle.acknowledge(8).is_err());
        assert!(!lifecycle.can_attach_buffer());
    }

    #[test]
    fn close_and_output_loss_stop_presentation() {
        let mut closed = LayerLifecycle::default();
        closed.assign_role().unwrap();
        closed.initial_bufferless_commit().unwrap();
        closed.configure(1, 800, 600).unwrap();
        closed.acknowledge(1).unwrap();
        closed.close();
        assert!(!closed.can_attach_buffer());

        let mut lost = LayerLifecycle::default();
        lost.assign_role().unwrap();
        lost.initial_bufferless_commit().unwrap();
        lost.configure(2, 800, 600).unwrap();
        lost.acknowledge(2).unwrap();
        lost.output_lost();
        assert!(!lost.can_attach_buffer());
    }
}
