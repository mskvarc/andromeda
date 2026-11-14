//! The two columns a `current_attributes` row holds its String field in, and
//! the two it holds its URI field in.
//!
//! A B-tree keys `value_string` and `value_uri`, and refuses an entry past
//! 2704 bytes, so a String or a Relationship object past the bound the schema
//! sets moves to the field's overflow column under the same value kind. The
//! placement is decided from the length alone, so the row writers and the `q`
//! reader, which routes an equality to the column its operand would be in,
//! agree on it.

use andromeda_postgresql_schema::{string_value_column::StringValueColumn, uri_value_column::UriValueColumn};

/// A row's content of the String field or of the URI field, in the column its
/// length places it in: at most one of the two is set.
#[derive(Debug, Default, PartialEq)]
pub struct TextColumns {
    /// The column a B-tree keys: `value_string` or `value_uri`.
    pub indexed: Option<String>,
    /// The overflow column past the bound: `value_long_text` or
    /// `value_long_uri`.
    pub overflow: Option<String>,
}

impl TextColumns {
    /// The String field holding `text`.
    #[must_use]
    pub fn of_string(text: String) -> TextColumns {
        match StringValueColumn::of(&text) {
            StringValueColumn::Indexed => TextColumns::indexed(text),
            StringValueColumn::Overflow => TextColumns::overflow(text),
        }
    }

    /// The URI field holding `uri`.
    #[must_use]
    pub fn of_uri(uri: String) -> TextColumns {
        match UriValueColumn::of(&uri) {
            UriValueColumn::Indexed => TextColumns::indexed(uri),
            UriValueColumn::Overflow => TextColumns::overflow(uri),
        }
    }

    /// `text` in the indexed column.
    const fn indexed(text: String) -> TextColumns {
        TextColumns {
            indexed: Some(text),
            overflow: None,
        }
    }

    /// `text` in the overflow column.
    const fn overflow(text: String) -> TextColumns {
        TextColumns {
            indexed: None,
            overflow: Some(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::current_attribute::text_columns::TextColumns;
    use andromeda_postgresql_schema::{string_value_column::INDEXED_STRING_BYTE_LIMIT, uri_value_column::INDEXED_URI_BYTE_LIMIT};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_string_within_the_bound_is_held_in_the_indexed_column() {
        let text = "a".repeat(INDEXED_STRING_BYTE_LIMIT);
        assert_eq!(
            TextColumns::of_string(text.clone()),
            TextColumns {
                indexed: Some(text),
                overflow: None
            }
        );
    }

    #[test]
    fn a_string_past_the_bound_is_held_in_the_overflow_column() {
        let text = "a".repeat(INDEXED_STRING_BYTE_LIMIT + 1);
        assert_eq!(
            TextColumns::of_string(text.clone()),
            TextColumns {
                indexed: None,
                overflow: Some(text)
            }
        );
    }

    #[test]
    fn a_uri_is_placed_by_its_own_bound() {
        let prefix = "urn:ngsi-ld:Building:";
        let at_limit = format!("{prefix}{}", "a".repeat(INDEXED_URI_BYTE_LIMIT - prefix.len()));
        let past = format!("{at_limit}a");
        assert_eq!(
            TextColumns::of_uri(at_limit.clone()),
            TextColumns {
                indexed: Some(at_limit),
                overflow: None
            }
        );
        assert_eq!(
            TextColumns::of_uri(past.clone()),
            TextColumns {
                indexed: None,
                overflow: Some(past)
            }
        );
    }
}
