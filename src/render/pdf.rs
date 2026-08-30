//! Krilla replay for backend-neutral page scenes.

use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image as PdfImage;
use krilla::num::NormalizedF32;
use krilla::page::Page;
use krilla::paint::{Fill, FillRule};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla::Data;

use super::{
    Error, PageScene, PageSceneOp, Result, SceneFontId, SceneFontResource, SceneGlyph,
    SceneGlyphRun, SceneImageEncoding, SceneImageId, SceneImageResource, ScenePoint, SceneRect,
};

#[cfg(test)]
use super::RunDraw;

impl SceneFontResource {
    pub(super) fn to_pdf_font(&self) -> Result<Font> {
        if !self.is_valid() {
            return Err(Error::Render(
                "page scene contains an invalid font resource".into(),
            ));
        }
        Font::new(self.bytes.clone().into(), self.index)
            .ok_or_else(|| Error::Render("page scene contains an invalid font resource".into()))
    }
}

impl SceneGlyph {
    fn to_krilla(&self) -> KrillaGlyph {
        KrillaGlyph::new(
            GlyphId::new(self.glyph_id),
            self.x_advance,
            self.x_offset,
            self.y_offset,
            self.y_advance,
            self.text_range.clone(),
            None,
        )
    }
}

impl SceneGlyphRun {
    fn width(&self) -> f32 {
        self.glyphs.iter().map(|glyph| glyph.x_advance).sum::<f32>() * self.size
    }
}

impl SceneImageResource {
    pub(super) fn to_pdf_image(&self) -> Result<PdfImage> {
        let invalid = || Error::Render("page scene contains an invalid image resource".into());
        if !self.is_valid() {
            return Err(invalid());
        }
        let data: Data = self.bytes.clone().into();
        let image = match self.encoding {
            SceneImageEncoding::Png => PdfImage::from_png(data, false).map_err(|_| invalid())?,
            SceneImageEncoding::Jpeg => PdfImage::from_jpeg(data, false).map_err(|_| invalid())?,
            SceneImageEncoding::Gif => PdfImage::from_gif(data, false).map_err(|_| invalid())?,
            SceneImageEncoding::Webp => PdfImage::from_webp(data, false).map_err(|_| invalid())?,
            SceneImageEncoding::Rgba8 => {
                let expected = (self.width_px as usize)
                    .checked_mul(self.height_px as usize)
                    .and_then(|pixels| pixels.checked_mul(4))
                    .ok_or_else(invalid)?;
                if self.bytes.len() != expected {
                    return Err(invalid());
                }
                PdfImage::from_rgba8(self.bytes.as_ref().clone(), self.width_px, self.height_px)
            }
        };
        if image.size() != (self.width_px, self.height_px) {
            return Err(invalid());
        }
        Ok(image)
    }
}

fn fill_rect_color(surface: &mut Surface<'_>, x: f32, y: f32, w: f32, h: f32, color: rgb::Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let mut path = PathBuilder::new();
    path.move_to(x, y);
    path.line_to(x + w, y);
    path.line_to(x + w, y + h);
    path.line_to(x, y + h);
    path.close();
    if let Some(path) = path.finish() {
        surface.set_fill(Some(Fill {
            paint: color.into(),
            rule: FillRule::NonZero,
            opacity: NormalizedF32::ONE,
        }));
        surface.draw_path(&path);
    }
}

fn push_pdf_rect_clip(surface: &mut Surface<'_>, rect: SceneRect) -> bool {
    let mut path = PathBuilder::new();
    path.move_to(rect.x, rect.y);
    path.line_to(rect.x + rect.width, rect.y);
    path.line_to(rect.x + rect.width, rect.y + rect.height);
    path.line_to(rect.x, rect.y + rect.height);
    path.close();
    let Some(path) = path.finish() else {
        return false;
    };
    surface.push_clip_path(&path, &FillRule::NonZero);
    true
}

fn fill_polygon_color(surface: &mut Surface<'_>, points: &[ScenePoint], color: rgb::Color) {
    let Some(first) = points.first() else {
        return;
    };
    let mut path = PathBuilder::new();
    path.move_to(first.x, first.y);
    for point in &points[1..] {
        path.line_to(point.x, point.y);
    }
    path.close();
    if let Some(path) = path.finish() {
        surface.set_fill(Some(Fill {
            paint: color.into(),
            rule: FillRule::NonZero,
            opacity: NormalizedF32::ONE,
        }));
        surface.draw_path(&path);
    }
}

fn replay_geometry_operation(surface: &mut Surface<'_>, operation: &PageSceneOp) -> bool {
    match operation {
        PageSceneOp::FillRect { rect, color } => {
            fill_rect_color(surface, rect.x, rect.y, rect.width, rect.height, *color);
        }
        PageSceneOp::FillPolygon { points, color } => {
            fill_polygon_color(surface, points, *color);
        }
        PageSceneOp::PushClipRect { rect } => {
            push_pdf_rect_clip(surface, *rect);
        }
        PageSceneOp::PopClip => surface.pop(),
        PageSceneOp::PushTransform { transform } => {
            surface.push_transform(&Transform::from_row(
                transform.sx(),
                transform.ky(),
                transform.kx(),
                transform.sy(),
                transform.tx(),
                transform.ty(),
            ));
        }
        PageSceneOp::PopTransform => surface.pop(),
        PageSceneOp::Link { .. } | PageSceneOp::Image { .. } | PageSceneOp::GlyphRun(_) => {
            return false;
        }
    }
    true
}

#[cfg(test)]
pub(super) fn replay_geometry_operations(
    surface: &mut Surface<'_>,
    scene: &PageScene,
    operations: std::ops::Range<usize>,
) {
    let Some(operations) = scene.operations.get(operations) else {
        return;
    };
    for operation in operations {
        replay_geometry_operation(surface, operation);
    }
}

fn scene_font(scene: &PageScene, cache: &mut [Option<Font>], id: SceneFontId) -> Result<Font> {
    let Some(resource) = scene.font_resources.get(id.0) else {
        return Err(Error::Render(
            "page scene references an unknown font resource".into(),
        ));
    };
    let Some(slot) = cache.get_mut(id.0) else {
        return Err(Error::Render(
            "page scene references an unknown font resource".into(),
        ));
    };
    if let Some(font) = slot {
        return Ok(font.clone());
    }
    let font = resource.to_pdf_font()?;
    *slot = Some(font.clone());
    Ok(font)
}

fn scene_image(
    scene: &PageScene,
    cache: &mut [Option<PdfImage>],
    id: SceneImageId,
) -> Result<PdfImage> {
    let Some(resource) = scene.image_resources.get(id.0) else {
        return Err(Error::Render(
            "page scene references an unknown image resource".into(),
        ));
    };
    let Some(slot) = cache.get_mut(id.0) else {
        return Err(Error::Render(
            "page scene references an unknown image resource".into(),
        ));
    };
    if let Some(image) = slot {
        return Ok(image.clone());
    }
    let image = resource.to_pdf_image()?;
    *slot = Some(image.clone());
    Ok(image)
}

fn draw_glyph_run(surface: &mut Surface<'_>, run: &SceneGlyphRun, font: Font) {
    let width = run.width();
    if let Some(highlight) = run.highlight {
        fill_rect_color(
            surface,
            run.origin.x,
            run.origin.y - run.ascent,
            width,
            run.ascent + run.descent,
            highlight,
        );
    }
    surface.set_fill(Some(Fill {
        paint: run.color.into(),
        rule: FillRule::NonZero,
        opacity: NormalizedF32::ONE,
    }));
    let glyphs = run
        .glyphs
        .iter()
        .map(SceneGlyph::to_krilla)
        .collect::<Vec<_>>();
    surface.draw_glyphs(
        Point::from_xy(run.origin.x, run.origin.y),
        &glyphs,
        font,
        &run.text,
        run.size,
        false,
    );
    if let Some(decoration) = run.underline {
        fill_rect_color(
            surface,
            run.origin.x,
            run.origin.y + decoration.offset,
            width,
            decoration.thickness,
            run.color,
        );
    }
    if let Some(decoration) = run.strikethrough {
        fill_rect_color(
            surface,
            run.origin.x,
            run.origin.y + decoration.offset,
            width,
            decoration.thickness,
            run.color,
        );
    }
}

fn draw_image(
    surface: &mut Surface<'_>,
    scene: &PageScene,
    operation_index: usize,
    image: PdfImage,
) -> bool {
    let Some(PageSceneOp::Image {
        width,
        height,
        transform,
        ..
    }) = scene.operations.get(operation_index)
    else {
        return false;
    };
    let Some(size) = Size::from_wh(*width, *height) else {
        return false;
    };
    surface.push_transform(&Transform::from_row(
        transform.sx(),
        transform.ky(),
        transform.kx(),
        transform.sy(),
        transform.tx(),
        transform.ty(),
    ));
    surface.draw_image(image, size);
    surface.pop();
    true
}

pub(super) fn replay_complete_page_scene(
    surface: &mut Surface<'_>,
    scene: &PageScene,
) -> Result<()> {
    let mut fonts = vec![None; scene.font_resources.len()];
    let mut images = vec![None; scene.image_resources.len()];
    for (operation_index, operation) in scene.operations.iter().enumerate() {
        if replay_geometry_operation(surface, operation) {
            continue;
        }
        match operation {
            PageSceneOp::Link { .. } => {}
            PageSceneOp::Image { resource, .. } => {
                let image = scene_image(scene, &mut images, *resource)?;
                if !draw_image(surface, scene, operation_index, image) {
                    return Err(Error::Render(
                        "page scene contains an invalid image operation".into(),
                    ));
                }
            }
            PageSceneOp::GlyphRun(run) => {
                let font = scene_font(scene, &mut fonts, run.font)?;
                draw_glyph_run(surface, run, font);
            }
            PageSceneOp::FillRect { .. }
            | PageSceneOp::FillPolygon { .. }
            | PageSceneOp::PushClipRect { .. }
            | PageSceneOp::PopClip
            | PageSceneOp::PushTransform { .. }
            | PageSceneOp::PopTransform => {}
        }
    }
    Ok(())
}

pub(super) fn replay_annotations(page: &mut Page<'_>, scene: &PageScene) {
    for operation in &scene.operations {
        let PageSceneOp::Link { rect, target } = operation else {
            continue;
        };
        let Some(rect) = Rect::from_ltrb(rect.left, rect.top, rect.right, rect.bottom) else {
            continue;
        };
        let target = Target::Action(LinkAction::new(target.to_string()).into());
        page.add_annotation(Annotation::new_link(
            LinkAnnotation::new(rect, target),
            None,
        ));
    }
}

#[cfg(test)]
pub(super) fn draw_image_for_test(
    surface: &mut Surface<'_>,
    scene: &PageScene,
    operation_index: usize,
    image: PdfImage,
) -> bool {
    draw_image(surface, scene, operation_index, image)
}

/// Independent direct-draw oracle for complete scene replay tests.
#[cfg(test)]
pub(super) fn draw_run_for_test(
    surface: &mut Surface<'_>,
    run: RunDraw,
    x_abs: f32,
    baseline_y: f32,
    font: Font,
) {
    let x = x_abs + run.x;
    let baseline = baseline_y + run.baseline_shift;
    let width = run.width();
    if let Some(highlight) = run.highlight {
        fill_rect_color(
            surface,
            x,
            baseline - run.ascent,
            width,
            run.ascent + run.descent,
            highlight,
        );
    }
    surface.set_fill(Some(Fill {
        paint: run.color.into(),
        rule: FillRule::NonZero,
        opacity: NormalizedF32::ONE,
    }));
    surface.draw_glyphs(
        Point::from_xy(x, baseline),
        &run.glyphs,
        font,
        &run.text,
        run.size,
        false,
    );
    if let Some(decoration) = run.underline {
        fill_rect_color(
            surface,
            x,
            baseline + decoration.offset,
            width,
            decoration.thickness,
            run.color,
        );
    }
    if let Some(decoration) = run.strikethrough {
        fill_rect_color(
            surface,
            x,
            baseline + decoration.offset,
            width,
            decoration.thickness,
            run.color,
        );
    }
}
