//! Bounded, ordered page-paint description shared by render backends.

use std::rc::Rc;
use std::sync::Arc;

use krilla::color::rgb;

use super::{Error, Result};

const MAX_OPERATIONS: usize = 262_144;
const MAX_PATH_POINTS: usize = 1_048_576;
const MAX_LINKS: usize = 16_384;
const MAX_IMAGE_RESOURCES: usize = 4_096;
const MAX_FONT_RESOURCES: usize = 4_096;
const MAX_GLYPHS: usize = 1_048_576;
const MAX_STATE_DEPTH: usize = 128;

#[derive(Clone)]
pub(super) struct SceneFontResource {
    pub(super) bytes: Arc<dyn AsRef<[u8]> + Send + Sync>,
    pub(super) source_id: u64,
    pub(super) index: u32,
}

impl SceneFontResource {
    pub(super) fn shares_source_with(&self, other: &Self) -> bool {
        self.source_id == other.source_id && self.index == other.index
    }

    pub(super) fn is_valid(&self) -> bool {
        !self.bytes.as_ref().as_ref().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TextDecoration {
    pub(super) offset: f32,
    pub(super) thickness: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SceneRect {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

impl SceneRect {
    pub(super) fn new(x: f32, y: f32, width: f32, height: f32) -> Option<Self> {
        if ![x, y, width, height, x + width, y + height]
            .into_iter()
            .all(f32::is_finite)
            || width <= 0.0
            || height <= 0.0
        {
            return None;
        }
        Some(Self {
            x,
            y,
            width,
            height,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ScenePoint {
    pub(super) x: f32,
    pub(super) y: f32,
}

impl ScenePoint {
    pub(super) fn new(x: f32, y: f32) -> Option<Self> {
        [x, y]
            .into_iter()
            .all(f32::is_finite)
            .then_some(Self { x, y })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SceneGlyph {
    pub(super) glyph_id: u32,
    pub(super) text_range: std::ops::Range<usize>,
    pub(super) x_advance: f32,
    pub(super) x_offset: f32,
    pub(super) y_offset: f32,
    pub(super) y_advance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SceneGlyphRun {
    pub(super) font: SceneFontId,
    pub(super) origin: ScenePoint,
    pub(super) glyphs: Box<[SceneGlyph]>,
    pub(super) text: Rc<str>,
    pub(super) size: f32,
    pub(super) color: rgb::Color,
    pub(super) highlight: Option<rgb::Color>,
    pub(super) ascent: f32,
    pub(super) descent: f32,
    pub(super) underline: Option<TextDecoration>,
    pub(super) strikethrough: Option<TextDecoration>,
    pub(super) link: Option<Rc<str>>,
    pub(super) is_rtl: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SceneLinkRect {
    // Preserve authored annotation bounds without a width subtraction/re-addition round trip.
    pub(super) left: f32,
    pub(super) top: f32,
    pub(super) right: f32,
    pub(super) bottom: f32,
}

impl SceneLinkRect {
    fn from_ltrb([left, top, right, bottom]: [f32; 4]) -> Option<Self> {
        if ![left, top, right, bottom].into_iter().all(f32::is_finite)
            || left >= right
            || top >= bottom
        {
            return None;
        }
        Some(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    fn intersection(self, clip: Self) -> Option<Self> {
        Self::from_ltrb([
            self.left.max(clip.left),
            self.top.max(clip.top),
            self.right.min(clip.right),
            self.bottom.min(clip.bottom),
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum LinkClip {
    Unbounded,
    Bounded(SceneLinkRect),
    Hidden,
}

impl LinkClip {
    pub(super) fn from_ltrb(bounds: [f32; 4]) -> Self {
        SceneLinkRect::from_ltrb(bounds).map_or(Self::Hidden, Self::Bounded)
    }

    fn apply(self, rect: SceneLinkRect) -> Option<SceneLinkRect> {
        match self {
            Self::Unbounded => Some(rect),
            Self::Bounded(clip) => rect.intersection(clip),
            Self::Hidden => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SceneImageEncoding {
    Png,
    Jpeg,
    Gif,
    Webp,
    Rgba8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SceneImageResource {
    pub(super) encoding: SceneImageEncoding,
    pub(super) bytes: Arc<Vec<u8>>,
    pub(super) width_px: u32,
    pub(super) height_px: u32,
}

impl SceneImageResource {
    fn shares_source_with(&self, other: &Self) -> bool {
        self.encoding == other.encoding
            && self.width_px == other.width_px
            && self.height_px == other.height_px
            && Arc::ptr_eq(&self.bytes, &other.bytes)
    }

    pub(super) fn is_valid(&self) -> bool {
        !self.bytes.is_empty() && self.width_px > 0 && self.height_px > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SceneImageId(pub(super) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SceneFontId(pub(super) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SceneStateKind {
    Clip,
    Transform,
}

impl SceneStateKind {
    fn name(self) -> &'static str {
        match self {
            Self::Clip => "clip",
            Self::Transform => "transform",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SceneTransform {
    sx: f32,
    ky: f32,
    kx: f32,
    sy: f32,
    tx: f32,
    ty: f32,
}

impl SceneTransform {
    pub(super) fn from_row(sx: f32, ky: f32, kx: f32, sy: f32, tx: f32, ty: f32) -> Self {
        Self {
            sx,
            ky,
            kx,
            sy,
            tx,
            ty,
        }
    }

    pub(super) fn from_translate(tx: f32, ty: f32) -> Self {
        Self::from_row(1.0, 0.0, 0.0, 1.0, tx, ty)
    }

    fn is_finite(self) -> bool {
        [self.sx, self.ky, self.kx, self.sy, self.tx, self.ty]
            .into_iter()
            .all(f32::is_finite)
    }

    pub(super) fn sx(self) -> f32 {
        self.sx
    }

    pub(super) fn ky(self) -> f32 {
        self.ky
    }

    pub(super) fn kx(self) -> f32 {
        self.kx
    }

    pub(super) fn sy(self) -> f32 {
        self.sy
    }

    pub(super) fn tx(self) -> f32 {
        self.tx
    }

    pub(super) fn ty(self) -> f32 {
        self.ty
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum PageSceneOp {
    FillRect {
        rect: SceneRect,
        color: rgb::Color,
    },
    FillPolygon {
        points: Box<[ScenePoint]>,
        color: rgb::Color,
    },
    Link {
        rect: SceneLinkRect,
        target: Rc<str>,
    },
    Image {
        resource: SceneImageId,
        width: f32,
        height: f32,
        transform: SceneTransform,
    },
    GlyphRun(SceneGlyphRun),
    PushClipRect {
        rect: SceneRect,
    },
    PopClip,
    PushTransform {
        transform: SceneTransform,
    },
    PopTransform,
}

pub(super) struct PageScene {
    pub(super) operations: Vec<PageSceneOp>,
    pub(super) operation_limit: usize,
    pub(super) path_point_count: usize,
    path_point_limit: usize,
    link_count: usize,
    link_limit: usize,
    pub(super) image_resources: Vec<SceneImageResource>,
    image_limit: usize,
    pub(super) font_resources: Vec<SceneFontResource>,
    pub(super) font_limit: usize,
    pub(super) glyph_count: usize,
    pub(super) glyph_limit: usize,
    state_stack: Vec<SceneStateKind>,
    state_limit: usize,
}

impl Default for PageScene {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
            operation_limit: MAX_OPERATIONS,
            path_point_count: 0,
            path_point_limit: MAX_PATH_POINTS,
            link_count: 0,
            link_limit: MAX_LINKS,
            image_resources: Vec::new(),
            image_limit: MAX_IMAGE_RESOURCES,
            font_resources: Vec::new(),
            font_limit: MAX_FONT_RESOURCES,
            glyph_count: 0,
            glyph_limit: MAX_GLYPHS,
            state_stack: Vec::new(),
            state_limit: MAX_STATE_DEPTH,
        }
    }
}

impl PageScene {
    #[cfg(test)]
    pub(super) fn with_operation_limit(operation_limit: usize) -> Self {
        Self {
            operations: Vec::new(),
            operation_limit,
            path_point_count: 0,
            path_point_limit: MAX_PATH_POINTS,
            link_count: 0,
            link_limit: MAX_LINKS,
            image_resources: Vec::new(),
            image_limit: MAX_IMAGE_RESOURCES,
            font_resources: Vec::new(),
            font_limit: MAX_FONT_RESOURCES,
            glyph_count: 0,
            glyph_limit: MAX_GLYPHS,
            state_stack: Vec::new(),
            state_limit: MAX_STATE_DEPTH,
        }
    }

    #[cfg(test)]
    pub(super) fn with_limits(operation_limit: usize, link_limit: usize) -> Self {
        Self {
            operations: Vec::new(),
            operation_limit,
            path_point_count: 0,
            path_point_limit: MAX_PATH_POINTS,
            link_count: 0,
            link_limit,
            image_resources: Vec::new(),
            image_limit: MAX_IMAGE_RESOURCES,
            font_resources: Vec::new(),
            font_limit: MAX_FONT_RESOURCES,
            glyph_count: 0,
            glyph_limit: MAX_GLYPHS,
            state_stack: Vec::new(),
            state_limit: MAX_STATE_DEPTH,
        }
    }

    #[cfg(test)]
    pub(super) fn with_image_limit(image_limit: usize) -> Self {
        Self {
            operations: Vec::new(),
            operation_limit: MAX_OPERATIONS,
            path_point_count: 0,
            path_point_limit: MAX_PATH_POINTS,
            link_count: 0,
            link_limit: MAX_LINKS,
            image_resources: Vec::new(),
            image_limit,
            font_resources: Vec::new(),
            font_limit: MAX_FONT_RESOURCES,
            glyph_count: 0,
            glyph_limit: MAX_GLYPHS,
            state_stack: Vec::new(),
            state_limit: MAX_STATE_DEPTH,
        }
    }

    #[cfg(test)]
    pub(super) fn with_state_limit(state_limit: usize) -> Self {
        Self {
            operations: Vec::new(),
            operation_limit: MAX_OPERATIONS,
            path_point_count: 0,
            path_point_limit: MAX_PATH_POINTS,
            link_count: 0,
            link_limit: MAX_LINKS,
            image_resources: Vec::new(),
            image_limit: MAX_IMAGE_RESOURCES,
            font_resources: Vec::new(),
            font_limit: MAX_FONT_RESOURCES,
            glyph_count: 0,
            glyph_limit: MAX_GLYPHS,
            state_stack: Vec::new(),
            state_limit,
        }
    }

    #[cfg(test)]
    pub(super) fn with_path_point_limit(path_point_limit: usize) -> Self {
        Self {
            path_point_limit,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(super) fn with_text_limits(font_limit: usize, glyph_limit: usize) -> Self {
        Self {
            font_limit,
            glyph_limit,
            ..Self::default()
        }
    }

    pub(super) fn push_operation(&mut self, operation: PageSceneOp) -> Result<()> {
        if self.operations.len() >= self.operation_limit {
            return Err(Error::Render(format!(
                "page scene exceeds the {}-operation limit",
                self.operation_limit
            )));
        }
        self.operations.push(operation);
        Ok(())
    }

    pub(super) fn push_fill_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: rgb::Color,
    ) -> Result<()> {
        let Some(rect) = SceneRect::new(x, y, width, height) else {
            return Ok(());
        };
        self.push_operation(PageSceneOp::FillRect { rect, color })
    }

    pub(super) fn push_fill_polygon(
        &mut self,
        points: &[(f32, f32)],
        color: rgb::Color,
    ) -> Result<()> {
        self.push_fill_polygon_iter(points.iter().copied(), color)
    }

    pub(super) fn push_fill_polygon_iter<I>(&mut self, points: I, color: rgb::Color) -> Result<()>
    where
        I: Clone + Iterator<Item = (f32, f32)>,
    {
        let mut point_count = 0usize;
        for (x, y) in points.clone() {
            let Some(next_point_count) = point_count.checked_add(1) else {
                return Err(Error::Render(format!(
                    "page scene exceeds the {}-path-point limit",
                    self.path_point_limit
                )));
            };
            point_count = next_point_count;
            if ScenePoint::new(x, y).is_none() {
                return Ok(());
            }
        }
        if point_count < 3 {
            return Ok(());
        }
        let Some(next_point_count) = self.path_point_count.checked_add(point_count) else {
            return Err(Error::Render(format!(
                "page scene exceeds the {}-path-point limit",
                self.path_point_limit
            )));
        };
        if next_point_count > self.path_point_limit {
            return Err(Error::Render(format!(
                "page scene exceeds the {}-path-point limit",
                self.path_point_limit
            )));
        }
        let projected = points.map(|(x, y)| ScenePoint { x, y }).collect::<Vec<_>>();
        self.push_operation(PageSceneOp::FillPolygon {
            points: projected.into_boxed_slice(),
            color,
        })?;
        self.path_point_count = next_point_count;
        Ok(())
    }

    pub(super) fn push_link_ltrb(
        &mut self,
        bounds: [f32; 4],
        target: Rc<str>,
        clip: LinkClip,
    ) -> Result<()> {
        let Some(rect) = SceneLinkRect::from_ltrb(bounds).and_then(|rect| clip.apply(rect)) else {
            return Ok(());
        };
        if self.link_count >= self.link_limit {
            return Err(Error::Render(format!(
                "page scene exceeds the {}-link limit",
                self.link_limit
            )));
        }
        self.push_operation(PageSceneOp::Link { rect, target })?;
        self.link_count += 1;
        Ok(())
    }

    pub(super) fn push_image(
        &mut self,
        resource: SceneImageResource,
        width: f32,
        height: f32,
        transform: SceneTransform,
    ) -> Result<Option<usize>> {
        if !resource.is_valid()
            || !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
            || !transform.is_finite()
        {
            return Ok(None);
        }
        if self.operations.len() >= self.operation_limit {
            return Err(Error::Render(format!(
                "page scene exceeds the {}-operation limit",
                self.operation_limit
            )));
        }
        let existing = self
            .image_resources
            .iter()
            .position(|candidate| candidate.shares_source_with(&resource));
        let resource_id = match existing {
            Some(index) => SceneImageId(index),
            None => {
                if self.image_resources.len() >= self.image_limit {
                    return Err(Error::Render(format!(
                        "page scene exceeds the {}-image-resource limit",
                        self.image_limit
                    )));
                }
                let id = SceneImageId(self.image_resources.len());
                self.image_resources.push(resource);
                id
            }
        };
        let operation_index = self.operations.len();
        self.operations.push(PageSceneOp::Image {
            resource: resource_id,
            width,
            height,
            transform,
        });
        Ok(Some(operation_index))
    }

    pub(super) fn push_clip_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Result<bool> {
        let Some(rect) = SceneRect::new(x, y, width, height) else {
            return Ok(false);
        };
        self.push_state(SceneStateKind::Clip, PageSceneOp::PushClipRect { rect })?;
        Ok(true)
    }

    pub(super) fn pop_clip(&mut self) -> Result<()> {
        self.pop_state(SceneStateKind::Clip, PageSceneOp::PopClip)
    }

    pub(super) fn push_transform(&mut self, transform: SceneTransform) -> Result<bool> {
        if !transform.is_finite() {
            return Ok(false);
        }
        self.push_state(
            SceneStateKind::Transform,
            PageSceneOp::PushTransform { transform },
        )?;
        Ok(true)
    }

    pub(super) fn pop_transform(&mut self) -> Result<()> {
        self.pop_state(SceneStateKind::Transform, PageSceneOp::PopTransform)
    }

    fn push_state(&mut self, kind: SceneStateKind, operation: PageSceneOp) -> Result<()> {
        if self.state_stack.len() >= self.state_limit {
            return Err(Error::Render(format!(
                "page scene state depth exceeds the {}-level limit",
                self.state_limit
            )));
        }
        self.push_operation(operation)?;
        self.state_stack.push(kind);
        Ok(())
    }

    fn pop_state(&mut self, expected: SceneStateKind, operation: PageSceneOp) -> Result<()> {
        match self.state_stack.last() {
            Some(actual) if *actual == expected => {}
            Some(actual) => {
                return Err(Error::Render(format!(
                    "page scene state mismatch: cannot pop {} above {}",
                    expected.name(),
                    actual.name()
                )));
            }
            None => {
                return Err(Error::Render(format!(
                    "page scene {} stack underflow",
                    expected.name()
                )));
            }
        }
        self.push_operation(operation)?;
        self.state_stack.pop();
        Ok(())
    }

    pub(super) fn ensure_balanced(&self) -> Result<()> {
        if self.state_stack.is_empty() {
            return Ok(());
        }
        Err(Error::Render(format!(
            "page scene has {} unclosed state operation{}",
            self.state_stack.len(),
            if self.state_stack.len() == 1 { "" } else { "s" }
        )))
    }
}
