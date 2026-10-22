//! Converting one `TimescaleDB` chunk to the columnstore.
//!
//! `convert_to_columnstore` is a procedure, so it is called by `CALL`, which
//! sea-query has no form for. The chunk is named by its OID or by the
//! schema-qualified name `TimescaleDB`'s own chunk catalogue reported, as a
//! literal cast to `regclass`.
//!
//! Every conversion runs with `timescaledb.compress_truncate_behaviour` at
//! `truncate_disabled` for the call alone: the rowstore rows are deleted
//! rather than truncated, so a write waiting on the chunk cannot deadlock the
//! call, and `if_not_columnstore` turns a chunk converted meanwhile into a
//! notice rather than an error (a partially converted chunk is still
//! recompressed).

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

/// `SET LOCAL timescaledb.compress_truncate_behaviour = 'truncate_disabled';
/// CALL convert_to_columnstore(<chunk>::regclass, if_not_columnstore => true)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvertToColumnstore {
    /// The chunk converted.
    chunk: ChunkReference,
}

impl ConvertToColumnstore {
    /// The conversion of `chunk`.
    #[must_use]
    pub const fn new(chunk: ChunkReference) -> ConvertToColumnstore {
        ConvertToColumnstore { chunk }
    }
}

impl RenderedCommand for ConvertToColumnstore {
    fn rendered(&self) -> String {
        let chunk = match &self.chunk {
            ChunkReference::Oid(oid) => oid.to_string(),
            ChunkReference::Name(name) => quoted_text(name),
        };
        let truncation = SettingAssignment::new(Setting::TimescaledbCompressTruncateBehaviour, "truncate_disabled", SettingScope::Transaction);
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
        columnstore_conversion::{ChunkReference, ConvertToColumnstore},
        command::rendering::RenderedCommand,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_chunk_named_by_oid_converts_with_truncation_disabled_for_the_call_alone() {
        assert_eq!(
            ConvertToColumnstore::new(ChunkReference::Oid(16_384)).rendered(),
            "SET LOCAL timescaledb.compress_truncate_behaviour = 'truncate_disabled'; CALL convert_to_columnstore(16384::regclass, if_not_columnstore => true)"
        );
    }

    #[test]
    fn a_chunk_named_by_relation_name_is_a_quoted_literal() {
        assert_eq!(
            ConvertToColumnstore::new(ChunkReference::Name("_timescaledb_internal._hyper_1_2_chunk".to_owned())).rendered(),
            "SET LOCAL timescaledb.compress_truncate_behaviour = 'truncate_disabled'; CALL convert_to_columnstore('_timescaledb_internal._hyper_1_2_chunk'::regclass, if_not_columnstore => true)"
        );
        assert!(
            ConvertToColumnstore::new(ChunkReference::Name("it's".to_owned()))
                .rendered()
                .contains("'it''s'::regclass")
        );
    }
}
