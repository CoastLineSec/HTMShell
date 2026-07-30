use super::effects::normalize_computed_filter;
use super::{
    ForegroundEffectCoverage, ForegroundEffectId, ForegroundEffectLayerMetadata,
    ForegroundEffectList, ForegroundEffectRejection, MAX_ACTIVE_FILTERED_ELEMENTS_PER_SURFACE,
    MAX_FILTER_DECLARATIONS_PER_DOCUMENT, MAX_FILTER_NESTING_DEPTH,
};
use super::{stable_hash_bytes, stable_hash_parts};
use crate::adapter::{
    border_radii, collect_fonts, collect_retained_paint_order, image_diagnostic, round, safe_rect,
    text_diagnostic,
};
use crate::identity::{IdentityRegistry, author_slots};
use crate::model::{CornerRadii, LogicalRect, ViewportSpec};
use crate::{
    ComponentResourceUsage, ExperimentalDocumentIdentity, ExperimentalNodeIdentity, RuntimeError,
};
use blitz_dom::node::NodeData;
use blitz_html::HtmlDocument;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use stylo::color::ColorSpace;

pub const MAX_SCENE_NODES: usize = 50_000;
pub const MAX_SCENE_DEPTH: usize = 256;
pub const MAX_SCENE_CHILDREN: usize = 10_000;
pub const MAX_RETAINED_RESOURCES: usize = 16_384;
pub const MAX_SCENE_DELTA_ENTRIES: usize = 4_096;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneRevision(pub u64);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SceneSubpart {
    Root,
    Box,
    Text,
    Image,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneNodeId {
    pub document: ExperimentalDocumentIdentity,
    pub dom: Option<ExperimentalNodeIdentity>,
    pub subpart: SceneSubpart,
    pub ordinal: u16,
}

impl SceneNodeId {
    fn root(document: ExperimentalDocumentIdentity) -> Self {
        Self {
            document,
            dom: None,
            subpart: SceneSubpart::Root,
            ordinal: 0,
        }
    }

    fn for_dom(
        document: ExperimentalDocumentIdentity,
        dom: ExperimentalNodeIdentity,
        subpart: SceneSubpart,
    ) -> Self {
        Self {
            document,
            dom: Some(dom),
            subpart,
            ordinal: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SceneNodeKind {
    SurfaceClear,
    Box,
    TextRun,
    Svg,
    RasterImage,
    UnavailableImage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneEffect {
    Opacity {
        value: f32,
    },
    Clip {
        bounds: LogicalRect,
        rounded: Option<CornerRadii>,
    },
    Transform {
        coefficients: [f64; 6],
    },
    BackgroundLayers {
        signature: u64,
    },
    BoxShadows {
        signature: u64,
        conservative_full_bounds: bool,
    },
    ForegroundFilter {
        list: ForegroundEffectList,
        source_graphic_bounds: LogicalRect,
        filtered_bounds: LogicalRect,
        nesting_depth: u8,
        coverage: ForegroundEffectCoverage,
        future_layer: ForegroundEffectLayerMetadata,
    },
    RejectedForegroundFilter {
        id: ForegroundEffectId,
        reason: ForegroundEffectRejection,
    },
    BackdropFilter {
        signature: u64,
        conservative_full_bounds: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneBounds {
    pub layout: LogicalRect,
    pub visual: LogicalRect,
    pub clip: LogicalRect,
    pub damage: LogicalRect,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Font,
    ShapedText,
    Svg,
    RasterImage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOwner {
    Process,
    Document(ExperimentalDocumentIdentity),
    Package { generation: u64, package_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneResourceKey {
    Dom { slot: usize, generation: u64 },
    Process { name: String },
    ComponentRaster { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneResourceId {
    pub owner: ResourceOwner,
    pub kind: ResourceKind,
    pub key: SceneResourceKey,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneResourceVersion(pub u64);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceLifecycle {
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SceneResource {
    pub id: SceneResourceId,
    pub version: SceneResourceVersion,
    pub lifecycle: ResourceLifecycle,
    pub diagnostic_key: String,
    pub byte_len: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneNode {
    pub id: SceneNodeId,
    pub parent: Option<SceneNodeId>,
    pub children: Vec<SceneNodeId>,
    pub kind: SceneNodeKind,
    pub tree_order: usize,
    pub paint_order: usize,
    pub visible: bool,
    pub bounds: SceneBounds,
    pub effects: Vec<SceneEffect>,
    pub resource: Option<(SceneResourceId, SceneResourceVersion)>,
    pub paint_signature: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetainedScene {
    pub document: ExperimentalDocumentIdentity,
    pub revision: SceneRevision,
    pub viewport: ViewportSpec,
    pub root: SceneNodeId,
    pub nodes: Vec<SceneNode>,
    pub resources: Vec<SceneResource>,
    pub content_fingerprint: u64,
}

impl RetainedScene {
    pub fn node(&self, id: SceneNodeId) -> Option<&SceneNode> {
        self.nodes
            .binary_search_by_key(&id, |node| node.id)
            .ok()
            .map(|index| &self.nodes[index])
    }

    pub fn resource(
        &self,
        id: &SceneResourceId,
        version: SceneResourceVersion,
    ) -> Option<&SceneResource> {
        self.resources
            .binary_search_by(|resource| resource.id.cmp(id))
            .ok()
            .map(|index| &self.resources[index])
            .filter(|resource| resource.version == version)
    }

    pub fn live_resources(&self) -> Vec<(SceneResourceId, SceneResourceVersion)> {
        self.resources
            .iter()
            .map(|resource| (resource.id.clone(), resource.version))
            .collect()
    }

    pub fn visually_eq(&self, other: &Self) -> bool {
        self.document == other.document
            && self.viewport == other.viewport
            && self.content_fingerprint == other.content_fingerprint
            && self.nodes == other.nodes
            && self.resources == other.resources
    }
}

impl PartialEq for RetainedScene {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision && self.visually_eq(other)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SceneChangeKind {
    Inserted,
    Removed,
    Geometry,
    Paint,
    TextOrResource,
    Effect,
    Clip,
    Visibility,
    StackingOrOrder,
    Reparented,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneNodeChange {
    pub id: SceneNodeId,
    pub kinds: BTreeSet<SceneChangeKind>,
    pub old_bounds: Option<SceneBounds>,
    pub new_bounds: Option<SceneBounds>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceChange {
    pub id: SceneResourceId,
    pub old_version: Option<SceneResourceVersion>,
    pub new_version: Option<SceneResourceVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneDelta {
    pub from_revision: Option<SceneRevision>,
    pub to_revision: SceneRevision,
    pub changes: Vec<SceneNodeChange>,
    pub resource_changes: Vec<ResourceChange>,
    pub full_scene_replacement: bool,
    pub unchanged_nodes: usize,
}

impl SceneDelta {
    pub fn is_empty(&self) -> bool {
        !self.full_scene_replacement && self.changes.is_empty() && self.resource_changes.is_empty()
    }
}

pub(crate) fn build_retained_scene(
    document: &HtmlDocument,
    identities: &IdentityRegistry,
    document_identity: ExperimentalDocumentIdentity,
    revision: SceneRevision,
    viewport: ViewportSpec,
) -> Result<RetainedScene, RuntimeError> {
    build_retained_scene_with_resources(
        document,
        identities,
        document_identity,
        revision,
        viewport,
        &[],
    )
}

pub(crate) fn build_retained_scene_with_resources(
    document: &HtmlDocument,
    identities: &IdentityRegistry,
    document_identity: ExperimentalDocumentIdentity,
    revision: SceneRevision,
    viewport: ViewportSpec,
    component_resources: &[ComponentResourceUsage],
) -> Result<RetainedScene, RuntimeError> {
    let surface = safe_rect(
        0.0,
        0.0,
        viewport.logical_width as f32,
        viewport.logical_height as f32,
    );
    validate_rect(&surface)?;
    let live = identities.live_identities(document)?;
    let slots = author_slots(document);
    let maximum_nodes = slots
        .len()
        .checked_mul(2)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| RuntimeError::LimitExceeded("scene-node count overflow".into()))?;
    if maximum_nodes > MAX_SCENE_NODES {
        return Err(RuntimeError::LimitExceeded(format!(
            "retained scene could require {maximum_nodes} nodes; limit is {MAX_SCENE_NODES}"
        )));
    }

    let paint_indices: BTreeMap<_, _> = collect_retained_paint_order(document)
        .into_iter()
        .enumerate()
        .map(|(index, slot)| (slot, index))
        .collect();
    let mut primary_ids = BTreeMap::new();
    for slot in &slots {
        let node = document.get_node(*slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "author-tree Blitz slot {slot} disappeared while building retained scene"
            ))
        })?;
        let identity = live[slot];
        primary_ids.insert(
            *slot,
            SceneNodeId::for_dom(
                document_identity,
                identity,
                subpart_for(node, image_diagnostic(node).as_ref()),
            ),
        );
    }

    let root = SceneNodeId::root(document_identity);
    let mut nodes = Vec::with_capacity(maximum_nodes);
    nodes.push(SceneNode {
        id: root,
        parent: None,
        children: Vec::new(),
        kind: SceneNodeKind::SurfaceClear,
        tree_order: 0,
        paint_order: 0,
        visible: true,
        bounds: SceneBounds {
            layout: surface.clone(),
            visual: surface.clone(),
            clip: surface.clone(),
            damage: surface.clone(),
        },
        effects: Vec::new(),
        resource: None,
        paint_signature: stable_hash_parts(&["surface-clear", "transparent"]),
    });

    let mut resources = BTreeMap::new();
    let component_resources = component_resources
        .iter()
        .map(|usage| (usage.node_slot(), usage))
        .collect::<BTreeMap<_, _>>();
    let mut effective_clips = BTreeMap::new();
    let mut depths = BTreeMap::new();
    let mut ancestor_suppression = BTreeMap::new();
    let mut visual_parents = BTreeMap::new();
    let mut computed_visibility = BTreeMap::new();
    let mut filter_depths = BTreeMap::new();
    let mut filter_declarations = BTreeSet::new();
    let mut active_filtered_elements = 0usize;
    for (tree_order, slot) in slots.iter().copied().enumerate() {
        let node = document.get_node(slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "author-tree Blitz slot {slot} disappeared while building retained scene"
            ))
        })?;
        let identity = live[&slot];
        let id = primary_ids[&slot];
        let effect_id = ForegroundEffectId::for_node(document_identity, identity);
        let mut foreground_filter = node.primary_styles().and_then(|styles| {
            (!styles.get_effects().filter.0.is_empty()).then(|| {
                normalize_computed_filter(
                    &styles.get_effects().filter.0,
                    &styles.clone_color(),
                    effect_id,
                )
            })
        });
        let inherited_filter_depth = node
            .parent
            .and_then(|parent| filter_depths.get(&parent).copied())
            .unwrap_or(0usize);
        if let Some(Ok(list)) = foreground_filter.as_ref() {
            filter_declarations.insert(list.version);
            if filter_declarations.len() > MAX_FILTER_DECLARATIONS_PER_DOCUMENT {
                filter_declarations.remove(&list.version);
                foreground_filter = Some(Err(ForegroundEffectRejection::DeclarationCount));
            }
        }
        let filter_depth = if foreground_filter.as_ref().is_some_and(Result::is_ok) {
            inherited_filter_depth.saturating_add(1)
        } else {
            inherited_filter_depth
        };
        if filter_depth > MAX_FILTER_NESTING_DEPTH {
            foreground_filter = Some(Err(ForegroundEffectRejection::NestingDepth));
            filter_depths.insert(slot, inherited_filter_depth);
        } else {
            filter_depths.insert(slot, filter_depth);
        }
        let visual_parent = node
            .parent
            .and_then(|parent| visual_parents.get(&parent).copied())
            .unwrap_or(root);
        let depth = node
            .parent
            .and_then(|parent| depths.get(&parent).copied())
            .unwrap_or(0usize)
            .saturating_add(1);
        if depth > MAX_SCENE_DEPTH {
            return Err(RuntimeError::LimitExceeded(format!(
                "retained scene depth exceeds {MAX_SCENE_DEPTH}"
            )));
        }
        depths.insert(slot, depth);

        let absolute = node.absolute_position(0.0, 0.0);
        let layout = safe_rect(
            absolute.x,
            absolute.y,
            node.final_layout.size.width,
            node.final_layout.size.height,
        );
        validate_rect(&layout)?;
        let display = format!("{:?}", node.style.display).to_ascii_lowercase();
        let visibility = node.primary_styles().map_or_else(
            || {
                node.parent
                    .and_then(|parent| computed_visibility.get(&parent).cloned())
                    .unwrap_or_else(|| "visible".into())
            },
            |styles| format!("{:?}", styles.get_inherited_box().visibility).to_ascii_lowercase(),
        );
        computed_visibility.insert(slot, visibility.clone());
        let opacity = node
            .primary_styles()
            .map(|styles| round(styles.get_effects().opacity))
            .unwrap_or(1.0);
        if !opacity.is_finite() {
            return Err(RuntimeError::InvalidPackage(
                "retained scene contains nonfinite opacity".into(),
            ));
        }
        let parent_suppressed = node
            .parent
            .and_then(|parent| ancestor_suppression.get(&parent).copied())
            .unwrap_or(false);
        let subtree_suppressed = parent_suppressed || display == "none" || opacity == 0.0;
        ancestor_suppression.insert(slot, subtree_suppressed);
        let overflow_x = format!("{:?}", node.style.overflow.x).to_ascii_lowercase();
        let overflow_y = format!("{:?}", node.style.overflow.y).to_ascii_lowercase();
        let establishes_clip = overflow_x != "visible" || overflow_y != "visible";
        let inherited_clip = node
            .parent
            .and_then(|parent| effective_clips.get(&parent).cloned())
            .unwrap_or_else(|| surface.clone());
        let effective_clip = if establishes_clip {
            intersect_rect(&inherited_clip, &layout)
        } else {
            inherited_clip
        };
        effective_clips.insert(slot, effective_clip.clone());
        let visible = !subtree_suppressed && visibility == "visible";
        if !visible {
            visual_parents.insert(slot, visual_parent);
            continue;
        }
        if foreground_filter.as_ref().is_some_and(Result::is_ok) {
            active_filtered_elements = active_filtered_elements.saturating_add(1);
            if active_filtered_elements > MAX_ACTIVE_FILTERED_ELEMENTS_PER_SURFACE {
                foreground_filter = Some(Err(ForegroundEffectRejection::ActiveElementCount));
                filter_depths.insert(slot, inherited_filter_depth);
            }
        }
        let parent = Some(visual_parent);
        visual_parents.insert(slot, id);

        let transform = node.transform.map(|transform| transform.as_coeffs());
        if transform.is_some_and(|values| values.into_iter().any(|value| !value.is_finite())) {
            return Err(RuntimeError::InvalidPackage(
                "retained scene contains a nonfinite transform".into(),
            ));
        }
        let paint_node = if matches!(node.data, NodeData::Text(_)) {
            node.parent
                .and_then(|parent| document.get_node(parent))
                .unwrap_or(node)
        } else {
            node
        };
        let text = match &node.data {
            NodeData::Text(value) if !value.content.is_empty() => {
                let measured = text_diagnostic(paint_node).map(|text| text.measured_bounds);
                Some((value.content.clone(), measured))
            }
            _ => None,
        };
        let image = image_diagnostic(node);
        let base_visual = text
            .as_ref()
            .and_then(|(_, bounds)| bounds.clone())
            .unwrap_or_else(|| layout.clone());
        let transformed = transform
            .map(|coefficients| transformed_bounds(&base_visual, coefficients))
            .transpose()?
            .unwrap_or(base_visual);
        let style_effects = node.primary_styles().map(|styles| {
            let background = format!("{:?}", styles.get_background().background_image);
            let shadows = format!("{:?}", styles.get_effects().box_shadow);
            let backdrop = format!("{:?}", styles.get_effects().backdrop_filter);
            (
                background,
                shadows,
                !styles.get_effects().box_shadow.0.is_empty(),
                backdrop,
                !styles.get_effects().backdrop_filter.0.is_empty(),
            )
        });
        let expands_conservatively = style_effects
            .as_ref()
            .is_some_and(|(_, _, shadows, _, backdrop)| *shadows || *backdrop);
        let visual = if expands_conservatively {
            effective_clip.clone()
        } else {
            intersect_rect(&outset_rect(&transformed, 1.0), &effective_clip)
        };
        let damage = if expands_conservatively {
            surface.clone()
        } else {
            visual.clone()
        };

        let mut effects = Vec::new();
        let mut backdrop_effect = None;
        if let Some((background, shadows, has_shadows, backdrop, has_backdrop)) = style_effects {
            effects.push(SceneEffect::BackgroundLayers {
                signature: stable_hash_bytes(background.as_bytes()),
            });
            if has_shadows {
                effects.push(SceneEffect::BoxShadows {
                    signature: stable_hash_bytes(shadows.as_bytes()),
                    conservative_full_bounds: true,
                });
            }
            if has_backdrop {
                backdrop_effect = Some(SceneEffect::BackdropFilter {
                    signature: stable_hash_bytes(backdrop.as_bytes()),
                    conservative_full_bounds: true,
                });
            }
        }
        if let Some(filter) = foreground_filter {
            match filter {
                Ok(list) => {
                    let filtered_bounds = list
                        .propagated_bounds(&visual)
                        .map(|bounds| intersect_rect(&bounds, &effective_clip))
                        .unwrap_or_else(|_| visual.clone());
                    let future_layer = ForegroundEffectLayerMetadata::for_list(&list);
                    let coverage = ForegroundEffectCoverage::for_list(&list);
                    effects.push(SceneEffect::ForegroundFilter {
                        list,
                        source_graphic_bounds: visual.clone(),
                        filtered_bounds,
                        nesting_depth: u8::try_from(filter_depth).unwrap_or(u8::MAX),
                        coverage,
                        future_layer,
                    });
                }
                Err(reason) => {
                    effects.push(SceneEffect::RejectedForegroundFilter {
                        id: effect_id,
                        reason,
                    });
                }
            }
        }
        if establishes_clip {
            effects.push(SceneEffect::Clip {
                bounds: layout.clone(),
                rounded: border_radii(node),
            });
        }
        if opacity != 1.0 {
            effects.push(SceneEffect::Opacity { value: opacity });
        }
        if let Some(coefficients) = transform {
            effects.push(SceneEffect::Transform { coefficients });
        }
        effects.extend(backdrop_effect);

        let background = node.primary_styles().map(|styles| {
            let current = styles.clone_color();
            let color = styles
                .get_background()
                .background_color
                .resolve_to_absolute(&current)
                .to_color_space(ColorSpace::Srgb);
            let values = color.raw_components();
            [
                round(values[0]),
                round(values[1]),
                round(values[2]),
                round(values[3]),
            ]
        });
        let style_debug = paint_node
            .primary_styles()
            .map(|styles| {
                let effects = styles.get_effects();
                format!(
                    "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
                    styles.get_inherited_text(),
                    styles.get_font(),
                    styles.get_inherited_box(),
                    styles.get_background(),
                    styles.get_border(),
                    styles.get_box(),
                    effects.box_shadow,
                    effects.clip,
                    effects.opacity,
                    effects.mix_blend_mode,
                )
            })
            .unwrap_or_else(|| format!("{:?}", paint_node.style));

        let resource = if let Some(usage) = component_resources.get(&slot) {
            Some(register_component_resource(&mut resources, usage)?)
        } else if let Some(image) = image.as_ref() {
            let kind = match image.decoded_kind.as_str() {
                "svg" => ResourceKind::Svg,
                _ => ResourceKind::RasterImage,
            };
            Some(register_resource(
                &mut resources,
                document_identity,
                identity,
                kind,
                &image.source,
                &format!("{}|{}", image.source, image.decoded_kind),
                image.decoded_kind != "unavailable",
            )?)
        } else if let Some((content, _)) = text.as_ref() {
            Some(register_resource(
                &mut resources,
                document_identity,
                identity,
                ResourceKind::ShapedText,
                &format!("text:{}:{}", identity.slot, identity.generation),
                &format!("{content}|{style_debug}"),
                true,
            )?)
        } else {
            None
        };

        let kind = match (
            image.as_ref().map(|image| image.decoded_kind.as_str()),
            text.is_some(),
        ) {
            (Some("svg"), _) => SceneNodeKind::Svg,
            (Some("raster"), _) => SceneNodeKind::RasterImage,
            (Some(_), _) => SceneNodeKind::UnavailableImage,
            (None, true) => SceneNodeKind::TextRun,
            (None, false) => SceneNodeKind::Box,
        };
        let paint_signature = stable_hash_parts(&[
            &format!("{kind:?}"),
            &display,
            &visibility,
            &format!("{opacity:.6}"),
            &format!("{background:?}"),
            &style_debug,
            text.as_ref()
                .map(|(content, _)| content.as_str())
                .unwrap_or(""),
            image
                .as_ref()
                .map(|image| image.source.as_str())
                .unwrap_or(""),
        ]);
        let paint_order = paint_indices
            .get(&slot)
            .copied()
            .unwrap_or(tree_order.saturating_add(1))
            .saturating_add(1);
        nodes.push(SceneNode {
            id,
            parent,
            children: Vec::new(),
            kind,
            tree_order: tree_order.saturating_add(1),
            paint_order,
            visible,
            bounds: SceneBounds {
                layout,
                visual,
                clip: effective_clip,
                damage,
            },
            effects,
            resource,
            paint_signature,
        });
    }

    for font in collect_fonts(document) {
        let key = format!(
            "{}|{}|{}|{}",
            font.family,
            font.subfamily.as_deref().unwrap_or(""),
            font.postscript_name.as_deref().unwrap_or(""),
            font.face_index
        );
        let id = SceneResourceId {
            owner: ResourceOwner::Process,
            kind: ResourceKind::Font,
            key: SceneResourceKey::Process { name: key.clone() },
        };
        resources.entry(id.clone()).or_insert(SceneResource {
            id,
            version: SceneResourceVersion(stable_hash_bytes(key.as_bytes())),
            lifecycle: ResourceLifecycle::Ready,
            diagnostic_key: key,
            byte_len: None,
        });
    }

    if resources.len() > MAX_RETAINED_RESOURCES {
        return Err(RuntimeError::LimitExceeded(format!(
            "retained resource count {} exceeds {MAX_RETAINED_RESOURCES}",
            resources.len()
        )));
    }

    nodes.sort_by_key(|node| node.id);
    let node_ids: BTreeSet<_> = nodes.iter().map(|node| node.id).collect();
    if node_ids.len() != nodes.len() {
        return Err(RuntimeError::InvalidPackage(
            "retained scene contains duplicate scene-node identities".into(),
        ));
    }
    let mut children: BTreeMap<SceneNodeId, Vec<SceneNodeId>> = BTreeMap::new();
    for node in &nodes {
        if let Some(parent) = node.parent {
            if !node_ids.contains(&parent) {
                return Err(RuntimeError::InvalidPackage(
                    "retained scene node refers to a missing parent".into(),
                ));
            }
            let siblings = children.entry(parent).or_default();
            if siblings.len() == MAX_SCENE_CHILDREN {
                return Err(RuntimeError::LimitExceeded(format!(
                    "retained scene parent exceeds {MAX_SCENE_CHILDREN} children"
                )));
            }
            siblings.push(node.id);
        }
    }
    let order: BTreeMap<_, _> = nodes
        .iter()
        .map(|node| (node.id, (node.paint_order, node.tree_order, node.id)))
        .collect();
    for siblings in children.values_mut() {
        siblings.sort_by_key(|id| order[id]);
    }
    for node in &mut nodes {
        node.children = children.remove(&node.id).unwrap_or_default();
    }
    apply_foreground_effect_bounds(&mut nodes)?;

    let resources: Vec<_> = resources.into_values().collect();
    let fingerprint = stable_hash_parts(&[
        &format!("{viewport:?}"),
        &format!("{nodes:?}"),
        &format!("{resources:?}"),
    ]);
    Ok(RetainedScene {
        document: document_identity,
        revision,
        viewport,
        root,
        nodes,
        resources,
        content_fingerprint: fingerprint,
    })
}

fn register_component_resource(
    resources: &mut BTreeMap<SceneResourceId, SceneResource>,
    usage: &ComponentResourceUsage,
) -> Result<(SceneResourceId, SceneResourceVersion), RuntimeError> {
    let source = usage.source();
    let generation = usage.instance().snapshot_generation();
    let id = SceneResourceId {
        owner: ResourceOwner::Package {
            generation: generation.get(),
            package_id: source.package_id().to_string(),
        },
        kind: ResourceKind::RasterImage,
        key: SceneResourceKey::ComponentRaster {
            path: source.path().as_str().to_owned(),
        },
    };
    let version = SceneResourceVersion(stable_hash_bytes(
        source.semantic_version().deterministic_string().as_bytes(),
    ));
    let diagnostic_key = source.id().deterministic_string(generation);
    if diagnostic_key.len() > 4_096 {
        return Err(RuntimeError::LimitExceeded(
            "retained component raster key exceeds 4096 bytes".into(),
        ));
    }
    resources.insert(
        id.clone(),
        SceneResource {
            id: id.clone(),
            version,
            lifecycle: ResourceLifecycle::Ready,
            diagnostic_key,
            byte_len: usize::try_from(source.decoded_bytes()).ok(),
        },
    );
    Ok((id, version))
}

pub(crate) fn diff_retained_scenes(
    previous: Option<&RetainedScene>,
    current: &RetainedScene,
) -> SceneDelta {
    let Some(previous) = previous else {
        return SceneDelta {
            from_revision: None,
            to_revision: current.revision,
            changes: Vec::new(),
            resource_changes: Vec::new(),
            full_scene_replacement: true,
            unchanged_nodes: 0,
        };
    };
    if previous.document != current.document {
        return SceneDelta {
            from_revision: Some(previous.revision),
            to_revision: current.revision,
            changes: Vec::new(),
            resource_changes: Vec::new(),
            full_scene_replacement: true,
            unchanged_nodes: 0,
        };
    }

    let old_nodes: BTreeMap<_, _> = previous.nodes.iter().map(|node| (node.id, node)).collect();
    let new_nodes: BTreeMap<_, _> = current.nodes.iter().map(|node| (node.id, node)).collect();
    let ids: BTreeSet<_> = old_nodes.keys().chain(new_nodes.keys()).copied().collect();
    let common_ids: BTreeSet<_> = old_nodes
        .keys()
        .filter(|id| new_nodes.contains_key(id))
        .copied()
        .collect();
    let old_order = common_paint_order(previous, &common_ids);
    let new_order = common_paint_order(current, &common_ids);
    let reordered: BTreeSet<_> = if old_order == new_order {
        BTreeSet::new()
    } else {
        old_order
            .iter()
            .zip(&new_order)
            .filter_map(|(old, new)| (old != new).then_some([*old, *new]))
            .flatten()
            .collect()
    };
    let mut changes = Vec::new();
    let mut unchanged_nodes = 0usize;
    for id in ids {
        match (old_nodes.get(&id), new_nodes.get(&id)) {
            (None, Some(new)) => changes.push(SceneNodeChange {
                id,
                kinds: BTreeSet::from([SceneChangeKind::Inserted]),
                old_bounds: None,
                new_bounds: Some(new.bounds.clone()),
            }),
            (Some(old), None) => changes.push(SceneNodeChange {
                id,
                kinds: BTreeSet::from([SceneChangeKind::Removed]),
                old_bounds: Some(old.bounds.clone()),
                new_bounds: None,
            }),
            (Some(old), Some(new)) if *old == *new => {
                unchanged_nodes = unchanged_nodes.saturating_add(1);
            }
            (Some(old), Some(new)) => {
                let mut kinds = BTreeSet::new();
                if old.bounds.layout != new.bounds.layout
                    || old.bounds.visual != new.bounds.visual
                    || old.bounds.damage != new.bounds.damage
                {
                    kinds.insert(SceneChangeKind::Geometry);
                }
                if old.paint_signature != new.paint_signature || old.kind != new.kind {
                    kinds.insert(SceneChangeKind::Paint);
                }
                if old.resource != new.resource {
                    kinds.insert(SceneChangeKind::TextOrResource);
                }
                if old.effects != new.effects {
                    kinds.insert(SceneChangeKind::Effect);
                }
                if old.bounds.clip != new.bounds.clip {
                    kinds.insert(SceneChangeKind::Clip);
                }
                if old.visible != new.visible {
                    kinds.insert(SceneChangeKind::Visibility);
                }
                if reordered.contains(&id) {
                    kinds.insert(SceneChangeKind::StackingOrOrder);
                }
                if old.parent != new.parent {
                    kinds.insert(SceneChangeKind::Reparented);
                }
                if kinds.is_empty() {
                    unchanged_nodes = unchanged_nodes.saturating_add(1);
                } else {
                    changes.push(SceneNodeChange {
                        id,
                        kinds,
                        old_bounds: Some(old.bounds.clone()),
                        new_bounds: Some(new.bounds.clone()),
                    });
                }
            }
            (None, None) => unreachable!("identity came from one of the maps"),
        }
    }

    let old_resources: BTreeMap<_, _> = previous
        .resources
        .iter()
        .map(|resource| (resource.id.clone(), resource.version))
        .collect();
    let new_resources: BTreeMap<_, _> = current
        .resources
        .iter()
        .map(|resource| (resource.id.clone(), resource.version))
        .collect();
    let resource_ids: BTreeSet<_> = old_resources
        .keys()
        .chain(new_resources.keys())
        .cloned()
        .collect();
    let resource_changes: Vec<_> = resource_ids
        .into_iter()
        .filter_map(|id| {
            let old_version = old_resources.get(&id).copied();
            let new_version = new_resources.get(&id).copied();
            (old_version != new_version).then_some(ResourceChange {
                id,
                old_version,
                new_version,
            })
        })
        .collect();
    let full_scene_replacement = changes
        .len()
        .checked_add(resource_changes.len())
        .is_none_or(|count| count > MAX_SCENE_DELTA_ENTRIES);
    if full_scene_replacement {
        changes.clear();
    }
    SceneDelta {
        from_revision: Some(previous.revision),
        to_revision: current.revision,
        changes,
        resource_changes: if full_scene_replacement {
            Vec::new()
        } else {
            resource_changes
        },
        full_scene_replacement,
        unchanged_nodes,
    }
}

fn common_paint_order(
    scene: &RetainedScene,
    common_ids: &BTreeSet<SceneNodeId>,
) -> Vec<SceneNodeId> {
    let mut nodes: Vec<_> = scene
        .nodes
        .iter()
        .filter(|node| common_ids.contains(&node.id))
        .collect();
    nodes.sort_by_key(|node| (node.paint_order, node.tree_order, node.id));
    nodes.into_iter().map(|node| node.id).collect()
}

fn register_resource(
    resources: &mut BTreeMap<SceneResourceId, SceneResource>,
    document: ExperimentalDocumentIdentity,
    dom: ExperimentalNodeIdentity,
    kind: ResourceKind,
    diagnostic_key: &str,
    version_material: &str,
    ready: bool,
) -> Result<(SceneResourceId, SceneResourceVersion), RuntimeError> {
    if diagnostic_key.len() > 4_096 {
        return Err(RuntimeError::LimitExceeded(
            "retained resource key exceeds 4096 bytes".into(),
        ));
    }
    let id = SceneResourceId {
        owner: ResourceOwner::Document(document),
        kind,
        key: SceneResourceKey::Dom {
            slot: dom.slot,
            generation: dom.generation,
        },
    };
    let version = SceneResourceVersion(stable_hash_bytes(version_material.as_bytes()));
    resources.insert(
        id.clone(),
        SceneResource {
            id: id.clone(),
            version,
            lifecycle: if ready {
                ResourceLifecycle::Ready
            } else {
                ResourceLifecycle::Failed
            },
            diagnostic_key: diagnostic_key.to_owned(),
            byte_len: None,
        },
    );
    Ok((id, version))
}

fn subpart_for(
    node: &blitz_dom::Node,
    image: Option<&crate::model::ImageDiagnostic>,
) -> SceneSubpart {
    if image.is_some() {
        SceneSubpart::Image
    } else if matches!(node.data, NodeData::Text(_)) {
        SceneSubpart::Text
    } else {
        SceneSubpart::Box
    }
}

fn validate_rect(rect: &LogicalRect) -> Result<(), RuntimeError> {
    if [
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        rect.x + rect.width,
        rect.y + rect.height,
    ]
    .into_iter()
    .all(f32::is_finite)
        && rect.width >= 0.0
        && rect.height >= 0.0
    {
        Ok(())
    } else {
        Err(RuntimeError::InvalidPackage(
            "retained scene contains invalid geometry".into(),
        ))
    }
}

fn intersect_rect(left: &LogicalRect, right: &LogicalRect) -> LogicalRect {
    let x1 = left.x.max(right.x);
    let y1 = left.y.max(right.y);
    let x2 = (left.x + left.width).min(right.x + right.width);
    let y2 = (left.y + left.height).min(right.y + right.height);
    safe_rect(x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0))
}

fn union_rect(left: &LogicalRect, right: &LogicalRect) -> LogicalRect {
    let x1 = left.x.min(right.x);
    let y1 = left.y.min(right.y);
    let x2 = (left.x + left.width).max(right.x + right.width);
    let y2 = (left.y + left.height).max(right.y + right.height);
    safe_rect(x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0))
}

fn apply_foreground_effect_bounds(nodes: &mut [SceneNode]) -> Result<(), RuntimeError> {
    let indices: BTreeMap<_, _> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id, index))
        .collect();
    let mut ordered: Vec<_> = nodes
        .iter()
        .map(|node| (node.tree_order, node.id))
        .collect();
    ordered.sort_by_key(|(tree_order, id)| (std::cmp::Reverse(*tree_order), *id));

    let mut subtree_outputs = BTreeMap::new();
    let mut filtered_outputs = BTreeMap::new();
    for (_, id) in ordered {
        let index = indices[&id];
        let mut source = nodes[index].bounds.visual.clone();
        for child in nodes[index].children.clone() {
            if let Some(bounds) = subtree_outputs.get(&child) {
                source = union_rect(&source, bounds);
            }
        }

        let filter_index = nodes[index]
            .effects
            .iter()
            .position(|effect| matches!(effect, SceneEffect::ForegroundFilter { .. }));
        let mut output = source.clone();
        if let Some(filter_index) = filter_index {
            let (list, nesting_depth, coverage, future_layer) =
                match &nodes[index].effects[filter_index] {
                    SceneEffect::ForegroundFilter {
                        list,
                        nesting_depth,
                        coverage,
                        future_layer,
                        ..
                    } => (list.clone(), *nesting_depth, *coverage, *future_layer),
                    _ => unreachable!("position selected a foreground filter"),
                };
            match list.propagated_bounds(&source) {
                Ok(filtered) => {
                    let transformed = nodes[index]
                        .effects
                        .iter()
                        .any(|effect| matches!(effect, SceneEffect::Transform { .. }));
                    output = if transformed && list.expands_geometry() {
                        nodes[index].bounds.clip.clone()
                    } else {
                        intersect_rect(&filtered, &nodes[index].bounds.clip)
                    };
                    nodes[index].effects[filter_index] = SceneEffect::ForegroundFilter {
                        list,
                        source_graphic_bounds: source,
                        filtered_bounds: output.clone(),
                        nesting_depth,
                        coverage,
                        future_layer,
                    };
                    nodes[index].bounds.visual = output.clone();
                    nodes[index].bounds.damage = output.clone();
                    filtered_outputs.insert(id, output.clone());
                }
                Err(reason) => {
                    nodes[index].effects[filter_index] = SceneEffect::RejectedForegroundFilter {
                        id: list.id,
                        reason,
                    };
                }
            }
        }
        subtree_outputs.insert(id, output);
    }

    let parents: BTreeMap<_, _> = nodes.iter().map(|node| (node.id, node.parent)).collect();
    for node in nodes {
        let mut ancestor = node.parent;
        let mut remaining = MAX_SCENE_DEPTH;
        while let Some(parent) = ancestor {
            if let Some(filtered) = filtered_outputs.get(&parent) {
                node.bounds.damage = union_rect(&node.bounds.damage, filtered);
            }
            ancestor = parents.get(&parent).copied().flatten();
            if remaining == 0 {
                return Err(RuntimeError::LimitExceeded(
                    "foreground effect ancestry exceeds the scene-depth limit".into(),
                ));
            }
            remaining -= 1;
        }
    }
    Ok(())
}

fn outset_rect(rect: &LogicalRect, amount: f32) -> LogicalRect {
    safe_rect(
        rect.x - amount,
        rect.y - amount,
        rect.width + amount * 2.0,
        rect.height + amount * 2.0,
    )
}

fn transformed_bounds(
    rect: &LogicalRect,
    coefficients: [f64; 6],
) -> Result<LogicalRect, RuntimeError> {
    let [a, b, c, d, e, f] = coefficients;
    let corners = [
        (f64::from(rect.x), f64::from(rect.y)),
        (f64::from(rect.x + rect.width), f64::from(rect.y)),
        (f64::from(rect.x), f64::from(rect.y + rect.height)),
        (
            f64::from(rect.x + rect.width),
            f64::from(rect.y + rect.height),
        ),
    ];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x, y) in corners {
        let tx = a.mul_add(x, c.mul_add(y, e));
        let ty = b.mul_add(x, d.mul_add(y, f));
        min_x = min_x.min(tx);
        min_y = min_y.min(ty);
        max_x = max_x.max(tx);
        max_y = max_y.max(ty);
    }
    if [min_x, min_y, max_x, max_y]
        .into_iter()
        .any(|value| !value.is_finite())
    {
        return Err(RuntimeError::InvalidPackage(
            "transform produced nonfinite visual bounds".into(),
        ));
    }
    let values = [min_x, min_y, max_x - min_x, max_y - min_y];
    if values
        .into_iter()
        .any(|value| value < f32::MIN as f64 || value > f32::MAX as f64)
    {
        return Err(RuntimeError::InvalidPackage(
            "transform produced out-of-range visual bounds".into(),
        ));
    }
    Ok(safe_rect(
        min_x as f32,
        min_y as f32,
        (max_x - min_x) as f32,
        (max_y - min_y) as f32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{ForegroundEffect, ForegroundEffectColorSpace};
    use blitz_dom::{DocumentConfig, StyleThreading};
    use blitz_html::HtmlProvider;
    use blitz_traits::shell::{ColorScheme, Viewport};
    use std::fmt::Write as _;
    use std::sync::Arc;
    use std::time::Instant;

    fn document(html: &str) -> HtmlDocument {
        let mut document = HtmlDocument::from_html(
            html,
            DocumentConfig {
                viewport: Some(Viewport::new(200, 120, 1.0, ColorScheme::Dark)),
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                style_threading: StyleThreading::Sequential,
                ..Default::default()
            },
        );
        document.set_incremental_layout(true);
        document.resolve(0.0);
        document
    }

    fn retained(document: &HtmlDocument, revision: u64) -> RetainedScene {
        build_retained_scene(
            document,
            &IdentityRegistry::from_document(document),
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(revision),
            ViewportSpec {
                logical_width: 200,
                logical_height: 120,
                ..ViewportSpec::default()
            },
        )
        .unwrap()
    }

    fn scene(revision: u64, nodes: Vec<SceneNode>) -> RetainedScene {
        let document = ExperimentalDocumentIdentity { serial: 1 };
        let mut nodes = nodes;
        nodes.sort_by_key(|node| node.id);
        RetainedScene {
            document,
            revision: SceneRevision(revision),
            viewport: ViewportSpec::default(),
            root: SceneNodeId::root(document),
            resources: Vec::new(),
            content_fingerprint: stable_hash_parts(&[&format!("{nodes:?}")]),
            nodes,
        }
    }

    fn node(slot: usize, x: f32) -> SceneNode {
        let document = ExperimentalDocumentIdentity { serial: 1 };
        SceneNode {
            id: SceneNodeId::for_dom(
                document,
                ExperimentalNodeIdentity {
                    slot,
                    generation: 0,
                },
                SceneSubpart::Box,
            ),
            parent: Some(SceneNodeId::root(document)),
            children: Vec::new(),
            kind: SceneNodeKind::Box,
            tree_order: slot,
            paint_order: slot,
            visible: true,
            bounds: SceneBounds {
                layout: safe_rect(x, 0.0, 10.0, 10.0),
                visual: safe_rect(x, 0.0, 10.0, 10.0),
                clip: safe_rect(0.0, 0.0, 100.0, 100.0),
                damage: safe_rect(x, 0.0, 10.0, 10.0),
            },
            effects: Vec::new(),
            resource: None,
            paint_signature: 1,
        }
    }

    fn node_by_selector<'a>(
        document: &HtmlDocument,
        scene: &'a RetainedScene,
        selector: &str,
    ) -> &'a SceneNode {
        let slot = document.query_selector(selector).unwrap().unwrap();
        scene
            .nodes
            .iter()
            .find(|node| node.id.dom.is_some_and(|identity| identity.slot == slot))
            .expect("selector has a retained scene node")
    }

    fn foreground_filter(node: &SceneNode) -> &ForegroundEffectList {
        node.effects
            .iter()
            .find_map(|effect| match effect {
                SceneEffect::ForegroundFilter { list, .. } => Some(list),
                _ => None,
            })
            .expect("node has a normalized foreground filter")
    }

    fn rejected_filter(node: &SceneNode) -> Option<ForegroundEffectRejection> {
        node.effects.iter().find_map(|effect| match effect {
            SceneEffect::RejectedForegroundFilter { reason, .. } => Some(*reason),
            _ => None,
        })
    }

    #[test]
    fn no_op_diff_has_no_changes() {
        let old = scene(1, vec![node(1, 0.0)]);
        let mut new = old.clone();
        new.revision = SceneRevision(2);
        let delta = diff_retained_scenes(Some(&old), &new);
        assert!(delta.is_empty());
        assert_eq!(delta.unchanged_nodes, 1);
    }

    #[test]
    fn geometry_update_preserves_identity() {
        let old = scene(1, vec![node(1, 0.0)]);
        let new = scene(2, vec![node(1, 20.0)]);
        let delta = diff_retained_scenes(Some(&old), &new);
        assert_eq!(delta.changes.len(), 1);
        assert!(delta.changes[0].kinds.contains(&SceneChangeKind::Geometry));
        assert_eq!(delta.changes[0].id, old.nodes[0].id);
    }

    #[test]
    fn insertion_does_not_reclassify_unchanged_siblings_as_reordered() {
        let old = scene(1, vec![node(1, 20.0), node(2, 40.0)]);
        let new = scene(2, vec![node(0, 0.0), node(1, 20.0), node(2, 40.0)]);
        let delta = diff_retained_scenes(Some(&old), &new);
        assert_eq!(delta.changes.len(), 1);
        assert_eq!(
            delta.changes[0].kinds,
            BTreeSet::from([SceneChangeKind::Inserted])
        );
        assert_eq!(delta.unchanged_nodes, 2);
    }

    #[test]
    fn stale_document_generation_forces_replacement() {
        let old = scene(1, vec![node(1, 0.0)]);
        let mut new = scene(2, vec![node(1, 0.0)]);
        new.document = ExperimentalDocumentIdentity { serial: 2 };
        let delta = diff_retained_scenes(Some(&old), &new);
        assert!(delta.full_scene_replacement);
    }

    #[test]
    fn transforms_produce_finite_bounding_boxes() {
        let bounds = transformed_bounds(
            &safe_rect(-10.0, 5.0, 20.0, 10.0),
            [1.0, 0.0, 0.0, 1.0, 12.0, -3.0],
        )
        .unwrap();
        assert_eq!(bounds, safe_rect(2.0, 2.0, 20.0, 10.0));
        assert!(
            transformed_bounds(
                &safe_rect(0.0, 0.0, 1.0, 1.0),
                [f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0],
            )
            .is_err()
        );
    }

    #[test]
    fn source_scene_preserves_text_and_resource_identity_across_content_change() {
        let mut document = document(
            "<!doctype html><html><body><p id=\"copy\" style=\"color:white\">alpha</p></body></html>",
        );
        let identities = IdentityRegistry::from_document(&document);
        let first = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(1),
            ViewportSpec {
                logical_width: 200,
                logical_height: 120,
                ..ViewportSpec::default()
            },
        )
        .unwrap();
        let element = document.query_selector("#copy").unwrap().unwrap();
        let text = document.get_node(element).unwrap().children[0];
        let old = first
            .nodes
            .iter()
            .find(|node| node.id.dom.is_some_and(|identity| identity.slot == text))
            .unwrap();
        let old_id = old.id;
        let old_resource = old.resource.clone().unwrap();

        document.mutate().set_node_text(text, "beta");
        document.resolve(0.0);
        let second = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(2),
            ViewportSpec {
                logical_width: 200,
                logical_height: 120,
                ..ViewportSpec::default()
            },
        )
        .unwrap();
        let new = second.node(old_id).expect("stable text scene identity");
        let new_resource = new.resource.clone().unwrap();
        assert_eq!(old_resource.0, new_resource.0);
        assert_ne!(old_resource.1, new_resource.1);
        assert!(second.resource(&new_resource.0, old_resource.1).is_none());
        assert!(second.resource(&new_resource.0, new_resource.1).is_some());
        let delta = diff_retained_scenes(Some(&first), &second);
        let text_change = delta
            .changes
            .iter()
            .find(|change| change.id == old_id)
            .expect("exact text scene-node change");
        assert!(text_change.kinds.contains(&SceneChangeKind::TextOrResource));
    }

    #[test]
    fn preserved_whitespace_changes_text_resource_version_without_replacing_identity() {
        let mut document =
            document("<!doctype html><html><body><pre id=\"copy\"> alpha </pre></body></html>");
        let identities = IdentityRegistry::from_document(&document);
        let first = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(1),
            ViewportSpec {
                logical_width: 200,
                logical_height: 120,
                ..ViewportSpec::default()
            },
        )
        .unwrap();
        let element = document.query_selector("#copy").unwrap().unwrap();
        let text = document.get_node(element).unwrap().children[0];
        let old = first
            .nodes
            .iter()
            .find(|node| node.id.dom.is_some_and(|identity| identity.slot == text))
            .expect("preserved text scene node");
        let old_id = old.id;
        let old_resource = old.resource.clone().expect("preserved text resource");

        document.mutate().set_node_text(text, "  alpha ");
        document.resolve(0.0);
        let second = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(2),
            ViewportSpec {
                logical_width: 200,
                logical_height: 120,
                ..ViewportSpec::default()
            },
        )
        .unwrap();
        let new_resource = second
            .node(old_id)
            .and_then(|node| node.resource.clone())
            .expect("stable preserved text scene identity");
        assert_eq!(old_resource.0, new_resource.0);
        assert_ne!(old_resource.1, new_resource.1);
    }

    #[test]
    fn inherited_text_paint_change_preserves_identity_and_updates_resource() {
        let mut document = document(
            "<!doctype html><html><body><p id=\"copy\" style=\"color:red\">alpha</p></body></html>",
        );
        let identities = IdentityRegistry::from_document(&document);
        let first = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(1),
            ViewportSpec {
                logical_width: 200,
                logical_height: 120,
                ..ViewportSpec::default()
            },
        )
        .unwrap();
        let element = document.query_selector("#copy").unwrap().unwrap();
        let text = document.get_node(element).unwrap().children[0];
        let old = first
            .nodes
            .iter()
            .find(|node| node.id.dom.is_some_and(|identity| identity.slot == text))
            .expect("text scene node");
        let old_id = old.id;
        let old_resource = old.resource.clone().expect("text resource");

        document.mutate().set_attribute(
            element,
            blitz_dom::QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: blitz_dom::LocalName::from("style"),
            },
            "color:blue",
        );
        document.resolve(0.0);
        let second = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(2),
            ViewportSpec {
                logical_width: 200,
                logical_height: 120,
                ..ViewportSpec::default()
            },
        )
        .unwrap();
        let new_resource = second
            .node(old_id)
            .and_then(|node| node.resource.clone())
            .expect("stable text identity after inherited paint change");
        assert_eq!(old_resource.0, new_resource.0);
        assert_ne!(old_resource.1, new_resource.1);
        let delta = diff_retained_scenes(Some(&first), &second);
        assert!(
            delta.changes.iter().any(|change| change.id == old_id
                && change.kinds.contains(&SceneChangeKind::TextOrResource))
        );
    }

    #[test]
    fn invisible_visual_objects_and_descendants_leave_the_scene() {
        let document = document(
            "<!doctype html><html><body><div id=\"hidden\" style=\"display:none\"><span id=\"child\">hidden</span></div><div id=\"visible\">visible</div></body></html>",
        );
        let identities = IdentityRegistry::from_document(&document);
        let hidden = identities
            .identity_for_slot(
                &document,
                document.query_selector("#hidden").unwrap().unwrap(),
            )
            .unwrap();
        let child = identities
            .identity_for_slot(
                &document,
                document.query_selector("#child").unwrap().unwrap(),
            )
            .unwrap();
        let visible = identities
            .identity_for_slot(
                &document,
                document.query_selector("#visible").unwrap().unwrap(),
            )
            .unwrap();
        let scene = retained(&document, 1);
        assert!(
            scene
                .nodes
                .iter()
                .all(|node| { node.id.dom != Some(hidden) && node.id.dom != Some(child) })
        );
        assert!(scene.nodes.iter().any(|node| node.id.dom == Some(visible)));
    }

    #[test]
    fn visibility_override_reparents_visible_child_without_losing_ancestor_clip() {
        let document = document(
            "<!doctype html><html><body><div id=\"hidden\" style=\"visibility:hidden;overflow:hidden;width:30px;height:30px\"><span id=\"child\" style=\"visibility:visible\">visible override</span></div></body></html>",
        );
        let identities = IdentityRegistry::from_document(&document);
        let hidden = identities
            .identity_for_slot(
                &document,
                document.query_selector("#hidden").unwrap().unwrap(),
            )
            .unwrap();
        let child = identities
            .identity_for_slot(
                &document,
                document.query_selector("#child").unwrap().unwrap(),
            )
            .unwrap();
        let scene = retained(&document, 1);
        assert!(scene.nodes.iter().all(|node| node.id.dom != Some(hidden)));
        let child = scene
            .nodes
            .iter()
            .find(|node| node.id.dom == Some(child))
            .expect("visibility override remains paintable");
        assert_ne!(child.parent.and_then(|parent| parent.dom), Some(hidden));
        assert!(child.bounds.clip.width <= 30.0);
        assert!(child.bounds.clip.height <= 30.0);
    }

    #[test]
    fn finite_effect_inventory_covers_existing_css_paint_behaviors() {
        let document = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"width:40px;height:20px;overflow:hidden;opacity:.7;transform:translateX(2px);background:linear-gradient(90deg,#000,#fff);filter:brightness(1.1);box-shadow:0 0 3px #000\"></div></body></html>",
        );
        let slot = document.query_selector("#effect").unwrap().unwrap();
        let scene = retained(&document, 1);
        let node = scene
            .nodes
            .iter()
            .find(|node| node.id.dom.is_some_and(|identity| identity.slot == slot))
            .unwrap();
        assert!(
            node.effects
                .iter()
                .any(|effect| matches!(effect, SceneEffect::Opacity { .. }))
        );
        assert!(
            node.effects
                .iter()
                .any(|effect| matches!(effect, SceneEffect::Clip { .. }))
        );
        assert!(
            node.effects
                .iter()
                .any(|effect| matches!(effect, SceneEffect::Transform { .. }))
        );
        assert!(
            node.effects
                .iter()
                .any(|effect| matches!(effect, SceneEffect::BackgroundLayers { .. }))
        );
        assert!(
            node.effects
                .iter()
                .any(|effect| matches!(effect, SceneEffect::BoxShadows { .. }))
        );
        assert!(
            node.effects
                .iter()
                .any(|effect| matches!(effect, SceneEffect::ForegroundFilter { .. }))
        );
        let effect_position = |predicate: fn(&SceneEffect) -> bool| {
            node.effects
                .iter()
                .position(predicate)
                .expect("modeled effect stage")
        };
        assert!(
            effect_position(|effect| matches!(effect, SceneEffect::BackgroundLayers { .. }))
                < effect_position(|effect| matches!(effect, SceneEffect::BoxShadows { .. }))
        );
        assert!(
            effect_position(|effect| matches!(effect, SceneEffect::BoxShadows { .. }))
                < effect_position(|effect| matches!(effect, SceneEffect::ForegroundFilter { .. }))
        );
        assert!(
            effect_position(|effect| matches!(effect, SceneEffect::ForegroundFilter { .. }))
                < effect_position(|effect| matches!(effect, SceneEffect::Clip { .. }))
        );
        assert!(
            effect_position(|effect| matches!(effect, SceneEffect::Clip { .. }))
                < effect_position(|effect| matches!(effect, SceneEffect::Opacity { .. }))
        );
        assert!(
            effect_position(|effect| matches!(effect, SceneEffect::Opacity { .. }))
                < effect_position(|effect| matches!(effect, SceneEffect::Transform { .. }))
        );
        assert_eq!(node.bounds.damage, safe_rect(8.0, 8.0, 40.0, 20.0));
    }

    #[test]
    fn stylo_extraction_normalizes_all_approved_filter_functions_in_order() {
        let document = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"color:rgba(10,20,30,.4);width:20px;height:10px;filter:brightness(.5) contrast(120%) grayscale(2) hue-rotate(.5turn) invert(25%) opacity(.75) saturate(2) sepia(50%) blur(3.5px) drop-shadow(4px -5px 2px currentColor)\"></div></body></html>",
        );
        let scene = retained(&document, 1);
        let list = foreground_filter(node_by_selector(&document, &scene, "#effect"));
        assert_eq!(list.functions.len(), 10);
        assert_eq!(
            list.functions
                .iter()
                .map(ForegroundEffect::variant_name)
                .collect::<Vec<_>>(),
            vec![
                "brightness",
                "contrast",
                "grayscale",
                "hue_rotate",
                "invert",
                "opacity",
                "saturate",
                "sepia",
                "blur",
                "drop_shadow",
            ]
        );
        assert_eq!(
            list.serialize_semantics(),
            "foreground_effects_v1[brightness(0.5),contrast(1.2),grayscale(1),hue_rotate(3.1415927),invert(0.25),opacity(0.75),saturate(2),sepia(0.5),blur(3.5),drop_shadow(4,-5,2,0.039215688,0.078431375,0.11764706,0.4)]"
        );
        assert_eq!(list.color_space, ForegroundEffectColorSpace::EncodedSrgb);
        assert_eq!(list.color_matrix_runs().unwrap().len(), 1);
    }

    #[test]
    fn foreground_and_backdrop_models_remain_independent() {
        let first = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:brightness(1.2);backdrop-filter:blur(2px)\"></div></body></html>",
        );
        let second = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:brightness(1.2);backdrop-filter:blur(6px)\"></div></body></html>",
        );
        let first_scene = retained(&first, 1);
        let second_scene = retained(&second, 1);
        let first_node = node_by_selector(&first, &first_scene, "#effect");
        let second_node = node_by_selector(&second, &second_scene, "#effect");
        assert_eq!(
            foreground_filter(first_node).version,
            foreground_filter(second_node).version
        );
        assert!(
            first_node
                .effects
                .iter()
                .any(|effect| matches!(effect, SceneEffect::BackdropFilter { .. }))
        );
        assert!(
            second_node
                .effects
                .iter()
                .any(|effect| matches!(effect, SceneEffect::BackdropFilter { .. }))
        );
    }

    #[test]
    fn computed_equivalents_share_versions_and_structure_remains_significant() {
        let numeric = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:brightness(1.5) hue-rotate(180deg)\"></div></body></html>",
        );
        let percentage = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:brightness(150%) hue-rotate(.5turn)\"></div></body></html>",
        );
        let numeric_scene = retained(&numeric, 1);
        let percentage_scene = retained(&percentage, 1);
        let numeric_list = foreground_filter(node_by_selector(&numeric, &numeric_scene, "#effect"));
        let percentage_list =
            foreground_filter(node_by_selector(&percentage, &percentage_scene, "#effect"));
        assert_eq!(numeric_list.version, percentage_list.version);
        assert_eq!(
            numeric_list.serialize_semantics(),
            percentage_list.serialize_semantics()
        );

        let repeated = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:brightness(1) brightness(1)\"></div></body></html>",
        );
        let reordered = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:hue-rotate(.5turn) brightness(1.5)\"></div></body></html>",
        );
        let repeated_scene = retained(&repeated, 1);
        let reordered_scene = retained(&reordered, 1);
        assert_eq!(
            foreground_filter(node_by_selector(&repeated, &repeated_scene, "#effect"))
                .functions
                .len(),
            2
        );
        assert_ne!(
            numeric_list.version,
            foreground_filter(node_by_selector(&reordered, &reordered_scene, "#effect")).version
        );
    }

    #[test]
    fn angle_length_and_profile_boundaries_normalize_through_stylo() {
        let declarations = [
            "hue-rotate(0)",
            "hue-rotate(180deg)",
            "hue-rotate(200grad)",
            "hue-rotate(3.14159265rad)",
            "hue-rotate(.5turn)",
            "hue-rotate(-100turn)",
            "blur(64px)",
            "drop-shadow(-256px 256px 64px transparent)",
            "brightness(8)",
            "contrast(800%)",
            "saturate(8)",
        ];
        for declaration in declarations {
            let html = format!(
                "<!doctype html><html><body><div id=\"effect\" style=\"font-size:10px;filter:{declaration}\"></div></body></html>"
            );
            let document = document(&html);
            let scene = retained(&document, 1);
            assert!(
                rejected_filter(node_by_selector(&document, &scene, "#effect")).is_none(),
                "{declaration}"
            );
            assert_eq!(
                foreground_filter(node_by_selector(&document, &scene, "#effect"))
                    .functions
                    .len(),
                1,
                "{declaration}"
            );
        }

        let em = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"font-size:10px;filter:blur(.5em)\"></div></body></html>",
        );
        let em_scene = retained(&em, 1);
        assert_eq!(
            foreground_filter(node_by_selector(&em, &em_scene, "#effect")).serialize_semantics(),
            "foreground_effects_v1[blur(5)]"
        );

        let degrees = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:hue-rotate(180deg)\"></div></body></html>",
        );
        let grad = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:hue-rotate(200grad)\"></div></body></html>",
        );
        let radians = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:hue-rotate(3.14159265rad)\"></div></body></html>",
        );
        let turn = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:hue-rotate(.5turn)\"></div></body></html>",
        );
        let versions = [&degrees, &grad, &radians, &turn].map(|document| {
            let scene = retained(document, 1);
            foreground_filter(node_by_selector(document, &scene, "#effect")).version
        });
        assert!(versions.windows(2).all(|pair| pair[0] == pair[1]));

        let clamped = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:grayscale(2) invert(200%) opacity(3) sepia(400%)\"></div></body></html>",
        );
        let clamped_scene = retained(&clamped, 1);
        assert_eq!(
            foreground_filter(node_by_selector(&clamped, &clamped_scene, "#effect"))
                .serialize_semantics(),
            "foreground_effects_v1[grayscale(1),invert(1),opacity(1),sepia(1)]"
        );
    }

    #[test]
    fn blur_length_units_resolve_through_stylo_before_profile_validation() {
        for declaration in [
            "blur(1px)",
            "blur(.1in)",
            "blur(.1cm)",
            "blur(.1mm)",
            "blur(.1q)",
            "blur(.1pt)",
            "blur(.1pc)",
            "blur(.5em)",
            "blur(.5rem)",
            "blur(.5ex)",
            "blur(.5ch)",
            "blur(1vw)",
            "blur(1vh)",
            "blur(1vmin)",
            "blur(1vmax)",
        ] {
            let html = format!(
                "<!doctype html><html><body><div id=\"effect\" style=\"font-size:10px;filter:{declaration}\"></div></body></html>"
            );
            let document = document(&html);
            let scene = retained(&document, 1);
            assert!(
                rejected_filter(node_by_selector(&document, &scene, "#effect")).is_none(),
                "{declaration}"
            );
        }
    }

    #[test]
    fn omitted_filter_arguments_use_standard_computed_defaults() {
        let document = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:blur() brightness() contrast() grayscale() hue-rotate() invert() opacity() saturate() sepia()\"></div></body></html>",
        );
        let scene = retained(&document, 1);
        assert_eq!(
            foreground_filter(node_by_selector(&document, &scene, "#effect")).serialize_semantics(),
            "foreground_effects_v1[blur(0),brightness(1),contrast(1),grayscale(1),hue_rotate(0),invert(1),opacity(1),saturate(1),sepia(1)]"
        );
    }

    #[test]
    fn stylo_nonfinite_calc_results_never_publish_nonfinite_descriptors() {
        for (filter, expected) in [
            ("brightness(calc(infinity))", None),
            (
                "brightness(calc(NaN))",
                Some("foreground_effects_v1[brightness(0)]"),
            ),
            ("blur(calc(infinity * 1px))", None),
        ] {
            let html = format!(
                "<!doctype html><html><body><div id=\"effect\" style=\"filter:{filter}\"></div></body></html>"
            );
            let document = document(&html);
            let scene = retained(&document, 1);
            let node = node_by_selector(&document, &scene, "#effect");
            let normalized = node.effects.iter().find_map(|effect| match effect {
                SceneEffect::ForegroundFilter { list, .. } => Some(list.serialize_semantics()),
                _ => None,
            });
            assert_eq!(normalized.as_deref(), expected, "{filter}");
        }
    }

    #[test]
    fn profile_invalid_lists_are_rejected_whole_without_prefix_application() {
        for (filter, reason) in [
            (
                "brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1) brightness(1)",
                ForegroundEffectRejection::FunctionCount,
            ),
            (
                "drop-shadow(1px 1px) drop-shadow(2px 2px)",
                ForegroundEffectRejection::DropShadowCount,
            ),
            ("brightness(8.01)", ForegroundEffectRejection::FactorRange),
            (
                "hue-rotate(100.01turn)",
                ForegroundEffectRejection::HueRange,
            ),
            ("blur(64.01px)", ForegroundEffectRejection::BlurRange),
            (
                "drop-shadow(256.01px 0)",
                ForegroundEffectRejection::ShadowOffsetRange,
            ),
            (
                "blur(64px) blur(64px) blur(64px)",
                ForegroundEffectRejection::ExpansionLimit,
            ),
        ] {
            let html = format!(
                "<!doctype html><html><body><div id=\"effect\" style=\"width:10px;height:10px;filter:{filter}\"></div></body></html>"
            );
            let document = document(&html);
            let scene = retained(&document, 1);
            let node = node_by_selector(&document, &scene, "#effect");
            assert_eq!(rejected_filter(node), Some(reason), "{filter}");
            assert!(
                !node
                    .effects
                    .iter()
                    .any(|effect| matches!(effect, SceneEffect::ForegroundFilter { .. })),
                "{filter}"
            );
        }
    }

    #[test]
    fn parser_invalid_filter_declarations_resolve_as_none() {
        for filter in [
            "unknown(1)",
            "url(#legacy)",
            "blur(10%)",
            "brightness(-1)",
            "hue-rotate(2)",
            "drop-shadow(1px 1px 1px 1px black)",
        ] {
            let html = format!(
                "<!doctype html><html><body><div id=\"effect\" style=\"filter:{filter}\"></div></body></html>"
            );
            let document = document(&html);
            let scene = retained(&document, 1);
            let node = node_by_selector(&document, &scene, "#effect");
            assert!(
                !node.effects.iter().any(|effect| matches!(
                    effect,
                    SceneEffect::ForegroundFilter { .. }
                        | SceneEffect::RejectedForegroundFilter { .. }
                )),
                "{filter}"
            );
        }
    }

    #[test]
    fn foreground_identity_survives_parameter_change_and_damage_uses_old_and_new_bounds() {
        let mut document = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"width:20px;height:10px;filter:blur(1px)\"></div></body></html>",
        );
        let identities = IdentityRegistry::from_document(&document);
        let first = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(1),
            ViewportSpec {
                logical_width: 200,
                logical_height: 120,
                ..ViewportSpec::default()
            },
        )
        .unwrap();
        let element = document.query_selector("#effect").unwrap().unwrap();
        let old = node_by_selector(&document, &first, "#effect");
        let old_node_id = old.id;
        let old_effect_id = foreground_filter(old).id;
        let old_version = foreground_filter(old).version;
        let old_damage = old.bounds.damage.clone();

        document.mutate().set_attribute(
            element,
            blitz_dom::QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: blitz_dom::LocalName::from("style"),
            },
            "width:20px;height:10px;filter:blur(3px)",
        );
        document.resolve(0.0);
        let second = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(2),
            ViewportSpec {
                logical_width: 200,
                logical_height: 120,
                ..ViewportSpec::default()
            },
        )
        .unwrap();
        let new = node_by_selector(&document, &second, "#effect");
        assert_eq!(new.id, old_node_id);
        assert_eq!(foreground_filter(new).id, old_effect_id);
        assert_ne!(foreground_filter(new).version, old_version);
        assert_ne!(new.bounds.damage, old_damage);

        let delta = diff_retained_scenes(Some(&first), &second);
        let change = delta
            .changes
            .iter()
            .find(|change| change.id == old_node_id)
            .expect("filter parameter change");
        assert!(change.kinds.contains(&SceneChangeKind::Effect));
        assert!(!change.kinds.contains(&SceneChangeKind::Inserted));
        assert!(!change.kinds.contains(&SceneChangeKind::Removed));
        let damage = super::super::damage::DamageRegion::from_delta(Some(&first), &second, &delta);
        let rects = damage.logical_rects(200, 120);
        assert_eq!(rects.len(), 1);
        let rect = &rects[0];
        assert!(rect.x <= old_damage.x);
        assert!(rect.y <= old_damage.y);
        assert!(rect.x + rect.width >= new.bounds.damage.x + new.bounds.damage.width);
        assert!(rect.y + rect.height >= new.bounds.damage.y + new.bounds.damage.height);
    }

    #[test]
    fn filter_parameter_change_does_not_reclassify_source_paint_or_text_resource() {
        let mut document = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:brightness(1.1)\">text</div></body></html>",
        );
        let identities = IdentityRegistry::from_document(&document);
        let viewport = ViewportSpec {
            logical_width: 200,
            logical_height: 120,
            ..ViewportSpec::default()
        };
        let first = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(1),
            viewport,
        )
        .unwrap();
        let element = document.query_selector("#effect").unwrap().unwrap();
        let node_id = node_by_selector(&document, &first, "#effect").id;
        let resource_versions: BTreeMap<_, _> = first
            .resources
            .iter()
            .map(|resource| (resource.id.clone(), resource.version))
            .collect();

        document.mutate().set_attribute(
            element,
            blitz_dom::QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: blitz_dom::LocalName::from("style"),
            },
            "filter:brightness(1.2)",
        );
        document.resolve(0.0);
        let second = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(2),
            viewport,
        )
        .unwrap();
        let delta = diff_retained_scenes(Some(&first), &second);
        let effect_change = delta
            .changes
            .iter()
            .find(|change| change.id == node_id)
            .expect("foreground effect parameter change");
        assert_eq!(
            effect_change.kinds,
            BTreeSet::from([SceneChangeKind::Effect])
        );
        assert!(delta.resource_changes.is_empty());
        assert_eq!(
            second
                .resources
                .iter()
                .map(|resource| (resource.id.clone(), resource.version))
                .collect::<BTreeMap<_, _>>(),
            resource_versions
        );
    }

    #[test]
    fn adding_removing_and_reordering_filters_preserves_scene_identity() {
        let mut document = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"width:20px;height:10px\"></div></body></html>",
        );
        let identities = IdentityRegistry::from_document(&document);
        let viewport = ViewportSpec {
            logical_width: 200,
            logical_height: 120,
            ..ViewportSpec::default()
        };
        let first = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(1),
            viewport,
        )
        .unwrap();
        let element = document.query_selector("#effect").unwrap().unwrap();
        let node_id = node_by_selector(&document, &first, "#effect").id;

        let mut revisions = Vec::new();
        for (revision, style) in [
            (
                2,
                "width:20px;height:10px;filter:brightness(1) contrast(.5)",
            ),
            (
                3,
                "width:20px;height:10px;filter:contrast(.5) brightness(1)",
            ),
            (4, "width:20px;height:10px;filter:none"),
        ] {
            document.mutate().set_attribute(
                element,
                blitz_dom::QualName {
                    prefix: None,
                    ns: blitz_dom::Namespace::from(""),
                    local: blitz_dom::LocalName::from("style"),
                },
                style,
            );
            document.resolve(0.0);
            revisions.push(
                build_retained_scene(
                    &document,
                    &identities,
                    ExperimentalDocumentIdentity { serial: 5 },
                    SceneRevision(revision),
                    viewport,
                )
                .unwrap(),
            );
        }

        assert!(
            revisions
                .iter()
                .all(|scene| node_by_selector(&document, scene, "#effect").id == node_id)
        );
        let first_list = foreground_filter(revisions[0].node(node_id).unwrap());
        let reordered_list = foreground_filter(revisions[1].node(node_id).unwrap());
        assert_eq!(first_list.id, reordered_list.id);
        assert_ne!(first_list.version, reordered_list.version);
        assert!(
            revisions[2]
                .node(node_id)
                .unwrap()
                .effects
                .iter()
                .all(|effect| !matches!(effect, SceneEffect::ForegroundFilter { .. }))
        );
        for (old, new) in [
            (&first, &revisions[0]),
            (&revisions[0], &revisions[1]),
            (&revisions[1], &revisions[2]),
        ] {
            let delta = diff_retained_scenes(Some(old), new);
            let change = delta
                .changes
                .iter()
                .find(|change| change.id == node_id)
                .expect("effect update");
            assert!(change.kinds.contains(&SceneChangeKind::Effect));
            assert!(!change.kinds.contains(&SceneChangeKind::Inserted));
            assert!(!change.kinds.contains(&SceneChangeKind::Removed));
        }
    }

    #[test]
    fn filtered_movement_and_removal_damage_old_and_new_expanded_bounds() {
        let mut document = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"position:absolute;left:10px;top:10px;width:10px;height:10px;filter:blur(2px)\"></div></body></html>",
        );
        let identities = IdentityRegistry::from_document(&document);
        let viewport = ViewportSpec {
            logical_width: 200,
            logical_height: 120,
            ..ViewportSpec::default()
        };
        let first = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(1),
            viewport,
        )
        .unwrap();
        let element = document.query_selector("#effect").unwrap().unwrap();
        let node_id = node_by_selector(&document, &first, "#effect").id;
        let old_bounds = first.node(node_id).unwrap().bounds.damage.clone();

        document.mutate().set_attribute(
            element,
            blitz_dom::QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: blitz_dom::LocalName::from("style"),
            },
            "position:absolute;left:80px;top:30px;width:10px;height:10px;filter:blur(2px)",
        );
        document.resolve(0.0);
        let second = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(2),
            viewport,
        )
        .unwrap();
        let new_bounds = second.node(node_id).unwrap().bounds.damage.clone();
        let movement_delta = diff_retained_scenes(Some(&first), &second);
        let movement_damage =
            super::super::damage::DamageRegion::from_delta(Some(&first), &second, &movement_delta);
        let movement_rects = movement_damage.logical_rects(200, 120);
        assert!(
            movement_rects
                .iter()
                .any(|rect| contains(rect, &old_bounds))
        );
        assert!(
            movement_rects
                .iter()
                .any(|rect| contains(rect, &new_bounds))
        );

        assert!(document.mutate().remove_and_drop_node(element).is_some());
        document.resolve(0.0);
        let third = build_retained_scene(
            &document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(3),
            viewport,
        )
        .unwrap();
        let removal_delta = diff_retained_scenes(Some(&second), &third);
        let removal = removal_delta
            .changes
            .iter()
            .find(|change| change.id == node_id)
            .expect("filtered node removal");
        assert!(
            removal.kinds.contains(&SceneChangeKind::Removed),
            "{removal:?}"
        );
        let removal_damage =
            super::super::damage::DamageRegion::from_delta(Some(&second), &third, &removal_delta);
        assert!(
            removal_damage
                .logical_rects(200, 120)
                .iter()
                .any(|rect| contains(rect, &new_bounds))
        );
    }

    #[test]
    fn current_color_resolution_changes_effect_version_without_changing_identity() {
        let mut live_document = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"color:red;width:20px;height:10px;filter:drop-shadow(1px 2px currentColor)\"></div></body></html>",
        );
        let identities = IdentityRegistry::from_document(&live_document);
        let first = build_retained_scene(
            &live_document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(1),
            ViewportSpec::default(),
        )
        .unwrap();
        let element = live_document.query_selector("#effect").unwrap().unwrap();
        let first_node = node_by_selector(&live_document, &first, "#effect");
        let effect_id = foreground_filter(first_node).id;
        let first_version = foreground_filter(first_node).version;
        live_document.mutate().set_attribute(
            element,
            blitz_dom::QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: blitz_dom::LocalName::from("style"),
            },
            "color:blue;width:20px;height:10px;filter:drop-shadow(1px 2px currentColor)",
        );
        live_document.resolve(0.0);
        let second = build_retained_scene(
            &live_document,
            &identities,
            ExperimentalDocumentIdentity { serial: 5 },
            SceneRevision(2),
            ViewportSpec::default(),
        )
        .unwrap();
        let second_node = node_by_selector(&live_document, &second, "#effect");
        assert_eq!(foreground_filter(second_node).id, effect_id);
        assert_ne!(foreground_filter(second_node).version, first_version);

        let explicit = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"color:blue;width:20px;height:10px;filter:drop-shadow(1px 2px blue)\"></div></body></html>",
        );
        let explicit_scene = retained(&explicit, 2);
        assert_eq!(
            foreground_filter(second_node).version,
            foreground_filter(node_by_selector(&explicit, &explicit_scene, "#effect")).version
        );
    }

    #[test]
    fn filtered_source_graphic_includes_descendants_and_propagates_ancestor_damage() {
        let document = document(
            "<!doctype html><html><body><div id=\"parent\" style=\"width:20px;height:20px;filter:blur(2px)\"><span id=\"child\" style=\"display:block;width:40px;height:10px\">child</span></div></body></html>",
        );
        let scene = retained(&document, 1);
        let parent = node_by_selector(&document, &scene, "#parent");
        let child = node_by_selector(&document, &scene, "#child");
        let (source, filtered) = parent
            .effects
            .iter()
            .find_map(|effect| match effect {
                SceneEffect::ForegroundFilter {
                    source_graphic_bounds,
                    filtered_bounds,
                    ..
                } => Some((source_graphic_bounds, filtered_bounds)),
                _ => None,
            })
            .expect("filtered SourceGraphic metadata");
        assert!(source.width >= child.bounds.visual.width);
        assert!(filtered.width >= source.width);
        assert!(child.bounds.damage.x <= filtered.x);
        assert!(child.bounds.damage.y <= filtered.y);
        assert!(child.bounds.damage.x + child.bounds.damage.width >= filtered.x + filtered.width);
        assert!(child.bounds.damage.y + child.bounds.damage.height >= filtered.y + filtered.height);
    }

    #[test]
    fn active_filter_and_nesting_limits_use_central_constants() {
        let mut html = String::from("<!doctype html><html><body>");
        for index in 0..=MAX_ACTIVE_FILTERED_ELEMENTS_PER_SURFACE {
            write!(
                html,
                "<div id=\"effect-{index}\" style=\"filter:brightness(1)\"></div>"
            )
            .unwrap();
        }
        html.push_str("</body></html>");
        let active_document = document(&html);
        let scene = retained(&active_document, 1);
        let normalized = scene
            .nodes
            .iter()
            .filter(|node| {
                node.effects
                    .iter()
                    .any(|effect| matches!(effect, SceneEffect::ForegroundFilter { .. }))
            })
            .count();
        let rejected = scene
            .nodes
            .iter()
            .filter(|node| {
                rejected_filter(node) == Some(ForegroundEffectRejection::ActiveElementCount)
            })
            .count();
        assert_eq!(normalized, MAX_ACTIVE_FILTERED_ELEMENTS_PER_SURFACE);
        assert_eq!(rejected, 1);

        let mut declarations = String::from("<!doctype html><html><body>");
        for index in 0..MAX_FILTER_DECLARATIONS_PER_DOCUMENT {
            write!(
                declarations,
                "<div style=\"display:none;filter:brightness({})\"></div>",
                index as f32 / 100.0
            )
            .unwrap();
        }
        write!(
            declarations,
            "<div id=\"declaration-overflow\" style=\"filter:brightness({})\"></div>",
            MAX_FILTER_DECLARATIONS_PER_DOCUMENT as f32 / 100.0
        )
        .unwrap();
        declarations.push_str("</body></html>");
        let declaration_document = document(&declarations);
        let declaration_scene = retained(&declaration_document, 1);
        assert_eq!(
            rejected_filter(node_by_selector(
                &declaration_document,
                &declaration_scene,
                "#declaration-overflow"
            )),
            Some(ForegroundEffectRejection::DeclarationCount)
        );

        let mut nested = String::from("<!doctype html><html><body>");
        for index in 0..=MAX_FILTER_NESTING_DEPTH {
            write!(
                nested,
                "<div id=\"nested-{index}\" style=\"filter:brightness(1)\">"
            )
            .unwrap();
        }
        for _ in 0..=MAX_FILTER_NESTING_DEPTH {
            nested.push_str("</div>");
        }
        nested.push_str("</body></html>");
        let nested_document = document(&nested);
        let scene = retained(&nested_document, 1);
        assert_eq!(
            rejected_filter(node_by_selector(
                &nested_document,
                &scene,
                &format!("#nested-{}", MAX_FILTER_NESTING_DEPTH)
            )),
            Some(ForegroundEffectRejection::NestingDepth)
        );
    }

    #[test]
    fn foreground_effect_measurement_probe_is_bounded() {
        const EXTRACTION_ITERATIONS: usize = 100;
        const MODEL_ITERATIONS: usize = 10_000;

        let one = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"width:20px;height:10px;filter:brightness(1.2)\"></div></body></html>",
        );
        let one_identities = IdentityRegistry::from_document(&one);
        let one_started = Instant::now();
        for revision in 0..EXTRACTION_ITERATIONS {
            std::hint::black_box(
                build_retained_scene(
                    &one,
                    &one_identities,
                    ExperimentalDocumentIdentity { serial: 5 },
                    SceneRevision(revision as u64),
                    ViewportSpec::default(),
                )
                .unwrap(),
            );
        }
        let one_extraction_ns = one_started.elapsed().as_nanos() / EXTRACTION_ITERATIONS as u128;

        let ten = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"color:#1238;width:20px;height:10px;filter:brightness(.5) contrast(120%) grayscale(.2) hue-rotate(.5turn) invert(.25) opacity(.75) saturate(2) sepia(.5) blur(3px) drop-shadow(4px -5px 2px currentColor)\"></div></body></html>",
        );
        let ten_identities = IdentityRegistry::from_document(&ten);
        let ten_started = Instant::now();
        for revision in 0..EXTRACTION_ITERATIONS {
            std::hint::black_box(
                build_retained_scene(
                    &ten,
                    &ten_identities,
                    ExperimentalDocumentIdentity { serial: 6 },
                    SceneRevision(revision as u64),
                    ViewportSpec::default(),
                )
                .unwrap(),
            );
        }
        let ten_extraction_ns = ten_started.elapsed().as_nanos() / EXTRACTION_ITERATIONS as u128;
        let ten_scene = retained(&ten, 1);
        let list = foreground_filter(node_by_selector(&ten, &ten_scene, "#effect")).clone();

        let matrix_started = Instant::now();
        for _ in 0..MODEL_ITERATIONS {
            std::hint::black_box(list.color_matrix_runs().unwrap());
        }
        let matrix_ns = matrix_started.elapsed().as_nanos() / MODEL_ITERATIONS as u128;

        let serialization_started = Instant::now();
        for _ in 0..MODEL_ITERATIONS {
            std::hint::black_box(list.serialize_semantics());
        }
        let serialization_ns =
            serialization_started.elapsed().as_nanos() / MODEL_ITERATIONS as u128;

        let equality_started = Instant::now();
        let comparison = list.clone();
        for _ in 0..MODEL_ITERATIONS {
            std::hint::black_box(list.eq(&comparison));
        }
        let equality_ns = equality_started.elapsed().as_nanos() / MODEL_ITERATIONS as u128;

        let bounds = safe_rect(10.0, 20.0, 100.0, 40.0);
        let bounds_started = Instant::now();
        for _ in 0..MODEL_ITERATIONS {
            std::hint::black_box(list.propagated_bounds(&bounds).unwrap());
        }
        let bounds_ns = bounds_started.elapsed().as_nanos() / MODEL_ITERATIONS as u128;

        let no_op_started = Instant::now();
        for _ in 0..MODEL_ITERATIONS {
            std::hint::black_box(diff_retained_scenes(Some(&ten_scene), &ten_scene));
        }
        let no_op_ns = no_op_started.elapsed().as_nanos() / MODEL_ITERATIONS as u128;

        let mut update_document = document(
            "<!doctype html><html><body><div id=\"effect\" style=\"filter:brightness(1.1)\"></div></body></html>",
        );
        let update_identities = IdentityRegistry::from_document(&update_document);
        let update_before = build_retained_scene(
            &update_document,
            &update_identities,
            ExperimentalDocumentIdentity { serial: 9 },
            SceneRevision(1),
            ViewportSpec::default(),
        )
        .unwrap();
        let update_slot = update_document.query_selector("#effect").unwrap().unwrap();
        update_document.mutate().set_attribute(
            update_slot,
            blitz_dom::QualName {
                prefix: None,
                ns: blitz_dom::Namespace::from(""),
                local: blitz_dom::LocalName::from("style"),
            },
            "filter:brightness(1.2)",
        );
        update_document.resolve(0.0);
        let update_started = Instant::now();
        let update_after = build_retained_scene(
            &update_document,
            &update_identities,
            ExperimentalDocumentIdentity { serial: 9 },
            SceneRevision(2),
            ViewportSpec::default(),
        )
        .unwrap();
        let update_delta = diff_retained_scenes(Some(&update_before), &update_after);
        let effect_update_us = update_started.elapsed().as_micros();
        assert!(
            update_delta
                .changes
                .iter()
                .any(|change| change.kinds.contains(&SceneChangeKind::Effect))
        );

        let mut nested_html = String::from("<!doctype html><html><body>");
        for _ in 0..MAX_FILTER_NESTING_DEPTH {
            nested_html.push_str("<div style=\"filter:brightness(1.1)\">");
        }
        nested_html.push_str("<span>nested</span>");
        for _ in 0..MAX_FILTER_NESTING_DEPTH {
            nested_html.push_str("</div>");
        }
        nested_html.push_str("</body></html>");
        let nested = document(&nested_html);
        let nested_identities = IdentityRegistry::from_document(&nested);
        let nested_started = Instant::now();
        let nested_scene = build_retained_scene(
            &nested,
            &nested_identities,
            ExperimentalDocumentIdentity { serial: 8 },
            SceneRevision(1),
            ViewportSpec::default(),
        )
        .unwrap();
        let nested_scene_us = nested_started.elapsed().as_micros();
        assert!(nested_scene.nodes.len() > MAX_FILTER_NESTING_DEPTH);
        eprintln!(
            "foreground_effect_model one_extraction_ns={one_extraction_ns} ten_extraction_ns={ten_extraction_ns} matrix_ns={matrix_ns} serialization_ns={serialization_ns} equality_ns={equality_ns} bounds_ns={bounds_ns} effect_update_us={effect_update_us} noop_delta_ns={no_op_ns} nested_scene_us={nested_scene_us}"
        );
    }

    fn contains(outer: &LogicalRect, inner: &LogicalRect) -> bool {
        outer.x <= inner.x
            && outer.y <= inner.y
            && outer.x + outer.width >= inner.x + inner.width
            && outer.y + outer.height >= inner.y + inner.height
    }

    #[test]
    fn missing_image_resource_is_failed_and_generation_bound() {
        let document = document(
            "<!doctype html><html><body><img id=\"missing\" src=\"\" style=\"width:20px;height:20px\"></body></html>",
        );
        let scene = retained(&document, 1);
        let resource = scene
            .resources
            .iter()
            .find(|resource| resource.id.kind == ResourceKind::RasterImage)
            .expect("missing raster resource");
        assert_eq!(resource.lifecycle, ResourceLifecycle::Failed);
        assert!(matches!(
            resource.id.owner,
            ResourceOwner::Document(ExperimentalDocumentIdentity { serial: 5 })
        ));
        let mut replacement = retained(&document, 2);
        replacement.document = ExperimentalDocumentIdentity { serial: 6 };
        assert_ne!(scene.document, replacement.document);
        assert!(diff_retained_scenes(Some(&scene), &replacement).full_scene_replacement);
    }

    #[test]
    fn delta_classifies_insert_remove_paint_order_and_reparent() {
        let mut old = scene(1, vec![node(1, 0.0), node(2, 20.0), node(3, 40.0)]);
        let mut changed = node(1, 0.0);
        changed.paint_signature = 2;
        changed.paint_order = 3;
        changed.parent = Some(node(2, 20.0).id);
        let inserted = node(4, 60.0);
        let new = scene(2, vec![changed, node(2, 20.0), inserted]);
        old.content_fingerprint = 1;
        let delta = diff_retained_scenes(Some(&old), &new);
        assert!(delta.changes.iter().any(|change| {
            change.kinds.contains(&SceneChangeKind::Inserted)
                && change.old_bounds.is_none()
                && change.new_bounds.is_some()
        }));
        assert!(delta.changes.iter().any(|change| {
            change.kinds.contains(&SceneChangeKind::Removed)
                && change.old_bounds.is_some()
                && change.new_bounds.is_none()
        }));
        let updated = delta
            .changes
            .iter()
            .find(|change| change.id.dom.is_some_and(|identity| identity.slot == 1))
            .unwrap();
        assert!(updated.kinds.contains(&SceneChangeKind::Paint));
        assert!(updated.kinds.contains(&SceneChangeKind::StackingOrOrder));
        assert!(updated.kinds.contains(&SceneChangeKind::Reparented));
    }

    #[test]
    fn excessive_delta_collapses_to_bounded_full_replacement() {
        let old = scene(
            1,
            (0..=MAX_SCENE_DELTA_ENTRIES)
                .map(|slot| node(slot, slot as f32))
                .collect(),
        );
        let new = scene(2, Vec::new());
        let delta = diff_retained_scenes(Some(&old), &new);
        assert!(delta.full_scene_replacement);
        assert!(delta.changes.is_empty());
        assert!(delta.resource_changes.is_empty());
    }
}
