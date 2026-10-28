//! The three directories the migrations are kept in.
//!
//! Every directory has a ledger of its own in the broker, but all three draw
//! their versions from one version space: a version is held by exactly one file
//! across every set, and a new file of any set takes the next free number.

use serde_with::{DeserializeFromStr, SerializeDisplay};

/// One directory of migrations below `migrations/`, named by its directory.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, strum::Display, strum::EnumString, strum::EnumIter, SerializeDisplay, DeserializeFromStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum MigrationSet {
    /// The platform tables, applied only to the platform database.
    Platform,
    /// The tenant-global relations, applied in `public` of every tenant.
    Tenant,
    /// The relations every context branch holds, applied in `public` with the
    /// tenant set and alone in every Snapshot schema.
    Branch,
}

#[cfg(test)]
mod tests {
    use crate::migration_set::MigrationSet;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn every_set_is_named_by_its_directory() {
        let names = MigrationSet::iter().map(|set| set.to_string()).collect::<Vec<_>>();
        assert_eq!(names, ["platform", "tenant", "branch"]);
    }

    #[test]
    fn a_directory_name_reads_back_as_its_set() {
        assert_eq!(MigrationSet::from_str("branch"), Ok(MigrationSet::Branch));
        assert!(MigrationSet::from_str("Branch").is_err());
        assert!(MigrationSet::from_str("snapshot").is_err());
    }
}
