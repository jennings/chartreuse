//! The document: base image, annotations in z-order, and the selection.

use std::collections::BTreeSet;

use chartreuse_core::image::Image;

use super::annotation::{Annotation, AnnotationId, Shape};
use super::geometry::{Point, Rect, Size};
use super::style::Style;

/// An image being annotated.
///
/// Annotations are kept in z-order: index 0 is drawn first (bottom), the last
/// is drawn on top. The base image is never modified; annotations are
/// flattened onto a copy only at export.
///
/// The selection is a set of ids of annotations in the document; ids that stop
/// existing are dropped from it.
#[derive(Debug, Clone)]
pub struct Document {
    base: Image,
    annotations: Vec<Annotation>,
    selection: BTreeSet<AnnotationId>,
    next_id: u64,
}

impl Document {
    /// A document with no annotations over `base`.
    #[must_use]
    pub const fn new(base: Image) -> Self {
        Self {
            base,
            annotations: Vec::new(),
            selection: BTreeSet::new(),
            next_id: 0,
        }
    }

    /// The image being annotated.
    #[must_use]
    pub const fn base(&self) -> &Image {
        &self.base
    }

    /// The base image's extent in document coordinates: `(0, 0)` to
    /// `(width, height)`.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        let size = Size::new(self.base.width() as f32, self.base.height() as f32);
        Rect::new(Point::ORIGIN, size)
    }

    /// Every annotation, bottom to top.
    #[must_use]
    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    #[must_use]
    pub fn get(&self, id: AnnotationId) -> Option<&Annotation> {
        self.index_of(id).map(|index| &self.annotations[index])
    }

    /// The annotation's z-order position (0 = bottom).
    #[must_use]
    pub fn index_of(&self, id: AnnotationId) -> Option<usize> {
        self.annotations.iter().position(|a| a.id() == id)
    }

    /// Adds an annotation on top of the others and returns its new id. The
    /// selection is unchanged; select the new annotation explicitly if wanted.
    pub fn add(&mut self, shape: Shape, style: Style) -> AnnotationId {
        let id = AnnotationId(self.next_id);
        self.next_id += 1;
        self.annotations.push(Annotation::new(id, shape, style));
        id
    }

    /// The topmost annotation that `point` hits, with `tolerance` in document
    /// units (see [`Shape::hit`]).
    #[must_use]
    pub fn annotation_at(&self, point: Point, tolerance: f32) -> Option<AnnotationId> {
        self.annotations
            .iter()
            .rev()
            .find(|a| a.hit(point, tolerance))
            .map(Annotation::id)
    }

    /// The selected ids.
    #[must_use]
    pub const fn selection(&self) -> &BTreeSet<AnnotationId> {
        &self.selection
    }

    /// The selected annotations, bottom to top.
    pub fn selected(&self) -> impl Iterator<Item = &Annotation> {
        self.annotations
            .iter()
            .filter(|a| self.selection.contains(&a.id()))
    }

    #[must_use]
    pub fn is_selected(&self, id: AnnotationId) -> bool {
        self.selection.contains(&id)
    }

    /// Adds `id` to the selection. Returns false (and does nothing) if no such
    /// annotation exists.
    pub fn select(&mut self, id: AnnotationId) -> bool {
        let exists = self.index_of(id).is_some();
        if exists {
            self.selection.insert(id);
        }
        exists
    }

    /// Removes `id` from the selection. Returns whether it was selected.
    pub fn deselect(&mut self, id: AnnotationId) -> bool {
        self.selection.remove(&id)
    }

    /// Replaces the selection, ignoring ids of annotations that don't exist.
    pub fn set_selection(&mut self, ids: impl IntoIterator<Item = AnnotationId>) {
        self.selection.clear();
        for id in ids {
            self.select(id);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Records the laid-out size of text annotation `id` for its current
    /// content and font size, replacing the estimate used for hit-testing and
    /// bounds. The canvas calls this after laying the text out; any later
    /// change to the content or font size clears it again. This is a cache,
    /// not an edit: it is not recorded for undo.
    ///
    /// Returns false (and stores nothing) if `id` is not a text annotation or
    /// `size` is not finite and non-negative.
    pub fn set_text_size(&mut self, id: AnnotationId, size: Size) -> bool {
        let valid = |v: f32| v.is_finite() && v >= 0.0;
        if !valid(size.width) || !valid(size.height) {
            return false;
        }
        let Some(index) = self.index_of(id) else {
            return false;
        };
        match &mut self.annotations[index].shape {
            Shape::Text(text) => {
                text.set_measured(Some(size));
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use chartreuse_core::color::Rgba8;
    use chartreuse_core::geometry::PhysicalSize;

    use super::super::annotation::{Line, Rectangle, Text};
    use super::*;

    pub(in super::super) fn document() -> Document {
        Document::new(Image::filled(PhysicalSize::new(200, 100), Rgba8::WHITE))
    }

    pub(in super::super) fn line(ax: f32, ay: f32, bx: f32, by: f32) -> Shape {
        Shape::Line(Line {
            start: Point::new(ax, ay),
            end: Point::new(bx, by),
        })
    }

    pub(in super::super) fn rectangle(ax: f32, ay: f32, bx: f32, by: f32) -> Shape {
        Shape::Rectangle(Rectangle {
            rect: Rect::from_corners(Point::new(ax, ay), Point::new(bx, by)),
        })
    }

    #[test]
    fn bounds_cover_the_base_image() {
        let doc = document();
        assert_eq!(
            doc.bounds(),
            Rect::from_corners(Point::ORIGIN, Point::new(200.0, 100.0))
        );
    }

    #[test]
    fn added_annotations_stack_on_top_with_increasing_ids() {
        let mut doc = document();
        let a = doc.add(line(0.0, 0.0, 10.0, 10.0), Style::default());
        let b = doc.add(rectangle(0.0, 0.0, 10.0, 10.0), Style::default());
        assert!(a < b);
        assert_eq!(doc.index_of(a), Some(0));
        assert_eq!(doc.index_of(b), Some(1));
        assert_eq!(doc.get(b).map(Annotation::id), Some(b));
        assert!(doc.selection().is_empty());
    }

    #[test]
    fn annotation_at_returns_the_topmost_hit() {
        let mut doc = document();
        let bottom = doc.add(line(0.0, 50.0, 200.0, 50.0), Style::default());
        let top = doc.add(line(100.0, 0.0, 100.0, 100.0), Style::default());
        assert_eq!(doc.annotation_at(Point::new(100.0, 50.0), 0.0), Some(top));
        assert_eq!(doc.annotation_at(Point::new(20.0, 50.0), 0.0), Some(bottom));
        assert_eq!(doc.annotation_at(Point::new(20.0, 20.0), 0.0), None);
    }

    #[test]
    fn annotation_at_sees_through_rectangle_interiors() {
        let mut doc = document();
        let inner = doc.add(line(40.0, 50.0, 60.0, 50.0), Style::default());
        doc.add(rectangle(10.0, 10.0, 190.0, 90.0), Style::default());
        assert_eq!(doc.annotation_at(Point::new(50.0, 50.0), 0.0), Some(inner));
    }

    #[test]
    fn selection_holds_only_existing_annotations() {
        let mut doc = document();
        let a = doc.add(line(0.0, 0.0, 1.0, 1.0), Style::default());
        let b = doc.add(line(0.0, 0.0, 1.0, 1.0), Style::default());
        let ghost = AnnotationId(99);
        assert!(!doc.select(ghost));
        doc.set_selection([b, ghost, a]);
        assert_eq!(doc.selection().iter().copied().collect::<Vec<_>>(), [a, b]);
        let ids: Vec<_> = doc.selected().map(Annotation::id).collect();
        assert_eq!(ids, [a, b]);
        assert!(doc.deselect(a));
        assert!(!doc.deselect(a));
        assert!(doc.is_selected(b));
        doc.clear_selection();
        assert!(doc.selection().is_empty());
    }

    #[test]
    fn text_size_is_stored_only_for_text_and_valid_sizes() {
        let mut doc = document();
        let text = doc.add(
            Shape::Text(Text::new(Point::ORIGIN, "hi")),
            Style::default(),
        );
        let line = doc.add(line(0.0, 0.0, 1.0, 1.0), Style::default());
        assert!(!doc.set_text_size(line, Size::new(10.0, 10.0)));
        assert!(!doc.set_text_size(text, Size::new(f32::NAN, 10.0)));
        assert!(!doc.set_text_size(text, Size::new(-1.0, 10.0)));
        assert!(doc.set_text_size(text, Size::new(80.0, 30.0)));
        assert_eq!(
            doc.get(text).unwrap().bounds(),
            Rect::from_corners(Point::ORIGIN, Point::new(80.0, 30.0))
        );
        assert_eq!(doc.annotation_at(Point::new(79.0, 29.0), 0.0), Some(text));
    }
}
