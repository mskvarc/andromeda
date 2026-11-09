//! The `[extensions.cursor_pagination]` table: opaque, keyset-backed
//! pagination pointers.

use crate::schema::feature_mode::FeatureMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How a request that names neither `offset` nor `cursor` is paged while
/// cursors are offered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "How a request that names neither `offset` nor `cursor` is paged.")]
pub enum FreshRequestPaging {
    /// It walks by cursor; `offset` and `cursor=` both remain available.
    #[schemars(description = "It walks by cursor; `offset` and an empty `cursor=` both remain available.")]
    Cursor,
    /// It pages by offset; an empty `cursor=` opts into a walk.
    #[default]
    #[schemars(description = "It pages by offset; an empty `cursor=` opts into a cursor walk.")]
    Offset,
}

/// What a broker's collections serve, as `[extensions.cursor_pagination]`
/// names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum CursorPaginationMode {
    /// Only offsets (clause 5.5.9.2): a `cursor` parameter is refused with
    /// `InvalidRequest`.
    Disabled,
    /// A fresh request pages by offset; an empty `cursor=` opts into a walk.
    Available,
    /// A fresh request walks by cursor; `offset` and `cursor=` both remain
    /// available.
    Default,
}

/// Opaque, keyset-backed pagination pointers (clause 5.5.9.1).
///
/// Collections hand out `prev`/`next`/`last` pointers carrying an opaque
/// `cursor` token and follow them back. Clause 5.5.9.1 permits pointers
/// "implemented in a completely opaque way", which is why this is the one
/// extension a default broker serves. A default broker still pages a fresh
/// request by offset, so a client that never sends `cursor` sees clause
/// 5.5.9.2 paging.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(
    description = "Opaque, keyset-backed pagination pointers: collections hand out `prev`, `next` and `last` links carrying an opaque `cursor` token. The one extension a default broker serves, since NGSI-LD permits pointers implemented in a completely opaque way. A default broker pages a fresh request by offset and walks by cursor once a request sends an empty `cursor=`."
)]
pub struct CursorPaginationConfig {
    /// Whether cursors are offered.
    #[schemars(
        description = "Whether cursors are offered. Off, collections page by offset only and a `cursor` parameter is refused with `InvalidRequest`. With `extensions.mode` set to `\"conformance\"`, collections page by offset whatever this says."
    )]
    pub mode: FeatureMode,
    /// How a request naming neither `offset` nor `cursor` is paged while
    /// cursors are offered.
    #[schemars(
        description = "How a request that names neither `offset` nor `cursor` is paged while cursors are offered. Read only when `extensions.cursor_pagination.mode` is `\"enabled\"`."
    )]
    pub fresh_requests: FreshRequestPaging,
}

impl From<&CursorPaginationConfig> for CursorPaginationMode {
    fn from(config: &CursorPaginationConfig) -> CursorPaginationMode {
        match (config.mode, config.fresh_requests) {
            (FeatureMode::Disabled, FreshRequestPaging::Cursor | FreshRequestPaging::Offset) => CursorPaginationMode::Disabled,
            (FeatureMode::Enabled, FreshRequestPaging::Offset) => CursorPaginationMode::Available,
            (FeatureMode::Enabled, FreshRequestPaging::Cursor) => CursorPaginationMode::Default,
        }
    }
}

impl From<CursorPaginationMode> for CursorPaginationConfig {
    fn from(served: CursorPaginationMode) -> CursorPaginationConfig {
        let (mode, fresh_requests) = match served {
            CursorPaginationMode::Disabled => (FeatureMode::Disabled, FreshRequestPaging::default()),
            CursorPaginationMode::Available => (FeatureMode::Enabled, FreshRequestPaging::Offset),
            CursorPaginationMode::Default => (FeatureMode::Enabled, FreshRequestPaging::Cursor),
        };
        CursorPaginationConfig { mode, fresh_requests }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::extensions::cursor_pagination::{CursorPaginationConfig, CursorPaginationMode};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_default_broker_offers_cursors_and_pages_a_fresh_request_by_offset() {
        assert_eq!(CursorPaginationMode::from(&CursorPaginationConfig::default()), CursorPaginationMode::Available);
    }

    #[test]
    fn each_table_names_what_its_collections_serve() {
        for (table, served) in [
            ("mode = \"disabled\"\nfresh_requests = \"cursor\"\n", CursorPaginationMode::Disabled),
            ("mode = \"disabled\"\nfresh_requests = \"offset\"\n", CursorPaginationMode::Disabled),
            ("mode = \"enabled\"\nfresh_requests = \"offset\"\n", CursorPaginationMode::Available),
            ("mode = \"enabled\"\nfresh_requests = \"cursor\"\n", CursorPaginationMode::Default),
        ] {
            let config: CursorPaginationConfig = toml::from_str(table).expect("a table");
            assert_eq!(CursorPaginationMode::from(&config), served, "{table}");
        }
    }

    #[test]
    fn what_a_broker_serves_is_named_by_one_table() {
        for served in [CursorPaginationMode::Disabled, CursorPaginationMode::Available, CursorPaginationMode::Default] {
            assert_eq!(CursorPaginationMode::from(&CursorPaginationConfig::from(served)), served);
        }
    }

    #[test]
    fn a_mode_that_is_not_on_or_off_is_refused() {
        assert!(toml::from_str::<CursorPaginationConfig>("mode = \"available\"\nfresh_requests = \"cursor\"\n").is_err());
        assert!(toml::from_str::<CursorPaginationConfig>("mode = \"default\"\nfresh_requests = \"cursor\"\n").is_err());
    }
}
