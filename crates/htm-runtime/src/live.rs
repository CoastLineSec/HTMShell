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
use std::time::Instant;

const MAX_HTML_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LOGICAL_DIMENSION: u32 = 16_384;
const MAX_PIXEL_BYTES: usize = 256 * 1024 * 1024;

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
    last_pointer: Option<Point<f32>>,
    pressed_action: bool,
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
        Self::load_inner(package.as_ref(), logical_width, logical_height)
    }

    fn load_inner(
        package: &Path,
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
        let source = package_root.join("index.html");
        let metadata = source
            .metadata()
            .map_err(|error| RuntimeError::io("inspect live index.html", &source, error))?;
        if metadata.len() > MAX_HTML_BYTES {
            return Err(RuntimeError::LimitExceeded(format!(
                "index.html is {} bytes; limit is {MAX_HTML_BYTES}",
                metadata.len()
            )));
        }
        let html = std::fs::read_to_string(&source)
            .map_err(|error| RuntimeError::io("read live index.html as UTF-8", &source, error))?;
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

        required_selector(&document, "#shell-card")?;
        required_selector(&document, "#primary-action")?;
        required_selector(&document, "#status-label")?;

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
            document_identity: ExperimentalDocumentIdentity { serial: 1 },
            parse_count: 1,
            started: Instant::now(),
            frame_generation: 0,
            last_pointer: None,
            pressed_action: false,
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
        if self.pressed_action {
            let point = self.last_pointer.unwrap_or_default();
            self.document
                .handle_ui_event(UiEvent::PointerUp(pointer_event(point.x, point.y, false)));
            self.pressed_action = false;
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
        let action = self.bounds_for("#primary-action")?;
        let inside = contains(&action, point.x, point.y);
        match pressed {
            true if inside && !self.pressed_action => {
                self.document
                    .handle_ui_event(UiEvent::PointerDown(pointer_event(point.x, point.y, true)));
                self.pressed_action = true;
                self.resolve();
                Ok(true)
            }
            false if self.pressed_action => {
                self.document
                    .handle_ui_event(UiEvent::PointerUp(pointer_event(point.x, point.y, false)));
                self.pressed_action = false;
                if inside {
                    self.apply_click_mutation()?;
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
        let card = self.bounds_for("#shell-card")?;
        let action = self.bounds_for("#primary-action")?;
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
        let card_slot = required_selector(&self.document, "#shell-card")?;
        let action_slot = required_selector(&self.document, "#primary-action")?;
        let card = self
            .document
            .get_node(card_slot)
            .ok_or_else(|| RuntimeError::InvalidMutationTarget("#shell-card disappeared".into()))?;
        let action = self.document.get_node(action_slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget("#primary-action disappeared".into())
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

    fn apply_click_mutation(&mut self) -> Result<(), RuntimeError> {
        self.click_count = self.click_count.saturating_add(1);
        let status = required_selector(&self.document, "#status-label")?;
        let text = self
            .document
            .get_node(status)
            .and_then(|node| {
                node.children.iter().copied().find(|child| {
                    self.document
                        .get_node(*child)
                        .is_some_and(|node| matches!(node.data, NodeData::Text(_)))
                })
            })
            .ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(
                    "#status-label does not contain a text node".into(),
                )
            })?;
        self.document
            .mutate()
            .set_node_text(text, &format!("Activated {} time(s)", self.click_count));

        let card = required_selector(&self.document, "#shell-card")?;
        self.document.mutate().set_attribute(
            card,
            QualName {
                prefix: None,
                ns: ns!(),
                local: local_name!("class"),
            },
            "shell-card activated",
        );
        Ok(())
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
}
