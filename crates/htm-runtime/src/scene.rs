use crate::adapter::{
    border_radii, collect_retained_paint_order, image_diagnostic, round, safe_rect, text_diagnostic,
};
use crate::identity::{IdentityRegistry, author_slots};
use crate::incremental::{
    ClipSnapshot, DamageEstimate, ExperimentalDocumentIdentity, ExperimentalSceneDiff,
    ExperimentalSceneSnapshot, FieldChange, InteractionStateSnapshot, MutationPhase,
    NodeMetadataSnapshot, SceneDiffSummary, SceneNodeChange, SceneNodeSnapshot, SceneTextSnapshot,
};
use crate::model::{LogicalRect, OverflowDiagnostic, ViewportSpec};
use crate::{INCREMENTAL_DIFF_SCHEMA_VERSION, INCREMENTAL_SNAPSHOT_SCHEMA_VERSION, RuntimeError};
use blitz_dom::node::NodeData;
use blitz_html::HtmlDocument;
use std::collections::{BTreeMap, BTreeSet};
use stylo::color::ColorSpace;

pub(crate) fn build_scene_snapshot(
    document: &HtmlDocument,
    identities: &IdentityRegistry,
    phase: MutationPhase,
    document_identity: ExperimentalDocumentIdentity,
    document_parse_count: u32,
    document_instance_retained: bool,
    viewport: ViewportSpec,
) -> Result<ExperimentalSceneSnapshot, RuntimeError> {
    let live = identities.live_identities(document)?;
    let paint_indices: BTreeMap<_, _> = collect_retained_paint_order(document)
        .into_iter()
        .enumerate()
        .map(|(index, slot)| (slot, index))
        .collect();
    let slots = author_slots(document);
    let mut nodes = Vec::with_capacity(slots.len());
    for (tree_order, slot) in slots.into_iter().enumerate() {
        let node = document.get_node(slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "author-tree Blitz slot {slot} disappeared while snapshotting"
            ))
        })?;
        let identity = *live.get(&slot).ok_or_else(|| {
            RuntimeError::InvalidMutationTarget(format!(
                "author-tree Blitz slot {slot} has no live experimental identity"
            ))
        })?;
        let parent_identity = node.parent.and_then(|parent| live.get(&parent).copied());
        let absolute = node.absolute_position(0.0, 0.0);
        let logical_bounds = safe_rect(
            absolute.x,
            absolute.y,
            node.final_layout.size.width,
            node.final_layout.size.height,
        );
        validate_rect(&logical_bounds)?;

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
        classes.dedup();

        let display = format!("{:?}", node.style.display).to_ascii_lowercase();
        let position = format!("{:?}", node.style.position).to_ascii_lowercase();
        let overflow_x = format!("{:?}", node.style.overflow.x).to_ascii_lowercase();
        let overflow_y = format!("{:?}", node.style.overflow.y).to_ascii_lowercase();
        let visibility = node
            .primary_styles()
            .map(|styles| {
                format!("{:?}", styles.get_inherited_box().visibility).to_ascii_lowercase()
            })
            .unwrap_or_else(|| "unresolved".into());
        let visible = display != "none" && visibility == "visible";
        let opacity = node
            .primary_styles()
            .map(|styles| round(styles.get_effects().opacity))
            .unwrap_or(1.0);
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
        let radii = border_radii(node);
        let border_signature = stable_hash(
            &node
                .primary_styles()
                .map(|styles| format!("{:?}", styles.get_border()))
                .unwrap_or_else(|| "unresolved".into()),
        );
        let transform_signature = node.transform.map(|transform| {
            transform
                .as_coeffs()
                .into_iter()
                .map(|value| format!("{value:.6}"))
                .collect::<Vec<_>>()
                .join(",")
        });
        let text = match &node.data {
            NodeData::Text(value) if !value.content.trim().is_empty() => {
                let content = value.content.trim().to_owned();
                Some(SceneTextSnapshot {
                    stable_hash: stable_hash(&content),
                    content,
                    measured_bounds: None,
                })
            }
            _ => text_diagnostic(node).map(|value| SceneTextSnapshot {
                stable_hash: stable_hash(&value.content),
                content: value.content,
                measured_bounds: Some(value.measured_bounds),
            }),
        };
        let resource = image_diagnostic(node);
        let clip = ClipSnapshot {
            overflow: OverflowDiagnostic {
                establishes_clip: overflow_x != "visible" || overflow_y != "visible",
                x: overflow_x,
                y: overflow_y,
            },
            establishes_stacking_context: node.stacking_context.is_some(),
        };
        let interaction = InteractionStateSnapshot {
            hovered: node.is_hovered(),
            active: node.is_active(),
            focused: node.is_focussed(),
        };
        let style_paint_signature = stable_hash(&format!(
            "{display}|{position}|{visibility}|{opacity:.3}|{background_srgba:?}|{radii:?}|{border_signature}|{clip:?}"
        ));

        nodes.push(SceneNodeSnapshot {
            identity,
            parent_identity,
            tree_order,
            paint_order: paint_indices.get(&slot).copied(),
            node_type: format!("{:?}", node.data.kind()).to_ascii_lowercase(),
            tag,
            metadata: NodeMetadataSnapshot { html_id, classes },
            logical_bounds,
            visibility,
            visible,
            display,
            position,
            opacity,
            background_srgba,
            border_radii: radii,
            border_signature,
            transform_signature,
            style_paint_signature,
            text,
            resource,
            clip,
            interaction,
        });
    }

    Ok(ExperimentalSceneSnapshot {
        schema_version: INCREMENTAL_SNAPSHOT_SCHEMA_VERSION,
        phase,
        document_identity,
        document_parse_count,
        blitz_document_instance_retained: document_instance_retained,
        viewport,
        node_count: nodes.len(),
        nodes,
    })
}

pub(crate) fn diff_scenes(
    old: &ExperimentalSceneSnapshot,
    new: &ExperimentalSceneSnapshot,
) -> ExperimentalSceneDiff {
    let old_nodes: BTreeMap<_, _> = old.nodes.iter().map(|node| (node.identity, node)).collect();
    let new_nodes: BTreeMap<_, _> = new.nodes.iter().map(|node| (node.identity, node)).collect();
    let old_ids: BTreeSet<_> = old_nodes.keys().copied().collect();
    let new_ids: BTreeSet<_> = new_nodes.keys().copied().collect();

    let created_nodes: Vec<_> = new_ids
        .difference(&old_ids)
        .map(|identity| (*new_nodes[identity]).clone())
        .collect();
    let removed_nodes: Vec<_> = old_ids
        .difference(&new_ids)
        .map(|identity| (*old_nodes[identity]).clone())
        .collect();
    let mut retained_unchanged = Vec::new();
    let mut changed_nodes = Vec::new();
    for identity in old_ids.intersection(&new_ids) {
        let old_node = old_nodes[identity];
        let new_node = new_nodes[identity];
        if old_node == new_node {
            retained_unchanged.push(*identity);
            continue;
        }
        let geometry = changed(&old_node.logical_bounds, &new_node.logical_bounds);
        let style_or_paint = changed(
            &old_node.style_paint_signature,
            &new_node.style_paint_signature,
        );
        let metadata = changed(&old_node.metadata, &new_node.metadata);
        let text = changed(&old_node.text, &new_node.text);
        let resource = changed(&old_node.resource, &new_node.resource);
        let parent = changed(&old_node.parent_identity, &new_node.parent_identity);
        let tree_order = changed(&old_node.tree_order, &new_node.tree_order);
        let paint_order = changed(&old_node.paint_order, &new_node.paint_order);
        let clip = changed(&old_node.clip, &new_node.clip);
        let transform = changed(&old_node.transform_signature, &new_node.transform_signature);
        let interaction = changed(&old_node.interaction, &new_node.interaction);
        changed_nodes.push(SceneNodeChange {
            identity: *identity,
            geometry,
            style_or_paint,
            metadata,
            text,
            resource,
            parent,
            tree_order,
            paint_order,
            clip,
            transform,
            interaction,
        });
    }

    let summary = SceneDiffSummary {
        created: created_nodes.len(),
        removed: removed_nodes.len(),
        retained_unchanged: retained_unchanged.len(),
        changed: changed_nodes.len(),
        geometry_changes: changed_nodes
            .iter()
            .filter(|change| change.geometry.is_some())
            .count(),
        style_or_paint_changes: changed_nodes
            .iter()
            .filter(|change| change.style_or_paint.is_some())
            .count(),
        text_changes: changed_nodes
            .iter()
            .filter(|change| change.text.is_some())
            .count(),
        resource_changes: changed_nodes
            .iter()
            .filter(|change| change.resource.is_some())
            .count(),
        parent_changes: changed_nodes
            .iter()
            .filter(|change| change.parent.is_some())
            .count(),
        order_changes: changed_nodes
            .iter()
            .filter(|change| change.tree_order.is_some() || change.paint_order.is_some())
            .count(),
        clip_changes: changed_nodes
            .iter()
            .filter(|change| change.clip.is_some())
            .count(),
        interaction_changes: changed_nodes
            .iter()
            .filter(|change| change.interaction.is_some())
            .count(),
    };
    let damage_estimate = estimate_damage(
        &created_nodes,
        &removed_nodes,
        &changed_nodes,
        &old_nodes,
        &new_nodes,
    );
    let is_empty = created_nodes.is_empty() && removed_nodes.is_empty() && changed_nodes.is_empty();

    ExperimentalSceneDiff {
        schema_version: INCREMENTAL_DIFF_SCHEMA_VERSION,
        from_phase: old.phase,
        to_phase: new.phase,
        is_empty,
        summary,
        created_nodes,
        removed_nodes,
        retained_unchanged,
        changed_nodes,
        damage_estimate,
    }
}

fn changed<T: PartialEq + Clone>(old: &T, new: &T) -> Option<FieldChange<T>> {
    (old != new).then(|| FieldChange {
        old: old.clone(),
        new: new.clone(),
    })
}

fn estimate_damage(
    created: &[SceneNodeSnapshot],
    removed: &[SceneNodeSnapshot],
    changed: &[SceneNodeChange],
    old_nodes: &BTreeMap<crate::ExperimentalNodeIdentity, &SceneNodeSnapshot>,
    new_nodes: &BTreeMap<crate::ExperimentalNodeIdentity, &SceneNodeSnapshot>,
) -> DamageEstimate {
    let mut bounds = Vec::new();
    bounds.extend(created.iter().map(|node| node.logical_bounds.clone()));
    bounds.extend(removed.iter().map(|node| node.logical_bounds.clone()));
    for change in changed {
        if let Some(old) = old_nodes.get(&change.identity) {
            bounds.push(old.logical_bounds.clone());
        }
        if let Some(new) = new_nodes.get(&change.identity) {
            bounds.push(new.logical_bounds.clone());
        }
    }
    bounds.retain(|rect| {
        rect.x.is_finite()
            && rect.y.is_finite()
            && rect.width.is_finite()
            && rect.height.is_finite()
            && rect.width >= 0.0
            && rect.height >= 0.0
    });
    let total_bounds = bounds.iter().cloned().reduce(union_rect);
    DamageEstimate {
        label: "headless diagnostic estimate; not compositor-ready damage",
        changed_node_bounds: bounds,
        total_bounds,
        excluded_expansion: vec![
            "shadows",
            "filters",
            "antialiasing",
            "transformed bounds",
            "backdrop effects",
            "material sampling expansion",
            "renderer-specific paint expansion",
            "compositor damage rules",
        ],
    }
}

fn union_rect(left: LogicalRect, right: LogicalRect) -> LogicalRect {
    let x1 = left.x.min(right.x);
    let y1 = left.y.min(right.y);
    let x2 = (left.x + left.width).max(right.x + right.width);
    let y2 = (left.y + left.height).max(right.y + right.height);
    safe_rect(x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0))
}

fn validate_rect(rect: &LogicalRect) -> Result<(), RuntimeError> {
    if [rect.x, rect.y, rect.width, rect.height]
        .into_iter()
        .all(f32::is_finite)
        && rect.width >= 0.0
        && rect.height >= 0.0
    {
        return Ok(());
    }
    Err(RuntimeError::InvalidPackage(
        "resolved scene contains nonfinite or negative geometry".into(),
    ))
}

pub(crate) fn stable_hash(value: &str) -> String {
    let hash = value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("fnv1a64:{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(identity: u64, text: &str, x: f32) -> SceneNodeSnapshot {
        SceneNodeSnapshot {
            identity: crate::ExperimentalNodeIdentity {
                slot: identity as usize,
                generation: 0,
            },
            parent_identity: None,
            tree_order: identity as usize,
            paint_order: Some(identity as usize),
            node_type: "element".into(),
            tag: Some("div".into()),
            metadata: NodeMetadataSnapshot {
                html_id: None,
                classes: Vec::new(),
            },
            logical_bounds: safe_rect(x, 0.0, 10.0, 10.0),
            visibility: "visible".into(),
            visible: true,
            display: "block".into(),
            position: "relative".into(),
            opacity: 1.0,
            background_srgba: None,
            border_radii: None,
            border_signature: stable_hash("border"),
            transform_signature: None,
            style_paint_signature: stable_hash("style"),
            text: Some(SceneTextSnapshot {
                content: text.into(),
                stable_hash: stable_hash(text),
                measured_bounds: None,
            }),
            resource: None,
            clip: ClipSnapshot {
                overflow: OverflowDiagnostic {
                    x: "visible".into(),
                    y: "visible".into(),
                    establishes_clip: false,
                },
                establishes_stacking_context: false,
            },
            interaction: InteractionStateSnapshot {
                hovered: false,
                active: false,
                focused: false,
            },
        }
    }

    fn empty_snapshot(phase: MutationPhase) -> ExperimentalSceneSnapshot {
        ExperimentalSceneSnapshot {
            schema_version: INCREMENTAL_SNAPSHOT_SCHEMA_VERSION,
            phase,
            document_identity: ExperimentalDocumentIdentity { serial: 1 },
            document_parse_count: 1,
            blitz_document_instance_retained: true,
            viewport: ViewportSpec::default(),
            node_count: 0,
            nodes: Vec::new(),
        }
    }

    #[test]
    fn identical_snapshots_have_empty_diffs() {
        let snapshot = empty_snapshot(MutationPhase::Initial);
        let diff = diff_scenes(&snapshot, &snapshot);
        assert!(diff.is_empty);
        assert_eq!(diff.summary.changed, 0);
        assert!(diff.damage_estimate.total_bounds.is_none());
    }

    #[test]
    fn union_damage_is_finite_and_nonnegative() {
        let result = union_rect(
            safe_rect(10.0, 20.0, 30.0, 40.0),
            safe_rect(-5.0, 30.0, 10.0, 5.0),
        );
        assert_eq!(result, safe_rect(-5.0, 20.0, 45.0, 40.0));
        assert!(result.width >= 0.0 && result.height >= 0.0);
    }

    #[test]
    fn text_only_change_does_not_report_geometry() {
        let mut old = empty_snapshot(MutationPhase::Initial);
        old.nodes.push(node(1, "before", 0.0));
        old.node_count = 1;
        let mut new = old.clone();
        new.phase = MutationPhase::TextMutation;
        new.nodes[0].text = Some(SceneTextSnapshot {
            content: "after".into(),
            stable_hash: stable_hash("after"),
            measured_bounds: None,
        });
        let diff = diff_scenes(&old, &new);
        assert_eq!(diff.summary.text_changes, 1);
        assert_eq!(diff.summary.geometry_changes, 0);
        assert!(diff.changed_nodes[0].geometry.is_none());
    }

    #[test]
    fn geometry_change_carries_old_and_new_bounds() {
        let mut old = empty_snapshot(MutationPhase::Initial);
        old.nodes.push(node(1, "same", 0.0));
        old.node_count = 1;
        let mut new = old.clone();
        new.phase = MutationPhase::ClassMutation;
        new.nodes[0].logical_bounds.x = 20.0;
        let diff = diff_scenes(&old, &new);
        let geometry = diff.changed_nodes[0].geometry.as_ref().unwrap();
        assert_eq!(geometry.old.x, 0.0);
        assert_eq!(geometry.new.x, 20.0);
        assert!(diff.damage_estimate.total_bounds.as_ref().unwrap().width >= 30.0);
    }

    #[test]
    fn created_and_removed_nodes_are_identity_sorted() {
        let mut old = empty_snapshot(MutationPhase::Initial);
        old.nodes = vec![node(3, "three", 0.0), node(1, "one", 0.0)];
        old.node_count = 2;
        let mut new = empty_snapshot(MutationPhase::ListAppend);
        new.nodes = vec![node(4, "four", 0.0), node(2, "two", 0.0)];
        new.node_count = 2;
        let diff = diff_scenes(&old, &new);
        assert_eq!(
            diff.removed_nodes
                .iter()
                .map(|node| node.identity.slot)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(
            diff.created_nodes
                .iter()
                .map(|node| node.identity.slot)
                .collect::<Vec<_>>(),
            vec![2, 4]
        );
        let first = serde_json::to_vec(&diff).unwrap();
        let second = serde_json::to_vec(&diff_scenes(&old, &new)).unwrap();
        assert_eq!(first, second);
    }
}
