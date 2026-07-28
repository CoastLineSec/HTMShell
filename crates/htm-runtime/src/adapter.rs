use crate::ExperimentalDocumentIdentity;
use crate::error::RuntimeError;
use crate::identity::IdentityRegistry;
use crate::model::{
    Artifact, CornerRadii, DiagnosticMessage, DiagnosticNode, DiagnosticReport, ExperimentOptions,
    ExperimentRun, FontRecord, ImageDiagnostic, InteractionEvidence, LogicalRect,
    OverflowDiagnostic, Phase, RunMeasurements, TextDiagnostic, ViewportSpec,
};
use crate::render::{CpuRenderSession, FrameReason, FrameReasonSet, RenderSurfaceId};
use crate::resource::{LocalOnlyResourceProvider, ResourceAudit};
use crate::{BLITZ_REVISION, DIAGNOSTIC_SCHEMA_VERSION};
use blitz_dom::node::ImageData;
use blitz_dom::{Document, DocumentConfig, Node, StyleThreading};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use stylo::color::ColorSpace;
use stylo::values::computed::CSSPixelLength;

const MAX_DOM_NODES: usize = 10_000;
const MAX_DOM_DEPTH: usize = 256;
const MAX_RESOURCE_RESOLVE_PASSES: usize = 8;

pub fn run_package(package: impl AsRef<Path>) -> Result<ExperimentRun, RuntimeError> {
    let package = package.as_ref();
    run_package_with_options(
        package,
        ExperimentOptions {
            output_directory: Some(package.join("output")),
            ..Default::default()
        },
    )
}

pub fn run_package_with_options(
    package: impl AsRef<Path>,
    options: ExperimentOptions,
) -> Result<ExperimentRun, RuntimeError> {
    let package = package.as_ref().to_path_buf();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_inner(&package, options)
    }))
    .map_err(|payload| RuntimeError::EnginePanic(panic_message(payload)))?
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

fn run_inner(package: &Path, options: ExperimentOptions) -> Result<ExperimentRun, RuntimeError> {
    validate_viewport(&options)?;
    let total_started = Instant::now();
    let read_started = Instant::now();
    let mut package_loader = crate::PackageSnapshotLoader::new();
    let package_snapshot = package_loader.load_headless(package)?;
    let root = package_snapshot
        .root_package()
        .canonical_root()
        .to_path_buf();
    let entry = package_snapshot.headless_entry().ok_or_else(|| {
        RuntimeError::InvalidPackage("headless package snapshot has no index.html entry".into())
    })?;
    let source = entry.canonical_path().to_path_buf();
    let html = entry.html();
    let package_read_ms = elapsed_ms(read_started);

    let audit = Arc::new(ResourceAudit::default());
    let provider = Arc::new(LocalOnlyResourceProvider::new(
        root.clone(),
        Arc::clone(&audit),
    ));
    let physical_width =
        ((options.viewport.logical_width as f32) * options.viewport.scale_factor).round() as u32;
    let physical_height =
        ((options.viewport.logical_height as f32) * options.viewport.scale_factor).round() as u32;

    let parse_started = Instant::now();
    let mut document = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                physical_width,
                physical_height,
                options.viewport.scale_factor,
                ColorScheme::Dark,
            )),
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

    let mut messages = inspect_document_profile(&document);
    let initial_resolve_started = Instant::now();
    resolve_resources(&mut document, &audit, 0.0, &mut messages);
    let initial_resolve_ms = elapsed_ms(initial_resolve_started);
    let identities = IdentityRegistry::from_document(&document);
    let document_identity = ExperimentalDocumentIdentity { serial: 1 };
    let mut render_session = CpuRenderSession::default();

    let initial_paint_started = Instant::now();
    let initial_png = options
        .render_png
        .then(|| {
            render_png_retained(
                &mut render_session,
                &mut document,
                &identities,
                document_identity,
                options.viewport,
                physical_width,
                physical_height,
            )
        })
        .transpose()?;
    let initial_paint_ms = elapsed_ms(initial_paint_started);

    let paint_order = collect_retained_paint_order(&document);
    let initial_report = build_report(
        &document,
        &source,
        options.viewport,
        Phase::Initial,
        &audit,
        &messages,
        &paint_order,
        None,
    );
    let mut artifacts = vec![make_artifact(Phase::Initial, initial_report, initial_png)?];

    let mut measurements = RunMeasurements {
        package_read_ms,
        html_parse_ms,
        initial_resolve_ms,
        initial_paint_ms,
        ..Default::default()
    };

    if options.run_interaction
        && let Some(target_id) = document.query_selector("#primary-action").ok().flatten()
    {
        let baseline = artifacts[0].report.clone();
        let target = document
            .get_node(target_id)
            .ok_or_else(|| RuntimeError::InvalidPackage("interaction target disappeared".into()))?;
        let point = target.absolute_position(
            target.final_layout.size.width / 2.0,
            target.final_layout.size.height / 2.0,
        );

        let hover_resolve_started = Instant::now();
        let state_changed = document.set_hover_to(point.x, point.y);
        let dirty_before = count_dirty_descendants(&document);
        let damaged_before = count_damaged_nodes(&document);
        document.resolve(0.0);
        let animation_running = document.is_animating();
        document.resolve(0.2);
        measurements.hover_resolve_ms = Some(elapsed_ms(hover_resolve_started));

        let mut hover_report = build_report(
            &document,
            &source,
            options.viewport,
            Phase::Hover,
            &audit,
            &messages,
            &collect_retained_paint_order(&document),
            None,
        );
        hover_report.interaction = Some(compare_interaction(
            &baseline,
            &hover_report,
            Phase::Hover,
            target_id,
            state_changed,
            dirty_before,
            damaged_before,
            animation_running,
        ));
        let hover_paint_started = Instant::now();
        let hover_png = options
            .render_png
            .then(|| {
                render_png_retained(
                    &mut render_session,
                    &mut document,
                    &identities,
                    document_identity,
                    options.viewport,
                    physical_width,
                    physical_height,
                )
            })
            .transpose()?;
        measurements.hover_paint_ms = Some(elapsed_ms(hover_paint_started));
        artifacts.push(make_artifact(Phase::Hover, hover_report, hover_png)?);

        let hover_baseline = artifacts
            .last()
            .expect("hover artifact was just pushed")
            .report
            .clone();
        let active_resolve_started = Instant::now();
        document.handle_ui_event(UiEvent::PointerDown(pointer_event(point.x, point.y, true)));
        let dirty_before = count_dirty_descendants(&document);
        let damaged_before = count_damaged_nodes(&document);
        document.resolve(0.25);
        let animation_running = document.is_animating();
        document.resolve(0.45);
        measurements.active_resolve_ms = Some(elapsed_ms(active_resolve_started));

        let mut active_report = build_report(
            &document,
            &source,
            options.viewport,
            Phase::Active,
            &audit,
            &messages,
            &collect_retained_paint_order(&document),
            None,
        );
        active_report.interaction = Some(compare_interaction(
            &hover_baseline,
            &active_report,
            Phase::Active,
            target_id,
            true,
            dirty_before,
            damaged_before,
            animation_running,
        ));
        let active_paint_started = Instant::now();
        let active_png = options
            .render_png
            .then(|| {
                render_png_retained(
                    &mut render_session,
                    &mut document,
                    &identities,
                    document_identity,
                    options.viewport,
                    physical_width,
                    physical_height,
                )
            })
            .transpose()?;
        measurements.active_paint_ms = Some(elapsed_ms(active_paint_started));
        artifacts.push(make_artifact(Phase::Active, active_report, active_png)?);

        document.handle_ui_event(UiEvent::PointerUp(pointer_event(point.x, point.y, false)));
    } else if options.run_interaction {
        messages.push(DiagnosticMessage {
            level: "warning".into(),
            code: "interaction.target_missing".into(),
            message: "No #primary-action element was found; interaction phases were skipped."
                .into(),
            node_id: None,
        });
    }

    let write_started = Instant::now();
    if let Some(output_directory) = &options.output_directory {
        write_artifacts(&mut artifacts, output_directory)?;
    }
    measurements.artifact_write_ms = elapsed_ms(write_started);
    measurements.total_ms = elapsed_ms(total_started);

    Ok(ExperimentRun {
        artifacts,
        measurements,
        package_root: root,
        package_snapshot,
    })
}

fn validate_viewport(options: &ExperimentOptions) -> Result<(), RuntimeError> {
    let viewport = options.viewport;
    if viewport.logical_width == 0 || viewport.logical_height == 0 {
        return Err(RuntimeError::InvalidPackage(
            "viewport dimensions must be nonzero".into(),
        ));
    }
    if !viewport.scale_factor.is_finite() || viewport.scale_factor <= 0.0 {
        return Err(RuntimeError::InvalidPackage(
            "viewport scale must be positive and finite".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_document_limits(document: &HtmlDocument) -> Result<(), RuntimeError> {
    if document.tree().len() > MAX_DOM_NODES {
        return Err(RuntimeError::LimitExceeded(format!(
            "parsed DOM contains {} nodes; limit is {MAX_DOM_NODES}",
            document.tree().len()
        )));
    }
    let mut stack = vec![(0usize, 0usize)];
    while let Some((node_id, depth)) = stack.pop() {
        if depth > MAX_DOM_DEPTH {
            return Err(RuntimeError::LimitExceeded(format!(
                "DOM nesting exceeds {MAX_DOM_DEPTH} levels"
            )));
        }
        let Some(node) = document.get_node(node_id) else {
            continue;
        };
        stack.extend(node.children.iter().rev().map(|child| (*child, depth + 1)));
    }
    Ok(())
}

fn inspect_document_profile(document: &HtmlDocument) -> Vec<DiagnosticMessage> {
    let mut messages = Vec::new();
    const UNSUPPORTED_TAGS: [&str; 12] = [
        "audio", "embed", "iframe", "object", "portal", "script", "source", "track", "video",
        "webview", "frame", "frameset",
    ];
    for (_, node) in document.tree().iter() {
        let Some(element) = node.element_data() else {
            continue;
        };
        let tag = element.name.local.as_ref();
        if UNSUPPORTED_TAGS.contains(&tag) {
            messages.push(DiagnosticMessage {
                level: "warning".into(),
                code: "html.element_unsupported".into(),
                message: format!("<{tag}> is outside the supported desktop profile and is inert."),
                node_id: Some(node.id),
            });
        }
        if tag == "a"
            && let Some(href) = element.attr(blitz_dom::local_name!("href"))
        {
            messages.push(DiagnosticMessage {
                level: "warning".into(),
                code: "navigation.disabled".into(),
                message: format!(
                    "Navigation is disabled; link target `{href}` will not be opened."
                ),
                node_id: Some(node.id),
            });
        }
    }
    messages.sort();
    messages
}

pub(crate) fn resolve_resources(
    document: &mut HtmlDocument,
    audit: &ResourceAudit,
    time: f64,
    messages: &mut Vec<DiagnosticMessage>,
) {
    let mut last_requests = usize::MAX;
    for _ in 0..MAX_RESOURCE_RESOLVE_PASSES {
        document.resolve(time);
        let requests = audit.request_count();
        if !document.has_pending_critical_resources() && requests == last_requests {
            return;
        }
        last_requests = requests;
    }
    messages.push(DiagnosticMessage {
        level: "warning".into(),
        code: "resource.resolve_limit".into(),
        message: format!(
            "Resource resolution did not converge within {MAX_RESOURCE_RESOLVE_PASSES} passes."
        ),
        node_id: None,
    });
}

#[allow(clippy::too_many_arguments)]
fn render_png_retained(
    session: &mut CpuRenderSession,
    document: &mut HtmlDocument,
    identities: &IdentityRegistry,
    document_identity: ExperimentalDocumentIdentity,
    viewport: ViewportSpec,
    physical_width: u32,
    physical_height: u32,
) -> Result<Vec<u8>, RuntimeError> {
    let scale_numerator = (f64::from(viewport.scale_factor) * 120.0).round() as u32;
    let mut reasons = FrameReasonSet::new();
    reasons.insert(FrameReason::ExplicitInvalidation);
    let frame = session
        .render_document(
            document,
            identities,
            document_identity,
            viewport,
            RenderSurfaceId {
                instance: 1,
                generation: 1,
            },
            physical_width,
            physical_height,
            scale_numerator,
            120,
            reasons,
            true,
        )?
        .ok_or_else(|| {
            RuntimeError::InvalidPackage(
                "forced headless retained rendering did not produce a frame".into(),
            )
        })?;
    encode_png(&frame.pixels, physical_width, physical_height)
}

pub(crate) fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RuntimeError> {
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| RuntimeError::Png(error.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|error| RuntimeError::Png(error.to_string()))?;
        writer
            .finish()
            .map_err(|error| RuntimeError::Png(error.to_string()))?;
    }
    Ok(encoded)
}

#[allow(clippy::too_many_arguments)]
fn build_report(
    document: &HtmlDocument,
    source: &Path,
    viewport: crate::model::ViewportSpec,
    phase: Phase,
    audit: &ResourceAudit,
    base_messages: &[DiagnosticMessage],
    paint_order: &[usize],
    interaction: Option<InteractionEvidence>,
) -> DiagnosticReport {
    let paint_indices: BTreeMap<_, _> = paint_order
        .iter()
        .enumerate()
        .map(|(index, node_id)| (*node_id, index))
        .collect();
    let mut diagnostics = base_messages.to_vec();
    for record in audit.records() {
        if record.decision != "loaded" {
            diagnostics.push(DiagnosticMessage {
                level: "warning".into(),
                code: format!("resource.{}", record.decision),
                message: format!("{}: {}", record.url, record.detail),
                node_id: None,
            });
        }
    }
    diagnostics.sort();
    diagnostics.dedup();

    DiagnosticReport {
        schema_version: DIAGNOSTIC_SCHEMA_VERSION,
        phase,
        viewport,
        renderer: "HTMShell retained scene + AnyRender Vello CPU reference rasterizer".into(),
        blitz_revision: BLITZ_REVISION,
        document_source: source.display().to_string(),
        node_count: document.tree().len(),
        retained_scene_order_kind: "Blitz element paint traversal; text paints inside inline roots"
            .into(),
        retained_scene_order: paint_order.to_vec(),
        fonts: collect_fonts(document),
        resources: audit.records(),
        diagnostics,
        unsupported_features: vec![
            "Detailed html5ever parse errors are not exposed by HtmlDocument::from_html.".into(),
            "Detailed Stylo CSS parse errors are not exposed by the selected convenience API."
                .into(),
            "Exact restyled-node and recomputed-layout-node counters are not exposed.".into(),
            "The CPU reference rasterizer rebuilds one backend-private AnyRender recording for each accepted retained-scene revision.".into(),
            "Networking, navigation, scripting, media playback, and embedded documents are disabled."
                .into(),
        ],
        interaction,
        tree: build_node(document, 0, &paint_indices),
    }
}

fn build_node(
    document: &HtmlDocument,
    node_id: usize,
    paint_indices: &BTreeMap<usize, usize>,
) -> DiagnosticNode {
    let node = document
        .get_node(node_id)
        .expect("bounded traversal only contains live nodes");
    let absolute = node.absolute_position(0.0, 0.0);
    let logical_bounds = safe_rect(
        absolute.x,
        absolute.y,
        node.final_layout.size.width,
        node.final_layout.size.height,
    );
    let element = node.element_data();
    let tag = element.map(|value| value.name.local.to_string());
    let html_id = element
        .and_then(|value| value.attr(blitz_dom::local_name!("id")))
        .map(str::to_owned);
    let mut classes: Vec<_> = element
        .and_then(|value| value.attr(blitz_dom::local_name!("class")))
        .into_iter()
        .flat_map(str::split_ascii_whitespace)
        .map(str::to_owned)
        .collect();
    classes.sort();

    let display = format!("{:?}", node.style.display).to_ascii_lowercase();
    let position = format!("{:?}", node.style.position).to_ascii_lowercase();
    let overflow_x = format!("{:?}", node.style.overflow.x).to_ascii_lowercase();
    let overflow_y = format!("{:?}", node.style.overflow.y).to_ascii_lowercase();
    let visibility = node
        .primary_styles()
        .map(|styles| format!("{:?}", styles.get_inherited_box().visibility).to_ascii_lowercase())
        .unwrap_or_else(|| "unresolved".into());
    let visible = display != "none" && visibility == "visible";

    let background_srgba = node.primary_styles().map(|styles| {
        let current = styles.clone_color();
        let color = styles
            .get_background()
            .background_color
            .resolve_to_absolute(&current)
            .to_color_space(ColorSpace::Srgb);
        let components = color.raw_components();
        [
            round(components[0]),
            round(components[1]),
            round(components[2]),
            round(components[3]),
        ]
    });
    let border_radii = border_radii(node);
    let text = text_diagnostic(node);
    let image = image_diagnostic(node);

    let mut child_ids = Vec::with_capacity(node.children.len() + 2);
    if let Some(before) = node.before {
        child_ids.push(before);
    }
    child_ids.extend(node.children.iter().copied());
    if let Some(after) = node.after {
        child_ids.push(after);
    }

    DiagnosticNode {
        experiment_node_id: node.id,
        parent_node_id: node.parent,
        node_kind: format!("{:?}", node.data.kind()).to_ascii_lowercase(),
        tag,
        html_id,
        classes,
        logical_bounds,
        display,
        position,
        visibility,
        visible,
        overflow: OverflowDiagnostic {
            establishes_clip: overflow_x != "visible" || overflow_y != "visible",
            x: overflow_x,
            y: overflow_y,
        },
        background_srgba,
        border_radii,
        text,
        image,
        hovered: node.is_hovered(),
        active: node.is_active(),
        retained_paint_order: paint_indices.get(&node.id).copied(),
        children: child_ids
            .into_iter()
            .filter(|child| document.get_node(*child).is_some())
            .map(|child| build_node(document, child, paint_indices))
            .collect(),
    }
}

pub(crate) fn border_radii(node: &Node) -> Option<CornerRadii> {
    let styles = node.primary_styles()?;
    let width = CSSPixelLength::new(node.final_layout.size.width);
    let height = CSSPixelLength::new(node.final_layout.size.height);
    let resolve = |radius: &stylo::values::computed::BorderCornerRadius| {
        [
            round(radius.0.width.0.resolve(width).px()),
            round(radius.0.height.0.resolve(height).px()),
        ]
    };
    let border = styles.get_border();
    let radii = CornerRadii {
        top_left: resolve(&border.border_top_left_radius),
        top_right: resolve(&border.border_top_right_radius),
        bottom_right: resolve(&border.border_bottom_right_radius),
        bottom_left: resolve(&border.border_bottom_left_radius),
    };
    let any_nonzero = [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ]
    .into_iter()
    .flatten()
    .any(|value| value > 0.0);
    any_nonzero.then_some(radii)
}

pub(crate) fn text_diagnostic(node: &Node) -> Option<TextDiagnostic> {
    let element = node.element_data()?;
    let layout = element.inline_layout_data.as_ref()?;
    let content = node.text_content();
    if content.trim().is_empty() {
        return None;
    }
    let scale = layout.layout.scale();
    let origin = node.absolute_position(
        node.final_layout.content_box_x(),
        node.final_layout.content_box_y(),
    );
    Some(TextDiagnostic {
        content: content.trim().to_owned(),
        measured_bounds: safe_rect(
            origin.x,
            origin.y,
            layout.layout.full_width() / scale,
            layout.layout.height() / scale,
        ),
        line_count: layout.layout.len(),
        right_to_left: layout.layout.is_rtl(),
    })
}

pub(crate) fn image_diagnostic(node: &Node) -> Option<ImageDiagnostic> {
    let element = node.element_data()?;
    let source = element.attr(blitz_dom::local_name!("src"))?.to_owned();
    let decoded_kind = match element.image_data() {
        Some(ImageData::Raster(_)) => "raster",
        Some(ImageData::Svg(_)) => "svg",
        Some(ImageData::None) | None => "unavailable",
    };
    Some(ImageDiagnostic {
        source,
        decoded_kind: decoded_kind.into(),
    })
}

pub(crate) fn collect_fonts(document: &HtmlDocument) -> Vec<FontRecord> {
    let mut fonts = BTreeSet::new();
    for (_, node) in document.tree().iter() {
        let Some(layout) = node
            .element_data()
            .and_then(|element| element.inline_layout_data.as_ref())
        else {
            continue;
        };
        for run in layout.layout.lines().flat_map(|line| line.runs()) {
            let font_data = run.font();
            let Ok(font) = FontRef::from_index(font_data.data.as_ref(), font_data.index) else {
                continue;
            };
            let string = |id: StringId| {
                font.localized_strings(id)
                    .english_or_first()
                    .map(|value| value.to_string())
            };
            fonts.insert(FontRecord {
                family: string(StringId::FAMILY_NAME)
                    .unwrap_or_else(|| "unknown family".to_owned()),
                subfamily: string(StringId::SUBFAMILY_NAME),
                postscript_name: string(StringId::POSTSCRIPT_NAME),
                face_index: font_data.index,
            });
        }
    }
    fonts.into_iter().collect()
}

pub(crate) fn collect_retained_paint_order(document: &HtmlDocument) -> Vec<usize> {
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(root) = document.try_root_element() {
        collect_paint_node(document, root.id, &mut seen, &mut order);
    }
    order
}

fn collect_paint_node(
    document: &HtmlDocument,
    node_id: usize,
    seen: &mut BTreeSet<usize>,
    order: &mut Vec<usize>,
) {
    if !seen.insert(node_id) {
        return;
    }
    order.push(node_id);
    let Some(node) = document.get_node(node_id) else {
        return;
    };
    if let Some(stacking) = &node.stacking_context {
        for child in stacking.neg_z_hoisted_children() {
            collect_paint_node(document, child.node_id, seen, order);
        }
    }
    if let Some(children) = node.paint_children.borrow().as_ref() {
        for child in children {
            collect_paint_node(document, *child, seen, order);
        }
    }
    if let Some(stacking) = &node.stacking_context {
        for child in stacking.pos_z_hoisted_children() {
            collect_paint_node(document, child.node_id, seen, order);
        }
    }
}

fn pointer_event(x: f32, y: f32, pressed: bool) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: MouseEventButton::Main,
        buttons: if pressed {
            MouseEventButtons::from(MouseEventButton::Main)
        } else {
            MouseEventButtons::empty()
        },
        mods: Default::default(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    }
}

pub(crate) fn count_dirty_descendants(document: &HtmlDocument) -> usize {
    document
        .tree()
        .iter()
        .filter(|(_, node)| node.has_dirty_descendants())
        .count()
}

pub(crate) fn count_damaged_nodes(document: &HtmlDocument) -> usize {
    document
        .tree()
        .iter()
        .filter(|(_, node)| node.damage().is_some_and(|damage| !damage.is_empty()))
        .count()
}

#[allow(clippy::too_many_arguments)]
fn compare_interaction(
    before: &DiagnosticReport,
    after: &DiagnosticReport,
    phase: Phase,
    target_node_id: usize,
    state_changed: bool,
    dirty_before: usize,
    damaged_before: usize,
    animation_running: bool,
) -> InteractionEvidence {
    let before_nodes = flatten_nodes(&before.tree);
    let after_nodes = flatten_nodes(&after.tree);
    let identity_retained = before_nodes.keys().eq(after_nodes.keys())
        && before_nodes.iter().all(|(id, node)| {
            after_nodes
                .get(id)
                .is_some_and(|other| node.node_kind == other.node_kind && node.tag == other.tag)
        });
    let changed_layout = before_nodes
        .iter()
        .filter(|(id, node)| {
            after_nodes
                .get(id)
                .is_some_and(|other| node.logical_bounds != other.logical_bounds)
        })
        .count();
    let changed_style = before_nodes
        .iter()
        .filter(|(id, node)| {
            after_nodes.get(id).is_some_and(|other| {
                node.display != other.display
                    || node.position != other.position
                    || node.visibility != other.visibility
                    || node.background_srgba != other.background_srgba
                    || node.border_radii != other.border_radii
                    || node.hovered != other.hovered
                    || node.active != other.active
            })
        })
        .count();
    let changed_paint = before_nodes
        .iter()
        .filter(|(id, node)| {
            after_nodes.get(id).is_some_and(|other| {
                node.background_srgba != other.background_srgba
                    || node.border_radii != other.border_radii
                    || node.visibility != other.visibility
                    || node.text != other.text
                    || node.image != other.image
                    || node.hovered != other.hovered
                    || node.active != other.active
            })
        })
        .count();
    let target = after_nodes.get(&target_node_id);
    InteractionEvidence {
        phase,
        target_node_id,
        state_changed,
        target_hovered: target.is_some_and(|node| node.hovered),
        target_active: target.is_some_and(|node| node.active),
        node_identity_retained: identity_retained,
        dom_tree_rebuilt: !identity_retained,
        dirty_descendant_flags_before_resolve: dirty_before,
        damaged_nodes_before_resolve: damaged_before,
        observably_changed_style_nodes: changed_style,
        observably_changed_layout_nodes: changed_layout,
        observably_changed_paint_signature_nodes: changed_paint,
        exact_nodes_restyled: None,
        exact_layout_nodes_recomputed: None,
        exact_paint_nodes_regenerated: None,
        animation_running_after_state_change: animation_running,
        full_anyrender_scene_rebuilt: true,
    }
}

fn flatten_nodes(root: &DiagnosticNode) -> BTreeMap<usize, &DiagnosticNode> {
    let mut nodes = BTreeMap::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        nodes.insert(node.experiment_node_id, node);
        stack.extend(node.children.iter().rev());
    }
    nodes
}

fn make_artifact(
    phase: Phase,
    report: DiagnosticReport,
    png: Option<Vec<u8>>,
) -> Result<Artifact, RuntimeError> {
    let mut diagnostic_json = serde_json::to_vec_pretty(&report)?;
    diagnostic_json.push(b'\n');
    Ok(Artifact {
        phase,
        report,
        diagnostic_json,
        png,
        diagnostic_path: None,
        png_path: None,
    })
}

fn write_artifacts(
    artifacts: &mut [Artifact],
    output_directory: &Path,
) -> Result<(), RuntimeError> {
    std::fs::create_dir_all(output_directory).map_err(|error| {
        RuntimeError::io("create artifact output directory", output_directory, error)
    })?;
    for artifact in artifacts {
        let json_path = output_directory.join(format!("{}.json", artifact.phase.filename()));
        std::fs::write(&json_path, &artifact.diagnostic_json)
            .map_err(|error| RuntimeError::io("write diagnostic JSON", &json_path, error))?;
        artifact.diagnostic_path = Some(json_path);
        if let Some(png) = &artifact.png {
            let png_path = output_directory.join(format!("{}.png", artifact.phase.filename()));
            let mut file = std::fs::File::create(&png_path)
                .map_err(|error| RuntimeError::io("create PNG", &png_path, error))?;
            file.write_all(png)
                .map_err(|error| RuntimeError::io("write PNG", &png_path, error))?;
            artifact.png_path = Some(png_path);
        }
    }
    Ok(())
}

pub(crate) fn safe_rect(x: f32, y: f32, width: f32, height: f32) -> LogicalRect {
    LogicalRect {
        x: safe_float(x),
        y: safe_float(y),
        width: safe_float(width),
        height: safe_float(height),
    }
}

fn safe_float(value: f32) -> f32 {
    if value.is_finite() { round(value) } else { 0.0 }
}

pub(crate) fn round(value: f32) -> f32 {
    (value * 1000.0).round() / 1000.0
}

pub(crate) fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
