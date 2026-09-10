//! The value one scope-history row carries.

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use andromeda_postgresql_codes::scope_value_kind::ScopeValueKind;
use chrono::{DateTime, Utc};

/// What one scope-history row records.
///
/// A scope is not an attribute — clause 4.5.5 gives it no dataset dimension and
/// no attribute family — so scope history has its own two-member value
/// vocabulary rather than a borrowed subset of the attribute one. A scope that
/// stops applying is recorded as a row carrying a deletion instant and no path,
/// never as an invented sentinel path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScopeHistoryValue {
    /// The row ends the scope's applicability at the given instant.
    Tombstone {
        /// The instant the scope stopped applying.
        deleted_at: DateTime<Utc>,
    },
    /// The row records a scope path.
    Path(ScopePath),
}

impl ScopeHistoryValue {
    /// The code the row records for this value's shape.
    #[must_use]
    pub const fn kind(&self) -> ScopeValueKind {
        match self {
            ScopeHistoryValue::Tombstone { .. } => ScopeValueKind::Tombstone,
            ScopeHistoryValue::Path(_) => ScopeValueKind::ScopePath,
        }
    }

    /// The instant this row records a deletion at, when it records one.
    #[must_use]
    pub const fn deleted_at(&self) -> Option<DateTime<Utc>> {
        match self {
            ScopeHistoryValue::Tombstone { deleted_at } => Some(*deleted_at),
            ScopeHistoryValue::Path(_) => None,
        }
    }

    /// The scope path the row carries, when it carries one.
    #[must_use]
    pub const fn path(&self) -> Option<&ScopePath> {
        match self {
            ScopeHistoryValue::Tombstone { .. } => None,
            ScopeHistoryValue::Path(path) => Some(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::scope_history_value::ScopeHistoryValue;
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
    use andromeda_postgresql_codes::scope_value_kind::ScopeValueKind;
    use chrono::{TimeZone, Utc};

    #[test]
    fn a_path_row_declares_the_path_kind_and_records_no_deletion() {
        let path = "/Madrid/Gardens".parse::<ScopePath>().expect("a valid scope path");
        let value = ScopeHistoryValue::Path(path);
        pretty_assertions::assert_eq!(value.kind(), ScopeValueKind::ScopePath);
        assert!(value.deleted_at().is_none());
        assert!(value.path().is_some());
    }

    #[test]
    fn a_tombstone_row_declares_the_reserved_kind_and_carries_no_path() {
        let deleted_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let value = ScopeHistoryValue::Tombstone { deleted_at };
        pretty_assertions::assert_eq!(value.kind(), ScopeValueKind::Tombstone);
        pretty_assertions::assert_eq!(value.deleted_at(), Some(deleted_at));
        assert!(value.path().is_none());
    }
}
