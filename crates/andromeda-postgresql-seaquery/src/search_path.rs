//! The value of the `search_path` setting: the schemas an unqualified
//! relation name resolves in, first to last.

/// A session's search path, spelled as the setting takes it.
///
/// A search path read back from a session is whatever the session was set to,
/// `"$user", public` unless configured otherwise, so it is kept as the server
/// spells it and only ever set again as it was read.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
#[display("{_0}")]
pub struct SearchPath(String);

impl SearchPath {
    /// The search path that resolves in `schemas`, first to last, each an
    /// unquoted identifier.
    #[must_use]
    pub fn of<'schema>(schemas: impl IntoIterator<Item = &'schema str>) -> SearchPath {
        SearchPath(schemas.into_iter().collect::<Vec<_>>().join(", "))
    }

    /// The search path a session reports, as it spells it.
    #[must_use]
    pub const fn reported(value: String) -> SearchPath {
        SearchPath(value)
    }

    /// The value as the setting takes it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::search_path::SearchPath;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_search_path_lists_its_schemas_in_resolution_order() {
        assert_eq!(SearchPath::of(["snap_7", "public"]).as_str(), "snap_7, public");
        assert_eq!(SearchPath::of(["snap_7", "public"]).to_string(), "snap_7, public");
    }

    #[test]
    fn a_reported_search_path_is_kept_as_the_server_spells_it() {
        assert_eq!(SearchPath::reported("\"$user\", public".to_owned()).as_str(), "\"$user\", public");
    }
}
