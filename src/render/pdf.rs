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
use skrifa::MetadataProvider;

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

pub(super) fn validate_fixed_glyphs(scene: &PageScene) -> Result<()> {
    let mut checked = std::collections::HashSet::new();
    for operation in &scene.operations {
        let PageSceneOp::GlyphRun(run) = operation else {
            continue;
        };
        let resource = scene
            .font_resources
            .get(run.font.0)
            .ok_or_else(|| Error::Render("page scene contains an invalid font resource".into()))?;
        let face = skrifa::FontRef::from_index(resource.bytes.as_ref().as_ref(), resource.index)
            .map_err(|_| Error::Render("page scene contains an invalid font resource".into()))?;
        let outlines = face.outline_glyphs();
        let colors = face.color_glyphs();
        let bitmaps = face.bitmap_strikes();
        for glyph in &run.glyphs {
            if !run
                .text
                .get(glyph.text_range.clone())
                .is_some_and(super::has_visible_text)
                || !checked.insert((run.font.0, glyph.glyph_id))
            {
                continue;
            }
            let id = skrifa::GlyphId::new(glyph.glyph_id);
            let has_artwork = outlines.get(id).is_some()
                || colors.get(id).is_some()
                || bitmaps
                    .glyph_for_size(skrifa::instance::Size::unscaled(), id)
                    .is_some_and(|bitmap| match bitmap.data {
                        skrifa::bitmap::BitmapData::Png(data) => {
                            PdfImage::from_png(data.to_vec().into(), false).is_ok()
                        }
                        _ => false,
                    });
            if glyph.glyph_id == 0 || !has_artwork {
                return Err(Error::Render(
                    "fixed-font PDF text requires a glyph absent from the supplied fonts".into(),
                ));
            }
        }
    }
    Ok(())
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

fn replay_geometry_operation(surface: &mut Surface<'_>, operation: &PageSceneOp) -> Result<bool> {
    match operation {
        PageSceneOp::FillRect { rect, color } => {
            fill_rect_color(surface, rect.x, rect.y, rect.width, rect.height, *color);
        }
        PageSceneOp::FillPolygon { points, color } => {
            fill_polygon_color(surface, points, *color);
        }
        PageSceneOp::PushClipRect { rect } => {
            if !push_pdf_rect_clip(surface, *rect) {
                return Err(Error::Render(
                    "page scene contains an invalid clip path".into(),
                ));
            }
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
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
pub(super) fn replay_geometry_operations(
    surface: &mut Surface<'_>,
    scene: &PageScene,
    operations: std::ops::Range<usize>,
) -> Result<()> {
    let Some(operations) = scene.operations.get(operations) else {
        return Ok(());
    };
    for operation in operations {
        replay_geometry_operation(surface, operation)?;
    }
    Ok(())
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

fn draw_glyph_span(
    surface: &mut Surface<'_>,
    run: &SceneGlyphRun,
    font: Font,
    x_offset: f32,
    glyphs: &[SceneGlyph],
) {
    let glyphs = glyphs.iter().map(SceneGlyph::to_krilla).collect::<Vec<_>>();
    surface.draw_glyphs(
        Point::from_xy(run.origin.x + x_offset, run.origin.y),
        &glyphs,
        font,
        &run.text,
        run.size,
        false,
    );
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
    let is_tab = |glyph: &SceneGlyph| run.text.get(glyph.text_range.clone()) == Some("\t");
    if run.glyphs.iter().any(is_tab) {
        // Tabs retain layout advances and decorations, but have no glyph ink.
        let mut start = 0;
        let mut offset = 0.0;
        let mut advance = 0.0;
        for (index, glyph) in run.glyphs.iter().enumerate() {
            if is_tab(glyph) {
                if start < index {
                    draw_glyph_span(
                        surface,
                        run,
                        font.clone(),
                        offset,
                        &run.glyphs[start..index],
                    );
                }
                start = index + 1;
                offset = advance + glyph.x_advance * run.size;
            }
            advance += glyph.x_advance * run.size;
        }
        if start < run.glyphs.len() {
            draw_glyph_span(surface, run, font, offset, &run.glyphs[start..]);
        }
    } else {
        draw_glyph_span(surface, run, font, 0.0, &run.glyphs);
    }
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
        if replay_geometry_operation(surface, operation)? {
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

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::*;
    use crate::render::TextDecoration;

    fn tab_run(text: &str) -> SceneGlyphRun {
        let bytes = rwml_fonts::noto_sans_kr_subset();
        let face = skrifa::FontRef::from_index(bytes, 0).unwrap();
        let glyphs = text
            .char_indices()
            .map(|(start, ch)| SceneGlyph {
                glyph_id: if ch == '\t' {
                    0
                } else {
                    face.charmap().map(ch).unwrap().to_u32()
                },
                text_range: start..start + ch.len_utf8(),
                x_advance: if ch == '\t' { 2.0 } else { 0.5 },
                x_offset: 0.1,
                y_offset: 0.2,
                y_advance: 0.0,
            })
            .collect();
        SceneGlyphRun {
            font: SceneFontId(0),
            origin: ScenePoint { x: 10.0, y: 50.0 },
            glyphs,
            text: text.into(),
            size: 10.0,
            color: rgb::Color::new(20, 40, 60),
            highlight: None,
            ascent: 9.0,
            descent: 3.0,
            underline: None,
            strikethrough: None,
            link: Some("https://example.com/tab-content".into()),
            is_rtl: false,
        }
    }

    fn run_pdf(run: &SceneGlyphRun, spans: Option<&[(f32, Range<usize>)]>) -> Vec<u8> {
        let font = Font::new(rwml_fonts::noto_sans_kr_subset().to_vec().into(), 0).unwrap();
        let mut document = krilla::Document::new();
        let settings = krilla::page::PageSettings::from_wh(200.0, 100.0).unwrap();
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        if let Some(spans) = spans {
            // Independently draw the specified visible spans at known positions.
            if let Some(color) = run.highlight {
                fill_rect_color(
                    &mut surface,
                    run.origin.x,
                    run.origin.y - run.ascent,
                    run.width(),
                    run.ascent + run.descent,
                    color,
                );
            }
            surface.set_fill(Some(Fill {
                paint: run.color.into(),
                rule: FillRule::NonZero,
                opacity: NormalizedF32::ONE,
            }));
            for (offset, range) in spans {
                let glyphs = run.glyphs[range.clone()]
                    .iter()
                    .map(SceneGlyph::to_krilla)
                    .collect::<Vec<_>>();
                surface.draw_glyphs(
                    Point::from_xy(run.origin.x + offset, run.origin.y),
                    &glyphs,
                    font.clone(),
                    &run.text,
                    run.size,
                    false,
                );
            }
            for decoration in [run.underline, run.strikethrough].into_iter().flatten() {
                fill_rect_color(
                    &mut surface,
                    run.origin.x,
                    run.origin.y + decoration.offset,
                    run.width(),
                    decoration.thickness,
                    run.color,
                );
            }
        } else {
            draw_glyph_run(&mut surface, run, font);
        }
        surface.finish();
        page.finish();
        document.finish().unwrap()
    }

    fn decorate(run: &mut SceneGlyphRun) {
        run.highlight = Some(rgb::Color::new(240, 230, 180));
        run.underline = Some(TextDecoration {
            offset: 1.0,
            thickness: 0.5,
        });
        run.strikethrough = Some(TextDecoration {
            offset: -3.0,
            thickness: 0.5,
        });
    }

    fn assert_span_pdf(run: &SceneGlyphRun, spans: &[(f32, Range<usize>)]) {
        let original = run.clone();
        let actual = run_pdf(run, None);
        assert!(
            actual == run_pdf(run, Some(spans)),
            "unexpected glyph ink or position for {:?}",
            run.text
        );
        assert!(
            actual == run_pdf(run, None),
            "PDF replay must repeat exactly"
        );
        assert_eq!(
            &original, run,
            "painting must preserve the complete scene run"
        );
    }

    #[test]
    fn pdf_tab_glyphs_preserve_advance_without_painting() {
        for (text, spans) in [
            ("A\tB", vec![(0.0, 0..1), (25.0, 2..3)]),
            ("\tA\t\tB\t", vec![(20.0, 1..2), (65.0, 4..5)]),
            ("\u{ac00}\tA", vec![(0.0, 0..1), (25.0, 2..3)]),
            ("AB", vec![(0.0, 0..2)]),
        ] {
            let mut run = tab_run(text);
            assert_span_pdf(&run, &spans);
            decorate(&mut run);
            assert_span_pdf(&run, &spans);
        }
    }

    #[test]
    fn pdf_tab_only_runs_keep_decorations() {
        let mut run = tab_run("\t\t");
        assert_span_pdf(&run, &[]);
        decorate(&mut run);
        assert_span_pdf(&run, &[]);
    }

    #[test]
    fn pdf_tab_glyphs_preserve_visual_order_and_scene_metadata() {
        let mut run = tab_run("AB\tC");
        run.glyphs.reverse();
        run.is_rtl = true;
        decorate(&mut run);
        assert_span_pdf(&run, &[(0.0, 0..1), (25.0, 2..4)]);
    }

    #[test]
    fn pdf_tab_nonzero_glyph_is_not_drawn() {
        let mut run = tab_run("A\tB");
        run.glyphs[1].glyph_id = run.glyphs[0].glyph_id;
        assert_span_pdf(&run, &[(0.0, 0..1), (25.0, 2..3)]);
    }

    #[test]
    fn pdf_non_tab_missing_glyphs_are_not_suppressed() {
        let mut run = tab_run("A B");
        run.glyphs[0].glyph_id = 0;
        run.glyphs[1].glyph_id = 0;
        assert_span_pdf(&run, &[(0.0, 0..3)]);
    }

    #[test]
    fn pdf_tab_mixed_source_cluster_keeps_visible_glyph() {
        let mut run = tab_run("A\tB");
        run.glyphs[1].text_range = 1..3;
        run.glyphs[1].glyph_id = run.glyphs[2].glyph_id;
        run.glyphs = run.glyphs[..2].into();
        assert_span_pdf(&run, &[(0.0, 0..2)]);
    }
}
