use crate::render::{DamageRegion, FramePlan, PhysicalDamageRect, logical_damage_to_physical};
use std::collections::BTreeSet;

pub(super) const DAMAGE_TILE_SIZE: u32 = 64;
pub(super) const DAMAGE_TILE_GUARD: u32 = 2;
pub(super) const MAX_SELECTED_TILES: usize = 512;
pub(super) const MAX_PARTIAL_TILE_REPLAYS: usize = 16;
pub(super) const MAX_PARTIAL_AREA_PERCENT: u64 = 30;
pub(super) const MAX_WAYLAND_DAMAGE_RECTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveGpuFrameMode {
    NoFrame,
    Partial,
    FullGpu,
    CpuFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FullRenderReason {
    Initial,
    AuthoritativeFullDamage,
    ForcedRecovery,
    Fragmentation,
    ReplayThreshold,
    AreaThreshold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DamageTile {
    pub core: PhysicalDamageRect,
    pub scratch_origin_x: u32,
    pub scratch_origin_y: u32,
    pub source_x: u32,
    pub source_y: u32,
}

impl DamageTile {
    pub(super) fn core_pixels(self) -> u64 {
        u64::from(self.core.width) * u64::from(self.core.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DamageRenderDecision {
    NoFrame,
    Partial {
        damage: Vec<PhysicalDamageRect>,
        tiles: Vec<DamageTile>,
        tile_pixels: u64,
    },
    FullGpu {
        damage: Vec<PhysicalDamageRect>,
        reason: FullRenderReason,
    },
}

impl DamageRenderDecision {
    pub(super) fn mode(&self) -> LiveGpuFrameMode {
        match self {
            Self::NoFrame => LiveGpuFrameMode::NoFrame,
            Self::Partial { .. } => LiveGpuFrameMode::Partial,
            Self::FullGpu { .. } => LiveGpuFrameMode::FullGpu,
        }
    }

    pub(super) fn physical_damage(&self) -> &[PhysicalDamageRect] {
        match self {
            Self::NoFrame => &[],
            Self::Partial { damage, .. } | Self::FullGpu { damage, .. } => damage,
        }
    }
}

pub(super) fn select_damage_work(
    plan: &FramePlan,
    backing_initialized: bool,
    force_full: bool,
) -> DamageRenderDecision {
    if matches!(plan.damage, DamageRegion::Empty) {
        return DamageRenderDecision::NoFrame;
    }
    let mut damage = logical_damage_to_physical(
        &plan.damage,
        plan.logical_width,
        plan.logical_height,
        plan.physical_width,
        plan.physical_height,
        plan.scale_numerator,
        plan.scale_denominator,
    );
    normalize_physical_damage(&mut damage, plan.physical_width, plan.physical_height);
    if damage.is_empty() {
        return DamageRenderDecision::FullGpu {
            damage: full_damage(plan.physical_width, plan.physical_height),
            reason: FullRenderReason::ForcedRecovery,
        };
    }
    if !backing_initialized {
        return DamageRenderDecision::FullGpu {
            damage,
            reason: FullRenderReason::Initial,
        };
    }
    if matches!(plan.damage, DamageRegion::Full) {
        return DamageRenderDecision::FullGpu {
            damage,
            reason: FullRenderReason::AuthoritativeFullDamage,
        };
    }
    if force_full {
        return DamageRenderDecision::FullGpu {
            damage,
            reason: FullRenderReason::ForcedRecovery,
        };
    }

    let mut coordinates = BTreeSet::new();
    for rect in &damage {
        let last_x = rect.x.saturating_add(rect.width).saturating_sub(1);
        let last_y = rect.y.saturating_add(rect.height).saturating_sub(1);
        let start_x = rect.x / DAMAGE_TILE_SIZE;
        let start_y = rect.y / DAMAGE_TILE_SIZE;
        let end_x = last_x / DAMAGE_TILE_SIZE;
        let end_y = last_y / DAMAGE_TILE_SIZE;
        for tile_y in start_y..=end_y {
            for tile_x in start_x..=end_x {
                coordinates.insert((tile_y, tile_x));
                if coordinates.len() > MAX_SELECTED_TILES {
                    return DamageRenderDecision::FullGpu {
                        damage,
                        reason: FullRenderReason::Fragmentation,
                    };
                }
            }
        }
    }

    let mut tiles = Vec::with_capacity(coordinates.len());
    let mut tile_pixels = 0u64;
    for (tile_y, tile_x) in coordinates {
        let x = tile_x.saturating_mul(DAMAGE_TILE_SIZE);
        let y = tile_y.saturating_mul(DAMAGE_TILE_SIZE);
        let width = DAMAGE_TILE_SIZE.min(plan.physical_width.saturating_sub(x));
        let height = DAMAGE_TILE_SIZE.min(plan.physical_height.saturating_sub(y));
        if width == 0 || height == 0 {
            continue;
        }
        let scratch_origin_x = x.saturating_sub(DAMAGE_TILE_GUARD);
        let scratch_origin_y = y.saturating_sub(DAMAGE_TILE_GUARD);
        let tile = DamageTile {
            core: PhysicalDamageRect {
                x,
                y,
                width,
                height,
            },
            scratch_origin_x,
            scratch_origin_y,
            source_x: x - scratch_origin_x,
            source_y: y - scratch_origin_y,
        };
        tile_pixels = tile_pixels.saturating_add(tile.core_pixels());
        tiles.push(tile);
    }

    if tiles.len() > MAX_PARTIAL_TILE_REPLAYS {
        return DamageRenderDecision::FullGpu {
            damage,
            reason: FullRenderReason::ReplayThreshold,
        };
    }
    let target_pixels = u64::from(plan.physical_width) * u64::from(plan.physical_height);
    if tile_pixels.saturating_mul(100) > target_pixels.saturating_mul(MAX_PARTIAL_AREA_PERCENT) {
        return DamageRenderDecision::FullGpu {
            damage,
            reason: FullRenderReason::AreaThreshold,
        };
    }

    DamageRenderDecision::Partial {
        damage,
        tiles,
        tile_pixels,
    }
}

pub fn bounded_wayland_damage(
    damage: &[PhysicalDamageRect],
    width: u32,
    height: u32,
) -> Vec<PhysicalDamageRect> {
    let mut normalized = damage.to_vec();
    normalize_physical_damage(&mut normalized, width, height);
    if normalized.len() > MAX_WAYLAND_DAMAGE_RECTS {
        return full_damage(width, height);
    }
    normalized
}

fn normalize_physical_damage(rects: &mut Vec<PhysicalDamageRect>, width: u32, height: u32) {
    for rect in rects.iter_mut() {
        let right = rect.x.saturating_add(rect.width).min(width);
        let bottom = rect.y.saturating_add(rect.height).min(height);
        rect.x = rect.x.min(width);
        rect.y = rect.y.min(height);
        rect.width = right.saturating_sub(rect.x);
        rect.height = bottom.saturating_sub(rect.y);
    }
    rects.retain(|rect| rect.width > 0 && rect.height > 0);
    rects.sort_by_key(|rect| (rect.y, rect.x, rect.height, rect.width));
    rects.dedup();
}

fn full_damage(width: u32, height: u32) -> Vec<PhysicalDamageRect> {
    (width > 0 && height > 0)
        .then_some(PhysicalDamageRect {
            x: 0,
            y: 0,
            width,
            height,
        })
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{
        FrameReasonSet, PixelFormat, RenderSurfaceId, RetainedScene, SceneDelta, SceneNodeId,
        SceneRevision, SceneSubpart,
    };
    use crate::{ExperimentalDocumentIdentity, ViewportSpec};
    use std::sync::Arc;

    fn plan(damage: DamageRegion, width: u32, height: u32) -> FramePlan {
        let document = ExperimentalDocumentIdentity { serial: 1 };
        let scene = RetainedScene {
            document,
            revision: SceneRevision(2),
            viewport: ViewportSpec {
                logical_width: width,
                logical_height: height,
                ..ViewportSpec::default()
            },
            root: SceneNodeId {
                document,
                dom: None,
                subpart: SceneSubpart::Root,
                ordinal: 0,
            },
            nodes: Vec::new(),
            resources: Vec::new(),
            content_fingerprint: 1,
        };
        FramePlan {
            surface: RenderSurfaceId {
                instance: 1,
                generation: 1,
            },
            document,
            scene_revision: SceneRevision(2),
            prior_scene_revision: Some(SceneRevision(1)),
            logical_width: width,
            logical_height: height,
            physical_width: width,
            physical_height: height,
            scale_numerator: 120,
            scale_denominator: 120,
            pixel_format: PixelFormat::PremultipliedRgba8,
            clear: true,
            scene: Arc::new(scene),
            delta: SceneDelta {
                from_revision: Some(SceneRevision(1)),
                to_revision: SceneRevision(2),
                changes: Vec::new(),
                resource_changes: Vec::new(),
                full_scene_replacement: false,
                unchanged_nodes: 0,
            },
            damage,
            reasons: FrameReasonSet::new(),
            full_repaint: false,
            presentation_eligible: true,
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> crate::model::LogicalRect {
        crate::model::LogicalRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn empty_damage_selects_no_frame() {
        assert_eq!(
            select_damage_work(&plan(DamageRegion::Empty, 800, 600), true, false),
            DamageRenderDecision::NoFrame
        );
    }

    #[test]
    fn initial_and_full_damage_select_full_gpu() {
        assert!(matches!(
            select_damage_work(
                &plan(
                    DamageRegion::Rects(vec![rect(1.0, 1.0, 1.0, 1.0)]),
                    800,
                    600
                ),
                false,
                false
            ),
            DamageRenderDecision::FullGpu {
                reason: FullRenderReason::Initial,
                ..
            }
        ));
        assert!(matches!(
            select_damage_work(&plan(DamageRegion::Full, 800, 600), true, false),
            DamageRenderDecision::FullGpu {
                reason: FullRenderReason::AuthoritativeFullDamage,
                ..
            }
        ));
    }

    #[test]
    fn one_pixel_selects_one_guarded_tile() {
        let coordinate = DAMAGE_TILE_SIZE * 2 + 1;
        let decision = select_damage_work(
            &plan(
                DamageRegion::Rects(vec![rect(
                    coordinate as f32,
                    (coordinate + 1) as f32,
                    1.0,
                    1.0,
                )]),
                900,
                700,
            ),
            true,
            false,
        );
        let DamageRenderDecision::Partial {
            damage,
            tiles,
            tile_pixels,
        } = decision
        else {
            panic!("small damage should be partial");
        };
        assert_eq!(
            damage,
            vec![PhysicalDamageRect {
                x: coordinate,
                y: coordinate + 1,
                width: 1,
                height: 1,
            }]
        );
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].core.x, DAMAGE_TILE_SIZE * 2);
        assert_eq!(tiles[0].core.y, DAMAGE_TILE_SIZE * 2);
        assert_eq!(
            tiles[0].scratch_origin_x,
            DAMAGE_TILE_SIZE * 2 - DAMAGE_TILE_GUARD
        );
        assert_eq!(
            tiles[0].scratch_origin_y,
            DAMAGE_TILE_SIZE * 2 - DAMAGE_TILE_GUARD
        );
        assert_eq!(tiles[0].source_x, DAMAGE_TILE_GUARD);
        assert_eq!(
            tile_pixels,
            u64::from(DAMAGE_TILE_SIZE) * u64::from(DAMAGE_TILE_SIZE)
        );
    }

    #[test]
    fn crossing_a_boundary_selects_deterministic_distinct_tiles() {
        let boundary = DAMAGE_TILE_SIZE as f32;
        let decision = select_damage_work(
            &plan(
                DamageRegion::Rects(vec![
                    rect(boundary - 1.0, 20.0, 2.0, 2.0),
                    rect(boundary + 44.0, 20.0, 1.0, 1.0),
                ]),
                1024,
                768,
            ),
            true,
            false,
        );
        let DamageRenderDecision::Partial { tiles, .. } = decision else {
            panic!("bounded boundary damage should be partial");
        };
        assert_eq!(
            tiles.iter().map(|tile| tile.core.x).collect::<Vec<_>>(),
            vec![0, DAMAGE_TILE_SIZE]
        );
    }

    #[test]
    fn negative_fractional_damage_is_clipped_outward() {
        let mut fractional = plan(
            DamageRegion::Rects(vec![rect(-0.25, -0.25, 2.0, 2.0)]),
            1_000,
            1_000,
        );
        fractional.physical_width = 1_250;
        fractional.physical_height = 1_250;
        fractional.scale_numerator = 150;
        let DamageRenderDecision::Partial { damage, tiles, .. } =
            select_damage_work(&fractional, true, false)
        else {
            panic!("fractional damage should be partial");
        };
        assert_eq!(
            damage,
            vec![PhysicalDamageRect {
                x: 0,
                y: 0,
                width: 3,
                height: 3,
            }]
        );
        assert_eq!(tiles.len(), 1);
    }

    #[test]
    fn replay_and_area_thresholds_collapse_to_full_gpu() {
        let rectangles = (0..17)
            .map(|index| {
                rect(
                    (index % 5) as f32 * DAMAGE_TILE_SIZE as f32,
                    (index / 5) as f32 * DAMAGE_TILE_SIZE as f32,
                    1.0,
                    1.0,
                )
            })
            .collect();
        assert!(matches!(
            select_damage_work(
                &plan(DamageRegion::Rects(rectangles), 1536, 1024),
                true,
                false
            ),
            DamageRenderDecision::FullGpu {
                reason: FullRenderReason::ReplayThreshold,
                ..
            }
        ));

        assert!(matches!(
            select_damage_work(
                &plan(
                    DamageRegion::Rects(vec![rect(0.0, 0.0, 129.0, 129.0)]),
                    300,
                    300
                ),
                true,
                false
            ),
            DamageRenderDecision::FullGpu {
                reason: FullRenderReason::AreaThreshold,
                ..
            }
        ));
    }

    #[test]
    fn excessive_selected_tiles_collapse_before_allocation() {
        let width = DAMAGE_TILE_SIZE.saturating_mul(513);
        assert!(matches!(
            select_damage_work(
                &plan(
                    DamageRegion::Rects(vec![rect(0.0, 0.0, width as f32, 1.0)]),
                    width,
                    512
                ),
                true,
                false
            ),
            DamageRenderDecision::FullGpu {
                reason: FullRenderReason::Fragmentation,
                ..
            }
        ));
    }

    #[test]
    fn right_and_bottom_edge_tiles_are_clipped() {
        let DamageRenderDecision::Partial { tiles, .. } = select_damage_work(
            &plan(
                DamageRegion::Rects(vec![rect(899.0, 699.0, 1.0, 1.0)]),
                900,
                700,
            ),
            true,
            false,
        ) else {
            panic!("edge damage should be partial");
        };
        assert_eq!(tiles[0].core.width, 900 % DAMAGE_TILE_SIZE);
        assert_eq!(tiles[0].core.height, 700 % DAMAGE_TILE_SIZE);
    }

    #[test]
    fn wayland_damage_is_clipped_deduplicated_and_bounded() {
        let damage = bounded_wayland_damage(
            &[
                PhysicalDamageRect {
                    x: 90,
                    y: 90,
                    width: 20,
                    height: 20,
                },
                PhysicalDamageRect {
                    x: 90,
                    y: 90,
                    width: 20,
                    height: 20,
                },
            ],
            100,
            100,
        );
        assert_eq!(
            damage,
            vec![PhysicalDamageRect {
                x: 90,
                y: 90,
                width: 10,
                height: 10,
            }]
        );
    }

    #[test]
    fn modeled_no_op_and_partial_damage_stress_remains_bounded() {
        let no_op = plan(DamageRegion::Empty, 1_920, 1_080);
        let partial = plan(
            DamageRegion::Rects(vec![rect(719.25, 19.25, 32.5, 24.5)]),
            1_920,
            1_080,
        );
        for _ in 0..1_000 {
            assert_eq!(
                select_damage_work(&no_op, true, false),
                DamageRenderDecision::NoFrame
            );
            let DamageRenderDecision::Partial { damage, tiles, .. } =
                select_damage_work(&partial, true, false)
            else {
                panic!("representative small damage should remain partial");
            };
            assert!(damage.len() <= MAX_WAYLAND_DAMAGE_RECTS);
            assert!(tiles.len() <= MAX_PARTIAL_TILE_REPLAYS);
        }
    }
}
