//! Validation of PostgreSQL concurrent-reindex recovery artifacts.

use crate::candidate::IndexTarget;
use andromeda_postgresql_seaquery::maintenance::{MaintenanceSchema, MaintenanceStatement, drop_index_concurrently};
use sea_query::Alias;

/// PostgreSQL's documented invalid concurrent-reindex artifact class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArtifactKind {
    /// An incomplete replacement index that should be dropped before retry.
    New,
    /// An obsolete original index left after a successful replacement build.
    Old,
}

/// A catalog name proven to belong to one exact allowlisted index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactIndexName {
    /// Exact catalog name.
    name: String,
    /// Allowlisted index the artifact belongs to.
    owner: IndexTarget,
}

impl ArtifactIndexName {
    /// Render the concurrently safe drop command for the validated identifier.
    pub(crate) fn drop_statement(&self) -> MaintenanceStatement {
        drop_index_concurrently(&MaintenanceSchema::Public, &Alias::new(&self.name))
    }

    /// Borrow the catalog identifier for structured reporting.
    pub(crate) fn as_str(&self) -> &str {
        &self.name
    }

    /// Return the allowlisted index that created this artifact.
    pub(crate) const fn owner(&self) -> IndexTarget {
        self.owner
    }
}

/// One leftover index a failed concurrent build left behind: the validated
/// catalog name to drop and the documented class it belongs to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryArtifact {
    /// Catalog name proven to belong to one allowlisted index.
    pub(crate) name: ArtifactIndexName,
    /// Whether the artifact is the incomplete replacement or the obsolete original.
    pub(crate) kind: ArtifactKind,
}

impl RecoveryArtifact {
    /// Recognize only `<allowlisted>_ccnew[digits]` and
    /// `<allowlisted>_ccold[digits]` names.
    pub(crate) fn parse(name: &str) -> Option<RecoveryArtifact> {
        IndexTarget::ALL.into_iter().find_map(|target| {
            let suffix = name.strip_prefix(target.relation_name())?;
            let kind_and_digits = suffix
                .strip_prefix("_ccnew")
                .map(|digits| (ArtifactKind::New, digits))
                .or_else(|| suffix.strip_prefix("_ccold").map(|digits| (ArtifactKind::Old, digits)))?;
            let (kind, digits) = kind_and_digits;
            if digits.bytes().all(|byte| byte.is_ascii_digit()) {
                Some(RecoveryArtifact {
                    name: ArtifactIndexName {
                        name: name.to_owned(),
                        owner: target,
                    },
                    kind,
                })
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::artifact::{ArtifactKind, RecoveryArtifact};
    use pretty_assertions::assert_eq;

    #[test]
    fn documented_artifacts_for_allowlisted_indexes_are_recognized() {
        let new = RecoveryArtifact::parse("idx_cav_attr_number_ccnew1");
        let old = RecoveryArtifact::parse("idx_cav_attr_geo_ccold");
        assert!(matches!(new, Some(RecoveryArtifact { kind: ArtifactKind::New, .. })));
        assert!(matches!(old, Some(RecoveryArtifact { kind: ArtifactKind::Old, .. })));
    }

    #[test]
    fn arbitrary_and_malformed_identifiers_never_enter_drop_sql() {
        for name in [
            "unrelated_ccnew",
            "idx_cav_attr_number_ccnew_x",
            "idx_cav_attr_number_ccnew1;DROP TABLE entities",
            "idx_cav_attr_number",
        ] {
            assert!(RecoveryArtifact::parse(name).is_none(), "{name}");
        }
    }

    #[test]
    fn recognized_identifier_is_quoted_in_the_fixed_drop_shape() {
        let Some(RecoveryArtifact { name, kind: ArtifactKind::Old }) = RecoveryArtifact::parse("idx_cav_attr_string_ccold2") else {
            panic!("a well-formed ccold artifact of an allowlisted index is recognized");
        };
        assert_eq!(
            name.drop_statement().as_sql(),
            "DROP INDEX CONCURRENTLY \"public\".\"idx_cav_attr_string_ccold2\""
        );
    }
}
