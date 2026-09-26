use chartreuse_core::color::Rgba8;
use chartreuse_core::geometry::PhysicalSize;
use chartreuse_core::image::Image;

use super::*;
use crate::font;
use crate::model::{
    distance_to_segment, distance_to_triangle, Arrow, Line, Rect, Rectangle, Style, Text,
};

/// How far outside a shape's edge a pixel's center can be and still be
/// partly covered: half a pixel's diagonal, plus slack for anti-aliasing.
const EDGE: f32 = 0.75;

const BLUE: Rgba8 = Rgba8::rgb(0x20, 0x40, 0xf0);

/// A base image with every pixel different, including translucent ones, so
/// any pixel flatten touches by mistake shows.
fn base(width: u32, height: u32) -> Image {
    Image::from_fn(PhysicalSize::new(width, height), |x, y| {
        Rgba8::new(
            (x * 7) as u8,
            (y * 11) as u8,
            ((x + y) * 3) as u8,
            if (x + y) % 3 == 0 { 90 } else { 255 },
        )
    })
}

fn style(color: Rgba8, stroke_width: f32) -> Style {
    Style {
        color,
        stroke_width,
        ..Style::default()
    }
}

fn flattened(base: Image, shapes: impl IntoIterator<Item = (Shape, Style)>) -> Image {
    let mut document = Document::new(base);
    for (shape, style) in shapes {
        document.add(shape, style);
    }
    flatten(&document).expect("flattens")
}

fn line(ax: f32, ay: f32, bx: f32, by: f32) -> Shape {
    Shape::Line(Line {
        start: Point::new(ax, ay),
        end: Point::new(bx, by),
    })
}

/// Checks every pixel of `result` against `base` given the signed distance
/// from a pixel's center to the drawn area (negative inside): pixels well
/// inside are exactly `color` (opaque), pixels well outside keep their bytes.
fn assert_covers(base: &Image, result: &Image, color: Rgba8, distance: impl Fn(Point) -> f32) {
    let (mut inside, mut outside) = (0, 0);
    for y in 0..base.height() {
        for x in 0..base.width() {
            let d = distance(Point::new(x as f32 + 0.5, y as f32 + 0.5));
            let pixel = result.pixel(x, y).unwrap();
            if d < -EDGE {
                assert_eq!(pixel, color, "inside at ({x}, {y})");
                inside += 1;
            } else if d > EDGE {
                assert_eq!(pixel, base.pixel(x, y).unwrap(), "outside at ({x}, {y})");
                outside += 1;
            }
        }
    }
    assert!(
        inside > 0 && outside > 0,
        "{inside} inside, {outside} outside"
    );
}

#[test]
fn a_stroke_covers_exactly_the_points_within_half_its_width() {
    let image = base(60, 40);
    // Runs off the left edge: flattening clips to the image.
    let (a, b) = (Point::new(-10.0, 12.5), Point::new(45.3, 30.0));
    let result = flattened(
        image.clone(),
        [(line(a.x, a.y, b.x, b.y), style(BLUE, 7.0))],
    );
    assert_covers(&image, &result, BLUE, |p| {
        distance_to_segment(p, a, b) - 3.5
    });
}

#[test]
fn a_rectangle_is_its_outline_with_round_outer_corners() {
    let image = base(60, 50);
    let rect = Rect::from_corners(Point::new(10.0, 8.0), Point::new(50.0, 40.0));
    let result = flattened(
        image.clone(),
        [(Shape::Rectangle(Rectangle { rect }), style(BLUE, 6.0))],
    );
    assert_covers(&image, &result, BLUE, |p| rect.distance_to_outline(p) - 3.0);
}

#[test]
fn an_arrow_is_a_shaft_to_the_head_base_and_a_filled_head() {
    let image = base(80, 50);
    let arrow = Arrow {
        start: Point::new(8.0, 30.0),
        end: Point::new(70.0, 12.0),
    };
    // Thick enough that a shaft stroked to the tip would poke out past it.
    let width = 8.0;
    let head = arrow.head(width).unwrap();
    let result = flattened(
        image.clone(),
        [(Shape::Arrow(arrow.clone()), style(BLUE, width))],
    );
    assert_covers(&image, &result, BLUE, |p| {
        let shaft = distance_to_segment(p, arrow.start, head.base) - width / 2.0;
        // distance_to_triangle is 0 inside, so only the outside is checked
        // for the head; its interior is as deep as the shaft allows.
        shaft.min(distance_to_triangle(p, head.corners()))
    });
}

#[test]
fn zero_length_strokes_are_discs_and_zero_width_draws_nothing() {
    let image = base(30, 30);
    let center = Point::new(14.5, 15.0);
    let point = Rect::from_corners(center, center);
    for shape in [
        line(center.x, center.y, center.x, center.y),
        Shape::Arrow(Arrow {
            start: center,
            end: center,
        }),
        Shape::Rectangle(Rectangle { rect: point }),
    ] {
        let result = flattened(image.clone(), [(shape.clone(), style(BLUE, 10.0))]);
        assert_covers(&image, &result, BLUE, |p| (p - center).length() - 5.0);

        let invisible = flattened(image.clone(), [(shape, style(BLUE, 0.0))]);
        assert!(invisible == image, "zero width drew something");
    }
}

#[test]
fn translucent_colors_blend_source_over_in_straight_alpha() {
    let clear = Image::filled(PhysicalSize::new(20, 20), Rgba8::new(0, 0, 0, 0));
    let white = Image::filled(PhysicalSize::new(20, 20), Rgba8::rgb(255, 255, 255));
    let red = Rgba8::new(255, 0, 0, 128);
    let stroke = || [(line(0.0, 10.0, 20.0, 10.0), style(red, 8.0))];

    // Over nothing, the color itself: straight alpha, not darkened.
    let over_clear = flattened(clear, stroke()).pixel(10, 10).unwrap();
    assert_eq!(over_clear, red);
    // Over white, half of each.
    let over_white = flattened(white.clone(), stroke()).pixel(10, 10).unwrap();
    assert_eq!(over_white, Rgba8::rgb(255, 127, 127));
    // Two coats over white: the second blends over the first.
    let twice = flattened(white, stroke().into_iter().chain(stroke()))
        .pixel(10, 10)
        .unwrap();
    assert_eq!(twice, Rgba8::rgb(255, 63, 63));
}

#[test]
fn later_annotations_are_drawn_over_earlier_ones() {
    let image = base(20, 20);
    let green = Rgba8::rgb(0, 200, 0);
    let result = flattened(
        image,
        [
            (line(0.0, 10.0, 20.0, 10.0), style(BLUE, 6.0)),
            (line(10.0, 0.0, 10.0, 20.0), style(green, 6.0)),
        ],
    );
    assert_eq!(result.pixel(10, 10), Some(green));
    assert_eq!(result.pixel(2, 10), Some(BLUE));
}

#[test]
fn text_is_drawn_in_its_color_only_around_its_layout_box() {
    let image = base(160, 60);
    let style = Style {
        color: BLUE,
        font_size: 36.0,
        ..Style::default()
    };
    let position = Point::new(10.25, 6.5);
    let content = "HIT\nll";
    let result = flattened(
        image.clone(),
        [(Shape::Text(Text::new(position, content)), style)],
    );
    let size = font::measure(content, style.font_size);
    // Glyph ink can overhang the advance box a little; nothing reaches
    // further than this.
    let reach = Rect::new(position, size).expand(style.font_size * 0.1);
    let mut solid = 0;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = result.pixel(x, y).unwrap();
            if !reach.contains(Point::new(x as f32 + 0.5, y as f32 + 0.5)) {
                assert_eq!(pixel, image.pixel(x, y).unwrap(), "at ({x}, {y})");
            }
            solid += usize::from(pixel == BLUE);
        }
    }
    // The bold stems of "HIT" and "ll" fully cover many pixels.
    assert!(solid > 150, "{solid} pixels in the text color");
}

#[test]
fn an_empty_image_flattens_to_itself() {
    let empty = Image::filled(PhysicalSize::new(0, 0), BLUE);
    let result = flattened(
        empty.clone(),
        [(line(0.0, 0.0, 5.0, 5.0), style(BLUE, 4.0))],
    );
    assert!(result == empty);
}

mod canvas {
    //! Flatten against the canvas itself, drawn headlessly by iced's
    //! tiny-skia renderer at 1:1.

    use iced::advanced::renderer::{self, Headless};
    use iced::futures::executor::block_on;
    use iced::{mouse, Color, Renderer, Theme};
    use iced_runtime::user_interface::{self, UserInterface};

    use super::*;
    use crate::canvas::MARGIN;
    use crate::editor::testing::{self, at, click, drag, named, type_text};
    use crate::tools::ToolKind;
    use crate::{Editor, Message};

    /// The canvas at [`testing::CANVAS`], cut down to the image (which the
    /// fitted view shows 1:1, [`MARGIN`] from the canvas's corner).
    fn canvas_image(editor: &Editor) -> Image {
        let mut renderer = block_on(<Renderer as Headless>::new(
            iced::Font::DEFAULT,
            iced::Pixels(16.0),
            Some("tiny-skia"),
        ))
        .expect("a tiny-skia renderer");
        let mut ui = UserInterface::build(
            crate::canvas::view(editor),
            testing::CANVAS,
            user_interface::Cache::default(),
            &mut renderer,
        );
        ui.draw(
            &mut renderer,
            &Theme::Dark,
            &renderer::Style {
                text_color: Color::WHITE,
            },
            mouse::Cursor::Unavailable,
        );
        let canvas_width = testing::CANVAS.width as u32;
        let size = iced::Size::new(canvas_width, testing::CANVAS.height as u32);
        let screenshot = renderer.screenshot(size, 1.0, Color::BLACK);
        let base = editor.document().base();
        let margin = MARGIN as u32;
        Image::from_fn(base.size(), |x, y| {
            let i = (((y + margin) * canvas_width + x + margin) * 4) as usize;
            let [r, g, b, a] = screenshot[i..i + 4] else {
                unreachable!()
            };
            Rgba8::new(r, g, b, a)
        })
    }

    /// Ends a gesture's aftermath: the new annotation is selected, and a
    /// style change would restyle it.
    fn deselect(editor: &mut Editor) {
        named(editor, iced::keyboard::key::Named::Escape);
        assert_eq!(editor.document().selected().count(), 0);
    }

    fn draw(
        editor: &mut Editor,
        tool: ToolKind,
        color: Rgba8,
        width: f32,
        from: iced::Point,
        to: iced::Point,
    ) {
        editor.update(Message::Color(color));
        editor.update(Message::StrokeWidth(width));
        editor.update(Message::Tool(tool));
        drag(editor, from, to);
        deselect(editor);
    }

    #[test]
    fn flatten_matches_the_canvas_at_actual_size() {
        // iced's tiny-skia renderer draws the canvas's base image one pixel
        // left of where it belongs: it rotates the image's corner around its
        // center by -2π in f32 (`Rectangle::with_vertices`), getting x =
        // 15.99997 instead of 16, and truncates that to an integer. So the
        // base varies only down the image, where the shift doesn't show, and
        // the last column (backdrop on the canvas) is not compared.
        let mut editor = Editor::new(Image::from_fn(PhysicalSize::new(400, 300), |_, y| {
            Rgba8::rgb((y * 7 % 256) as u8, (255 - y * 3 / 4) as u8, 200)
        }));
        let yellow = Rgba8::rgb(255, 214, 10);
        let translucent = Rgba8::new(40, 220, 90, 150);
        draw(
            &mut editor,
            ToolKind::Line,
            yellow,
            7.0,
            at(20.0, 30.5),
            at(180.25, 90.0),
        );
        draw(
            &mut editor,
            ToolKind::Rectangle,
            Rgba8::rgb(255, 59, 48),
            5.0,
            at(60.0, 40.0),
            at(240.5, 160.0),
        );
        draw(
            &mut editor,
            ToolKind::Arrow,
            translucent,
            12.0,
            at(30.0, 250.0),
            at(300.0, 120.0),
        );

        editor.update(Message::Color(Rgba8::rgb(250, 250, 250)));
        editor.update(Message::FontSize(30.0));
        editor.update(Message::Tool(ToolKind::Text));
        click(&mut editor, at(150.3, 180.6));
        type_text(&mut editor, "Flatten Åg\nmatches!");
        named(&mut editor, iced::keyboard::key::Named::Escape);
        deselect(&mut editor);
        // A translucent stroke over the text, so z-order across the canvas's
        // layers is compared too.
        draw(
            &mut editor,
            ToolKind::Line,
            translucent,
            10.0,
            at(140.0, 200.0),
            at(380.0, 230.0),
        );
        assert_eq!(editor.document().annotations().len(), 5);

        let flat = flatten(editor.document()).unwrap();
        let canvas = canvas_image(&editor);
        let (mut worst, mut differ) = (0, 0);
        for y in 0..flat.height() {
            for x in 0..flat.width() - 1 {
                let (a, b) = (flat.pixel(x, y).unwrap(), canvas.pixel(x, y).unwrap());
                let diff = a
                    .to_array()
                    .iter()
                    .zip(b.to_array())
                    .map(|(a, b)| a.abs_diff(b))
                    .max()
                    .unwrap();
                worst = worst.max(diff);
                differ += usize::from(diff > 2);
            }
        }
        // Rounding in blending leaves every pixel within 2; the stroker's
        // float math, run at different offsets, can shade a stray edge pixel
        // (measured: one, by 11) differently.
        assert!(
            worst <= 16 && differ <= 8,
            "worst channel difference {worst}, {differ} pixels differ by more than 2"
        );
    }
}
