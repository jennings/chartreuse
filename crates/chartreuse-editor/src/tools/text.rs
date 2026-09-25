//! The text tool and in-place text editing.
//!
//! Clicking with the text tool opens a [`TextEdit`]: of the text annotation
//! under the click if there is one, otherwise of a new, empty text whose first
//! line is centered on the click. While it is open, typing inserts at the end
//! of the text, Backspace deletes the last character, and Enter starts a new
//! line. Escape, a click elsewhere, or switching tools commits it as one undo
//! step: a new text is added (and selected), an existing one gets its new
//! content ([`Command::EditText`]). Blank text (nothing but whitespace) is
//! discarded: a new blank text adds nothing, and an existing text edited
//! down to blank is deleted.

use iced::mouse::Interaction;
use unicode_segmentation::UnicodeSegmentation;

use super::{Context, Pointer, Preview, Tool, ToolKind};
use crate::model::{
    AnnotationId, Command, Document, Point, Shape, Style, StylePatch, Text, Vector,
};

/// A change to the text being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInput {
    /// Typed text, inserted at the end. Control characters are dropped.
    Insert(String),
    /// Deletes the last character (grapheme cluster).
    Backspace,
    /// Starts a new line.
    Newline,
}

/// Which annotation a [`TextEdit`] edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTarget {
    /// A text annotation that does not exist yet.
    New,
    /// An existing text annotation, which the canvas hides while the edit
    /// shows its new content.
    Existing(AnnotationId),
}

/// Text being edited in place (see the [module docs](self)).
#[derive(Debug, Clone, PartialEq)]
pub struct TextEdit {
    target: TextTarget,
    position: Point,
    content: String,
    style: Style,
}

impl TextEdit {
    /// A new, empty text whose first line is vertically centered on `at`.
    #[must_use]
    pub fn new(at: Point, style: Style) -> Self {
        let half_line = style.font_size.max(0.0) * Text::LINE_HEIGHT / 2.0;
        Self {
            target: TextTarget::New,
            position: at - Vector::new(0.0, half_line),
            content: String::new(),
            style,
        }
    }

    /// An edit of the text annotation `id`, or `None` if `id` is not one.
    #[must_use]
    pub fn existing(document: &Document, id: AnnotationId) -> Option<Self> {
        let annotation = document.get(id)?;
        let Shape::Text(text) = &annotation.shape else {
            return None;
        };
        Some(Self {
            target: TextTarget::Existing(id),
            position: text.position,
            content: text.content.clone(),
            style: annotation.style,
        })
    }

    #[must_use]
    pub const fn target(&self) -> TextTarget {
        self.target
    }

    /// The top-left corner of the text's layout box.
    #[must_use]
    pub const fn position(&self) -> Point {
        self.position
    }

    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// The style the text is shown and committed in.
    #[must_use]
    pub const fn style(&self) -> Style {
        self.style
    }

    /// Whether the text is empty or only whitespace, and so would be
    /// discarded.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.content.trim().is_empty()
    }

    pub fn input(&mut self, input: TextInput) {
        match input {
            TextInput::Insert(text) => self
                .content
                .extend(text.chars().filter(|c| !c.is_control())),
            TextInput::Backspace => {
                if let Some((last, _)) = self.content.grapheme_indices(true).next_back() {
                    self.content.truncate(last);
                }
            }
            TextInput::Newline => self.content.push('\n'),
        }
    }

    /// Changes the style the text is shown in. (For an existing annotation
    /// the editor restyles the annotation itself separately.)
    pub fn restyle(&mut self, patch: &StylePatch) {
        self.style = self.style.patched(patch);
    }

    /// Commits the edit to `document` as at most one undo step (see the
    /// [module docs](self)). Returns the text annotation's id if it exists
    /// afterwards.
    pub fn commit(self, document: &mut Document) -> Option<AnnotationId> {
        let blank = self.is_blank();
        match self.target {
            TextTarget::New if blank => None,
            TextTarget::New => {
                let text = Text::new(self.position, self.content);
                let id = document.add(Shape::Text(text), self.style);
                document.set_selection([id]);
                Some(id)
            }
            TextTarget::Existing(id) if blank => {
                document.apply(Command::Delete { ids: vec![id] });
                None
            }
            TextTarget::Existing(id) => {
                document.apply(Command::EditText {
                    id,
                    content: self.content,
                });
                Some(id)
            }
        }
    }
}

/// The topmost annotation under `at` if it is text.
pub(super) fn text_at(document: &Document, at: Point, tolerance: f32) -> Option<AnnotationId> {
    let id = document.annotation_at(at, tolerance)?;
    matches!(document.get(id)?.shape, Shape::Text(_)).then_some(id)
}

/// Opens an edit for a click at `at`: of the text under it, selected, or of a
/// new text, with nothing selected.
pub(super) fn open(at: Point, cx: &mut Context<'_>) -> TextEdit {
    let existing =
        text_at(cx.document, at, cx.tolerance()).and_then(|id| TextEdit::existing(cx.document, id));
    match existing {
        Some(edit) => {
            if let TextTarget::Existing(id) = edit.target {
                cx.document.set_selection([id]);
            }
            edit
        }
        None => {
            cx.document.clear_selection();
            TextEdit::new(at, cx.style)
        }
    }
}

/// The text tool.
#[derive(Debug, Default)]
pub struct TextTool {
    edit: Option<TextEdit>,
}

impl Tool for TextTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Text
    }

    fn pointer(&mut self, pointer: Pointer, cx: &mut Context<'_>) {
        if let Pointer::Press { at, .. } = pointer {
            self.finish(cx);
            self.edit = Some(open(at, cx));
        }
    }

    fn escape(&mut self, cx: &mut Context<'_>) -> bool {
        let open = self.edit.is_some();
        self.finish(cx);
        open
    }

    fn finish(&mut self, cx: &mut Context<'_>) {
        if let Some(edit) = self.edit.take() {
            edit.commit(cx.document);
        }
    }

    fn is_active(&self) -> bool {
        self.edit.is_some()
    }

    fn preview(&self) -> Preview<'_> {
        self.edit.as_ref().map_or(Preview::None, Preview::Text)
    }

    fn text_edit(&mut self) -> Option<&mut TextEdit> {
        self.edit.as_mut()
    }

    fn cursor(&self, _document: &Document, _at: Point, _pixel: f32) -> Interaction {
        Interaction::Text
    }
}

#[cfg(test)]
mod tests {
    use super::super::drag::testing::{document, send};
    use super::*;

    fn typed(edit: &mut TextEdit, inputs: impl IntoIterator<Item = TextInput>) {
        for input in inputs {
            edit.input(input);
        }
    }

    fn insert(text: &str) -> TextInput {
        TextInput::Insert(text.to_owned())
    }

    #[test]
    fn typing_inserts_backspace_deletes_and_enter_breaks_lines() {
        let mut edit = TextEdit::new(Point::ORIGIN, Style::default());
        typed(
            &mut edit,
            [
                insert("Hi"),
                TextInput::Newline,
                insert("thee"),
                TextInput::Backspace,
            ],
        );
        assert_eq!(edit.content(), "Hi\nthe");
        typed(&mut edit, [insert("re\u{7}\r"), TextInput::Backspace]);
        assert_eq!(edit.content(), "Hi\nther", "control characters are dropped");
        typed(&mut edit, std::iter::repeat_n(TextInput::Backspace, 9));
        assert_eq!(edit.content(), "", "backspace on empty text does nothing");
    }

    #[test]
    fn backspace_deletes_a_whole_grapheme() {
        let mut edit = TextEdit::new(Point::ORIGIN, Style::default());
        typed(&mut edit, [insert("ae\u{301}👍🏽"), TextInput::Backspace]);
        assert_eq!(edit.content(), "ae\u{301}");
        edit.input(TextInput::Backspace);
        assert_eq!(edit.content(), "a");
    }

    #[test]
    fn committing_new_text_adds_one_selected_annotation() {
        let mut document = document();
        let mut edit = TextEdit::new(Point::new(10.0, 50.0), Style::default());
        typed(&mut edit, [insert("Look")]);
        let position = edit.position();
        assert_eq!(position, Point::new(10.0, 50.0 - 24.0 * 1.2 / 2.0));

        let id = edit.commit(&mut document).expect("added");
        let annotation = document.get(id).unwrap();
        assert_eq!(annotation.shape, Shape::Text(Text::new(position, "Look")));
        assert!(document.is_selected(id));
        assert!(document.undo());
        assert!(document.annotations().is_empty());
    }

    #[test]
    fn blank_new_text_is_discarded() {
        let mut document = document();
        let mut edit = TextEdit::new(Point::ORIGIN, Style::default());
        typed(&mut edit, [insert(" "), TextInput::Newline]);
        assert_eq!(edit.commit(&mut document), None);
        assert!(document.annotations().is_empty());
        assert!(!document.can_undo());
    }

    #[test]
    fn editing_existing_text_is_one_edit_text_step_and_blank_deletes_it() {
        let mut document = document();
        let id = document.add(
            Shape::Text(Text::new(Point::ORIGIN, "old")),
            Style::default(),
        );
        let mut edit = TextEdit::existing(&document, id).unwrap();
        typed(&mut edit, [TextInput::Backspace, insert("ne")]);
        assert_eq!(edit.commit(&mut document), Some(id));
        let content = |document: &Document| match &document.get(id).unwrap().shape {
            Shape::Text(text) => text.content.clone(),
            other => panic!("not text: {other:?}"),
        };
        assert_eq!(content(&document), "olne");
        assert!(document.undo());
        assert_eq!(content(&document), "old");

        let mut edit = TextEdit::existing(&document, id).unwrap();
        typed(&mut edit, std::iter::repeat_n(TextInput::Backspace, 3));
        assert_eq!(edit.commit(&mut document), None);
        assert!(document.get(id).is_none(), "edited to blank: deleted");
        assert!(document.undo());
        assert_eq!(content(&document), "old");
    }

    #[test]
    fn the_tool_edits_text_under_the_click_and_commits_on_click_away() {
        let mut document = document();
        let mut tool = TextTool::default();
        let first = Point::new(20.0, 40.0);
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press {
                at: first,
                clicks: 1,
            },
        );
        tool.text_edit().unwrap().input(insert("One"));
        // Clicking elsewhere commits "One" and opens a new edit there.
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press {
                at: Point::new(200.0, 200.0),
                clicks: 1,
            },
        );
        assert_eq!(document.annotations().len(), 1);
        let id = document.annotations()[0].id();
        assert_eq!(tool.text_edit().unwrap().target(), TextTarget::New);
        assert!(document.selection().is_empty());

        // Clicking the committed text discards the blank new edit and
        // reopens "One".
        send(
            &mut tool,
            &mut document,
            false,
            Pointer::Press {
                at: first,
                clicks: 1,
            },
        );
        let edit = tool.text_edit().unwrap();
        assert_eq!(edit.target(), TextTarget::Existing(id));
        assert_eq!(edit.content(), "One");
        assert!(document.is_selected(id));
        assert_eq!(document.annotations().len(), 1);
    }
}
