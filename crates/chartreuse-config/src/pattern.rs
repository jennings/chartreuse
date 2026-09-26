//! File name patterns: the stem of a saved file, with date and time tokens.
//!
//! A pattern is literal text with tokens in braces, expanded with the time of
//! the capture:
//!
//! | Token    | Expands to            | Example      |
//! |----------|-----------------------|--------------|
//! | `{date}` | `{yyyy}-{MM}-{dd}`    | `2026-09-25` |
//! | `{time}` | `{HH}.{mm}.{ss}`      | `14.03.07`   |
//! | `{yyyy}` | the four-digit year   | `2026`       |
//! | `{MM}`   | the month, `01`–`12`  | `09`         |
//! | `{dd}`   | the day, `01`–`31`    | `25`         |
//! | `{HH}`   | the hour, `00`–`23`   | `14`         |
//! | `{mm}`   | the minute, `00`–`59` | `03`         |
//! | `{ss}`   | the second, `00`–`59` | `07`         |
//!
//! Tokens are case-sensitive (`{MM}` is the month, `{mm}` the minute). `{{`
//! and `}}` stand for literal braces. [`DEFAULT_PATTERN`] gives names such as
//! `Chartreuse 2026-09-25 at 14.03.07`; `{time}` uses dots because macOS
//! Finder shows `:` in file names as `/`.
//!
//! A pattern names a file, not a path, and must give a valid file name on
//! every platform Chartreuse supports, so settings files can move between
//! them: it may not contain `/ \ : * ? " < > |` or control characters, start
//! with `.` (a hidden file), or end with `.` or a space (Windows drops them).
//!
//! [`FileNamePattern::file_name`] appends the format's extension.
//! [`candidate_names`] and [`unique_file_name`] add a ` (2)`, ` (3)`, …
//! suffix when a name is taken.

use std::fmt;
use std::str::FromStr;

use chrono::{Datelike as _, NaiveDateTime, Timelike as _};
use serde::{Deserialize, Serialize};

use crate::format::SaveFormat;

/// The default pattern: `Chartreuse 2026-09-25 at 14.03.07`.
pub const DEFAULT_PATTERN: &str = "Chartreuse {date} at {time}";

/// How many names [`candidate_names`] offers before giving up: the plain name,
/// then suffixes ` (2)` through ` (10000)`.
pub const MAX_CANDIDATES: u32 = 10_000;

/// Characters no file name may contain on at least one supported platform.
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// A date or time token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Date,
    Time,
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

impl Token {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "date" => Self::Date,
            "time" => Self::Time,
            "yyyy" => Self::Year,
            "MM" => Self::Month,
            "dd" => Self::Day,
            "HH" => Self::Hour,
            "mm" => Self::Minute,
            "ss" => Self::Second,
            _ => return None,
        })
    }

    fn expand(self, time: NaiveDateTime, out: &mut String) {
        use fmt::Write as _;
        // Writing to a String cannot fail.
        let _ = match self {
            Self::Date => write!(
                out,
                "{:04}-{:02}-{:02}",
                time.year(),
                time.month(),
                time.day()
            ),
            Self::Time => write!(
                out,
                "{:02}.{:02}.{:02}",
                time.hour(),
                time.minute(),
                time.second()
            ),
            Self::Year => write!(out, "{:04}", time.year()),
            Self::Month => write!(out, "{:02}", time.month()),
            Self::Day => write!(out, "{:02}", time.day()),
            Self::Hour => write!(out, "{:02}", time.hour()),
            Self::Minute => write!(out, "{:02}", time.minute()),
            Self::Second => write!(out, "{:02}", time.second()),
        };
    }
}

/// A piece of a parsed pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Token(Token),
}

/// Why a file name pattern was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    /// The pattern is empty.
    Empty,
    /// `{name}` is not one of the documented tokens.
    UnknownToken(String),
    /// A `{` has no matching `}`.
    UnclosedBrace,
    /// A `}` has no matching `{` (write `}}` for a literal brace).
    UnmatchedBrace,
    /// The character is not allowed in file names on some platform.
    ForbiddenChar(char),
    /// The name would start with `.`, which hides the file.
    LeadingDot,
    /// The name would end with `.` or whitespace, which Windows drops.
    TrailingDotOrSpace,
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the file name pattern is empty"),
            Self::UnknownToken(name) => write!(
                f,
                "{{{name}}} is not a file name token (use {{date}}, {{time}}, {{yyyy}}, {{MM}}, \
                 {{dd}}, {{HH}}, {{mm}} or {{ss}})"
            ),
            Self::UnclosedBrace => f.write_str("a `{` in the file name pattern is never closed"),
            Self::UnmatchedBrace => f.write_str(
                "a `}` in the file name pattern has no matching `{` (write `}}` for a brace)",
            ),
            Self::ForbiddenChar(c) if c.is_control() => {
                write!(f, "file names cannot contain the control character {c:?}")
            }
            Self::ForbiddenChar(c) => write!(f, "file names cannot contain `{c}`"),
            Self::LeadingDot => f.write_str("file names cannot start with `.`"),
            Self::TrailingDotOrSpace => f.write_str("file names cannot end with `.` or a space"),
        }
    }
}

impl std::error::Error for PatternError {}

impl From<PatternError> for chartreuse_core::Error {
    fn from(error: PatternError) -> Self {
        Self::Config(error.to_string())
    }
}

/// A validated file name pattern (see the [module docs](self)).
///
/// `Display` and [`FromStr`] use the pattern text, exactly as written, and so
/// does the settings file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FileNamePattern {
    text: String,
    segments: Vec<Segment>,
}

impl FileNamePattern {
    /// Parses and validates `text`.
    ///
    /// # Errors
    ///
    /// A [`PatternError`] saying what is wrong with it.
    pub fn new(text: impl Into<String>) -> Result<Self, PatternError> {
        let text = text.into();
        let segments = parse(&text)?;
        Ok(Self { text, segments })
    }

    /// The pattern as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The file stem for a capture taken at local time `time`.
    #[must_use]
    pub fn expand(&self, time: NaiveDateTime) -> String {
        let mut stem = String::with_capacity(self.text.len() + 16);
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => stem.push_str(text),
                Segment::Token(token) => token.expand(time, &mut stem),
            }
        }
        stem
    }

    /// The file name, with `format`'s extension, for a capture taken at local
    /// time `time`: for the default pattern and PNG,
    /// `Chartreuse 2026-09-25 at 14.03.07.png`.
    #[must_use]
    pub fn file_name(&self, time: NaiveDateTime, format: SaveFormat) -> String {
        let mut name = self.expand(time);
        name.push('.');
        name.push_str(format.extension());
        name
    }
}

impl Default for FileNamePattern {
    fn default() -> Self {
        Self::new(DEFAULT_PATTERN).expect("the default pattern is valid")
    }
}

impl fmt::Display for FileNamePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl FromStr for FileNamePattern {
    type Err = PatternError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl TryFrom<String> for FileNamePattern {
    type Error = PatternError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

impl From<FileNamePattern> for String {
    fn from(pattern: FileNamePattern) -> Self {
        pattern.text
    }
}

fn parse(text: &str) -> Result<Vec<Segment>, PatternError> {
    if text.is_empty() {
        return Err(PatternError::Empty);
    }
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => return Err(PatternError::UnclosedBrace),
                    }
                }
                let token = Token::from_name(&name).ok_or(PatternError::UnknownToken(name))?;
                if !literal.is_empty() {
                    segments.push(Segment::Literal(std::mem::take(&mut literal)));
                }
                segments.push(Segment::Token(token));
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '}' => return Err(PatternError::UnmatchedBrace),
            c if c.is_control() || FORBIDDEN.contains(&c) => {
                return Err(PatternError::ForbiddenChar(c));
            }
            c => literal.push(c),
        }
    }
    if !literal.is_empty() {
        segments.push(Segment::Literal(literal));
    }
    // Tokens expand to digits at both ends, so only literal ends can break these.
    if let Some(Segment::Literal(first)) = segments.first()
        && first.starts_with('.')
    {
        return Err(PatternError::LeadingDot);
    }
    if let Some(Segment::Literal(last)) = segments.last()
        && last.ends_with(|c: char| c == '.' || c.is_whitespace())
    {
        return Err(PatternError::TrailingDotOrSpace);
    }
    Ok(segments)
}

/// The names to try, in order, when saving as `stem` with `extension`:
/// `stem.ext`, then `stem (2).ext`, `stem (3).ext`, …, [`MAX_CANDIDATES`] in
/// all.
///
/// Use this with exclusive file creation (`create_new`) to pick a free name
/// without a race; [`unique_file_name`] is the same search over a predicate.
pub fn candidate_names<'a>(stem: &'a str, extension: &'a str) -> impl Iterator<Item = String> + 'a {
    (1..=MAX_CANDIDATES).map(move |attempt| {
        if attempt == 1 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem} ({attempt}).{extension}")
        }
    })
}

/// The first of [`candidate_names`] for which `exists` is false, or `None`
/// if every candidate is taken.
pub fn unique_file_name(
    stem: &str,
    extension: &str,
    mut exists: impl FnMut(&str) -> bool,
) -> Option<String> {
    candidate_names(stem, extension).find(|name| !exists(name))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::NaiveDate;

    use super::*;

    fn time(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, s)
            .unwrap()
    }

    #[test]
    fn the_default_reproduces_the_quick_export_name() {
        let pattern = FileNamePattern::default();
        assert_eq!(pattern.as_str(), DEFAULT_PATTERN);
        assert_eq!(
            pattern.expand(time(2026, 9, 25, 14, 3, 7)),
            "Chartreuse 2026-09-25 at 14.03.07"
        );
        assert_eq!(
            pattern.file_name(time(2026, 9, 5, 4, 3, 7), SaveFormat::Png),
            "Chartreuse 2026-09-05 at 04.03.07.png"
        );
    }

    #[test]
    fn every_token_expands_zero_padded() {
        let pattern = FileNamePattern::new("{yyyy}{MM}{dd}-{HH}{mm}{ss} {date}_{time}").unwrap();
        assert_eq!(
            pattern.expand(time(2026, 1, 2, 3, 4, 5)),
            "20260102-030405 2026-01-02_03.04.05"
        );
    }

    #[test]
    fn tokens_are_case_sensitive() {
        let pattern = FileNamePattern::new("{MM}-{mm}").unwrap();
        assert_eq!(pattern.expand(time(2026, 12, 1, 0, 59, 0)), "12-59");
        assert_eq!(
            FileNamePattern::new("{DATE}"),
            Err(PatternError::UnknownToken("DATE".into()))
        );
    }

    #[test]
    fn doubled_braces_are_literal() {
        let pattern = FileNamePattern::new("{{{yyyy}}} }}{{").unwrap();
        assert_eq!(pattern.expand(time(2026, 1, 1, 0, 0, 0)), "{2026} }{");
        assert_eq!(pattern.to_string(), "{{{yyyy}}} }}{{");
    }

    #[test]
    fn a_pattern_without_tokens_is_constant() {
        let pattern = FileNamePattern::new("Screenshot").unwrap();
        assert_eq!(pattern.expand(time(2026, 1, 1, 0, 0, 0)), "Screenshot");
    }

    #[test]
    fn malformed_patterns_are_rejected() {
        let error = |text: &str| FileNamePattern::new(text).unwrap_err();
        assert_eq!(error(""), PatternError::Empty);
        assert_eq!(
            error("Shot {week}"),
            PatternError::UnknownToken("week".into())
        );
        assert_eq!(error("Shot {}"), PatternError::UnknownToken(String::new()));
        assert_eq!(error("Shot {date"), PatternError::UnclosedBrace);
        assert_eq!(error("Shot date}"), PatternError::UnmatchedBrace);
    }

    #[test]
    fn path_separators_and_other_unportable_characters_are_rejected() {
        for c in [
            '/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0', '\n', '\t',
        ] {
            assert_eq!(
                FileNamePattern::new(format!("Shot{c}{{date}}")),
                Err(PatternError::ForbiddenChar(c)),
                "{c:?}"
            );
        }
        assert_eq!(
            FileNamePattern::new("../{date}"),
            Err(PatternError::ForbiddenChar('/'))
        );
    }

    #[test]
    fn hidden_and_windows_truncated_names_are_rejected() {
        assert_eq!(
            FileNamePattern::new(".{date}"),
            Err(PatternError::LeadingDot)
        );
        assert_eq!(
            FileNamePattern::new("{date}."),
            Err(PatternError::TrailingDotOrSpace)
        );
        assert_eq!(
            FileNamePattern::new("{date} "),
            Err(PatternError::TrailingDotOrSpace)
        );
        // Dots and spaces inside the name are fine.
        assert!(FileNamePattern::new("Shot. {date} .v2").is_ok());
    }

    #[test]
    fn pattern_errors_become_config_errors() {
        let error: chartreuse_core::Error = PatternError::ForbiddenChar('/').into();
        assert!(matches!(error, chartreuse_core::Error::Config(_)));
        assert!(error.to_string().contains('/'), "{error}");
    }

    #[test]
    fn a_free_name_is_used_as_is() {
        assert_eq!(
            unique_file_name("Shot", "png", |_| false).as_deref(),
            Some("Shot.png")
        );
    }

    #[test]
    fn taken_names_get_the_first_free_number() {
        let taken: HashSet<&str> = ["Shot.png", "Shot (2).png", "Shot (4).png"].into();
        assert_eq!(
            unique_file_name("Shot", "png", |name| taken.contains(name)).as_deref(),
            Some("Shot (3).png")
        );
        // Other extensions do not collide.
        assert_eq!(
            unique_file_name("Shot", "jpg", |name| taken.contains(name)).as_deref(),
            Some("Shot.jpg")
        );
    }

    #[test]
    fn the_search_gives_up_after_the_last_candidate() {
        let mut asked = 0;
        let result = unique_file_name("Shot", "png", |_| {
            asked += 1;
            true
        });
        assert_eq!(result, None);
        assert_eq!(asked, MAX_CANDIDATES);
        assert_eq!(
            candidate_names("Shot", "png").last().as_deref(),
            Some("Shot (10000).png")
        );
    }
}
