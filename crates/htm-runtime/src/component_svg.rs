use crate::component_resource::{
    ComponentResourcePath, ComponentResourceSemanticVersion, ComponentResourceSourceId,
    MAX_COMPONENT_SVG_DEPTH, MAX_COMPONENT_SVG_HEIGHT, MAX_COMPONENT_SVG_NODES,
    MAX_COMPONENT_SVG_PATH_SEGMENTS, MAX_COMPONENT_SVG_PIXELS, MAX_COMPONENT_SVG_WIDTH,
};
use crate::package::PackageId;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use svgtypes::{Length, LengthUnit, PathParser, PathSegment, TransformListParser, ViewBox};

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const MAX_COMPONENT_SVG_XML_NODES: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComponentSvgViewBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentSvgStatistics {
    pub node_count: usize,
    pub maximum_depth: usize,
    pub path_segment_count: usize,
}

#[derive(Debug)]
pub struct ComponentSvgSource {
    id: ComponentResourceSourceId,
    encoded_bytes: u64,
    width: f32,
    height: f32,
    view_box: ComponentSvgViewBox,
    tree: Arc<usvg::Tree>,
    statistics: ComponentSvgStatistics,
    semantic_version: ComponentResourceSemanticVersion,
}

impl ComponentSvgSource {
    pub(crate) fn parse(
        id: ComponentResourceSourceId,
        encoded: &[u8],
    ) -> Result<(Self, ComponentSvgResolverStatistics), ComponentSvgError> {
        let text = std::str::from_utf8(encoded).map_err(|_| {
            ComponentSvgError::new(
                ComponentSvgErrorKind::MalformedXml,
                "SVG source must be UTF-8 XML",
            )
        })?;
        let options = usvg::roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_COMPONENT_SVG_XML_NODES,
            entity_resolver: None,
        };
        let document =
            usvg::roxmltree::Document::parse_with_options(text, options).map_err(xml_error)?;
        let validated = validate_document(&document)?;

        let resolver_statistics = ResolverCounters::default();
        let image_data_calls = Arc::clone(&resolver_statistics.image_data_calls);
        let image_string_calls = Arc::clone(&resolver_statistics.image_string_calls);
        let font_select_calls = Arc::clone(&resolver_statistics.font_select_calls);
        let font_fallback_calls = Arc::clone(&resolver_statistics.font_fallback_calls);
        let parser_options = usvg::Options {
            resources_dir: None,
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_data: Box::new(move |_, _, _| {
                    image_data_calls.fetch_add(1, Ordering::Relaxed);
                    None
                }),
                resolve_string: Box::new(move |_, _| {
                    image_string_calls.fetch_add(1, Ordering::Relaxed);
                    None
                }),
            },
            font_resolver: usvg::FontResolver {
                select_font: Box::new(move |_, _| {
                    font_select_calls.fetch_add(1, Ordering::Relaxed);
                    None
                }),
                select_fallback: Box::new(move |_, _, _| {
                    font_fallback_calls.fetch_add(1, Ordering::Relaxed);
                    None
                }),
            },
            fontdb: Arc::new(usvg::fontdb::Database::new()),
            style_sheet: None,
            ..usvg::Options::default()
        };
        let tree = usvg::Tree::from_xmltree(&document, &parser_options).map_err(|_| {
            ComponentSvgError::new(
                ComponentSvgErrorKind::ParseFailure,
                "validated simple SVG could not be normalized",
            )
        })?;
        let resolver_statistics = resolver_statistics.snapshot();
        if resolver_statistics.image_data_resolver_calls != 0
            || resolver_statistics.image_string_resolver_calls != 0
            || resolver_statistics.font_selection_calls != 0
            || resolver_statistics.font_fallback_calls != 0
        {
            return Err(ComponentSvgError::new(
                ComponentSvgErrorKind::SubresourceForbidden,
                "simple SVG attempted to invoke a disabled subresource resolver",
            ));
        }
        let path_segment_count = validate_normalized_tree(&tree)?;
        let statistics = ComponentSvgStatistics {
            node_count: validated.node_count,
            maximum_depth: validated.maximum_depth,
            path_segment_count,
        };
        let semantic_version = ComponentResourceSemanticVersion::new(svg_semantic_version(
            encoded,
            validated.width,
            validated.height,
            validated.view_box,
            statistics,
        ));
        Ok((
            Self {
                id,
                encoded_bytes: encoded.len() as u64,
                width: validated.width,
                height: validated.height,
                view_box: validated.view_box,
                tree: Arc::new(tree),
                statistics,
                semantic_version,
            },
            resolver_statistics,
        ))
    }

    pub fn id(&self) -> &ComponentResourceSourceId {
        &self.id
    }

    pub fn package_id(&self) -> &PackageId {
        self.id.package_id()
    }

    pub fn path(&self) -> &ComponentResourcePath {
        self.id.path()
    }

    pub fn encoded_bytes(&self) -> u64 {
        self.encoded_bytes
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn view_box(&self) -> ComponentSvgViewBox {
        self.view_box
    }

    pub fn tree(&self) -> &Arc<usvg::Tree> {
        &self.tree
    }

    pub fn statistics(&self) -> ComponentSvgStatistics {
        self.statistics
    }

    pub fn semantic_version(&self) -> &ComponentResourceSemanticVersion {
        &self.semantic_version
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComponentSvgResolverStatistics {
    pub secondary_filesystem_reads: usize,
    pub data_image_decodes: usize,
    pub image_data_resolver_calls: usize,
    pub image_string_resolver_calls: usize,
    pub font_selection_calls: usize,
    pub font_fallback_calls: usize,
    pub network_attempts: usize,
}

impl ComponentSvgResolverStatistics {
    pub(crate) fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            secondary_filesystem_reads: self
                .secondary_filesystem_reads
                .checked_add(other.secondary_filesystem_reads)?,
            data_image_decodes: self
                .data_image_decodes
                .checked_add(other.data_image_decodes)?,
            image_data_resolver_calls: self
                .image_data_resolver_calls
                .checked_add(other.image_data_resolver_calls)?,
            image_string_resolver_calls: self
                .image_string_resolver_calls
                .checked_add(other.image_string_resolver_calls)?,
            font_selection_calls: self
                .font_selection_calls
                .checked_add(other.font_selection_calls)?,
            font_fallback_calls: self
                .font_fallback_calls
                .checked_add(other.font_fallback_calls)?,
            network_attempts: self.network_attempts.checked_add(other.network_attempts)?,
        })
    }
}

#[derive(Default)]
struct ResolverCounters {
    image_data_calls: Arc<AtomicUsize>,
    image_string_calls: Arc<AtomicUsize>,
    font_select_calls: Arc<AtomicUsize>,
    font_fallback_calls: Arc<AtomicUsize>,
}

impl ResolverCounters {
    fn snapshot(&self) -> ComponentSvgResolverStatistics {
        ComponentSvgResolverStatistics {
            image_data_resolver_calls: self.image_data_calls.load(Ordering::Relaxed),
            image_string_resolver_calls: self.image_string_calls.load(Ordering::Relaxed),
            font_selection_calls: self.font_select_calls.load(Ordering::Relaxed),
            font_fallback_calls: self.font_fallback_calls.load(Ordering::Relaxed),
            ..ComponentSvgResolverStatistics::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComponentSvgErrorKind {
    MalformedXml,
    DoctypeForbidden,
    EntityForbidden,
    ProcessingInstructionForbidden,
    RootInvalid,
    ElementForbidden,
    AttributeForbidden,
    NamespaceForbidden,
    CssForbidden,
    StyleAttributeForbidden,
    SubresourceForbidden,
    ImageForbidden,
    DataImageForbidden,
    FontForbidden,
    TextForbidden,
    LinkForbidden,
    FragmentForbidden,
    ScriptForbidden,
    AnimationForbidden,
    PaintServerForbidden,
    GradientForbidden,
    PatternForbidden,
    ClipForbidden,
    MaskForbidden,
    FilterForbidden,
    MarkerForbidden,
    SymbolForbidden,
    UseForbidden,
    IntrinsicSizeInvalid,
    NaturalDimensionLimit,
    NodeLimit,
    DepthLimit,
    PathSegmentLimit,
    NonfiniteGeometry,
    TransformInvalid,
    ParseFailure,
    TreeValidationFailure,
}

#[derive(Debug, Clone)]
pub(crate) struct ComponentSvgError {
    pub kind: ComponentSvgErrorKind,
    pub message: &'static str,
    pub line: u32,
    pub column: u32,
}

impl ComponentSvgError {
    fn new(kind: ComponentSvgErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            line: 1,
            column: 1,
        }
    }

    fn at(mut self, position: usvg::roxmltree::TextPos) -> Self {
        self.line = position.row;
        self.column = position.col;
        self
    }
}

struct ValidatedDocument {
    width: f32,
    height: f32,
    view_box: ComponentSvgViewBox,
    node_count: usize,
    maximum_depth: usize,
}

fn xml_error(error: usvg::roxmltree::Error) -> ComponentSvgError {
    let message = error.to_string();
    let kind = if message.contains("DTD") {
        ComponentSvgErrorKind::DoctypeForbidden
    } else if message.contains("entity") || message.contains("Entity") {
        ComponentSvgErrorKind::EntityForbidden
    } else {
        ComponentSvgErrorKind::MalformedXml
    };
    ComponentSvgError::new(kind, "component SVG is not permitted well-formed XML").at(error.pos())
}

fn validate_document(
    document: &usvg::roxmltree::Document<'_>,
) -> Result<ValidatedDocument, ComponentSvgError> {
    for node in document.descendants() {
        match node.node_type() {
            usvg::roxmltree::NodeType::PI => {
                return Err(node_error(
                    document,
                    node,
                    ComponentSvgErrorKind::ProcessingInstructionForbidden,
                    "processing instructions are forbidden in component SVG",
                ));
            }
            usvg::roxmltree::NodeType::Text => {
                let parent_is_allowed = node.parent_element().is_some_and(|parent| {
                    matches!(
                        parent.tag_name().name(),
                        "svg"
                            | "g"
                            | "path"
                            | "rect"
                            | "circle"
                            | "ellipse"
                            | "line"
                            | "polyline"
                            | "polygon"
                    )
                });
                if parent_is_allowed && node.text().is_some_and(|value| !value.trim().is_empty()) {
                    return Err(node_error(
                        document,
                        node,
                        ComponentSvgErrorKind::TextForbidden,
                        "text content is forbidden in component SVG",
                    ));
                }
            }
            _ => {}
        }
    }

    let root = document.root_element();
    if root.tag_name().namespace() != Some(SVG_NAMESPACE) || root.tag_name().name() != "svg" {
        return Err(node_error(
            document,
            root,
            ComponentSvgErrorKind::RootInvalid,
            "component SVG must have one SVG namespace svg root",
        ));
    }

    let view_box_value = root.attribute("viewBox").ok_or_else(|| {
        node_error(
            document,
            root,
            ComponentSvgErrorKind::IntrinsicSizeInvalid,
            "component SVG root requires a positive finite viewBox",
        )
    })?;
    let view_box = ViewBox::from_str(view_box_value).map_err(|_| {
        attribute_error(
            document,
            root,
            "viewBox",
            ComponentSvgErrorKind::IntrinsicSizeInvalid,
            "component SVG viewBox must contain four finite numbers and positive dimensions",
        )
    })?;
    if ![view_box.x, view_box.y, view_box.w, view_box.h]
        .into_iter()
        .all(finite_internal)
    {
        return Err(attribute_error(
            document,
            root,
            "viewBox",
            ComponentSvgErrorKind::NonfiniteGeometry,
            "component SVG viewBox values must be finite",
        ));
    }
    let width_attribute = root.attribute("width");
    let height_attribute = root.attribute("height");
    let (width, height) = match (width_attribute, height_attribute) {
        (None, None) => (view_box.w, view_box.h),
        (Some(width), Some(height)) => (
            parse_positive_context_free_length(width).map_err(|kind| {
                attribute_error(
                    document,
                    root,
                    "width",
                    kind,
                    "component SVG width must be a positive finite unitless or px length",
                )
            })?,
            parse_positive_context_free_length(height).map_err(|kind| {
                attribute_error(
                    document,
                    root,
                    "height",
                    kind,
                    "component SVG height must be a positive finite unitless or px length",
                )
            })?,
        ),
        _ => {
            return Err(node_error(
                document,
                root,
                ComponentSvgErrorKind::IntrinsicSizeInvalid,
                "component SVG width and height must be omitted together or provided together",
            ));
        }
    };
    validate_natural_size(width, height).map_err(|kind| {
        node_error(
            document,
            root,
            kind,
            "component SVG natural size is invalid",
        )
    })?;

    let mut node_count = 0usize;
    let mut maximum_depth = 0usize;
    for element in document
        .descendants()
        .filter(usvg::roxmltree::Node::is_element)
    {
        if element.tag_name().namespace() != Some(SVG_NAMESPACE) {
            return Err(node_error(
                document,
                element,
                ComponentSvgErrorKind::NamespaceForbidden,
                "foreign namespace elements are forbidden in component SVG",
            ));
        }
        let name = element.tag_name().name();
        let allowed = matches!(
            name,
            "svg" | "g" | "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon"
        );
        if !allowed {
            let kind = if name == "image"
                && element.attributes().any(|attribute| {
                    attribute.name() == "href" && attribute.value().starts_with("data:")
                }) {
                ComponentSvgErrorKind::DataImageForbidden
            } else {
                forbidden_element_kind(name)
            };
            return Err(node_error(
                document,
                element,
                kind,
                "SVG element is outside the simple component SVG profile",
            ));
        }
        if name == "svg" && element != root {
            return Err(node_error(
                document,
                element,
                ComponentSvgErrorKind::ElementForbidden,
                "nested svg elements are forbidden in component SVG",
            ));
        }
        node_count = node_count.checked_add(1).ok_or_else(|| {
            node_error(
                document,
                element,
                ComponentSvgErrorKind::NodeLimit,
                "component SVG node count overflowed",
            )
        })?;
        if node_count > MAX_COMPONENT_SVG_NODES {
            return Err(node_error(
                document,
                element,
                ComponentSvgErrorKind::NodeLimit,
                "component SVG exceeds the allowed node count",
            ));
        }
        let depth = element
            .ancestors()
            .filter(usvg::roxmltree::Node::is_element)
            .count();
        maximum_depth = maximum_depth.max(depth);
        if depth > MAX_COMPONENT_SVG_DEPTH {
            return Err(node_error(
                document,
                element,
                ComponentSvgErrorKind::DepthLimit,
                "component SVG exceeds the allowed element depth",
            ));
        }
        validate_element_attributes(document, element)?;
        validate_geometry(document, element)?;
    }

    Ok(ValidatedDocument {
        width: width as f32,
        height: height as f32,
        view_box: ComponentSvgViewBox {
            x: view_box.x as f32,
            y: view_box.y as f32,
            width: view_box.w as f32,
            height: view_box.h as f32,
        },
        node_count,
        maximum_depth,
    })
}

fn validate_element_attributes(
    document: &usvg::roxmltree::Document<'_>,
    element: usvg::roxmltree::Node<'_, '_>,
) -> Result<(), ComponentSvgError> {
    for attribute in element.attributes() {
        let name = attribute.name();
        if attribute.namespace().is_some() {
            return Err(attribute_error(
                document,
                element,
                name,
                if name == "href" {
                    ComponentSvgErrorKind::LinkForbidden
                } else {
                    ComponentSvgErrorKind::NamespaceForbidden
                },
                "namespaced attributes are forbidden in component SVG",
            ));
        }
        let common = matches!(
            name,
            "opacity"
                | "fill"
                | "fill-opacity"
                | "fill-rule"
                | "stroke"
                | "stroke-opacity"
                | "stroke-width"
                | "stroke-linecap"
                | "stroke-linejoin"
                | "stroke-miterlimit"
                | "stroke-dasharray"
                | "stroke-dashoffset"
        );
        let per_element = match element.tag_name().name() {
            "svg" => matches!(name, "width" | "height" | "viewBox" | "preserveAspectRatio"),
            "g" => name == "transform",
            "path" => matches!(name, "d" | "transform"),
            "rect" => matches!(
                name,
                "x" | "y" | "width" | "height" | "rx" | "ry" | "transform"
            ),
            "circle" => matches!(name, "cx" | "cy" | "r" | "transform"),
            "ellipse" => matches!(name, "cx" | "cy" | "rx" | "ry" | "transform"),
            "line" => matches!(name, "x1" | "y1" | "x2" | "y2" | "transform"),
            "polyline" | "polygon" => matches!(name, "points" | "transform"),
            _ => false,
        };
        if !common && !per_element {
            return Err(attribute_error(
                document,
                element,
                name,
                forbidden_attribute_kind(name),
                "SVG attribute is outside the simple component SVG profile",
            ));
        }
        validate_attribute_value(document, element, attribute)?;
    }
    Ok(())
}

fn validate_attribute_value(
    document: &usvg::roxmltree::Document<'_>,
    element: usvg::roxmltree::Node<'_, '_>,
    attribute: usvg::roxmltree::Attribute<'_, '_>,
) -> Result<(), ComponentSvgError> {
    let name = attribute.name();
    let value = attribute.value();
    let invalid = |kind, message| attribute_error(document, element, name, kind, message);
    match name {
        "fill" | "stroke" => validate_paint(value).map_err(|kind| {
            invalid(
                kind,
                "component SVG paint must be none or an explicit hexadecimal solid color",
            )
        }),
        "opacity" | "fill-opacity" | "stroke-opacity" => {
            validate_unit_interval(value).map_err(|kind| {
                invalid(
                    kind,
                    "component SVG opacity must be a finite number from zero through one",
                )
            })
        }
        "fill-rule" => {
            if matches!(value, "nonzero" | "evenodd") {
                Ok(())
            } else {
                Err(invalid(
                    ComponentSvgErrorKind::AttributeForbidden,
                    "component SVG fill-rule must be nonzero or evenodd",
                ))
            }
        }
        "stroke-linecap" => validate_keyword(value, &["butt", "round", "square"], invalid),
        "stroke-linejoin" => {
            validate_keyword(value, &["miter", "miter-clip", "round", "bevel"], invalid)
        }
        "stroke-width" | "stroke-miterlimit" => validate_nonnegative_context_free_length(value)
            .map_err(|kind| {
                invalid(
                    kind,
                    "component SVG stroke values must be finite nonnegative unitless or px lengths",
                )
            }),
        "stroke-dasharray" => validate_dasharray(value).map_err(|kind| {
            invalid(
                kind,
                "component SVG dash arrays must contain finite nonnegative unitless or px lengths",
            )
        }),
        "stroke-dashoffset" => parse_context_free_length(value)
            .map(|_| ())
            .map_err(|kind| {
                invalid(
                    kind,
                    "component SVG dash offsets must be finite unitless or px lengths",
                )
            }),
        "transform" => validate_transform(value).map_err(|_| {
            invalid(
                ComponentSvgErrorKind::TransformInvalid,
                "component SVG transform is invalid or nonfinite",
            )
        }),
        "preserveAspectRatio" => {
            let value = svgtypes::AspectRatio::from_str(value).map_err(|_| {
                invalid(
                    ComponentSvgErrorKind::AttributeForbidden,
                    "component SVG preserveAspectRatio is invalid",
                )
            })?;
            if value.defer {
                return Err(invalid(
                    ComponentSvgErrorKind::AttributeForbidden,
                    "component SVG preserveAspectRatio does not permit defer",
                ));
            }
            Ok(())
        }
        "viewBox" => Ok(()),
        "d" | "points" => Ok(()),
        "width" | "height" if element.tag_name().name() == "svg" => Ok(()),
        _ => parse_context_free_length(value)
            .map(|_| ())
            .map_err(|kind| {
                invalid(
                    kind,
                    "component SVG geometry must use finite unitless or px lengths",
                )
            }),
    }
}

fn validate_geometry(
    document: &usvg::roxmltree::Document<'_>,
    element: usvg::roxmltree::Node<'_, '_>,
) -> Result<(), ComponentSvgError> {
    let invalid =
        |attribute, kind, message| attribute_error(document, element, attribute, kind, message);
    match element.tag_name().name() {
        "path" => {
            let data = element.attribute("d").ok_or_else(|| {
                node_error(
                    document,
                    element,
                    ComponentSvgErrorKind::AttributeForbidden,
                    "component SVG path requires d",
                )
            })?;
            let mut count = 0usize;
            for segment in PathParser::from(data) {
                let segment = segment.map_err(|_| {
                    invalid(
                        "d",
                        ComponentSvgErrorKind::TreeValidationFailure,
                        "component SVG path data is invalid",
                    )
                })?;
                validate_path_segment(segment).map_err(|kind| {
                    invalid("d", kind, "component SVG path values must be finite")
                })?;
                count = count.checked_add(1).ok_or_else(|| {
                    invalid(
                        "d",
                        ComponentSvgErrorKind::PathSegmentLimit,
                        "component SVG path segment count overflowed",
                    )
                })?;
                if count > MAX_COMPONENT_SVG_PATH_SEGMENTS {
                    return Err(invalid(
                        "d",
                        ComponentSvgErrorKind::PathSegmentLimit,
                        "component SVG exceeds the path segment limit",
                    ));
                }
            }
            if count == 0 {
                return Err(invalid(
                    "d",
                    ComponentSvgErrorKind::TreeValidationFailure,
                    "component SVG path data must contain geometry",
                ));
            }
        }
        "rect" => {
            require_positive_geometry(document, element, "width")?;
            require_positive_geometry(document, element, "height")?;
        }
        "circle" => require_positive_geometry(document, element, "r")?,
        "ellipse" => {
            require_positive_geometry(document, element, "rx")?;
            require_positive_geometry(document, element, "ry")?;
        }
        "polyline" | "polygon" => {
            let points = element.attribute("points").ok_or_else(|| {
                node_error(
                    document,
                    element,
                    ComponentSvgErrorKind::AttributeForbidden,
                    "component SVG polyline and polygon require points",
                )
            })?;
            let values = parse_points_strict(points).map_err(|kind| {
                invalid(
                    "points",
                    kind,
                    "component SVG point values must be a complete finite coordinate list",
                )
            })?;
            let minimum = if element.tag_name().name() == "polygon" {
                3
            } else {
                2
            };
            if values < minimum {
                return Err(invalid(
                    "points",
                    ComponentSvgErrorKind::TreeValidationFailure,
                    "component SVG point list contains too few points",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_normalized_tree(tree: &usvg::Tree) -> Result<usize, ComponentSvgError> {
    if tree.has_defs_nodes() {
        return Err(ComponentSvgError::new(
            ComponentSvgErrorKind::TreeValidationFailure,
            "normalized component SVG unexpectedly contains definition resources",
        ));
    }
    let mut segment_count = 0usize;
    let mut groups = vec![tree.root()];
    while let Some(group) = groups.pop() {
        if !group.id().is_empty()
            || group.clip_path().is_some()
            || group.mask().is_some()
            || !group.filters().is_empty()
            || !group.transform().is_finite()
            || !group.abs_transform().is_finite()
        {
            return Err(ComponentSvgError::new(
                ComponentSvgErrorKind::TreeValidationFailure,
                "normalized component SVG contains forbidden group state",
            ));
        }
        for node in group.children() {
            if !node.id().is_empty() || !node.abs_transform().is_finite() {
                return Err(ComponentSvgError::new(
                    ComponentSvgErrorKind::TreeValidationFailure,
                    "normalized component SVG contains forbidden node state",
                ));
            }
            match node {
                usvg::Node::Group(child) => groups.push(child),
                usvg::Node::Path(path) => {
                    if path
                        .fill()
                        .is_some_and(|fill| !matches!(fill.paint(), usvg::Paint::Color(_)))
                        || path
                            .stroke()
                            .is_some_and(|stroke| !matches!(stroke.paint(), usvg::Paint::Color(_)))
                    {
                        return Err(ComponentSvgError::new(
                            ComponentSvgErrorKind::PaintServerForbidden,
                            "normalized component SVG contains a paint server",
                        ));
                    }
                    segment_count = segment_count
                        .checked_add(path.data().segments().count())
                        .ok_or_else(|| {
                            ComponentSvgError::new(
                                ComponentSvgErrorKind::PathSegmentLimit,
                                "normalized component SVG path segment count overflowed",
                            )
                        })?;
                    if segment_count > MAX_COMPONENT_SVG_PATH_SEGMENTS {
                        return Err(ComponentSvgError::new(
                            ComponentSvgErrorKind::PathSegmentLimit,
                            "component SVG exceeds the normalized path segment limit",
                        ));
                    }
                }
                usvg::Node::Image(_) => {
                    return Err(ComponentSvgError::new(
                        ComponentSvgErrorKind::ImageForbidden,
                        "normalized component SVG unexpectedly contains an image",
                    ));
                }
                usvg::Node::Text(_) => {
                    return Err(ComponentSvgError::new(
                        ComponentSvgErrorKind::TextForbidden,
                        "normalized component SVG unexpectedly contains text",
                    ));
                }
            }
        }
    }
    Ok(segment_count)
}

fn validate_natural_size(width: f64, height: f64) -> Result<(), ComponentSvgErrorKind> {
    if !finite_internal(width) || !finite_internal(height) {
        return Err(ComponentSvgErrorKind::NonfiniteGeometry);
    }
    if width <= 0.0 || height <= 0.0 {
        return Err(ComponentSvgErrorKind::IntrinsicSizeInvalid);
    }
    if width > f64::from(MAX_COMPONENT_SVG_WIDTH) || height > f64::from(MAX_COMPONENT_SVG_HEIGHT) {
        return Err(ComponentSvgErrorKind::NaturalDimensionLimit);
    }
    let pixels = width.ceil() * height.ceil();
    if !pixels.is_finite() || pixels > MAX_COMPONENT_SVG_PIXELS as f64 {
        return Err(ComponentSvgErrorKind::NaturalDimensionLimit);
    }
    Ok(())
}

fn parse_positive_context_free_length(value: &str) -> Result<f64, ComponentSvgErrorKind> {
    let value = parse_context_free_length(value)?;
    if value <= 0.0 {
        Err(ComponentSvgErrorKind::IntrinsicSizeInvalid)
    } else {
        Ok(value)
    }
}

fn parse_context_free_length(value: &str) -> Result<f64, ComponentSvgErrorKind> {
    let length = Length::from_str(value).map_err(|_| ComponentSvgErrorKind::AttributeForbidden)?;
    if !matches!(length.unit, LengthUnit::None | LengthUnit::Px) {
        return Err(ComponentSvgErrorKind::AttributeForbidden);
    }
    if !finite_internal(length.number) {
        return Err(ComponentSvgErrorKind::NonfiniteGeometry);
    }
    Ok(length.number)
}

fn validate_nonnegative_context_free_length(value: &str) -> Result<(), ComponentSvgErrorKind> {
    let value = parse_context_free_length(value)?;
    if value < 0.0 {
        Err(ComponentSvgErrorKind::AttributeForbidden)
    } else {
        Ok(())
    }
}

fn validate_unit_interval(value: &str) -> Result<(), ComponentSvgErrorKind> {
    let value = f64::from_str(value).map_err(|_| ComponentSvgErrorKind::AttributeForbidden)?;
    if !finite_internal(value) {
        return Err(ComponentSvgErrorKind::NonfiniteGeometry);
    }
    if !(0.0..=1.0).contains(&value) {
        return Err(ComponentSvgErrorKind::AttributeForbidden);
    }
    Ok(())
}

fn validate_paint(value: &str) -> Result<(), ComponentSvgErrorKind> {
    if value == "none" {
        return Ok(());
    }
    if value == "currentColor" || value.contains("var(") {
        return Err(ComponentSvgErrorKind::CssForbidden);
    }
    if value.contains("url(") || value.starts_with('#') && !is_hex_color(value) {
        return Err(ComponentSvgErrorKind::PaintServerForbidden);
    }
    if is_hex_color(value) {
        Ok(())
    } else {
        Err(ComponentSvgErrorKind::AttributeForbidden)
    }
}

fn is_hex_color(value: &str) -> bool {
    matches!(value.len(), 4 | 5 | 7 | 9)
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_keyword<F>(value: &str, allowed: &[&str], invalid: F) -> Result<(), ComponentSvgError>
where
    F: Fn(ComponentSvgErrorKind, &'static str) -> ComponentSvgError,
{
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(invalid(
            ComponentSvgErrorKind::AttributeForbidden,
            "component SVG attribute value is outside the allowed profile",
        ))
    }
}

fn validate_dasharray(value: &str) -> Result<(), ComponentSvgErrorKind> {
    if value == "none" {
        return Ok(());
    }
    let normalized = value.replace(',', " ");
    let mut count = 0usize;
    for part in normalized.split_ascii_whitespace() {
        validate_nonnegative_context_free_length(part)?;
        count += 1;
    }
    if count == 0 {
        return Err(ComponentSvgErrorKind::AttributeForbidden);
    }
    Ok(())
}

fn validate_transform(value: &str) -> Result<(), ()> {
    let mut count = 0usize;
    for token in TransformListParser::from(value) {
        let token = token.map_err(|_| ())?;
        let finite = match token {
            svgtypes::TransformListToken::Matrix { a, b, c, d, e, f } => {
                [a, b, c, d, e, f].into_iter().all(finite_internal)
            }
            svgtypes::TransformListToken::Translate { tx, ty } => {
                [tx, ty].into_iter().all(finite_internal)
            }
            svgtypes::TransformListToken::Scale { sx, sy } => {
                [sx, sy].into_iter().all(finite_internal)
            }
            svgtypes::TransformListToken::Rotate { angle }
            | svgtypes::TransformListToken::SkewX { angle }
            | svgtypes::TransformListToken::SkewY { angle } => finite_internal(angle),
        };
        if !finite {
            return Err(());
        }
        count += 1;
    }
    if count == 0 { Err(()) } else { Ok(()) }
}

fn validate_path_segment(segment: PathSegment) -> Result<(), ComponentSvgErrorKind> {
    let finite = match segment {
        PathSegment::MoveTo { x, y, .. }
        | PathSegment::LineTo { x, y, .. }
        | PathSegment::SmoothQuadratic { x, y, .. } => [x, y].into_iter().all(finite_internal),
        PathSegment::HorizontalLineTo { x, .. } => finite_internal(x),
        PathSegment::VerticalLineTo { y, .. } => finite_internal(y),
        PathSegment::CurveTo {
            x1,
            y1,
            x2,
            y2,
            x,
            y,
            ..
        } => [x1, y1, x2, y2, x, y].into_iter().all(finite_internal),
        PathSegment::SmoothCurveTo { x2, y2, x, y, .. } => {
            [x2, y2, x, y].into_iter().all(finite_internal)
        }
        PathSegment::Quadratic { x1, y1, x, y, .. } => {
            [x1, y1, x, y].into_iter().all(finite_internal)
        }
        PathSegment::EllipticalArc {
            rx,
            ry,
            x_axis_rotation,
            x,
            y,
            ..
        } => [rx, ry, x_axis_rotation, x, y]
            .into_iter()
            .all(finite_internal),
        PathSegment::ClosePath { .. } => true,
    };
    if finite {
        Ok(())
    } else {
        Err(ComponentSvgErrorKind::NonfiniteGeometry)
    }
}

fn parse_points_strict(value: &str) -> Result<usize, ComponentSvgErrorKind> {
    let normalized = value.replace(',', " ");
    let parts = normalized.split_ascii_whitespace().collect::<Vec<_>>();
    if parts.is_empty() || parts.len() % 2 != 0 {
        return Err(ComponentSvgErrorKind::TreeValidationFailure);
    }
    for part in &parts {
        let number =
            f64::from_str(part).map_err(|_| ComponentSvgErrorKind::TreeValidationFailure)?;
        if !finite_internal(number) {
            return Err(ComponentSvgErrorKind::NonfiniteGeometry);
        }
    }
    Ok(parts.len() / 2)
}

fn finite_internal(value: f64) -> bool {
    value.is_finite() && (value as f32).is_finite()
}

fn require_positive_geometry(
    document: &usvg::roxmltree::Document<'_>,
    element: usvg::roxmltree::Node<'_, '_>,
    attribute: &'static str,
) -> Result<(), ComponentSvgError> {
    let value = element.attribute(attribute).ok_or_else(|| {
        node_error(
            document,
            element,
            ComponentSvgErrorKind::AttributeForbidden,
            "component SVG shape is missing required positive geometry",
        )
    })?;
    parse_positive_context_free_length(value).map_err(|kind| {
        attribute_error(
            document,
            element,
            attribute,
            kind,
            "component SVG shape geometry must be positive and finite",
        )
    })?;
    Ok(())
}

fn forbidden_element_kind(name: &str) -> ComponentSvgErrorKind {
    match name {
        "style" => ComponentSvgErrorKind::CssForbidden,
        "script" => ComponentSvgErrorKind::ScriptForbidden,
        "text" | "tspan" | "textPath" => ComponentSvgErrorKind::TextForbidden,
        "image" | "foreignObject" => ComponentSvgErrorKind::ImageForbidden,
        "a" => ComponentSvgErrorKind::LinkForbidden,
        "use" => ComponentSvgErrorKind::UseForbidden,
        "symbol" => ComponentSvgErrorKind::SymbolForbidden,
        "linearGradient" | "radialGradient" => ComponentSvgErrorKind::GradientForbidden,
        "pattern" => ComponentSvgErrorKind::PatternForbidden,
        "clipPath" => ComponentSvgErrorKind::ClipForbidden,
        "mask" => ComponentSvgErrorKind::MaskForbidden,
        "filter" => ComponentSvgErrorKind::FilterForbidden,
        name if name.starts_with("fe") => ComponentSvgErrorKind::FilterForbidden,
        "marker" => ComponentSvgErrorKind::MarkerForbidden,
        "animate" | "animateTransform" | "animateMotion" | "set" => {
            ComponentSvgErrorKind::AnimationForbidden
        }
        _ => ComponentSvgErrorKind::ElementForbidden,
    }
}

fn forbidden_attribute_kind(name: &str) -> ComponentSvgErrorKind {
    match name {
        "id" => ComponentSvgErrorKind::FragmentForbidden,
        "class" => ComponentSvgErrorKind::CssForbidden,
        "style" => ComponentSvgErrorKind::StyleAttributeForbidden,
        "href" | "xlink:href" => ComponentSvgErrorKind::LinkForbidden,
        "src" => ComponentSvgErrorKind::SubresourceForbidden,
        "clip-path" => ComponentSvgErrorKind::ClipForbidden,
        "mask" => ComponentSvgErrorKind::MaskForbidden,
        "filter" => ComponentSvgErrorKind::FilterForbidden,
        "marker" | "marker-start" | "marker-mid" | "marker-end" => {
            ComponentSvgErrorKind::MarkerForbidden
        }
        name if name.starts_with("on") => ComponentSvgErrorKind::ScriptForbidden,
        name if name.starts_with("font") => ComponentSvgErrorKind::FontForbidden,
        name if name.starts_with("animation") || name.starts_with("transition") => {
            ComponentSvgErrorKind::AnimationForbidden
        }
        _ => ComponentSvgErrorKind::AttributeForbidden,
    }
}

fn node_error(
    document: &usvg::roxmltree::Document<'_>,
    node: usvg::roxmltree::Node<'_, '_>,
    kind: ComponentSvgErrorKind,
    message: &'static str,
) -> ComponentSvgError {
    ComponentSvgError::new(kind, message).at(document.text_pos_at(node.range().start))
}

fn attribute_error(
    document: &usvg::roxmltree::Document<'_>,
    node: usvg::roxmltree::Node<'_, '_>,
    attribute_name: &str,
    kind: ComponentSvgErrorKind,
    message: &'static str,
) -> ComponentSvgError {
    let position = node
        .attributes()
        .find(|attribute| attribute.name() == attribute_name)
        .map(|attribute| document.text_pos_at(attribute.range().start))
        .unwrap_or_else(|| document.text_pos_at(node.range().start));
    ComponentSvgError::new(kind, message).at(position)
}

fn svg_semantic_version(
    encoded: &[u8],
    width: f32,
    height: f32,
    view_box: ComponentSvgViewBox,
    statistics: ComponentSvgStatistics,
) -> String {
    let mut first = 0xcbf29ce484222325u64;
    let mut second = 0x84222325cbf29ce4u64;
    let semantic_bytes = b"component-svg-v1"
        .iter()
        .copied()
        .chain(width.to_bits().to_le_bytes())
        .chain(height.to_bits().to_le_bytes())
        .chain(view_box.x.to_bits().to_le_bytes())
        .chain(view_box.y.to_bits().to_le_bytes())
        .chain(view_box.width.to_bits().to_le_bytes())
        .chain(view_box.height.to_bits().to_le_bytes())
        .chain((statistics.node_count as u64).to_le_bytes())
        .chain((statistics.maximum_depth as u64).to_le_bytes())
        .chain((statistics.path_segment_count as u64).to_le_bytes())
        .chain(encoded.iter().copied());
    for byte in semantic_bytes {
        first ^= u64::from(byte);
        first = first.wrapping_mul(0x100000001b3);
        second ^= u64::from(byte);
        second = second.rotate_left(7).wrapping_mul(0x9e3779b185ebca87);
    }
    format!("component-svg-v1:{first:016x}{second:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_resource::{
        ComponentResourceKind, ComponentResourcePath, ComponentResourceSourceId,
    };
    use crate::package::PackageId;

    fn source(svg: &str) -> Result<ComponentSvgSource, ComponentSvgError> {
        let id = ComponentResourceSourceId::new(
            PackageId::parse("org.example.test").unwrap(),
            ComponentResourcePath::new("assets/test.svg".to_owned()),
            ComponentResourceKind::Svg,
        );
        ComponentSvgSource::parse(id, svg.as_bytes()).map(|(source, _)| source)
    }

    #[test]
    fn geometry_only_svg_normalizes_without_resolver_activity() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g opacity="0.8" transform="translate(1 2)"><path d="M0 0L10 10Z" fill="#abc"/><rect x="1" y="2" width="3" height="4" stroke="#123456" fill="none"/><circle cx="4" cy="4" r="2" fill="#11223344"/></g></svg>"##;
        let parsed = source(svg).unwrap();
        assert_eq!(parsed.width(), 24.0);
        assert_eq!(parsed.height(), 24.0);
        assert_eq!(parsed.statistics().node_count, 5);
        assert!(parsed.statistics().path_segment_count > 0);
        assert!(
            parsed
                .semantic_version()
                .deterministic_string()
                .starts_with("component-svg-v1:")
        );
    }

    #[test]
    fn active_or_reference_bearing_structures_are_typed_failures() {
        let cases = [
            (
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><image href="data:image/png;base64,x"/></svg>"#,
                ComponentSvgErrorKind::DataImageForbidden,
            ),
            (
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><text>x</text></svg>"#,
                ComponentSvgErrorKind::TextForbidden,
            ),
            (
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><style>path{fill:red}</style></svg>"#,
                ComponentSvgErrorKind::CssForbidden,
            ),
            (
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><path id="x" d="M0 0L1 1"/></svg>"#,
                ComponentSvgErrorKind::FragmentForbidden,
            ),
            (
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><path d="M0 0L1 1" fill="url(#x)"/></svg>"#,
                ComponentSvgErrorKind::PaintServerForbidden,
            ),
        ];
        for (svg, kind) in cases {
            assert_eq!(source(svg).unwrap_err().kind, kind);
        }
    }

    #[test]
    fn dtd_and_processing_instructions_are_rejected_before_usvg() {
        let dtd = r#"<!DOCTYPE svg [<!ENTITY x "x">]><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><path d="M0 0L1 1"/></svg>"#;
        assert_eq!(
            source(dtd).unwrap_err().kind,
            ComponentSvgErrorKind::DoctypeForbidden
        );
        let pi = r#"<?xml-stylesheet href="x.css"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><path d="M0 0L1 1"/></svg>"#;
        assert_eq!(
            source(pi).unwrap_err().kind,
            ComponentSvgErrorKind::ProcessingInstructionForbidden
        );
    }

    #[test]
    fn intrinsic_and_complexity_boundaries_are_explicit() {
        assert!(source(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4096 4096"><path d="M0 0L1 1"/></svg>"#).is_ok());
        assert_eq!(
            source(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4097 1"><path d="M0 0L1 1"/></svg>"#)
                .unwrap_err()
                .kind,
            ComponentSvgErrorKind::NaturalDimensionLimit
        );
        assert_eq!(
            source(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="10" viewBox="0 0 10 10"><path d="M0 0L1 1"/></svg>"#)
                .unwrap_err()
                .kind,
            ComponentSvgErrorKind::AttributeForbidden
        );
    }
}
