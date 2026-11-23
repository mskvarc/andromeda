//! How firmly a configured feature, and the tables and keys that configure it,
//! hold across releases.

use serde::{Deserialize, Serialize};

/// How firmly a feature and its configuration hold across releases.
///
/// A table or key is marked with `x-andromeda-stability` in the schema, and a
/// key inside a marked table takes the table's stability unless it is marked
/// itself. An unmarked one is stable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stability {
    /// Kept under its name, with its meaning, for the whole major version.
    #[default]
    Stable,
    /// May change or go away in any release.
    Experimental,
    /// Still served, and removed only in a major release.
    Deprecated,
}

impl Stability {
    /// The sentence the commented configuration adds under a table or key
    /// marked with this stability; none for a stable one.
    #[must_use]
    pub(crate) const fn operator_note(self) -> Option<&'static str> {
        match self {
            Stability::Stable => None,
            Stability::Experimental => Some("This is experimental, so it may change or go away in any release."),
            Stability::Deprecated => Some("This is deprecated, and goes away in a later major release."),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::stability::Stability;
    use pretty_assertions::assert_eq;

    #[test]
    fn each_stability_is_spelled_in_kebab_case_and_reads_back() {
        for (stability, spelled) in [
            (Stability::Stable, "\"stable\""),
            (Stability::Experimental, "\"experimental\""),
            (Stability::Deprecated, "\"deprecated\""),
        ] {
            assert_eq!(serde_json::to_string(&stability).expect("written"), spelled);
            assert_eq!(serde_json::from_str::<Stability>(spelled).expect("read"), stability);
        }
    }

    #[test]
    fn only_a_stable_table_or_key_goes_without_a_note() {
        assert_eq!(Stability::Stable.operator_note(), None);
        assert!(Stability::Experimental.operator_note().is_some());
        assert!(Stability::Deprecated.operator_note().is_some());
    }

    #[test]
    fn an_unmarked_table_or_key_is_stable() {
        assert_eq!(Stability::default(), Stability::Stable);
    }
}
