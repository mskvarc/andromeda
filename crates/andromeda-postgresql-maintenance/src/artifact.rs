//! Validation of PostgreSQL concurrent-reindex recovery artifacts.
//!
//! A concurrent rebuild interrupted before it completes leaves the index it
//! was building, named `<index>_ccnew[digits]`, invalid beside the original;
//! one interrupted after the swap leaves the original as `<index>_ccold`. An
//! allowlisted index's artifact sits in its branch schema, and a history
//! chunk's sits beside the chunk, or in `pg_toast` on the chunk's TOAST table,
//! which the catalogue read pairs it with.

use crate::{
    branch::Branch,
    candidate::{history_table::HistoryTable, index_target::IndexTarget},
    columnstore_partiality::HistoryChunk,
};
use andromeda_postgresql_seaquery::maintenance::{MaintenanceCommand, drop_index_concurrently};
use sea_query::Alias;

/// PostgreSQL's documented invalid concurrent-reindex artifact class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArtifactKind {
    /// An incomplete replacement index that should be dropped before retry.
    New,
    /// An obsolete original index left after a successful replacement build.
    Old,
}

/// What an artifact was left by: the rebuild of an allowlisted index, or the
/// rebuild of every index of a history chunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ArtifactOwner {
    /// An allowlisted index, in the schema of `branch`.
    Index {
        /// The branch whose schema holds the artifact.
        branch: Branch,
        /// The allowlisted index the artifact belongs to.
        index: IndexTarget,
    },
    /// A chunk of a history hypertable, in the schema holding the chunk.
    Chunk {
        /// The chunk's hypertable.
        table: HistoryTable,
        /// The chunk the artifact was built on.
        chunk: HistoryChunk,
        /// The schema holding the artifact.
        schema: String,
    },
}

/// A catalog name proven to be a concurrent-reindex artifact of one owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactIndexName {
    /// What left the artifact.
    owner: ArtifactOwner,
    /// Exact catalog name.
    name: String,
}

impl ArtifactIndexName {
    /// The concurrently safe drop command for the validated identifier.
    pub(crate) fn drop_statement(&self) -> MaintenanceCommand {
        match &self.owner {
            ArtifactOwner::Index { branch, .. } => drop_index_concurrently(branch.schema(), Alias::new(&self.name)),
            ArtifactOwner::Chunk { schema, .. } => drop_index_concurrently(Alias::new(schema), Alias::new(&self.name)),
        }
    }

    /// The branch whose schema holds the artifact: a history chunk's is the
    /// live branch, the only one holding hypertables.
    pub(crate) const fn branch(&self) -> Branch {
        match &self.owner {
            ArtifactOwner::Index { branch, .. } => *branch,
            ArtifactOwner::Chunk { .. } => Branch::Live,
        }
    }

    /// Borrow the catalog identifier for structured reporting.
    pub(crate) fn as_str(&self) -> &str {
        &self.name
    }

    /// What left the artifact.
    pub(crate) const fn owner(&self) -> &ArtifactOwner {
        &self.owner
    }
}

/// One leftover index a failed concurrent build left behind: the validated
/// catalog name to drop and the documented class it belongs to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryArtifact {
    /// Catalog name proven to be an artifact of its owner.
    pub(crate) name: ArtifactIndexName,
    /// Whether the artifact is the incomplete replacement or the obsolete original.
    pub(crate) kind: ArtifactKind,
}

impl RecoveryArtifact {
    /// Recognize only `<allowlisted>_ccnew[digits]` and
    /// `<allowlisted>_ccold[digits]` names, in the schema of `branch`.
    pub(crate) fn parse(branch: Branch, name: &str) -> Option<RecoveryArtifact> {
        IndexTarget::ALL.into_iter().find_map(|index| {
            let kind = artifact_kind(name.strip_prefix(index.relation_name())?)?;
            Some(RecoveryArtifact {
                name: ArtifactIndexName {
                    owner: ArtifactOwner::Index { branch, index },
                    name: name.to_owned(),
                },
                kind,
            })
        })
    }

    /// Recognize an invalid index the catalogue reports on `chunk` of `table`,
    /// in `schema`, by a `_ccnew[digits]` or `_ccold[digits]` suffix after a
    /// non-empty name.
    pub(crate) fn of_chunk(table: HistoryTable, chunk: HistoryChunk, schema: String, name: &str) -> Option<RecoveryArtifact> {
        let kind = [("_ccnew", ArtifactKind::New), ("_ccold", ArtifactKind::Old)]
            .into_iter()
            .find_map(|(marker, kind)| {
                let (base, digits) = name.rsplit_once(marker)?;
                (!base.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(kind)
            })?;
        Some(RecoveryArtifact {
            name: ArtifactIndexName {
                owner: ArtifactOwner::Chunk { table, chunk, schema },
                name: name.to_owned(),
            },
            kind,
        })
    }
}

/// The artifact class a name's suffix after its index name marks:
/// `_ccnew` or `_ccold` followed by digits alone.
fn artifact_kind(suffix: &str) -> Option<ArtifactKind> {
    let (kind, digits) = suffix
        .strip_prefix("_ccnew")
        .map(|digits| (ArtifactKind::New, digits))
        .or_else(|| suffix.strip_prefix("_ccold").map(|digits| (ArtifactKind::Old, digits)))?;
    digits.bytes().all(|byte| byte.is_ascii_digit()).then_some(kind)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        artifact::{ArtifactKind, RecoveryArtifact},
        branch::Branch,
        candidate::history_table::HistoryTable,
        columnstore_partiality::HistoryChunk,
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_seaquery::maintenance::drop_index_concurrently;
    use pretty_assertions::assert_eq;
    use sea_query::Alias;

    #[test]
    fn documented_artifacts_for_allowlisted_indexes_are_recognized() {
        let new = RecoveryArtifact::parse(Branch::Live, "idx_ca_value_number_ccnew1");
        let old = RecoveryArtifact::parse(Branch::Live, "idx_ca_value_geometry_ccold");
        assert!(matches!(new, Some(RecoveryArtifact { kind: ArtifactKind::New, .. })));
        assert!(matches!(old, Some(RecoveryArtifact { kind: ArtifactKind::Old, .. })));
    }

    #[test]
    fn arbitrary_and_malformed_identifiers_never_enter_drop_sql() {
        for name in [
            "unrelated_ccnew",
            "idx_ca_value_number_ccnew_x",
            "idx_ca_value_number_ccnew1;DROP TABLE entities",
            "idx_ca_value_number",
        ] {
            assert!(RecoveryArtifact::parse(Branch::Live, name).is_none(), "{name}");
        }
    }

    #[test]
    fn recognized_identifier_is_dropped_by_the_fixed_drop_command() {
        let Some(RecoveryArtifact { name, kind: ArtifactKind::Old }) = RecoveryArtifact::parse(Branch::Live, "idx_ca_value_string_ccold2") else {
            panic!("a well-formed ccold artifact of an allowlisted index is recognized");
        };
        assert_eq!(
            name.drop_statement(),
            drop_index_concurrently(Alias::new("public"), Alias::new("idx_ca_value_string_ccold2"))
        );
    }

    #[test]
    fn recovery_artifact_keeps_its_schema() {
        let branch = Branch::Snapshot(SnapshotKey::try_from(42).expect("positive snapshot key"));
        let Some(artifact) = RecoveryArtifact::parse(branch, "idx_ca_value_number_ccnew") else {
            panic!("a well-formed ccnew artifact of an allowlisted index is recognized in a branch");
        };
        assert_eq!(
            artifact.name.drop_statement(),
            drop_index_concurrently(Alias::new("snap_42"), Alias::new("idx_ca_value_number_ccnew"))
        );
    }

    fn chunk() -> HistoryChunk {
        HistoryChunk {
            oid: 16_384,
            relation: "_timescaledb_internal._hyper_1_2_chunk".to_owned(),
        }
    }

    /// An interrupted chunk rebuild leaves `<index>_ccnew` beside the chunk;
    /// it is dropped from the chunk's schema, in the live branch.
    #[test]
    fn a_chunk_rebuild_artifact_is_dropped_from_the_chunk_schema() {
        let Some(artifact) = RecoveryArtifact::of_chunk(
            HistoryTable::AttributeHistory,
            chunk(),
            "_timescaledb_internal".to_owned(),
            "_hyper_1_2_chunk_idx_attribute_history_stream_observed_placement_ccnew",
        ) else {
            panic!("a ccnew index on a chunk is an artifact");
        };
        assert_eq!(artifact.kind, ArtifactKind::New);
        assert_eq!(artifact.name.branch(), Branch::Live);
        assert_eq!(
            artifact.name.drop_statement(),
            drop_index_concurrently(
                Alias::new("_timescaledb_internal"),
                Alias::new("_hyper_1_2_chunk_idx_attribute_history_stream_observed_placement_ccnew")
            )
        );
    }

    #[test]
    fn an_invalid_chunk_index_without_an_artifact_suffix_is_not_recovered() {
        for name in [
            "_hyper_1_2_chunk_idx_attribute_history_stream_observed_placement",
            "_ccnew",
            "_hyper_1_2_chunk_x_ccold_y",
        ] {
            let artifact = RecoveryArtifact::of_chunk(HistoryTable::AttributeHistory, chunk(), "_timescaledb_internal".to_owned(), name);
            assert!(artifact.is_none(), "{name}");
        }
    }

    /// A chunk rebuild also rebuilds the index of the chunk's TOAST table, so
    /// an interruption leaves its `_ccnew` in `pg_toast`, dropped from there.
    #[test]
    fn a_chunk_toast_artifact_is_dropped_from_pg_toast() {
        let Some(artifact) = RecoveryArtifact::of_chunk(HistoryTable::AttributeHistory, chunk(), "pg_toast".to_owned(), "pg_toast_29697_index_ccnew") else {
            panic!("a ccnew index on a chunk's TOAST table is an artifact");
        };
        assert_eq!(
            artifact.name.drop_statement(),
            drop_index_concurrently(Alias::new("pg_toast"), Alias::new("pg_toast_29697_index_ccnew"))
        );
    }
}
