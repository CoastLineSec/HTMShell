use anyrender::{Filter, NormalizedCoord, Paint, PaintRef, PaintScene, RenderContext};
use kurbo::{Affine, Rect, Shape, Stroke};
use std::sync::Arc;
use vello::peniko::{BlendMode, BrushRef, Color, Fill, FontData, StyleRef};

/// Replays the renderer-neutral AnyRender recording into Vello without
/// exposing Vello objects to document or scene state.
pub(super) struct VelloScenePainter<'a> {
    scene: &'a mut vello::Scene,
    unsupported_brush_or_filter: bool,
}

impl<'a> VelloScenePainter<'a> {
    pub(super) fn new(scene: &'a mut vello::Scene) -> Self {
        Self {
            scene,
            unsupported_brush_or_filter: false,
        }
    }

    pub(super) fn unsupported(&self) -> bool {
        self.unsupported_brush_or_filter
    }

    fn brush<'b>(&mut self, paint: PaintRef<'b>) -> BrushRef<'b> {
        match paint {
            Paint::Solid(color) => BrushRef::Solid(color),
            Paint::Gradient(gradient) => BrushRef::Gradient(gradient),
            Paint::Image(image) => BrushRef::Image(image),
            Paint::Resource(_) | Paint::Custom(_) => {
                self.unsupported_brush_or_filter = true;
                BrushRef::Solid(Color::TRANSPARENT)
            }
        }
    }
}

impl RenderContext for VelloScenePainter<'_> {}

impl PaintScene for VelloScenePainter<'_> {
    fn reset(&mut self) {
        self.scene.reset();
    }

    fn push_layer(
        &mut self,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) {
        if filter.is_some() || backdrop_filter.is_some() {
            self.unsupported_brush_or_filter = true;
        }
        self.scene
            .push_layer(Fill::NonZero, blend, alpha, transform, clip);
    }

    fn push_clip_layer(&mut self, transform: Affine, clip: &impl Shape) {
        self.scene.push_clip_layer(Fill::NonZero, transform, clip);
    }

    fn pop_layer(&mut self) {
        self.scene.pop_layer();
    }

    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        paint: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let brush = self.brush(paint.into());
        self.scene
            .stroke(style, transform, brush, brush_transform, shape);
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        paint: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let brush = self.brush(paint.into());
        self.scene
            .fill(style, transform, brush, brush_transform, shape);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        embolden: kurbo::Vec2,
        style: impl Into<StyleRef<'a>>,
        paint: impl Into<PaintRef<'a>>,
        brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = anyrender::Glyph> + Clone,
    ) {
        let brush = self.brush(paint.into());
        self.scene
            .draw_glyphs(font)
            .font_size(font_size)
            .hint(hint)
            .normalized_coords(normalized_coords)
            .font_embolden(vello::FontEmbolden::new(kurbo::Diagonal2::new(
                embolden.x, embolden.y,
            )))
            .brush(brush)
            .brush_alpha(brush_alpha)
            .transform(transform)
            .glyph_transform(glyph_transform)
            .draw(
                style,
                glyphs.map(|glyph| vello::Glyph {
                    id: glyph.id,
                    x: glyph.x,
                    y: glyph.y,
                }),
            );
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: Color,
        radius: f64,
        std_dev: f64,
    ) {
        self.scene
            .draw_blurred_rounded_rect(transform, rect, brush, radius, std_dev);
    }
}
