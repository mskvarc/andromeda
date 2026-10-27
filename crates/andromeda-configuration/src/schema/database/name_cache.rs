//! How much memory each of a tenant's name dictionary caches may hold, and how
//! long an unused name stays in one.
//!
//! The dictionaries themselves are unbounded: ETSI GS CIM 009 clauses 4.5.1 and
//! 4.5.2 let an Entity carry any type or attribute IRI, so the broker never
//! refuses a write for minting one more name. What is bounded is the in-process
//! copy. A name that does not fit, or that sat unused past the idle timeout, is
//! dropped from memory and read back from its dictionary table the next time it
//! is resolved, which costs one indexed read and never refuses anything.

use crate::{human_byte_size::HumanByteSize, human_duration::HumanDuration};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use strum::{Display, EnumString};

/// When a tenant's name caches are filled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[schemars(description = "When a tenant's name caches are filled.")]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum NameLoading {
    /// Every stored name is read into its cache when the tenant opens, so
    /// the first requests resolve names from memory.
    #[default]
    #[schemars(description = "Read every stored name into the caches when the tenant is loaded, so its first requests resolve names from memory.")]
    Eager,
    /// A name is read into its cache the first time it is resolved.
    #[schemars(description = "Read a name into its cache the first time it is resolved.")]
    Lazy,
}

/// When a tenant's name dictionary caches are filled, how much memory each
/// may hold, and how long an unused name stays in one.
///
/// Each tenant holds two caches, one over `attribute_names` and one over
/// `entity_type_names`, and each is given the whole budget. A tenant therefore
/// holds at most twice [`max_size_per_dictionary`](Self::max_size_per_dictionary)
/// of cached names, and only as much as it has actually resolved: the budget is
/// a ceiling, not an allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[schemars(
    description = "How each tenant's attribute and entity type name caches are filled, how much memory each may hold, and how long an unused name stays in one."
)]
#[serde(deny_unknown_fields)]
pub struct NameCacheConfig {
    /// When the caches are filled: `eager` (as the tenant opens) or `lazy`
    /// (as names are first resolved).
    #[schemars(description = "When the caches are filled: as the tenant is loaded, or as names are first resolved.")]
    pub loading: NameLoading,
    /// Memory each dictionary's cache may hold, in human-readable form
    /// (`"16 MiB"`). When it is full, the least valuable names are dropped to
    /// make room; a dropped name is read from its table when next resolved.
    #[schemars(
        description = "The memory each of a tenant's two name caches may hold, in each broker process. A tenant holds at most twice this, and only as much as it has resolved. When a cache is full the least valuable names are dropped; a dropped name is read back from the database the next time it is resolved, so a full cache costs reads but never refuses a write."
    )]
    pub max_size_per_dictionary: HumanByteSize,
    /// How long a cached name may go unused before it is dropped, in
    /// human-readable form (`"8h"`), or `"unlimited"` to drop names only when
    /// the capacity is full.
    #[schemars(description = "How long a cached name may go unused before it is dropped, or `\"unlimited\"` to drop names only when a cache is full.")]
    pub idle_eviction_after: IdleEviction,
}

// Manual because the defaults are chosen values rather than each field's own
// default: no derive can state "16 MiB" and "8h".
impl Default for NameCacheConfig {
    fn default() -> NameCacheConfig {
        NameCacheConfig {
            loading: NameLoading::Eager,
            // Roughly a hundred thousand names, against the hundreds a data
            // space's models actually use, so a normal tenant never evicts.
            max_size_per_dictionary: HumanByteSize::mib(16),
            idle_eviction_after: IdleEviction::After(HumanDuration::from_secs(8 * 60 * 60)),
        }
    }
}

impl NameCacheConfig {
    /// The capacity of one dictionary's cache, in bytes.
    #[must_use]
    pub const fn capacity_bytes(&self) -> u64 {
        self.max_size_per_dictionary.as_u64()
    }

    /// How long a cached name may go unused, or `None` when only the capacity
    /// evicts.
    #[must_use]
    pub const fn idle_timeout(&self) -> Option<Duration> {
        match self.idle_eviction_after {
            IdleEviction::After(idle) => Some(idle.duration()),
            IdleEviction::Never(_) => None,
        }
    }
}

/// When an unused name leaves the cache on its own.
///
/// Written in configuration either as a duration (`"8h"`) or as `"unlimited"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[schemars(description = "How long an unused name stays cached: a duration, or `\"unlimited\"`.")]
#[serde(untagged)]
pub enum IdleEviction {
    /// Names are dropped only to make room within the capacity.
    #[schemars(description = "Drop names only to make room when the cache is full.")]
    Never(Unlimited),
    /// Names unused for this long are dropped.
    #[schemars(description = "Drop a name once it has gone unused this long.")]
    After(HumanDuration),
}

/// The `"unlimited"` spelling of [`IdleEviction::Never`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[schemars(description = "No idle timeout: names leave a cache only when it is full.")]
#[serde(rename_all = "kebab-case")]
pub enum Unlimited {
    /// No idle timeout.
    #[schemars(description = "Keep names until the cache needs the room.")]
    Unlimited,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::database::name_cache::{IdleEviction, NameCacheConfig, Unlimited};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn the_default_budget_is_sixteen_mebibytes_with_an_eight_hour_idle_timeout() {
        let config = NameCacheConfig::default();
        assert_eq!(config.capacity_bytes(), 16 * 1024 * 1024);
        assert_eq!(config.idle_timeout(), Some(Duration::from_hours(8)));
    }

    #[test]
    fn a_human_readable_capacity_and_idle_timeout_parse() {
        let config: NameCacheConfig =
            toml::from_str("loading = \"lazy\"\nmax_size_per_dictionary = \"32 MiB\"\nidle_eviction_after = \"30m\"").expect("a capacity and a duration parse");
        assert_eq!(config.capacity_bytes(), 32 * 1024 * 1024);
        assert_eq!(config.idle_timeout(), Some(Duration::from_mins(30)));
    }

    #[test]
    fn unlimited_idle_eviction_leaves_only_the_capacity_evicting() {
        let config: NameCacheConfig =
            toml::from_str("loading = \"lazy\"\nmax_size_per_dictionary = \"16 MiB\"\nidle_eviction_after = \"unlimited\"").expect("unlimited parses");
        assert_eq!(config.idle_eviction_after, IdleEviction::Never(Unlimited::Unlimited));
        assert_eq!(config.idle_timeout(), None);
    }

    #[test]
    fn an_idle_eviction_that_is_neither_a_duration_nor_unlimited_is_refused() {
        let parsed = toml::from_str::<NameCacheConfig>("loading = \"lazy\"\nmax_size_per_dictionary = \"16 MiB\"\nidle_eviction_after = \"forever\"");
        assert!(parsed.is_err(), "only a duration or \"unlimited\" is an idle timeout");
    }

    #[test]
    fn both_idle_eviction_spellings_render_as_they_are_written() {
        let rendered = toml::to_string(&NameCacheConfig::default()).expect("the default renders");
        assert!(rendered.contains("idle_eviction_after = \"8h\""), "{rendered}");
        assert!(rendered.contains("max_size_per_dictionary = \"16.0 MiB\""), "{rendered}");
        let unlimited = NameCacheConfig {
            idle_eviction_after: IdleEviction::Never(Unlimited::Unlimited),
            ..NameCacheConfig::default()
        };
        let rendered = toml::to_string(&unlimited).expect("unlimited renders");
        assert!(rendered.contains("idle_eviction_after = \"unlimited\""), "{rendered}");
    }
}
