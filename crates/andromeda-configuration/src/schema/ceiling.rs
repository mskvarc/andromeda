//! A limit an operator can lift: a value, or `"unlimited"`.
//!
//! TOML has no null, so a key whose default is a limit cannot be lifted by
//! leaving it out. Every limit that can be lifted therefore spells "no limit"
//! as `"unlimited"`, and leaving any key out always means its default.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A limit, or none.
///
/// Written in configuration either as the limit itself or as `"unlimited"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
#[schemars(description = "A limit, or `\"unlimited\"` for none.")]
pub enum Ceiling<T> {
    /// No limit.
    #[schemars(description = "No limit.")]
    Unlimited(Unlimited),
    /// The limit.
    #[schemars(description = "The limit.")]
    At(T),
}

impl<T: Copy> Ceiling<T> {
    /// The limit, or `None` when there is none.
    #[must_use]
    pub const fn limit(self) -> Option<T> {
        match self {
            Ceiling::Unlimited(Unlimited::Unlimited) => None,
            Ceiling::At(limit) => Some(limit),
        }
    }
}

/// The `"unlimited"` spelling of [`Ceiling::Unlimited`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(description = "No limit.")]
pub enum Unlimited {
    /// No limit.
    #[schemars(description = "No limit.")]
    Unlimited,
}

#[cfg(test)]
mod tests {
    use crate::{
        human_duration::HumanDuration,
        schema::ceiling::{Ceiling, Unlimited},
    };
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::time::Duration;
    use toml::de::Error;

    /// One limit as a table holds it.
    #[derive(Debug, Deserialize)]
    struct Table {
        /// The limit under test.
        limit: Ceiling<HumanDuration>,
    }

    #[test]
    fn unlimited_reads_as_no_limit_and_a_value_as_that_limit() -> Result<(), Error> {
        let lifted: Table = toml::from_str("limit = \"unlimited\"")?;
        assert_eq!(lifted.limit, Ceiling::Unlimited(Unlimited::Unlimited));
        assert_eq!(lifted.limit.limit(), None);
        let set: Table = toml::from_str("limit = \"8h\"")?;
        assert_eq!(set.limit.limit().map(HumanDuration::duration), Some(Duration::from_hours(8)));
        Ok(())
    }

    #[test]
    fn a_value_that_is_neither_is_refused() {
        assert!(toml::from_str::<Table>("limit = \"forever\"").is_err());
    }
}
