use crate::ShellHostError;
use htm_runtime::{LIVE_SCALE_DENOMINATOR, LiveRenderRequest, MAX_LIVE_SCALE_NUMERATOR};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationProfile {
    Scale1,
    FractionalViewport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceScaleState {
    surface_generation: u64,
    fractional_object_present: bool,
    viewport_present: bool,
    preferred_numerator: u32,
    preferred_received: bool,
    logical_size: Option<(u32, u32)>,
    presentation_revision: u64,
    applied_revision: u64,
}

impl SurfaceScaleState {
    pub fn new(surface_generation: u64, fractional_objects_present: bool) -> Self {
        Self {
            surface_generation,
            fractional_object_present: fractional_objects_present,
            viewport_present: fractional_objects_present,
            preferred_numerator: LIVE_SCALE_DENOMINATOR,
            preferred_received: false,
            logical_size: None,
            presentation_revision: 0,
            applied_revision: 0,
        }
    }

    pub fn surface_generation(self) -> u64 {
        self.surface_generation
    }

    pub fn preferred_numerator(self) -> u32 {
        self.preferred_numerator
    }

    pub fn profile(self) -> PresentationProfile {
        if self.fractional_object_present && self.viewport_present && self.preferred_received {
            PresentationProfile::FractionalViewport
        } else {
            PresentationProfile::Scale1
        }
    }

    pub fn set_logical_size(&mut self, width: u32, height: u32) -> bool {
        if self.logical_size == Some((width, height)) {
            return false;
        }
        self.logical_size = Some((width, height));
        self.presentation_revision = self.presentation_revision.saturating_add(1);
        true
    }

    pub fn receive_preferred(
        &mut self,
        surface_generation: u64,
        numerator: u32,
    ) -> Result<bool, ShellHostError> {
        if surface_generation != self.surface_generation {
            return Err(ShellHostError::Wayland(
                "preferred-scale event belongs to a stale surface generation".into(),
            ));
        }
        if !self.fractional_object_present || !self.viewport_present {
            return Err(ShellHostError::Wayland(
                "preferred-scale event arrived without the complete fractional profile".into(),
            ));
        }
        if numerator == 0 || numerator > MAX_LIVE_SCALE_NUMERATOR {
            return Err(ShellHostError::InvalidDimensions(format!(
                "preferred scale numerator {numerator} is outside 1..={MAX_LIVE_SCALE_NUMERATOR}"
            )));
        }
        if self.preferred_received && self.preferred_numerator == numerator {
            return Ok(false);
        }
        self.preferred_received = true;
        self.preferred_numerator = numerator;
        self.presentation_revision = self.presentation_revision.saturating_add(1);
        Ok(true)
    }

    pub fn render_request(self) -> Result<Option<LiveRenderRequest>, ShellHostError> {
        let Some((width, height)) = self.logical_size else {
            return Ok(None);
        };
        let numerator = match self.profile() {
            PresentationProfile::Scale1 => LIVE_SCALE_DENOMINATOR,
            PresentationProfile::FractionalViewport => self.preferred_numerator,
        };
        LiveRenderRequest::new(width, height, numerator)
            .map(Some)
            .map_err(ShellHostError::from)
    }

    pub fn pending_revision(self) -> u64 {
        self.presentation_revision
    }

    pub fn applied_revision(self) -> u64 {
        self.applied_revision
    }

    pub fn mark_applied(&mut self) {
        self.applied_revision = self.presentation_revision;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_pairs_select_one_complete_profile() {
        for (fractional, viewporter, expected) in [
            (true, true, PresentationProfile::Scale1),
            (true, false, PresentationProfile::Scale1),
            (false, true, PresentationProfile::Scale1),
            (false, false, PresentationProfile::Scale1),
        ] {
            let paired = fractional && viewporter;
            assert_eq!(SurfaceScaleState::new(1, paired).profile(), expected);
        }
        let mut state = SurfaceScaleState::new(1, true);
        assert!(state.receive_preferred(1, 180).unwrap());
        assert_eq!(state.profile(), PresentationProfile::FractionalViewport);
    }

    #[test]
    fn scale_and_configure_coalesce_into_latest_revision() {
        let mut state = SurfaceScaleState::new(7, true);
        assert!(state.set_logical_size(101, 51));
        assert!(state.receive_preferred(7, 150).unwrap());
        assert!(state.set_logical_size(103, 53));
        assert!(state.receive_preferred(7, 210).unwrap());
        let request = state.render_request().unwrap().unwrap();
        assert_eq!((request.logical_width, request.logical_height), (103, 53));
        assert_eq!((request.buffer_width, request.buffer_height), (181, 93));
        assert!(state.pending_revision() > state.applied_revision());
        state.mark_applied();
        assert_eq!(state.pending_revision(), state.applied_revision());
    }

    #[test]
    fn duplicates_and_stale_or_extreme_events_are_contained() {
        let mut state = SurfaceScaleState::new(4, true);
        assert!(state.receive_preferred(4, 180).unwrap());
        let revision = state.pending_revision();
        assert!(!state.receive_preferred(4, 180).unwrap());
        assert_eq!(state.pending_revision(), revision);
        assert!(state.receive_preferred(3, 150).is_err());
        assert!(state.receive_preferred(4, 0).is_err());
        assert!(
            state
                .receive_preferred(4, MAX_LIVE_SCALE_NUMERATOR + 1)
                .is_err()
        );
    }

    #[test]
    fn odd_dimensions_use_checked_ceiling_at_standard_scales() {
        for (numerator, expected) in [
            (120, (101, 51)),
            (150, (127, 64)),
            (180, (152, 77)),
            (210, (177, 90)),
            (240, (202, 102)),
        ] {
            let mut state = SurfaceScaleState::new(1, true);
            state.set_logical_size(101, 51);
            state.receive_preferred(1, numerator).unwrap();
            let request = state.render_request().unwrap().unwrap();
            assert_eq!((request.buffer_width, request.buffer_height), expected);
        }
    }
}
