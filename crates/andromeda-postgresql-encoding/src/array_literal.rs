//! PostgreSQL array literals, the text a statement casts to an integer or a
//! text array.
//!
//! A set-write binds one array per row, and the arrays differ in length, so
//! they cannot travel as one two-dimensional array parameter: each row's array
//! is carried as one text element instead. Of the text forms PostgreSQL can
//! turn into an array, its own array literal (`{3,11,11}`) is the one the
//! server reads with the array type's input function alone, several times
//! faster per row than expanding a JSON array element by element. Integers need
//! no quoting or escaping in it, so an integer literal is exact by
//! construction; a text literal quotes every element and escapes the two
//! characters a quoted element gives meaning to, `"` and `\`, so any text,
//! including one holding a comma, a brace or the word `NULL`, reads back as
//! written.

/// An array literal of integers, `{3,11,11}`, with the number of elements it
/// holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegerArrayLiteral {
    /// The literal text.
    text: String,
    /// How many elements the literal holds.
    len: usize,
}

impl IntegerArrayLiteral {
    /// The literal of `values`, in the order given.
    pub fn of(values: impl IntoIterator<Item = i64>) -> IntegerArrayLiteral {
        let mut text = String::from("{");
        let mut len = 0;
        for value in values {
            if len > 0 {
                text.push(',');
            }
            text.push_str(&value.to_string());
            len += 1;
        }
        text.push('}');
        IntegerArrayLiteral { text, len }
    }

    /// The literal text, as a statement binds it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether the literal holds no element.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// An array literal of text, `{"a","b,c"}`, every element quoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextArrayLiteral {
    /// The literal text.
    text: String,
}

impl TextArrayLiteral {
    /// The literal of `values`, in the order given.
    pub fn of<'value>(values: impl IntoIterator<Item = &'value str>) -> TextArrayLiteral {
        let mut text = String::from("{");
        for (index, value) in values.into_iter().enumerate() {
            if index > 0 {
                text.push(',');
            }
            text.push('"');
            for character in value.chars() {
                if matches!(character, '"' | '\\') {
                    text.push('\\');
                }
                text.push(character);
            }
            text.push('"');
        }
        text.push('}');
        TextArrayLiteral { text }
    }

    /// The literal text, as a statement binds it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

#[cfg(test)]
mod tests {
    use crate::array_literal::{IntegerArrayLiteral, TextArrayLiteral};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_literal_lists_its_elements_in_order() {
        let literal = IntegerArrayLiteral::of([3, 11, 11]);
        assert_eq!(literal.as_str(), "{3,11,11}");
        assert!(!literal.is_empty());
    }

    #[test]
    fn an_empty_literal_is_the_empty_array() {
        let literal = IntegerArrayLiteral::of([]);
        assert_eq!(literal.as_str(), "{}");
        assert!(literal.is_empty());
    }

    #[test]
    fn negative_and_wide_integers_are_written_exactly() {
        assert_eq!(IntegerArrayLiteral::of([-1, i64::from(i32::MAX)]).as_str(), "{-1,2147483647}");
    }

    #[test]
    fn a_text_literal_quotes_every_element_in_order() {
        assert_eq!(
            TextArrayLiteral::of(["Building", "https://example.org/Shop"]).as_str(),
            r#"{"Building","https://example.org/Shop"}"#
        );
        assert_eq!(TextArrayLiteral::of([]).as_str(), "{}");
    }

    #[test]
    fn a_text_literal_keeps_commas_braces_and_null_as_text() {
        assert_eq!(TextArrayLiteral::of(["a,b", "{c}", "NULL", ""]).as_str(), r#"{"a,b","{c}","NULL",""}"#);
    }

    #[test]
    fn a_text_literal_escapes_quotes_and_backslashes() {
        assert_eq!(TextArrayLiteral::of([r#"say "hi""#, r"back\slash"]).as_str(), r#"{"say \"hi\"","back\\slash"}"#);
    }
}
