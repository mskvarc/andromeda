//! What kind of failure a JSON reader met in a document that is no JSON.

use serde_json::error::Category;
use strum::Display;

/// The kind of failure a JSON reader met, as a refusal names it.
#[derive(Clone, Copy, Debug, Display, PartialEq, Eq)]
pub enum SyntaxCategory {
    /// The text could not be read at all.
    #[strum(serialize = "input")]
    Input,
    /// The text is not JSON.
    #[strum(serialize = "syntax")]
    Syntax,
    /// The JSON is not shaped as the reader expected.
    #[strum(serialize = "data")]
    Data,
    /// The text ended before the JSON did.
    #[strum(serialize = "end-of-input")]
    EndOfInput,
}

impl From<Category> for SyntaxCategory {
    // Hand-written: the category is a foreign enum, and each of its four kinds
    // maps to the one this crate names.
    fn from(category: Category) -> SyntaxCategory {
        match category {
            Category::Io => SyntaxCategory::Input,
            Category::Syntax => SyntaxCategory::Syntax,
            Category::Data => SyntaxCategory::Data,
            Category::Eof => SyntaxCategory::EndOfInput,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::syntax_category::SyntaxCategory;

    #[test]
    fn a_truncated_document_is_named_as_ending_early() {
        let error = serde_json::from_str::<serde_json::Value>("[1, 2").expect_err("truncated");
        pretty_assertions::assert_eq!(SyntaxCategory::from(error.classify()).to_string(), "end-of-input");
    }
}
