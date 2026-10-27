//! The `[extensions.cursor_pagination]` table: opaque, keyset-backed
//! pagination pointers.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};
use toml_comment::TomlComment;

/// Whether cursors are offered, and whether a fresh request walks by one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum CursorPaginationMode {
    /// Only offsets (clause 5.5.9.2): a `cursor` parameter is refused with
    /// `InvalidRequest`.
    Disabled,
    /// A fresh request pages by offset; an empty `cursor=` opts into a walk.
    Available,
    /// A fresh request walks by cursor; `offset` and `cursor=` both remain
    /// available.
    #[default]
    Default,
}

/// Opaque, keyset-backed pagination pointers (clause 5.5.9.1).
///
/// Collections hand out `prev`/`next`/`last` pointers carrying an opaque
/// `cursor` token and follow them back. Clause 5.5.9.1 permits pointers
/// "implemented in a completely opaque way", which is why this is the one
/// extension a default broker serves.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CursorPaginationConfig {
    /// Whether cursors are offered, and whether a fresh request walks by one:
    /// `disabled`, `available` or `default`.
    #[toml_comment(inline)]
    pub mode: CursorPaginationMode,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::extensions::cursor_pagination::{CursorPaginationConfig, CursorPaginationMode};
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn a_default_broker_walks_by_cursor() {
        assert_eq!(CursorPaginationConfig::default().mode, CursorPaginationMode::Default);
    }

    #[test]
    fn the_three_modes_read_and_any_other_value_is_refused() {
        for (token, mode) in [
            ("disabled", CursorPaginationMode::Disabled),
            ("available", CursorPaginationMode::Available),
            ("default", CursorPaginationMode::Default),
        ] {
            assert_eq!(CursorPaginationMode::from_str(token).expect("a mode"), mode);
        }
        assert!(toml::from_str::<CursorPaginationConfig>("mode = \"enabled\"\n").is_err());
    }
}
