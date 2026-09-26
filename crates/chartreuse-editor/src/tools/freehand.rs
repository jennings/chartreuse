//! The press-drag-release state machine shared by the freehand tools (pen,
//! highlighter): a stroke through the pointer's path.

use std::fmt;
use std::marker::PhantomData;

use iced::mouse::Interaction;

use super::{Context, Pointer, Preview, Tool, ToolKind};
use crate::model::{distance_to_segment, Document, Point, Shape};

/// How far the pointer must move from the last recorded point before it
/// records another, in canvas pixels.
pub const MIN_SPACING: f32 = 1.0;

/// How far a finished stroke's simplified path may stray from its smoothed
/// path, in canvas pixels.
pub const SIMPLIFY_TOLERANCE: f32 = 0.5;

/// The geometry of a freehand tool: which shape a path draws.
pub trait FreehandShape: fmt::Debug + 'static {
    const KIND: ToolKind;

    /// The shape through `points` (never empty).
    fn shape(points: Vec<Point>) -> Shape;
}

/// A tool that draws one `S` per press-drag-release, through the pointer's
/// path.
///
/// Pressing starts a stroke at the pointer (a click alone leaves a dot);
/// moving records the pointer every [`MIN_SPACING`] canvas pixels, and the
/// raw path is previewed. Releasing [smooths](smooth) the path,
/// [simplifies](simplify) it to within [`SIMPLIFY_TOLERANCE`] canvas pixels,
/// and adds the result (one undo step), selected.
pub struct FreehandTool<S> {
    points: Option<Vec<Point>>,
    shape: PhantomData<S>,
}

impl<S> Default for FreehandTool<S> {
    fn default() -> Self {
        Self {
            points: None,
            shape: PhantomData,
        }
    }
}

impl<S: FreehandShape> fmt::Debug for FreehandTool<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FreehandTool")
            .field("kind", &S::KIND)
            .field("points", &self.points.as_ref().map(Vec::len))
            .finish()
    }
}

impl<S: FreehandShape> Tool for FreehandTool<S> {
    fn kind(&self) -> ToolKind {
        S::KIND
    }

    fn pointer(&mut self, pointer: Pointer, cx: &mut Context<'_>) {
        match pointer {
            Pointer::Press { at, .. } => self.points = Some(vec![at]),
            Pointer::Move { at } => {
                if let Some(points) = &mut self.points
                    && points
                        .last()
                        .is_none_or(|last| last.distance(at) >= MIN_SPACING * cx.pixel)
                {
                    points.push(at);
                }
            }
            Pointer::Release { at } => {
                self.pointer(Pointer::Move { at }, cx);
                self.finish(cx);
            }
        }
    }

    fn escape(&mut self, _cx: &mut Context<'_>) -> bool {
        self.points.take().is_some()
    }

    fn finish(&mut self, cx: &mut Context<'_>) {
        if let Some(points) = self.points.take() {
            let points = simplify(&smooth(&smooth(&points)), SIMPLIFY_TOLERANCE * cx.pixel);
            let id = cx.document.add(S::shape(points), cx.style);
            cx.document.set_selection([id]);
        }
    }

    fn is_active(&self) -> bool {
        self.points.is_some()
    }

    fn preview(&self) -> Preview<'_> {
        match &self.points {
            Some(points) => Preview::New(S::shape(points.clone())),
            None => Preview::None,
        }
    }

    fn cursor(&self, _document: &Document, _at: Point, _pixel: f32) -> Interaction {
        Interaction::Crosshair
    }
}

/// One round of Chaikin corner cutting that keeps the endpoints: each
/// segment is replaced by the points a quarter and three quarters along it
/// (except next to the ends), rounding off the corners of a jagged mouse
/// path. Straight runs stay straight. Paths of fewer than three points are
/// returned as they are.
#[must_use]
pub fn smooth(points: &[Point]) -> Vec<Point> {
    let [first, .., last] = points else {
        return points.to_vec();
    };
    if points.len() < 3 {
        return points.to_vec();
    }
    let segments = points.len() - 1;
    let mut smoothed = Vec::with_capacity(2 * segments);
    smoothed.push(*first);
    for (i, pair) in points.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let along = b - a;
        if i > 0 {
            smoothed.push(a + along * 0.25);
        }
        if i + 1 < segments {
            smoothed.push(a + along * 0.75);
        }
    }
    smoothed.push(*last);
    smoothed
}

/// The Ramer–Douglas–Peucker simplification of `points`: a subset,
/// including both ends, such that no dropped point lies further than
/// `tolerance` from the simplified path.
#[must_use]
pub fn simplify(points: &[Point], tolerance: f32) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut spans = vec![(0, points.len() - 1)];
    while let Some((start, end)) = spans.pop() {
        let farthest = (start + 1..end)
            .map(|i| {
                (
                    i,
                    distance_to_segment(points[i], points[start], points[end]),
                )
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, distance)) = farthest
            && distance > tolerance
        {
            keep[i] = true;
            spans.push((start, i));
            spans.push((i, end));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(point, keep)| keep.then_some(*point))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(coords: &[(f32, f32)]) -> Vec<Point> {
        coords.iter().map(|&(x, y)| Point::new(x, y)).collect()
    }

    #[test]
    fn simplify_drops_only_points_within_the_tolerance() {
        let path = points(&[
            (0.0, 0.0),
            (5.0, 0.4),
            (10.0, 0.0),
            (15.0, 3.0),
            (20.0, 0.0),
        ]);
        // (5, 0.4) is 0.4 off the line through its neighbors; (15, 3) is 3.
        assert_eq!(
            simplify(&path, 0.5),
            points(&[(0.0, 0.0), (10.0, 0.0), (15.0, 3.0), (20.0, 0.0)])
        );
        assert_eq!(simplify(&path, 0.3), path);
        assert_eq!(
            simplify(&path, 5.0),
            points(&[(0.0, 0.0), (20.0, 0.0)]),
            "the ends always stay"
        );
        let dot = points(&[(3.0, 3.0)]);
        assert_eq!(simplify(&dot, 1.0), dot);
    }

    #[test]
    fn smooth_keeps_the_ends_and_cuts_the_corners() {
        let corner = points(&[(0.0, 0.0), (8.0, 0.0), (8.0, 8.0)]);
        assert_eq!(
            smooth(&corner),
            points(&[(0.0, 0.0), (6.0, 0.0), (8.0, 2.0), (8.0, 8.0)])
        );
        // A straight run stays on its line.
        let straight = points(&[(0.0, 0.0), (1.0, 1.0), (5.0, 5.0), (6.0, 6.0)]);
        assert!(smooth(&straight).iter().all(|p| (p.x - p.y).abs() < 1e-6));
        // Too short to have corners.
        let segment = points(&[(0.0, 0.0), (4.0, 4.0)]);
        assert_eq!(smooth(&segment), segment);
    }
}
