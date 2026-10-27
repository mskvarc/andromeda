//! The `[entity.batch]` table: how a batch operation is cut into chunks and
//! run.

use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use toml_comment::TomlComment;

/// How a batch operation is cut into chunks and run.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EntityBatchConfig {
    /// The entities one chunk of a batch holds.
    #[toml_comment(inline)]
    pub chunk_size: NonZeroUsize,
    /// The batch size from which the large-batch path is taken.
    #[toml_comment(inline)]
    pub large_threshold: NonZeroUsize,
    /// The batch size from which entities are prepared on the blocking
    /// thread pool.
    #[toml_comment(inline)]
    pub prepare_blocking_threshold: NonZeroUsize,
    /// The most chunks prepared at once.
    #[toml_comment(inline)]
    pub max_concurrent_chunks: NonZeroUsize,
    /// The most chunks written to the database at once.
    #[toml_comment(inline)]
    pub max_concurrent_chunk_writes: NonZeroUsize,
    /// The batch size from which a batch's side effects run after it commits
    /// rather than inside it. Left out, they always run inside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub async_side_effect_threshold: Option<NonZeroUsize>,
}

impl Default for EntityBatchConfig {
    fn default() -> EntityBatchConfig {
        EntityBatchConfig {
            chunk_size: NonZeroUsize::MIN.saturating_add(511),
            large_threshold: NonZeroUsize::MIN.saturating_add(511),
            prepare_blocking_threshold: NonZeroUsize::MIN.saturating_add(127),
            max_concurrent_chunks: NonZeroUsize::MIN.saturating_add(3),
            max_concurrent_chunk_writes: NonZeroUsize::MIN.saturating_add(3),
            async_side_effect_threshold: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::entity::batch::EntityBatchConfig;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_defaults_run_four_chunks_of_512_at_once() {
        let config = EntityBatchConfig::default();
        assert_eq!(config.chunk_size.get(), 512);
        assert_eq!(config.max_concurrent_chunks.get(), 4);
        assert_eq!(config.max_concurrent_chunk_writes.get(), 4);
        assert_eq!(config.async_side_effect_threshold, None);
    }

    #[test]
    fn a_zero_count_is_refused_as_it_is_read() {
        let rendered = toml::to_string(&EntityBatchConfig::default()).unwrap_or_default();
        assert!(toml::from_str::<EntityBatchConfig>(&rendered.replace("chunk_size = 512", "chunk_size = 0")).is_err());
        assert!(toml::from_str::<EntityBatchConfig>(&rendered).is_ok());
    }
}
