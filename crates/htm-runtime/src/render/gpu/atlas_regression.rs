//! Pixel regressions for the shared Vello renderer's persistent raster atlas.

use super::{OffscreenTarget, PixelFormat, RenderTarget, Renderer, VelloOffscreenRenderer, wgpu};
use vello::kurbo::{Affine, Rect};
use vello::peniko::{Blob, Color, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat};

const WIDTH: u32 = 32;
const HEIGHT: u32 = 32;

fn target(renderer: &mut VelloOffscreenRenderer) -> OffscreenTarget {
    renderer
        .allocate_target(RenderTarget {
            width: WIDTH,
            height: HEIGHT,
            pixel_format: PixelFormat::PremultipliedRgba8,
        })
        .unwrap()
}

fn render_pixels(
    renderer: &mut VelloOffscreenRenderer,
    scene: &vello::Scene,
    target: &OffscreenTarget,
) -> Vec<u8> {
    renderer
        .renderer
        .render_to_texture(
            &renderer.device,
            &renderer.queue,
            scene,
            &target.view,
            &vello::RenderParams {
                base_color: Color::TRANSPARENT,
                width: WIDTH,
                height: HEIGHT,
                antialiasing_method: vello::AaConfig::Area,
            },
        )
        .unwrap();
    let mut encoder = renderer
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        target.texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &target.readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(target.padded_bytes_per_row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    renderer.queue.submit([encoder.finish()]);
    VelloOffscreenRenderer::read_target(&renderer.device, target).unwrap()
}

fn image(width: u32, height: u32) -> ImageData {
    ImageData {
        data: Blob::from([255, 0, 0, 255].repeat((width * height) as usize)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    }
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> &[u8] {
    let offset = ((y * WIDTH + x) * 4) as usize;
    &pixels[offset..offset + 4]
}

#[test]
#[ignore = "requires a physical Vulkan or GLES adapter; run serially"]
fn hardware_raster_atlas_survives_resource_free_passes() {
    let mut renderer = VelloOffscreenRenderer::new(false).unwrap();
    assert_ne!(
        renderer.info().device_type,
        "Cpu",
        "physical adapter required"
    );
    eprintln!("atlas regression adapter={:?}", renderer.info());
    let target_a = target(&mut renderer);
    let target_b = target(&mut renderer);
    let mut raster = vello::Scene::new();
    raster.draw_image(&ImageBrush::new(image(8, 8)), Affine::translate((8.0, 8.0)));
    let mut solid = vello::Scene::new();
    solid.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::from_rgb8(0, 0, 255),
        None,
        &Rect::new(4.0, 4.0, 28.0, 28.0),
    );
    let empty = vello::Scene::new();
    let mut growth = vello::Scene::new();
    // Wider than the initial 1024-pixel atlas, forcing the existing growth path.
    growth.draw_image(&ImageBrush::new(image(1025, 2)), Affine::IDENTITY);

    for reset in 0..2 {
        if reset != 0 {
            renderer.reset().unwrap();
        }
        let expected = render_pixels(&mut renderer, &raster, &target_a);
        assert_eq!(pixel(&expected, 12, 12), [255, 0, 0, 255]);
        for round in 0..4 {
            for (name, intermediate) in [("solid", &solid), ("empty", &empty)] {
                let intermediate_pixels = render_pixels(&mut renderer, intermediate, &target_b);
                let expected_middle = if name == "solid" {
                    [0, 0, 255, 255]
                } else {
                    [0, 0, 0, 0]
                };
                assert_eq!(pixel(&intermediate_pixels, 12, 12), expected_middle);
                let replayed = render_pixels(&mut renderer, &raster, &target_a);
                assert_eq!(
                    pixel(&replayed, 12, 12),
                    [255, 0, 0, 255],
                    "raster disappeared after {name} pass: reset={reset} round={round}"
                );
                assert!(replayed == expected, "raster replay changed pixels");
            }
            let grown = render_pixels(&mut renderer, &growth, &target_b);
            assert_eq!(pixel(&grown, 12, 0), [255, 0, 0, 255]);
        }
    }
}

#[test]
#[ignore = "requires a physical Vulkan or GLES adapter; run serially"]
fn hardware_raster_override_dirty_state_survives_resource_free_passes() {
    let mut renderer = VelloOffscreenRenderer::new(false).unwrap();
    assert_ne!(
        renderer.info().device_type,
        "Cpu",
        "physical adapter required"
    );
    let target_a = target(&mut renderer);
    let target_b = target(&mut renderer);
    let image = image(8, 8);
    let mut raster = vello::Scene::new();
    raster.draw_image(
        &ImageBrush::new(image.clone()),
        Affine::translate((8.0, 8.0)),
    );
    let empty = vello::Scene::new();
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("atlas regression image override"),
        size: wgpu::Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for color in [[0, 255, 0, 255], [0, 0, 255, 255]] {
        renderer.queue.write_texture(
            texture.as_image_copy(),
            &color.repeat(64),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(32),
                rows_per_image: None,
            },
            texture.size(),
        );
        renderer.renderer.override_image(
            &image,
            Some(wgpu::TexelCopyTextureInfoBase {
                texture: texture.clone(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            }),
        );
        for _ in 0..3 {
            render_pixels(&mut renderer, &empty, &target_b);
        }
        let rendered = render_pixels(&mut renderer, &raster, &target_a);
        assert_eq!(pixel(&rendered, 12, 12), color);
    }
    renderer.renderer.override_image(&image, None);
    render_pixels(&mut renderer, &empty, &target_b);
    let restored = render_pixels(&mut renderer, &raster, &target_a);
    assert_eq!(pixel(&restored, 12, 12), [255, 0, 0, 255]);
}

#[test]
#[ignore = "requires a physical Vulkan or GLES adapter; run serially"]
fn hardware_raster_atlas_survives_filtered_svg_source_graphic() {
    use crate::ExperimentalDocumentIdentity;
    use crate::model::ViewportSpec;
    use crate::render::{
        DamageRegion, FramePlan, FrameReason, FrameReasonSet, RenderSurfaceId, SceneDelta,
        SceneRevision,
    };
    use anyrender::recording::RenderCommand;
    use blitz_dom::{DocumentConfig, StyleThreading};
    use blitz_html::{HtmlDocument, HtmlProvider};
    use blitz_traits::shell::{ColorScheme, Viewport};
    use std::sync::Arc;

    // No text, gradients, or raster resources: this SVG's filtered SourceGraphic
    // must take Vello's solid-only resolver path inside HTMShell's effect pass.
    let html = "<!doctype html><html><head><style>html,body{margin:0;background:transparent}#filtered{position:absolute;left:0;top:0;width:32px;height:32px;filter:brightness(.5)}svg{display:block;width:32px;height:32px}</style></head><body><div id=\"filtered\"><svg viewBox=\"0 0 32 32\"><circle cx=\"16\" cy=\"16\" r=\"8\" fill=\"#00ff00\"/></svg></div></body></html>";
    let viewport = ViewportSpec {
        logical_width: WIDTH,
        logical_height: HEIGHT,
        ..ViewportSpec::default()
    };
    let mut document = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(WIDTH, HEIGHT, 1.0, ColorScheme::Dark)),
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            style_threading: StyleThreading::Sequential,
            ..DocumentConfig::default()
        },
    );
    document.set_incremental_layout(true);
    document.resolve(0.0);
    let identities = crate::identity::IdentityRegistry::from_document(&document);
    let identity = ExperimentalDocumentIdentity { serial: 99_001 };
    let revision = SceneRevision(1);
    let scene =
        crate::render::build_retained_scene(&document, &identities, identity, revision, viewport)
            .unwrap();
    let effects = super::collect_effect_plans(&scene);
    assert_eq!(
        effects.len(),
        1,
        "the SVG has exactly one foreground effect"
    );
    let prepared = crate::render::cpu::prepare_scene(&mut document, revision, viewport).unwrap();
    for command in &prepared.recording.commands {
        match command {
            RenderCommand::GlyphRun(_) => panic!("the SVG fixture must not contain text"),
            RenderCommand::Fill(command) => {
                assert!(matches!(command.brush, anyrender::Paint::Solid(_)))
            }
            RenderCommand::Stroke(command) => {
                assert!(matches!(command.brush, anyrender::Paint::Solid(_)))
            }
            _ => {}
        }
    }
    let plan = FramePlan {
        surface: RenderSurfaceId {
            instance: 99_001,
            generation: 1,
        },
        document: identity,
        scene_revision: revision,
        prior_scene_revision: None,
        logical_width: WIDTH,
        logical_height: HEIGHT,
        physical_width: WIDTH,
        physical_height: HEIGHT,
        scale_numerator: 120,
        scale_denominator: 120,
        pixel_format: PixelFormat::PremultipliedRgba8,
        clear: true,
        scene: Arc::new(scene),
        delta: SceneDelta {
            from_revision: None,
            to_revision: revision,
            changes: vec![],
            resource_changes: vec![],
            full_scene_replacement: true,
            unchanged_nodes: 0,
        },
        damage: DamageRegion::Full,
        reasons: FrameReasonSet::from([FrameReason::InitialPresentation]),
        full_repaint: true,
        presentation_eligible: true,
    };

    let mut renderer = VelloOffscreenRenderer::new(false).unwrap();
    assert_ne!(
        renderer.info().device_type,
        "Cpu",
        "physical adapter required"
    );
    let target_a = target(&mut renderer);
    let target_b = RenderTarget {
        width: WIDTH,
        height: HEIGHT,
        pixel_format: PixelFormat::PremultipliedRgba8,
    };
    renderer.create_target(plan.surface, target_b).unwrap();
    let mut raster = vello::Scene::new();
    raster.draw_image(&ImageBrush::new(image(8, 8)), Affine::translate((8.0, 8.0)));
    let expected = render_pixels(&mut renderer, &raster, &target_a);
    assert_eq!(pixel(&expected, 12, 12), [255, 0, 0, 255]);
    let mut previous_svg = None;
    for round in 0..4 {
        renderer
            .prepare(
                &plan,
                super::GpuPreparedScene::from_cpu(
                    identity,
                    prepared.clone(),
                    plan.scene.live_resources(),
                    effects.clone(),
                ),
            )
            .unwrap();
        let result = renderer.render(&plan, target_b).unwrap();
        let svg = renderer.readback(result).unwrap();
        let center = pixel(&svg, 16, 16);
        assert_eq!([center[0], center[2], center[3]], [0, 0, 255]);
        assert!(
            center[1] > 0 && center[1] < 255,
            "the SVG must be visible and filtered"
        );
        if let Some(previous) = &previous_svg {
            assert_eq!(
                &svg, previous,
                "filtered SVG pixels changed at round {round}"
            );
        }
        previous_svg = Some(svg);

        let replayed = render_pixels(&mut renderer, &raster, &target_a);
        assert_eq!(
            replayed, expected,
            "raster changed after HTMShell's filtered SVG pass at round {round}"
        );
    }
    assert_eq!(renderer.statistics().gpu_color_filter_passes, 4);
    assert_eq!(renderer.statistics().fallback_requests, 0);
}
