// Copyright 2026 the HTMShell Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::Resolver;
use crate::{Encoding, Transform};
use peniko::kurbo::Rect;
use peniko::{Blob, Color, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat};
use std::sync::Arc;

#[derive(Debug)]
struct ResolvedImages {
    size: (u32, u32),
    uploads: Vec<(u64, u32, u32)>,
    evicted: usize,
}

fn image(width: u32, height: u32) -> ImageData {
    ImageData {
        data: Blob::new(Arc::new(vec![255_u8; (width * height * 4) as usize])),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    }
}

fn rectangle() -> Encoding {
    let mut encoding = Encoding::new();
    encoding.encode_transform(Transform::IDENTITY);
    encoding.encode_fill_style(Fill::NonZero);
    assert!(encoding.encode_shape(&Rect::new(0.0, 0.0, 8.0, 8.0), true));
    encoding
}

fn raster(image: &ImageData) -> Encoding {
    let mut encoding = rectangle();
    encoding.encode_image(&ImageBrush::new(image.clone()), 1.0);
    encoding
}

fn solid() -> Encoding {
    let mut encoding = rectangle();
    encoding.encode_color(Color::WHITE);
    assert!(!encoding.is_empty());
    assert!(encoding.resources.patches.is_empty());
    encoding
}

fn resolve(resolver: &mut Resolver, encoding: &Encoding) -> ResolvedImages {
    let mut packed = Vec::new();
    let (_, _, images) = resolver.resolve(encoding, &mut packed);
    ResolvedImages {
        size: (images.width, images.height),
        uploads: images
            .images
            .iter()
            .map(|(image, x, y)| (image.data.id(), *x, *y))
            .collect(),
        evicted: images.evicted,
    }
}

fn assert_geometry_pass_preserves_atlas(geometry: &Encoding) {
    let image = image(8, 8);
    let raster = raster(&image);
    let mut resolver = Resolver::new();
    let first = resolve(&mut resolver, &raster);
    assert!(first.size.0 >= image.width);
    assert!(first.size.1 >= image.height);
    assert_eq!(first.uploads.len(), 1);
    assert_eq!(first.uploads[0].0, image.data.id());

    // Exercise the same resolver across repeated separate render passes. Atlas
    // metadata must describe its persistent contents, not this pass's brushes.
    for _ in 0..8 {
        let geometry = resolve(&mut resolver, geometry);
        assert_eq!(geometry.size, first.size);
        assert!(
            geometry.uploads.is_empty(),
            "solid passes must not replay old uploads"
        );
        assert_eq!(geometry.evicted, 0);

        let replay = resolve(&mut resolver, &raster);
        assert_eq!(replay.size, first.size);
        assert!(
            replay.uploads.is_empty(),
            "a clean resident image stays resident"
        );
        assert_eq!(replay.evicted, 0);
    }
}

#[test]
fn solid_only_pass_preserves_atlas_without_stale_uploads() {
    assert_geometry_pass_preserves_atlas(&solid());
}

#[test]
fn empty_pass_preserves_atlas_without_stale_uploads() {
    assert_geometry_pass_preserves_atlas(&Encoding::new());
}

#[test]
fn dirty_image_survives_unused_geometry_passes_until_uploaded() {
    let image = image(8, 8);
    let raster = raster(&image);
    let solid = solid();
    let empty = Encoding::new();
    let mut resolver = Resolver::new();
    let first = resolve(&mut resolver, &raster);
    assert_eq!(first.uploads.len(), 1);
    resolver.mark_image_dirty(&image);

    // A changed texture override may be marked dirty before a SourceGraphic
    // pass that does not use it. Such passes cannot consume its dirty flag.
    for geometry in [&empty, &solid, &empty, &solid] {
        let unused = resolve(&mut resolver, geometry);
        assert_eq!(unused.size, first.size);
        assert!(unused.uploads.is_empty());
    }

    let refreshed = resolve(&mut resolver, &raster);
    assert_eq!(refreshed.size, first.size);
    assert_eq!(refreshed.uploads, first.uploads);

    assert!(resolve(&mut resolver, &solid).uploads.is_empty());
    assert!(resolve(&mut resolver, &raster).uploads.is_empty());
}

#[test]
fn grown_atlas_and_unused_dirty_resident_survive_solid_pass() {
    let original = image(8, 8);
    let original_scene = raster(&original);
    let mut resolver = Resolver::new();
    let first = resolve(&mut resolver, &original_scene);
    assert_eq!(first.uploads.len(), 1);

    // A one-row image larger than the current atlas forces growth without a
    // large pixel allocation. The earlier image is repacked and marked dirty,
    // but is not referenced by the pass which grows the atlas.
    let wide = image(first.size.0 + 1, 1);
    let grown = resolve(&mut resolver, &raster(&wide));
    assert!(grown.size.0 > first.size.0);
    assert!(grown.size.1 > first.size.1);
    assert_eq!(grown.uploads.len(), 1);
    assert_eq!(grown.uploads[0].0, wide.data.id());

    let solid = resolve(&mut resolver, &solid());
    assert_eq!(solid.size, grown.size);
    assert!(solid.uploads.is_empty());

    let restored = resolve(&mut resolver, &original_scene);
    assert_eq!(restored.size, grown.size);
    assert_eq!(restored.uploads.len(), 1);
    assert_eq!(restored.uploads[0].0, original.data.id());
    assert!(resolve(&mut resolver, &original_scene).uploads.is_empty());
}

#[test]
fn fresh_resolver_uploads_image_after_geometry_only_passes() {
    let image = image(8, 8);
    let raster = raster(&image);
    let mut previous = Resolver::new();
    assert_eq!(resolve(&mut previous, &raster).uploads.len(), 1);
    assert!(resolve(&mut previous, &raster).uploads.is_empty());

    // Replacing a renderer replaces its resolver too. The same image identity
    // must be uploaded to the new renderer's otherwise empty atlas.
    let mut fresh = Resolver::new();
    assert!(resolve(&mut fresh, &Encoding::new()).uploads.is_empty());
    assert!(resolve(&mut fresh, &solid()).uploads.is_empty());
    let uploaded = resolve(&mut fresh, &raster);
    assert_eq!(uploaded.uploads.len(), 1);
    assert_eq!(uploaded.uploads[0].0, image.data.id());
    assert!(resolve(&mut fresh, &raster).uploads.is_empty());
}
