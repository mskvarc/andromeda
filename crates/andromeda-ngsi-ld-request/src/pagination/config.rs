//! Operator configuration of collection pagination (`[pagination]`).

use crate::pagination::paging_mode::PagingMode;
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// The `[pagination]` section: how a request that names neither `offset` nor
/// `cursor` is paged.
///
/// Whether opaque cursors are available at all is the `cursor_pagination`
/// extension switch; the two together resolve to the broker's pagination
/// policy at startup.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaginationConfig {
    /// The pointers a fresh request is paged by: `cursor` (clause 5.5.9.1
    /// opaque pointers) or `offset` (clause 5.5.9.2 `limit`/`offset`).
    ///
    /// `cursor` requires `extensions.cursor_pagination = "enabled"`.
    #[toml_comment(inline)]
    pub default_mode: PagingMode,
}

#[cfg(test)]
mod tests {
    use crate::pagination::{config::PaginationConfig, paging_mode::PagingMode};
    use pretty_assertions::assert_eq;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Section {
        pagination: PaginationConfig,
    }

    /// A broker with no `[pagination]` section walks by cursor.
    #[test]
    fn pagination_config_defaults_to_cursor() {
        assert_eq!(PaginationConfig::default().default_mode, PagingMode::Cursor);
    }

    /// The mode is read in kebab case.
    #[test]
    fn default_mode_reads_kebab_case() {
        let section: Section = toml::from_str("[pagination]\ndefault_mode = \"offset\"\n").expect("valid section");
        assert_eq!(section.pagination.default_mode, PagingMode::Offset);
    }

    /// A mode clause 5.5.9 does not define is refused.
    #[test]
    fn an_unknown_mode_is_refused() {
        assert!(toml::from_str::<Section>("[pagination]\ndefault_mode = \"page\"\n").is_err());
    }
}
