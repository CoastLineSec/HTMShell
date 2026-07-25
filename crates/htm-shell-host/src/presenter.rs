#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresenterState {
    Uninitialized,
    Cpu,
    GpuCreating,
    GpuReady,
    GpuRecovering,
    FallingBack,
    Destroyed,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfacePresenter {
    generation: u64,
    state: PresenterState,
    gpu_succeeded: bool,
    failures: u8,
}

impl SurfacePresenter {
    pub(crate) fn new(generation: u64) -> Self {
        Self {
            generation,
            state: PresenterState::Uninitialized,
            gpu_succeeded: false,
            failures: 0,
        }
    }

    pub(crate) fn generation(self) -> u64 {
        self.generation
    }

    pub(crate) fn state(self) -> PresenterState {
        self.state
    }

    pub(crate) fn begin_gpu(&mut self) -> bool {
        if self.state != PresenterState::Uninitialized {
            return false;
        }
        self.state = PresenterState::GpuCreating;
        true
    }

    pub(crate) fn gpu_ready(&mut self) {
        if matches!(
            self.state,
            PresenterState::GpuCreating | PresenterState::GpuRecovering
        ) {
            self.state = PresenterState::GpuReady;
        }
    }

    pub(crate) fn gpu_presented(&mut self) {
        if self.state == PresenterState::GpuReady {
            self.gpu_succeeded = true;
            self.failures = 0;
        }
    }

    pub(crate) fn begin_recovery(&mut self) -> bool {
        if self.state != PresenterState::GpuReady || self.failures >= 1 {
            return false;
        }
        self.failures = self.failures.saturating_add(1);
        self.state = PresenterState::GpuRecovering;
        true
    }

    pub(crate) fn fall_back(&mut self) {
        if self.state != PresenterState::Destroyed {
            self.state = PresenterState::FallingBack;
        }
    }

    pub(crate) fn select_cpu(&mut self) {
        if self.state == PresenterState::Uninitialized {
            self.state = PresenterState::Cpu;
        }
    }

    pub(crate) fn cpu_ready(&mut self) {
        if matches!(
            self.state,
            PresenterState::Uninitialized | PresenterState::FallingBack
        ) {
            self.state = PresenterState::Cpu;
        }
    }

    pub(crate) fn destroy(&mut self) {
        self.state = PresenterState::Destroyed;
    }

    pub(crate) fn gpu_succeeded(self) -> bool {
        self.gpu_succeeded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presenter_has_one_generation_scoped_owner() {
        let mut presenter = SurfacePresenter::new(7);
        assert!(presenter.begin_gpu());
        assert!(!presenter.begin_gpu());
        presenter.gpu_ready();
        presenter.gpu_presented();
        assert!(presenter.gpu_succeeded());
        assert!(presenter.begin_recovery());
        assert!(!presenter.begin_recovery());
        presenter.fall_back();
        assert_eq!(presenter.state(), PresenterState::FallingBack);
        presenter.cpu_ready();
        assert_eq!(presenter.state(), PresenterState::Cpu);
        assert_eq!(presenter.generation(), 7);
        presenter.destroy();
        assert_eq!(presenter.state(), PresenterState::Destroyed);
    }

    #[test]
    fn cpu_selection_never_overlaps_gpu() {
        let mut presenter = SurfacePresenter::new(1);
        presenter.select_cpu();
        assert_eq!(presenter.state(), PresenterState::Cpu);
        assert!(!presenter.begin_gpu());
    }

    #[test]
    fn replacement_generation_starts_without_stale_state() {
        let mut first = SurfacePresenter::new(1);
        first.begin_gpu();
        first.gpu_ready();
        first.destroy();
        let replacement = SurfacePresenter::new(2);
        assert_eq!(replacement.state(), PresenterState::Uninitialized);
        assert!(!replacement.gpu_succeeded());
    }

    #[test]
    fn cpu_fallback_is_sticky_for_one_surface_generation() {
        let mut presenter = SurfacePresenter::new(4);
        assert!(presenter.begin_gpu());
        presenter.gpu_ready();
        presenter.gpu_presented();
        presenter.fall_back();
        presenter.cpu_ready();
        assert_eq!(presenter.state(), PresenterState::Cpu);
        assert!(!presenter.begin_gpu());
        assert!(presenter.gpu_succeeded());
    }

    #[test]
    fn repeated_surface_generations_never_inherit_presenter_state() {
        for generation in 1..=100 {
            let mut presenter = SurfacePresenter::new(generation);
            assert_eq!(presenter.generation(), generation);
            assert_eq!(presenter.state(), PresenterState::Uninitialized);
            assert!(!presenter.gpu_succeeded());
            assert!(presenter.begin_gpu());
            presenter.gpu_ready();
            presenter.gpu_presented();
            presenter.destroy();
            assert_eq!(presenter.state(), PresenterState::Destroyed);
        }
    }
}
