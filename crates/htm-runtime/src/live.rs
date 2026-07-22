use crate::adapter::{elapsed_ms, render_rgba, resolve_resources, validate_document_limits};
use crate::identity::IdentityRegistry;
use crate::model::{DiagnosticMessage, LogicalRect, ViewportSpec};
use crate::resource::{LocalOnlyResourceProvider, ResourceAudit};
use crate::{ExperimentalDocumentIdentity, ExperimentalNodeIdentity, RuntimeError};
use blitz_dom::node::NodeData;
use blitz_dom::{Document, DocumentConfig, QualName, StyleThreading, local_name, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const MAX_HTML_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LOGICAL_DIMENSION: u32 = 16_384;
const MAX_PIXEL_BYTES: usize = 256 * 1024 * 1024;
static NEXT_LIVE_DOCUMENT_SERIAL: AtomicU64 = AtomicU64::new(1);

pub type LiveFrameRect = LogicalRect;

/// A transient in-process frame for the portable live-presentation experiment.
#[derive(Debug, Clone)]
pub struct LiveFrame {
    pub logical_width: u32,
    pub logical_height: u32,
    pub buffer_width: u32,
    pub buffer_height: u32,
    /// Premultiplied RGBA8 in row-major order.
    pub premultiplied_rgba: Vec<u8>,
    /// Conservative headless estimate. The live host may damage the full surface.
    pub damage_estimate: LogicalRect,
    pub input_regions: Vec<LogicalRect>,
    pub interactive_region: LogicalRect,
    pub generation: u64,
    pub render_ms: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveInteractionState {
    pub hovered: bool,
    pub active: bool,
    pub click_count: u64,
}

/// Fixture profile used by the portable live-presentation experiments.
///
/// This is not a stable component or package API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveDocumentKind {
    SingleOverlay,
    Panel,
    TransientOverlay,
}

/// Host-visible action emitted by a parse-once live document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveAction {
    SingleOverlayActivate,
    ToggleOverlay,
    CloseOverlay,
    ActivateOverlay,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveRuntimeSnapshot {
    pub document_identity: ExperimentalDocumentIdentity,
    pub document_parse_count: u32,
    pub viewport: ViewportSpec,
    pub card_identity: ExperimentalNodeIdentity,
    pub action_identity: ExperimentalNodeIdentity,
    pub card_bounds: LogicalRect,
    pub action_bounds: LogicalRect,
    pub interaction: LiveInteractionState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LiveRuntimeMeasurements {
    pub package_read_ms: f64,
    pub html_parse_ms: f64,
    pub initial_resolve_ms: f64,
    pub last_resolve_ms: f64,
    pub last_render_ms: f64,
}

/// Experimental parse-once document session for the portable live host.
///
/// Blitz remains private to this type. This is not the final runtime API.
pub struct LiveDocument {
    document: HtmlDocument,
    identities: IdentityRegistry,
    audit: Arc<ResourceAudit>,
    package_root: PathBuf,
    source: PathBuf,
    viewport: ViewportSpec,
    document_identity: ExperimentalDocumentIdentity,
    parse_count: u32,
    started: Instant,
    frame_generation: u64,
    kind: LiveDocumentKind,
    last_pointer: Option<Point<f32>>,
    pressed_action: Option<LiveAction>,
    pending_action: Option<LiveAction>,
    click_count: u64,
    measurements: LiveRuntimeMeasurements,
    diagnostics: Vec<DiagnosticMessage>,
}

impl LiveDocument {
    pub fn load(
        package: impl AsRef<Path>,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<Self, RuntimeError> {
        Self::load_inner(
            package.as_ref(),
            LiveDocumentKind::SingleOverlay,
            logical_width,
            logical_height,
        )
    }

    pub fn load_surface(
        package: impl AsRef<Path>,
        kind: LiveDocumentKind,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<Self, RuntimeError> {
        Self::load_inner(package.as_ref(), kind, logical_width, logical_height)
    }

    fn load_inner(
        package: &Path,
        kind: LiveDocumentKind,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<Self, RuntimeError> {
        validate_dimensions(logical_width, logical_height)?;
        let read_started = Instant::now();
        let package_root = package
            .canonicalize()
            .map_err(|error| RuntimeError::io("resolve live package directory", package, error))?;
        if !package_root.is_dir() {
            return Err(RuntimeError::InvalidPackage(format!(
                "{} is not a directory",
                package_root.display()
            )));
        }
        let source = package_root.join(kind.source_file());
        let metadata = source
            .metadata()
            .map_err(|error| RuntimeError::io("inspect live document", &source, error))?;
        if metadata.len() > MAX_HTML_BYTES {
            return Err(RuntimeError::LimitExceeded(format!(
                "index.html is {} bytes; limit is {MAX_HTML_BYTES}",
                metadata.len()
            )));
        }
        let html = std::fs::read_to_string(&source)
            .map_err(|error| RuntimeError::io("read live document as UTF-8", &source, error))?;
        let package_read_ms = elapsed_ms(read_started);

        let viewport = ViewportSpec {
            logical_width,
            logical_height,
            scale_factor: 1.0,
            color_space: "sRGB",
            dynamic_range: "SDR",
        };
        let audit = Arc::new(ResourceAudit::default());
        let provider = Arc::new(LocalOnlyResourceProvider::new(
            package_root.clone(),
            Arc::clone(&audit),
        ));
        let parse_started = Instant::now();
        let mut document = HtmlDocument::from_html(
            &html,
            DocumentConfig {
                viewport: Some(blitz_viewport(viewport)),
                base_url: Some(LocalOnlyResourceProvider::virtual_document_url().to_owned()),
                net_provider: Some(provider),
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..Default::default()
            },
        );
        document.set_incremental_layout(true);
        validate_document_limits(&document)?;
        let html_parse_ms = elapsed_ms(parse_started);

        for selector in kind.required_selectors() {
            required_selector(&document, selector)?;
        }

        let mut diagnostics = Vec::new();
        let resolve_started = Instant::now();
        resolve_resources(&mut document, &audit, 0.0, &mut diagnostics);
        let initial_resolve_ms = elapsed_ms(resolve_started);
        let identities = IdentityRegistry::from_document(&document);

        Ok(Self {
            document,
            identities,
            audit,
            package_root,
            source,
            viewport,
            document_identity: ExperimentalDocumentIdentity {
                serial: NEXT_LIVE_DOCUMENT_SERIAL.fetch_add(1, Ordering::Relaxed),
            },
            parse_count: 1,
            started: Instant::now(),
            frame_generation: 0,
            kind,
            last_pointer: None,
            pressed_action: None,
            pending_action: None,
            click_count: 0,
            measurements: LiveRuntimeMeasurements {
                package_read_ms,
                html_parse_ms,
                initial_resolve_ms,
                last_resolve_ms: initial_resolve_ms,
                last_render_ms: 0.0,
            },
            diagnostics,
        })
    }

    pub fn set_viewport(
        &mut self,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<bool, RuntimeError> {
        validate_dimensions(logical_width, logical_height)?;
        if self.viewport.logical_width == logical_width
            && self.viewport.logical_height == logical_height
        {
            return Ok(false);
        }
        self.viewport.logical_width = logical_width;
        self.viewport.logical_height = logical_height;
        self.document.set_viewport(blitz_viewport(self.viewport));
        self.resolve();
        Ok(true)
    }

    pub fn pointer_move(&mut self, x: f64, y: f64) -> Result<bool, RuntimeError> {
        let point = checked_point(x, y)?;
        self.last_pointer = Some(point);
        let changed = self.document.set_hover_to(point.x, point.y);
        if changed {
            self.resolve();
        }
        Ok(changed)
    }

    pub fn pointer_leave(&mut self) -> bool {
        let mut changed = self.document.clear_hover();
        if self.pressed_action.is_some() {
            let point = self.last_pointer.unwrap_or_default();
            self.document
                .handle_ui_event(UiEvent::PointerUp(pointer_event(point.x, point.y, false)));
            self.pressed_action = None;
            changed = true;
        }
        self.last_pointer = None;
        if changed {
            self.resolve();
        }
        changed
    }

    pub fn pointer_primary(&mut self, pressed: bool) -> Result<bool, RuntimeError> {
        let Some(point) = self.last_pointer else {
            return Ok(false);
        };
        match pressed {
            true if self.pressed_action.is_none() => {
                let Some(action) = self.action_at(point.x, point.y)? else {
                    return Ok(false);
                };
                self.document
                    .handle_ui_event(UiEvent::PointerDown(pointer_event(point.x, point.y, true)));
                self.pressed_action = Some(action);
                self.resolve();
                Ok(true)
            }
            false if self.pressed_action.is_some() => {
                let pressed_action = self.pressed_action.take().expect("checked above");
                self.document
                    .handle_ui_event(UiEvent::PointerUp(pointer_event(point.x, point.y, false)));
                if self.action_at(point.x, point.y)? == Some(pressed_action) {
                    self.click_count = self.click_count.saturating_add(1);
                    if pressed_action == LiveAction::SingleOverlayActivate {
                        self.apply_click_mutation()?;
                    }
                    self.pending_action = Some(pressed_action);
                }
                self.resolve();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn render(&mut self) -> Result<LiveFrame, RuntimeError> {
        let width = self.viewport.logical_width;
        let height = self.viewport.logical_height;
        validate_dimensions(width, height)?;
        let render_started = Instant::now();
        let premultiplied_rgba = render_rgba(&mut self.document, width, height);
        let expected = pixel_len(width, height)?;
        if premultiplied_rgba.len() != expected {
            return Err(RuntimeError::InvalidPackage(format!(
                "renderer returned {} bytes; expected {expected}",
                premultiplied_rgba.len()
            )));
        }
        let render_ms = elapsed_ms(render_started);
        self.measurements.last_render_ms = render_ms;
        self.frame_generation = self.frame_generation.saturating_add(1);
        let card = self.bounds_for(self.kind.region_selector())?;
        let action = self.bounds_for(self.kind.primary_action_selector())?;
        validate_rect(&card)?;
        validate_rect(&action)?;
        Ok(LiveFrame {
            logical_width: width,
            logical_height: height,
            buffer_width: width,
            buffer_height: height,
            premultiplied_rgba,
            damage_estimate: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            },
            input_regions: vec![card.clone()],
            interactive_region: action,
            generation: self.frame_generation,
            render_ms,
        })
    }

    pub fn snapshot(&self) -> Result<LiveRuntimeSnapshot, RuntimeError> {
        let card_slot = required_selector(&self.document, self.kind.region_selector())?;
        let action_slot = required_selector(&self.document, self.kind.primary_action_selector())?;
        let card = self
            .document
            .get_node(card_slot)
            .ok_or_else(|| RuntimeError::InvalidMutationTarget("live region disappeared".into()))?;
        let action = self.document.get_node(action_slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget("primary live action disappeared".into())
        })?;
        Ok(LiveRuntimeSnapshot {
            document_identity: self.document_identity,
            document_parse_count: self.parse_count,
            viewport: self.viewport,
            card_identity: self
                .identities
                .identity_for_slot(&self.document, card_slot)?,
            action_identity: self
                .identities
                .identity_for_slot(&self.document, action_slot)?,
            card_bounds: node_bounds(card),
            action_bounds: node_bounds(action),
            interaction: LiveInteractionState {
                hovered: action.is_hovered(),
                active: action.is_active(),
                click_count: self.click_count,
            },
        })
    }

    pub fn measurements(&self) -> LiveRuntimeMeasurements {
        self.measurements
    }

    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        &self.diagnostics
    }

    pub fn resource_request_count(&self) -> usize {
        self.audit.request_count()
    }

    pub fn kind(&self) -> LiveDocumentKind {
        self.kind
    }

    pub fn take_action(&mut self) -> Option<LiveAction> {
        self.pending_action.take()
    }

    pub fn update_panel_state(
        &mut self,
        overlay_open: bool,
        last_action: &str,
    ) -> Result<bool, RuntimeError> {
        if self.kind != LiveDocumentKind::Panel {
            return Err(RuntimeError::InvalidMutationTarget(
                "panel state can only update a panel document".into(),
            ));
        }
        let text = if overlay_open {
            format!("Overlay open · {last_action}")
        } else {
            format!("Overlay closed · {last_action}")
        };
        self.set_text("#panel-status", &text)?;
        self.set_class(
            "#panel-root",
            if overlay_open { "panel open" } else { "panel" },
        )?;
        self.resolve();
        Ok(true)
    }

    pub fn update_overlay_state(
        &mut self,
        activation_count: u64,
        last_action: &str,
    ) -> Result<bool, RuntimeError> {
        if self.kind != LiveDocumentKind::TransientOverlay {
            return Err(RuntimeError::InvalidMutationTarget(
                "overlay state can only update a transient-overlay document".into(),
            ));
        }
        self.set_text(
            "#overlay-status",
            &format!("Activated {activation_count} time(s) · {last_action}"),
        )?;
        self.set_class(
            "#overlay-card",
            if activation_count == 0 {
                "overlay-card"
            } else {
                "overlay-card activated"
            },
        )?;
        self.resolve();
        Ok(true)
    }

    fn apply_click_mutation(&mut self) -> Result<(), RuntimeError> {
        self.set_text(
            "#status-label",
            &format!("Activated {} time(s)", self.click_count),
        )?;
        self.set_class("#shell-card", "shell-card activated")?;
        Ok(())
    }

    fn set_text(&mut self, selector: &str, value: &str) -> Result<(), RuntimeError> {
        let parent = required_selector(&self.document, selector)?;
        let text = self
            .document
            .get_node(parent)
            .and_then(|node| {
                node.children.iter().copied().find(|child| {
                    self.document
                        .get_node(*child)
                        .is_some_and(|node| matches!(node.data, NodeData::Text(_)))
                })
            })
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "{selector} does not contain a text node"
                ))
            })?;
        self.document.mutate().set_node_text(text, value);
        Ok(())
    }

    fn set_class(&mut self, selector: &str, value: &str) -> Result<(), RuntimeError> {
        let node = required_selector(&self.document, selector)?;
        self.document.mutate().set_attribute(
            node,
            QualName {
                prefix: None,
                ns: ns!(),
                local: local_name!("class"),
            },
            value,
        );
        Ok(())
    }

    fn action_at(&self, x: f32, y: f32) -> Result<Option<LiveAction>, RuntimeError> {
        for (selector, action) in self.kind.actions() {
            if contains(&self.bounds_for(selector)?, x, y) {
                return Ok(Some(*action));
            }
        }
        Ok(None)
    }

    fn bounds_for(&self, selector: &str) -> Result<LogicalRect, RuntimeError> {
        let slot = required_selector(&self.document, selector)?;
        let node = self.document.get_node(slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!("{selector} disappeared"))
        })?;
        let bounds = node_bounds(node);
        validate_rect(&bounds)?;
        Ok(bounds)
    }

    fn resolve(&mut self) {
        let started = Instant::now();
        self.document.resolve(self.started.elapsed().as_secs_f64());
        self.measurements.last_resolve_ms = elapsed_ms(started);
    }
}

impl LiveDocumentKind {
    fn source_file(self) -> &'static str {
        match self {
            Self::SingleOverlay => "index.html",
            Self::Panel => "panel.html",
            Self::TransientOverlay => "overlay.html",
        }
    }

    fn region_selector(self) -> &'static str {
        match self {
            Self::SingleOverlay => "#shell-card",
            Self::Panel => "#panel-root",
            Self::TransientOverlay => "#overlay-card",
        }
    }

    fn primary_action_selector(self) -> &'static str {
        match self {
            Self::SingleOverlay => "#primary-action",
            Self::Panel => "#overlay-toggle",
            Self::TransientOverlay => "#overlay-action",
        }
    }

    fn required_selectors(self) -> &'static [&'static str] {
        match self {
            Self::SingleOverlay => &["#shell-card", "#primary-action", "#status-label"],
            Self::Panel => &["#panel-root", "#overlay-toggle", "#panel-status"],
            Self::TransientOverlay => &[
                "#overlay-card",
                "#overlay-close",
                "#overlay-action",
                "#overlay-status",
            ],
        }
    }

    fn actions(self) -> &'static [(&'static str, LiveAction)] {
        match self {
            Self::SingleOverlay => &[("#primary-action", LiveAction::SingleOverlayActivate)],
            Self::Panel => &[("#overlay-toggle", LiveAction::ToggleOverlay)],
            Self::TransientOverlay => &[
                ("#overlay-close", LiveAction::CloseOverlay),
                ("#overlay-action", LiveAction::ActivateOverlay),
            ],
        }
    }
}

fn blitz_viewport(viewport: ViewportSpec) -> Viewport {
    Viewport::new(
        viewport.logical_width,
        viewport.logical_height,
        viewport.scale_factor,
        ColorScheme::Dark,
    )
}

fn required_selector(document: &HtmlDocument, selector: &str) -> Result<usize, RuntimeError> {
    document
        .query_selector(selector)
        .map_err(|error| RuntimeError::InvalidMutationTarget(format!("{error:?}")))?
        .ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!("selector `{selector}` did not match"))
        })
}

fn node_bounds(node: &blitz_dom::Node) -> LogicalRect {
    let absolute = node.absolute_position(0.0, 0.0);
    LogicalRect {
        x: absolute.x,
        y: absolute.y,
        width: node.final_layout.size.width,
        height: node.final_layout.size.height,
    }
}

fn validate_rect(rect: &LogicalRect) -> Result<(), RuntimeError> {
    if [rect.x, rect.y, rect.width, rect.height]
        .into_iter()
        .all(f32::is_finite)
        && rect.width >= 0.0
        && rect.height >= 0.0
    {
        Ok(())
    } else {
        Err(RuntimeError::InvalidPackage(
            "live layout contains nonfinite or negative geometry".into(),
        ))
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), RuntimeError> {
    if width == 0 || height == 0 {
        return Err(RuntimeError::InvalidPackage(
            "live viewport dimensions must be nonzero".into(),
        ));
    }
    if width > MAX_LOGICAL_DIMENSION || height > MAX_LOGICAL_DIMENSION {
        return Err(RuntimeError::LimitExceeded(format!(
            "live viewport {width}x{height} exceeds {MAX_LOGICAL_DIMENSION}"
        )));
    }
    pixel_len(width, height)?;
    Ok(())
}

fn pixel_len(width: u32, height: u32) -> Result<usize, RuntimeError> {
    let len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| RuntimeError::LimitExceeded("live pixel dimensions overflow".into()))?;
    if len > MAX_PIXEL_BYTES {
        return Err(RuntimeError::LimitExceeded(format!(
            "live frame requires {len} bytes; limit is {MAX_PIXEL_BYTES}"
        )));
    }
    Ok(len)
}

fn checked_point(x: f64, y: f64) -> Result<Point<f32>, RuntimeError> {
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
        return Err(RuntimeError::InvalidMutationTarget(
            "pointer coordinates must be finite and nonnegative".into(),
        ));
    }
    Ok(Point {
        x: x as f32,
        y: y as f32,
    })
}

fn contains(rect: &LogicalRect, x: f32, y: f32) -> bool {
    x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height
}

fn pointer_event(x: f32, y: f32, pressed: bool) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            client_x: x,
            client_y: y,
            screen_x: x,
            screen_y: y,
        },
        button: MouseEventButton::Main,
        buttons: if pressed {
            MouseEventButtons::Primary
        } else {
            MouseEventButtons::None
        },
        mods: Default::default(),
        details: PointerDetails {
            pressure: if pressed { 0.5 } else { 0.0 },
            ..Default::default()
        },
        element: Point { x, y },
        active_pointers: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/live-overlay")
    }

    fn multi_fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/two-surface-shell")
    }

    fn click_action(live: &mut LiveDocument, bounds: &LogicalRect) -> LiveAction {
        let x = f64::from(bounds.x + bounds.width / 2.0);
        let y = f64::from(bounds.y + bounds.height / 2.0);
        assert!(live.pointer_move(x, y).unwrap());
        assert!(live.pointer_primary(true).unwrap());
        assert!(live.pointer_primary(false).unwrap());
        live.take_action().expect("action emitted")
    }

    #[test]
    fn live_document_parses_once_across_resize_and_interaction() {
        let mut live = LiveDocument::load(fixture(), 800, 600).unwrap();
        let initial = live.snapshot().unwrap();
        assert_eq!(initial.document_parse_count, 1);

        assert!(live.set_viewport(1024, 768).unwrap());
        let resized = live.snapshot().unwrap();
        assert_eq!(resized.document_parse_count, 1);
        assert_eq!(initial.document_identity, resized.document_identity);
        assert_eq!(initial.card_identity, resized.card_identity);
        assert_eq!(initial.action_identity, resized.action_identity);

        let point = (
            resized.action_bounds.x + resized.action_bounds.width / 2.0,
            resized.action_bounds.y + resized.action_bounds.height / 2.0,
        );
        assert!(
            live.pointer_move(f64::from(point.0), f64::from(point.1))
                .unwrap()
        );
        assert!(live.snapshot().unwrap().interaction.hovered);
        assert!(live.pointer_primary(true).unwrap());
        assert!(live.snapshot().unwrap().interaction.active);
        assert!(live.pointer_primary(false).unwrap());
        let clicked = live.snapshot().unwrap();
        assert!(!clicked.interaction.active);
        assert_eq!(clicked.interaction.click_count, 1);
        assert_eq!(clicked.document_parse_count, 1);
        assert_eq!(clicked.action_identity, initial.action_identity);
    }

    #[test]
    fn identical_pointer_motion_does_not_dirty_the_document() {
        let mut live = LiveDocument::load(fixture(), 800, 600).unwrap();
        let action = live.snapshot().unwrap().action_bounds;
        let x = f64::from(action.x + action.width / 2.0);
        let y = f64::from(action.y + action.height / 2.0);
        assert!(live.pointer_move(x, y).unwrap());
        assert!(!live.pointer_move(x, y).unwrap());
        assert!(live.pointer_leave());
        assert!(!live.snapshot().unwrap().interaction.hovered);
        assert!(!live.pointer_leave());
    }

    #[test]
    fn rendered_frame_is_premultiplied_rgba_and_has_input_region() {
        let mut live = LiveDocument::load(fixture(), 640, 480).unwrap();
        let frame = live.render().unwrap();
        assert_eq!(frame.premultiplied_rgba.len(), 640 * 480 * 4);
        assert_eq!(frame.input_regions.len(), 1);
        assert!(frame.input_regions[0].width > 0.0);
        assert!(frame.interactive_region.width > 0.0);
        for pixel in frame.premultiplied_rgba.chunks_exact(4) {
            assert!(pixel[0] <= pixel[3]);
            assert!(pixel[1] <= pixel[3]);
            assert!(pixel[2] <= pixel[3]);
        }
    }

    #[test]
    fn invalid_dimensions_are_rejected() {
        assert!(LiveDocument::load(fixture(), 0, 600).is_err());
        assert!(LiveDocument::load(fixture(), u32::MAX, 600).is_err());
    }

    #[test]
    fn invalid_package_path_is_contained_as_an_error() {
        assert!(LiveDocument::load(fixture().join("does-not-exist"), 800, 600).is_err());
    }

    #[test]
    fn panel_document_parses_once_and_emits_toggle_action() {
        let mut panel =
            LiveDocument::load_surface(multi_fixture(), LiveDocumentKind::Panel, 1280, 52).unwrap();
        let initial = panel.snapshot().unwrap();
        assert_eq!(initial.document_parse_count, 1);
        assert_eq!(
            click_action(&mut panel, &initial.action_bounds),
            LiveAction::ToggleOverlay
        );
        panel.update_panel_state(true, "Opened from panel").unwrap();
        panel.set_viewport(1440, 52).unwrap();
        let changed = panel.snapshot().unwrap();
        assert_eq!(changed.document_parse_count, 1);
        assert_eq!(initial.document_identity, changed.document_identity);
        assert_eq!(initial.card_identity, changed.card_identity);
        assert_eq!(initial.action_identity, changed.action_identity);
    }

    #[test]
    fn transient_overlay_emits_independent_actions_without_reparse() {
        let mut overlay = LiveDocument::load_surface(
            multi_fixture(),
            LiveDocumentKind::TransientOverlay,
            1280,
            720,
        )
        .unwrap();
        let initial = overlay.snapshot().unwrap();
        assert_eq!(
            click_action(&mut overlay, &initial.action_bounds),
            LiveAction::ActivateOverlay
        );
        overlay
            .update_overlay_state(1, "Overlay state updated")
            .unwrap();
        let close = overlay.bounds_for("#overlay-close").unwrap();
        assert_eq!(click_action(&mut overlay, &close), LiveAction::CloseOverlay);
        let changed = overlay.snapshot().unwrap();
        assert_eq!(changed.document_parse_count, 1);
        assert_eq!(initial.document_identity, changed.document_identity);
        assert_eq!(initial.card_identity, changed.card_identity);
        assert_eq!(initial.action_identity, changed.action_identity);
    }

    #[test]
    fn surface_state_updates_are_profile_scoped() {
        let mut panel =
            LiveDocument::load_surface(multi_fixture(), LiveDocumentKind::Panel, 800, 52).unwrap();
        let mut overlay = LiveDocument::load_surface(
            multi_fixture(),
            LiveDocumentKind::TransientOverlay,
            800,
            600,
        )
        .unwrap();
        assert!(panel.update_overlay_state(1, "invalid").is_err());
        assert!(overlay.update_panel_state(true, "invalid").is_err());
        let panel_snapshot = panel.snapshot().unwrap();
        let overlay_snapshot = overlay.snapshot().unwrap();
        assert_eq!(panel_snapshot.document_parse_count, 1);
        assert_eq!(overlay_snapshot.document_parse_count, 1);
        assert_ne!(
            panel_snapshot.document_identity,
            overlay_snapshot.document_identity
        );
    }
}
