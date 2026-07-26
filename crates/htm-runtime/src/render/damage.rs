use super::{RetainedScene, SceneChangeKind, SceneDelta};
use crate::model::LogicalRect;
use serde::{Deserialize, Serialize};

pub const MAX_DAMAGE_RECTS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "rects", rename_all = "snake_case")]
pub enum DamageRegion {
    Empty,
    Rects(Vec<LogicalRect>),
    Full,
}

impl DamageRegion {
    pub(crate) fn from_delta(
        previous: Option<&RetainedScene>,
        current: &RetainedScene,
        delta: &SceneDelta,
    ) -> Self {
        if delta.full_scene_replacement || previous.is_none() {
            return Self::Full;
        }
        if delta.is_empty() {
            return Self::Empty;
        }
        if delta.changes.iter().any(|change| {
            change.kinds.iter().any(|kind| {
                matches!(
                    kind,
                    SceneChangeKind::Clip
                        | SceneChangeKind::StackingOrOrder
                        | SceneChangeKind::Reparented
                )
            })
        }) {
            return Self::Full;
        }

        let surface = surface_rect(current);
        let mut region = Self::Empty;
        for change in &delta.changes {
            if let Some(bounds) = &change.old_bounds {
                region.add(bounds.damage.clone(), &surface);
            }
            if let Some(bounds) = &change.new_bounds {
                region.add(bounds.damage.clone(), &surface);
            }
            if matches!(region, Self::Full) {
                return region;
            }
        }
        if !delta.resource_changes.is_empty() {
            let changed: std::collections::BTreeSet<_> = delta
                .resource_changes
                .iter()
                .map(|change| change.id.clone())
                .collect();
            if let Some(previous) = previous {
                for node in &previous.nodes {
                    if node
                        .resource
                        .as_ref()
                        .is_some_and(|(id, _)| changed.contains(id))
                    {
                        region.add(node.bounds.damage.clone(), &surface);
                    }
                }
            }
            for node in &current.nodes {
                if node
                    .resource
                    .as_ref()
                    .is_some_and(|(id, _)| changed.contains(id))
                {
                    region.add(node.bounds.damage.clone(), &surface);
                }
            }
        }
        region
    }

    pub(crate) fn add(&mut self, rect: LogicalRect, surface: &LogicalRect) {
        if matches!(self, Self::Full) {
            return;
        }
        if !valid_rect(&rect) || !valid_rect(surface) {
            *self = Self::Full;
            return;
        }
        let Some(rect) = normalize_and_clip(rect, surface) else {
            return;
        };
        if rect == *surface {
            *self = Self::Full;
            return;
        }
        let rects = match self {
            Self::Empty => {
                *self = Self::Rects(vec![rect]);
                return;
            }
            Self::Rects(rects) => rects,
            Self::Full => return,
        };

        let mut merged = rect;
        let mut index = 0usize;
        while index < rects.len() {
            if touches_or_overlaps(&merged, &rects[index]) {
                merged = union_rect(&merged, &rects.remove(index));
                index = 0;
            } else {
                index += 1;
            }
        }
        rects.push(merged);
        rects.sort_by(|left, right| {
            left.y
                .total_cmp(&right.y)
                .then_with(|| left.x.total_cmp(&right.x))
                .then_with(|| left.height.total_cmp(&right.height))
                .then_with(|| left.width.total_cmp(&right.width))
        });
        if rects.len() > MAX_DAMAGE_RECTS {
            *self = Self::Full;
        }
    }

    pub fn logical_rects(&self, logical_width: u32, logical_height: u32) -> Vec<LogicalRect> {
        match self {
            Self::Empty => Vec::new(),
            Self::Rects(rects) => rects.clone(),
            Self::Full => vec![LogicalRect {
                x: 0.0,
                y: 0.0,
                width: logical_width as f32,
                height: logical_height as f32,
            }],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhysicalDamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub fn logical_damage_to_physical(
    damage: &DamageRegion,
    logical_width: u32,
    logical_height: u32,
    physical_width: u32,
    physical_height: u32,
    scale_numerator: u32,
    scale_denominator: u32,
) -> Vec<PhysicalDamageRect> {
    if scale_numerator == 0 || scale_denominator == 0 {
        return vec![PhysicalDamageRect {
            x: 0,
            y: 0,
            width: physical_width,
            height: physical_height,
        }];
    }
    let logical = damage.logical_rects(logical_width, logical_height);
    if logical.iter().any(|rect| !valid_rect(rect)) {
        return vec![PhysicalDamageRect {
            x: 0,
            y: 0,
            width: physical_width,
            height: physical_height,
        }];
    }
    logical
        .into_iter()
        .filter_map(|rect| {
            let scale = f64::from(scale_numerator) / f64::from(scale_denominator);
            let x1 = (f64::from(rect.x) * scale).floor();
            let y1 = (f64::from(rect.y) * scale).floor();
            let x2 = (f64::from(rect.x + rect.width) * scale).ceil();
            let y2 = (f64::from(rect.y + rect.height) * scale).ceil();
            let x1 = x1.max(0.0).min(f64::from(physical_width)) as u32;
            let y1 = y1.max(0.0).min(f64::from(physical_height)) as u32;
            let x2 = x2.max(f64::from(x1)).min(f64::from(physical_width)) as u32;
            let y2 = y2.max(f64::from(y1)).min(f64::from(physical_height)) as u32;
            (x2 > x1 && y2 > y1).then_some(PhysicalDamageRect {
                x: x1,
                y: y1,
                width: x2 - x1,
                height: y2 - y1,
            })
        })
        .collect()
}

fn surface_rect(scene: &RetainedScene) -> LogicalRect {
    LogicalRect {
        x: 0.0,
        y: 0.0,
        width: scene.viewport.logical_width as f32,
        height: scene.viewport.logical_height as f32,
    }
}

fn valid_rect(rect: &LogicalRect) -> bool {
    [
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
}

fn normalize_and_clip(rect: LogicalRect, surface: &LogicalRect) -> Option<LogicalRect> {
    if !valid_rect(&rect) || !valid_rect(surface) {
        return None;
    }
    let x1 = rect.x.max(surface.x);
    let y1 = rect.y.max(surface.y);
    let x2 = (rect.x + rect.width).min(surface.x + surface.width);
    let y2 = (rect.y + rect.height).min(surface.y + surface.height);
    (x2 > x1 && y2 > y1).then_some(LogicalRect {
        x: x1,
        y: y1,
        width: x2 - x1,
        height: y2 - y1,
    })
}

fn touches_or_overlaps(left: &LogicalRect, right: &LogicalRect) -> bool {
    const EPSILON: f32 = 0.001;
    left.x <= right.x + right.width + EPSILON
        && right.x <= left.x + left.width + EPSILON
        && left.y <= right.y + right.height + EPSILON
        && right.y <= left.y + left.height + EPSILON
}

fn union_rect(left: &LogicalRect, right: &LogicalRect) -> LogicalRect {
    let x1 = left.x.min(right.x);
    let y1 = left.y.min(right.y);
    let x2 = (left.x + left.width).max(right.x + right.width);
    let y2 = (left.y + left.height).max(right.y + right.height);
    LogicalRect {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0.0),
        height: (y2 - y1).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{
        ResourceChange, SceneBounds, SceneNodeChange, SceneNodeId, SceneResourceId,
        SceneResourceVersion, SceneRevision, SceneSubpart,
    };
    use crate::{ExperimentalDocumentIdentity, ExperimentalNodeIdentity};
    use std::collections::BTreeSet;

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> SceneBounds {
        let rect = LogicalRect {
            x,
            y,
            width,
            height,
        };
        SceneBounds {
            layout: rect.clone(),
            visual: rect.clone(),
            clip: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            damage: rect,
        }
    }

    fn id(slot: usize) -> SceneNodeId {
        SceneNodeId {
            document: ExperimentalDocumentIdentity { serial: 1 },
            dom: Some(ExperimentalNodeIdentity {
                slot,
                generation: 0,
            }),
            subpart: SceneSubpart::Box,
            ordinal: 0,
        }
    }

    fn delta(change: SceneNodeChange) -> SceneDelta {
        SceneDelta {
            from_revision: Some(SceneRevision(1)),
            to_revision: SceneRevision(2),
            changes: vec![change],
            resource_changes: Vec::new(),
            full_scene_replacement: false,
            unchanged_nodes: 0,
        }
    }

    #[test]
    fn overlapping_and_adjacent_rectangles_coalesce() {
        let surface = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let mut region = DamageRegion::Empty;
        region.add(
            LogicalRect {
                x: 1.0,
                y: 1.0,
                width: 10.0,
                height: 10.0,
            },
            &surface,
        );
        region.add(
            LogicalRect {
                x: 11.0,
                y: 1.0,
                width: 5.0,
                height: 10.0,
            },
            &surface,
        );
        assert_eq!(
            region,
            DamageRegion::Rects(vec![LogicalRect {
                x: 1.0,
                y: 1.0,
                width: 15.0,
                height: 10.0,
            }])
        );
    }

    #[test]
    fn coalescing_rechecks_rectangles_after_a_bridge_expands() {
        let surface = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let mut region = DamageRegion::Empty;
        for rect in [
            LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 5.0,
                height: 5.0,
            },
            LogicalRect {
                x: 10.0,
                y: 0.0,
                width: 5.0,
                height: 5.0,
            },
            LogicalRect {
                x: 5.0,
                y: 0.0,
                width: 5.0,
                height: 5.0,
            },
        ] {
            region.add(rect, &surface);
        }
        assert_eq!(
            region,
            DamageRegion::Rects(vec![LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 15.0,
                height: 5.0,
            }])
        );
    }

    #[test]
    fn fragmented_damage_collapses_to_full() {
        let surface = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 10_000.0,
            height: 10_000.0,
        };
        let mut region = DamageRegion::Empty;
        for index in 0..=MAX_DAMAGE_RECTS {
            region.add(
                LogicalRect {
                    x: index as f32 * 2.0,
                    y: 0.0,
                    width: 0.5,
                    height: 0.5,
                },
                &surface,
            );
        }
        assert_eq!(region, DamageRegion::Full);
    }

    #[test]
    fn fractional_conversion_rounds_outward_and_clips() {
        let damage = DamageRegion::Rects(vec![LogicalRect {
            x: -0.2,
            y: 1.1,
            width: 10.4,
            height: 3.1,
        }]);
        let physical = logical_damage_to_physical(&damage, 100, 50, 150, 75, 180, 120);
        assert_eq!(
            physical,
            vec![PhysicalDamageRect {
                x: 0,
                y: 1,
                width: 16,
                height: 6,
            }]
        );
    }

    #[test]
    fn nonfinite_conversion_uses_safe_full_damage() {
        let damage = DamageRegion::Rects(vec![LogicalRect {
            x: f32::NAN,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }]);
        assert_eq!(
            logical_damage_to_physical(&damage, 10, 10, 15, 15, 180, 120),
            vec![PhysicalDamageRect {
                x: 0,
                y: 0,
                width: 15,
                height: 15,
            }]
        );
    }

    #[test]
    fn moved_and_removed_nodes_damage_old_and_new_pixels() {
        let moved = delta(SceneNodeChange {
            id: id(1),
            kinds: BTreeSet::from([SceneChangeKind::Geometry]),
            old_bounds: Some(bounds(2.0, 3.0, 10.0, 10.0)),
            new_bounds: Some(bounds(40.0, 50.0, 10.0, 10.0)),
        });
        let current = RetainedScene {
            document: ExperimentalDocumentIdentity { serial: 1 },
            revision: SceneRevision(2),
            viewport: crate::ViewportSpec {
                logical_width: 100,
                logical_height: 100,
                ..crate::ViewportSpec::default()
            },
            root: SceneNodeId {
                document: ExperimentalDocumentIdentity { serial: 1 },
                dom: None,
                subpart: SceneSubpart::Root,
                ordinal: 0,
            },
            nodes: Vec::new(),
            resources: Vec::new(),
            content_fingerprint: 2,
        };
        assert_eq!(
            DamageRegion::from_delta(Some(&current), &current, &moved),
            DamageRegion::Rects(vec![
                LogicalRect {
                    x: 2.0,
                    y: 3.0,
                    width: 10.0,
                    height: 10.0,
                },
                LogicalRect {
                    x: 40.0,
                    y: 50.0,
                    width: 10.0,
                    height: 10.0,
                },
            ])
        );
        let removed = delta(SceneNodeChange {
            id: id(1),
            kinds: BTreeSet::from([SceneChangeKind::Removed]),
            old_bounds: Some(bounds(-5.0, 5.0, 10.0, 10.0)),
            new_bounds: None,
        });
        assert_eq!(
            DamageRegion::from_delta(Some(&current), &current, &removed),
            DamageRegion::Rects(vec![LogicalRect {
                x: 0.0,
                y: 5.0,
                width: 5.0,
                height: 10.0,
            }])
        );
    }

    #[test]
    fn effect_changes_use_bounds_while_clip_and_order_changes_use_full_damage() {
        let current = RetainedScene {
            document: ExperimentalDocumentIdentity { serial: 1 },
            revision: SceneRevision(2),
            viewport: crate::ViewportSpec {
                logical_width: 100,
                logical_height: 100,
                ..crate::ViewportSpec::default()
            },
            root: id(0),
            nodes: Vec::new(),
            resources: Vec::new(),
            content_fingerprint: 2,
        };
        let effect = delta(SceneNodeChange {
            id: id(1),
            kinds: BTreeSet::from([SceneChangeKind::Effect]),
            old_bounds: Some(bounds(1.0, 1.0, 2.0, 2.0)),
            new_bounds: Some(bounds(4.0, 4.0, 2.0, 2.0)),
        });
        assert_eq!(
            DamageRegion::from_delta(Some(&current), &current, &effect),
            DamageRegion::Rects(vec![
                LogicalRect {
                    x: 1.0,
                    y: 1.0,
                    width: 2.0,
                    height: 2.0,
                },
                LogicalRect {
                    x: 4.0,
                    y: 4.0,
                    width: 2.0,
                    height: 2.0,
                },
            ])
        );
        for kind in [
            SceneChangeKind::Clip,
            SceneChangeKind::StackingOrOrder,
            SceneChangeKind::Reparented,
        ] {
            let change = delta(SceneNodeChange {
                id: id(1),
                kinds: BTreeSet::from([kind]),
                old_bounds: Some(bounds(1.0, 1.0, 2.0, 2.0)),
                new_bounds: Some(bounds(1.0, 1.0, 2.0, 2.0)),
            });
            assert_eq!(
                DamageRegion::from_delta(Some(&current), &current, &change),
                DamageRegion::Full
            );
        }
        let mut region = DamageRegion::Empty;
        region.add(
            LogicalRect {
                x: f32::NAN,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            &LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!(region, DamageRegion::Full);
    }

    #[test]
    fn resource_change_damages_consumers_and_version_is_not_a_backend_handle() {
        let resource = SceneResourceId {
            owner: crate::render::ResourceOwner::Document(ExperimentalDocumentIdentity {
                serial: 1,
            }),
            kind: crate::render::ResourceKind::Svg,
            key: crate::render::SceneResourceKey::Process {
                name: "test-svg".into(),
            },
        };
        let old_version = SceneResourceVersion(1);
        let new_version = SceneResourceVersion(2);
        let node = crate::render::SceneNode {
            id: id(1),
            parent: None,
            children: Vec::new(),
            kind: crate::render::SceneNodeKind::Svg,
            tree_order: 1,
            paint_order: 1,
            visible: true,
            bounds: bounds(10.0, 20.0, 30.0, 40.0),
            effects: Vec::new(),
            resource: Some((resource.clone(), new_version)),
            paint_signature: 1,
        };
        let current = RetainedScene {
            document: ExperimentalDocumentIdentity { serial: 1 },
            revision: SceneRevision(2),
            viewport: crate::ViewportSpec {
                logical_width: 100,
                logical_height: 100,
                ..crate::ViewportSpec::default()
            },
            root: id(0),
            nodes: vec![node],
            resources: Vec::new(),
            content_fingerprint: 2,
        };
        let delta = SceneDelta {
            from_revision: Some(SceneRevision(1)),
            to_revision: SceneRevision(2),
            changes: Vec::new(),
            resource_changes: vec![ResourceChange {
                id: resource.clone(),
                old_version: Some(old_version),
                new_version: Some(new_version),
            }],
            full_scene_replacement: false,
            unchanged_nodes: 1,
        };
        assert_eq!(
            DamageRegion::from_delta(Some(&current), &current, &delta),
            DamageRegion::Rects(vec![LogicalRect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
            }])
        );
    }
}
