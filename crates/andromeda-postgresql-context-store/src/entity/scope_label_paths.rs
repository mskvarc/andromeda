//! The Scope label paths one entity's search row is matched by.

use serde::Serialize;

/// The `ltree` label paths of one entity's Scopes, in `ltree` dot notation.
///
/// Written as the JSON array of path texts the entity statements expand into
/// the `scope_paths` `ltree[]` column of the search row; the Scope itself is
/// read back from the stored document, which keeps the written text (clause
/// 4.6.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ScopeLabelPaths(Vec<String>);

impl ScopeLabelPaths {
    /// The label paths of one entity, one per Scope.
    #[must_use]
    pub const fn new(label_paths: Vec<String>) -> ScopeLabelPaths {
        ScopeLabelPaths(label_paths)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::scope_label_paths::ScopeLabelPaths;

    #[test]
    fn the_label_paths_are_written_as_a_json_array_of_texts() {
        let paths = ScopeLabelPaths::new(vec!["root.a".to_owned(), "Madrid".to_owned()]);
        pretty_assertions::assert_eq!(serde_json::to_string(&paths).expect("the paths serialise"), r#"["root.a","Madrid"]"#);
    }

    #[test]
    fn no_scope_is_written_as_an_empty_array() {
        pretty_assertions::assert_eq!(serde_json::to_string(&ScopeLabelPaths::new(Vec::new())).expect("the paths serialise"), "[]");
    }
}
