//! The bounded in-process maps from interned name to dictionary key and back.
//!
//! A mapping never changes once its dictionary row is committed, so nothing
//! here expires for staleness: entries leave only to keep the cache inside its
//! byte budget, or after going unused for the configured idle timeout. Either
//! way a departed name is simply a miss, which the cache already answers from
//! the dictionary table, so eviction can cost a read but never a wrong key.
//!
//! The eviction policy is moka's `TinyLFU`: when the cache is full, a newcomer is
//! admitted only if it is used more often than the entry it would displace. A
//! stream of names written once therefore cannot push out the core vocabulary
//! every request resolves.

use andromeda_configuration::schema::database::name_cache::NameCacheConfig;
use andromeda_postgresql_query::dictionary::interned_name::InternedName;
use moka::sync::Cache;

/// Bytes one cached mapping is charged on top of its name's text.
///
/// Covers the name and key values themselves and moka's per-entry bookkeeping:
/// the entry header with its timestamps and frequency counter, the nodes in
/// the access-order and write-order queues, and the hash-table slot. It is an
/// estimate of the allocator footprint, not a measurement, sized so that the
/// configured budget is an upper bound on what the cache really holds rather
/// than an understatement of it.
const ENTRY_OVERHEAD_BYTES: u32 = 160;

/// A cache from name to key over one dictionary, sized and timed by `config`.
pub(crate) fn bounded_keys<N: InternedName>(config: &NameCacheConfig) -> Cache<N, N::Key> {
    let builder = Cache::builder().max_capacity(config.capacity_bytes()).weigher(entry_weight::<N>);
    match config.idle_timeout() {
        Some(idle) => builder.time_to_idle(idle).build(),
        None => builder.build(),
    }
}

/// A cache from key to name over one dictionary, sized and timed by `config`
/// as the cache from name to key is, each holding its own budget.
pub(crate) fn bounded_names<N: InternedName>(config: &NameCacheConfig) -> Cache<N::Key, N> {
    let builder = Cache::builder()
        .max_capacity(config.capacity_bytes())
        .weigher(|key: &N::Key, name: &N| entry_weight::<N>(name, key));
    match config.idle_timeout() {
        Some(idle) => builder.time_to_idle(idle).build(),
        None => builder.build(),
    }
}

/// How many dictionary rows a preload reads at most.
///
/// Every entry weighs at least [`ENTRY_OVERHEAD_BYTES`] plus one byte of name,
/// so no more rows than this can fit in the budget at once; reading more would
/// only fetch names the cache must immediately drop again.
pub(crate) fn preload_row_limit(config: &NameCacheConfig) -> u64 {
    config.capacity_bytes() / (u64::from(ENTRY_OVERHEAD_BYTES) + 1)
}

/// The weight of one cached mapping, in bytes.
///
/// Saturates rather than wraps: a name longer than four gigabytes cannot exist
/// in a dictionary whose names are length-bounded, and if one did, charging it
/// the whole of `u32` keeps it out of the cache instead of letting it in as
/// nearly free.
fn entry_weight<N: InternedName>(name: &N, _key: &N::Key) -> u32 {
    u32::try_from(InternedName::as_str(name).len()).map_or(u32::MAX, |length| length.saturating_add(ENTRY_OVERHEAD_BYTES))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::bounded_keys::{ENTRY_OVERHEAD_BYTES, bounded_keys, bounded_names, entry_weight, preload_row_limit};
    use andromeda_configuration::{
        human_byte_size::HumanByteSize,
        human_duration::HumanDuration,
        schema::database::name_cache::{IdleEviction, NameCacheConfig, Unlimited},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn name(position: usize) -> AttributeName {
        format!("https://example.com/generated/{position}").parse().expect("valid name")
    }

    #[test]
    fn the_cache_is_bounded_by_the_configured_bytes_and_idle_timeout() {
        let keys = bounded_keys::<AttributeName>(&NameCacheConfig {
            max_size_per_dictionary: HumanByteSize::mib(4),
            idle_eviction_after: IdleEviction::After(HumanDuration::from_secs(90)),
            ..NameCacheConfig::default()
        });
        assert_eq!(keys.policy().max_capacity(), Some(4 * 1024 * 1024));
        assert_eq!(keys.policy().time_to_idle(), Some(Duration::from_secs(90)));
        assert_eq!(keys.policy().time_to_live(), None, "a committed mapping never goes stale");
    }

    #[test]
    fn the_cache_of_names_by_key_has_the_budget_and_idle_timeout_of_the_cache_of_keys() {
        let config = NameCacheConfig {
            max_size_per_dictionary: HumanByteSize::mib(4),
            idle_eviction_after: IdleEviction::After(HumanDuration::from_secs(90)),
            ..NameCacheConfig::default()
        };
        let names = bounded_names::<AttributeName>(&config);
        assert_eq!(names.policy().max_capacity(), Some(4 * 1024 * 1024));
        assert_eq!(names.policy().time_to_idle(), Some(Duration::from_secs(90)));
        assert_eq!(names.policy().time_to_live(), None, "a committed mapping never goes stale");
    }

    #[test]
    fn unlimited_idle_eviction_sets_no_idle_timeout() {
        let keys = bounded_keys::<AttributeName>(&NameCacheConfig {
            idle_eviction_after: IdleEviction::Never(Unlimited::Unlimited),
            ..NameCacheConfig::default()
        });
        assert_eq!(keys.policy().time_to_idle(), None);
        assert_eq!(keys.policy().time_to_live(), None);
    }

    #[test]
    fn an_entry_weighs_its_name_text_plus_the_fixed_overhead() {
        let name: AttributeName = "https://example.com/speed".parse().expect("valid name");
        let weight = entry_weight(&name, &AttributeNameKey::from(1));
        assert_eq!(weight, u32::try_from("https://example.com/speed".len()).expect("fits") + ENTRY_OVERHEAD_BYTES);
    }

    /// Inserting past the budget never fails or blocks; the cache drops what
    /// it must and stays inside its capacity.
    #[test]
    fn inserting_past_the_budget_keeps_the_cache_inside_it() {
        let config = NameCacheConfig {
            max_size_per_dictionary: HumanByteSize::kib(8),
            ..NameCacheConfig::default()
        };
        let keys = bounded_keys::<AttributeName>(&config);
        for position in 0..1_000 {
            keys.insert(name(position), AttributeNameKey::from(i32::try_from(position).expect("fits")));
        }
        keys.run_pending_tasks();
        assert!(keys.weighted_size() <= config.capacity_bytes(), "weighted size {}", keys.weighted_size());
        assert!(keys.entry_count() < 1_000, "an 8 KiB budget cannot hold a thousand names");
    }

    #[test]
    fn a_preload_reads_no_more_rows_than_could_fit_in_the_budget() {
        let config = NameCacheConfig {
            max_size_per_dictionary: HumanByteSize::b(16_100),
            ..NameCacheConfig::default()
        };
        assert_eq!(preload_row_limit(&config), 100);
    }
}
