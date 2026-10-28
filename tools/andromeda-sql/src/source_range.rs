//! A stretch of a migration's text, given as line and column positions.
//!
//! Lines and columns count from zero, and a column is a UTF-8 byte offset
//! within its line, which is how Squawk reports a position. The end is
//! exclusive.

use std::{iter, ops::Range};

/// One position in a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Display)]
#[display("{}:{}", line + 1, column + 1)]
pub struct LineColumn {
    /// The line, counted from zero.
    line: usize,
    /// The byte offset within the line, counted from zero.
    column: usize,
}

impl LineColumn {
    /// The position at `column` bytes into line `line`, both counted from zero.
    #[must_use]
    pub const fn new(line: usize, column: usize) -> LineColumn {
        LineColumn { line, column }
    }

    /// The position of byte `offset` of `text`; an offset past the end is the
    /// end of the text.
    #[must_use]
    pub fn of_offset(text: &str, offset: usize) -> LineColumn {
        let offset = offset.min(text.len());
        let before = &text[..floor_char_boundary(text, offset)];
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
        LineColumn {
            line: before.matches('\n').count(),
            column: offset - line_start,
        }
    }

    /// The byte offset of the position in `text`.
    ///
    /// A line past the end of the text is its end, and a column past the end
    /// of its line is that line's end; an offset inside a multi-byte character
    /// moves back to the character's start, so the offset always slices `text`.
    fn offset_in(self, text: &str) -> usize {
        let Some(line_start) = line_starts(text).nth(self.line) else {
            return text.len();
        };
        let line_end = text[line_start..].find('\n').map_or(text.len(), |length| line_start + length);
        floor_char_boundary(text, (line_start + self.column).min(line_end))
    }
}

/// A stretch of text from one position up to, not including, another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    /// Where the stretch starts.
    start: LineColumn,
    /// Where the stretch ends, exclusive.
    end: LineColumn,
}

impl SourceRange {
    /// The stretch from `start` up to `end`; an `end` before `start` is read as `start`.
    #[must_use]
    pub fn new(start: LineColumn, end: LineColumn) -> SourceRange {
        SourceRange { start, end: end.max(start) }
    }

    /// The stretch `bytes` of `text` covers.
    #[must_use]
    pub fn of_bytes(text: &str, bytes: Range<usize>) -> SourceRange {
        SourceRange::new(LineColumn::of_offset(text, bytes.start), LineColumn::of_offset(text, bytes.end))
    }

    /// Where the stretch starts.
    #[must_use]
    pub const fn start(&self) -> LineColumn {
        self.start
    }

    /// The bytes of `text` the stretch covers.
    #[must_use]
    pub fn byte_range(&self, text: &str) -> Range<usize> {
        self.start.offset_in(text)..self.end.offset_in(text)
    }
}

/// The byte offset every line of `text` starts at.
fn line_starts(text: &str) -> impl Iterator<Item = usize> {
    iter::once(0).chain(text.match_indices('\n').map(|(newline, _)| newline + 1))
}

/// `offset`, moved back to the start of the character it falls inside.
fn floor_char_boundary(text: &str, offset: usize) -> usize {
    (0..=offset).rev().find(|candidate| text.is_char_boundary(*candidate)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crate::source_range::{LineColumn, SourceRange};
    use pretty_assertions::assert_eq;

    const TEXT: &str = "-- čšž\ncreate table t (\"čč\" text, b int);\n";

    #[test]
    fn a_column_is_a_byte_offset_within_its_line() {
        let range = SourceRange::new(LineColumn::new(1, 31), LineColumn::new(1, 34));
        assert_eq!(&TEXT[range.byte_range(TEXT)], "int");
    }

    #[test]
    fn a_range_may_span_lines() {
        let range = SourceRange::new(LineColumn::new(0, 3), LineColumn::new(1, 6));
        assert_eq!(&TEXT[range.byte_range(TEXT)], "čšž\ncreate");
    }

    #[test]
    fn positions_past_the_text_are_clamped_to_it() {
        let range = SourceRange::new(LineColumn::new(1, 37), LineColumn::new(9, 0));
        assert_eq!(range.byte_range(TEXT), TEXT.len() - 1..TEXT.len());
    }

    #[test]
    fn an_offset_inside_a_character_moves_to_its_start() {
        let range = SourceRange::new(LineColumn::new(0, 4), LineColumn::new(0, 5));
        assert_eq!(&TEXT[range.byte_range(TEXT)], "č");
    }

    #[test]
    fn an_end_before_the_start_is_an_empty_range() {
        let range = SourceRange::new(LineColumn::new(1, 6), LineColumn::new(1, 2));
        assert_eq!(range.byte_range(TEXT).len(), 0);
    }

    #[test]
    fn a_byte_offset_reads_back_as_its_line_and_column() {
        let offset = TEXT.find("int").unwrap_or_default();
        assert_eq!(LineColumn::of_offset(TEXT, offset), LineColumn::new(1, 31));
        assert_eq!(SourceRange::of_bytes(TEXT, offset..offset + 3).byte_range(TEXT), offset..offset + 3);
    }

    #[test]
    fn a_position_is_written_one_based() {
        assert_eq!(LineColumn::new(1, 31).to_string(), "2:32");
    }
}
