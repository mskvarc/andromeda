//! Converting one `TimescaleDB` chunk to the columnstore.
//!
//! `convert_to_columnstore` is a procedure, so it is called by `CALL`, which
//! sea-query has no form for. The chunk is named by its OID or by the
//! schema-qualified name `TimescaleDB`'s own chunk catalogue reported, as a
//! literal cast to `regclass`.
//!
//! Every conversion sets `timescaledb.compress_truncate_behaviour` for the call
//! alone ([`RowstoreRemoval`]), and `if_not_columnstore` turns a chunk
//! converted meanwhile into a notice rather than an error (a partially
//! converted chunk is still recompressed).

use crate::{
    command::{Command, rendering::RenderedCommand},
    literal::quoted_text,
    setting::{Setting, SettingAssignment, SettingScope},
};

/// How a conversion names its chunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChunkReference {
    /// The chunk relation's OID.
    Oid(u32),
    /// The chunk relation's schema-qualified name, as the chunk catalogue
    /// reports it.
    Name(String),
}

/// How a conversion removes the rows it moved out of the chunk's rowstore.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RowstoreRemoval {
    /// `truncate_only`: the rowstore relation and its indexes are truncated,
    /// waiting for the `AccessExclusiveLock` a truncate needs. Only this
    /// removal leaves the rows in place until the chunk's chunk-skipping
    /// ranges are computed, so only it records them. A write waiting on the
    /// chunk deadlocks the call.
    Truncate,
    /// `truncate_or_delete`: the rowstore relation and its indexes are
    /// truncated when the `AccessExclusiveLock` a truncate needs is free at
    /// once, and the rows are deleted otherwise. A write waiting on the chunk
    /// therefore never deadlocks the call, and an uncontended conversion
    /// leaves no rowstore pages behind.
    TruncateWhenFree,
    /// `truncate_disabled`: the rows are always deleted, and the relation and
    /// its indexes keep their pages until vacuum reclaims what it can.
    Delete,
}

impl RowstoreRemoval {
    /// The `timescaledb.compress_truncate_behaviour` value.
    const fn setting(self) -> &'static str {
        match self {
            RowstoreRemoval::Truncate => "truncate_only",
            RowstoreRemoval::TruncateWhenFree => "truncate_or_delete",
            RowstoreRemoval::Delete => "truncate_disabled",
        }
    }
}

/// `SET LOCAL timescaledb.compress_truncate_behaviour = '<removal>';
/// CALL convert_to_columnstore(<chunk>::regclass, if_not_columnstore => true)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvertToColumnstore {
    /// The chunk converted.
    chunk: ChunkReference,
    /// How its rowstore rows are removed.
    removal: RowstoreRemoval,
}

impl ConvertToColumnstore {
    /// The conversion of `chunk`, removing its rowstore rows by `removal`.
    #[must_use]
    pub const fn new(chunk: ChunkReference, removal: RowstoreRemoval) -> ConvertToColumnstore {
        ConvertToColumnstore { chunk, removal }
    }
}

impl RenderedCommand for ConvertToColumnstore {
    fn rendered(&self) -> String {
        let chunk = match &self.chunk {
            ChunkReference::Oid(oid) => oid.to_string(),
            ChunkReference::Name(name) => quoted_text(name),
        };
        let truncation = SettingAssignment::new(Setting::TimescaledbCompressTruncateBehaviour, self.removal.setting(), SettingScope::Transaction);
        format!(
            "{}; CALL convert_to_columnstore({chunk}::regclass, if_not_columnstore => true)",
            truncation.rendered()
        )
    }
}

impl Command for ConvertToColumnstore {}

#[cfg(test)]
mod tests {
    use crate::{
        columnstore_conversion::{ChunkReference, ConvertToColumnstore, RowstoreRemoval},
        command::rendering::RenderedCommand,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_chunk_named_by_oid_converts_with_truncation_disabled_for_the_call_alone() {
        assert_eq!(
            ConvertToColumnstore::new(ChunkReference::Oid(16_384), RowstoreRemoval::Delete).rendered(),
            "SET LOCAL timescaledb.compress_truncate_behaviour = 'truncate_disabled'; CALL convert_to_columnstore(16384::regclass, if_not_columnstore => true)"
        );
    }

    #[test]
    fn a_truncating_conversion_sets_truncate_only() {
        assert_eq!(
            ConvertToColumnstore::new(ChunkReference::Oid(16_384), RowstoreRemoval::Truncate).rendered(),
            "SET LOCAL timescaledb.compress_truncate_behaviour = 'truncate_only'; CALL convert_to_columnstore(16384::regclass, if_not_columnstore => true)"
        );
    }

    #[test]
    fn a_chunk_named_by_relation_name_is_a_quoted_literal_truncated_when_its_lock_is_free() {
        assert_eq!(
            ConvertToColumnstore::new(
                ChunkReference::Name("_timescaledb_internal._hyper_1_2_chunk".to_owned()),
                RowstoreRemoval::TruncateWhenFree
            )
            .rendered(),
            "SET LOCAL timescaledb.compress_truncate_behaviour = 'truncate_or_delete'; CALL convert_to_columnstore('_timescaledb_internal._hyper_1_2_chunk'::regclass, if_not_columnstore => true)"
        );
        assert!(
            ConvertToColumnstore::new(ChunkReference::Name("it's".to_owned()), RowstoreRemoval::Delete)
                .rendered()
                .contains("'it''s'::regclass")
        );
    }
}
