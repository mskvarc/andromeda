//! PostgreSQL array literals of integers, the text a statement casts to an
//! integer array.
//!
//! A set-write binds one array per entity, and the arrays differ in length, so
//! they cannot travel as one two-dimensional array parameter: each entity's
//! array is carried as one text element instead. Of the text forms PostgreSQL
//! can turn into an `integer[]`, its own array literal (`{3,11,11}`) is the one
//! the server reads with the array type's input function alone, several times
//! faster per row than expanding a JSON array element by element. Integers need
//! no quoting or escaping in it, so the literal is exact by construction.

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

#[cfg(test)]
mod tests {
    use crate::array_literal::IntegerArrayLiteral;
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
}
