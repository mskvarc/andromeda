//! The `[jsonld.cache]` table: how long resolved contexts are kept in memory.

use crate::{human_duration::HumanDuration, schema::configured_lifetime::ConfiguredLifetime};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

/// How long resolved contexts are kept in memory.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "How long resolved contexts are kept in memory.")]
pub struct JsonLdCacheConfig {
    /// The most entries each in-process context cache holds.
    #[schemars(description = "The most entries each in-process context cache holds.")]
    pub max_entries: NonZeroU64,
    /// How long a term-cache entry lives, and how long a downloaded `@context`
    /// whose response states no expiration time is kept, in human-readable form
    /// (`"1h"`).
    ///
    /// Clause 6.3.16 requires compliance with the `Expires` header field and
    /// with the `max-age` and `s-maxage` directives of `Cache-Control` carried
    /// by a downloaded `@context`, so a response that names one of those sets
    /// its own lifetime and this setting never applies to it. It is the
    /// heuristic expiration time the same clause recommends assigning when the
    /// origin server specifies none. A span of zero or below one second is
    /// refused as the configuration is read.
    #[schemars(
        description = "How long a downloaded `@context` is kept when its response sets no `Expires` and no `Cache-Control` `max-age` or `s-maxage`, and how long a term-cache entry lives. A response that sets one of those keeps its own lifetime."
    )]
    pub default_lifetime: ConfiguredLifetime,
    /// How long this process uses its in-memory copy of an `@context` the
    /// broker stores itself before reading the stored row again, in
    /// human-readable form (`"1m"`).
    ///
    /// Covers the `"Hosted"` and `"ImplicitlyCreated"` entries of clause
    /// 5.13.1, which no origin server can make stale but another broker process
    /// sharing the database can delete. The same span is the `max-age` the
    /// Serve response advertises for them, so a remote party caching the
    /// served document is held to it too. A span of zero or below one second
    /// is refused as the configuration is read, since `max-age` states whole
    /// seconds.
    #[schemars(
        description = "How long a broker process uses its in-memory copy of a `@context` the broker stores itself before reading it from the database again, since another process sharing the database may have deleted it. It is also the `max-age` sent when such a `@context` is served, in whole seconds."
    )]
    pub hosted_revalidation_interval: ConfiguredLifetime,
    /// How often the stored contexts' hit counters are written back, in
    /// human-readable form (`"5s"`).
    #[schemars(description = "How often the `numberOfHits` counters of stored `@context`s are written to the database.")]
    pub hit_flush_interval: HumanDuration,
}

impl Default for JsonLdCacheConfig {
    fn default() -> JsonLdCacheConfig {
        JsonLdCacheConfig {
            max_entries: NonZeroU64::MIN.saturating_add(1023),
            default_lifetime: ConfiguredLifetime::CACHE_TTL_DEFAULT,
            hosted_revalidation_interval: ConfiguredLifetime::HOSTED_REVALIDATION_DEFAULT,
            hit_flush_interval: HumanDuration::from_secs(5),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::jsonld::cache::JsonLdCacheConfig;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    /// A `[jsonld.cache]` table with the two lifetimes as given.
    fn table(default_lifetime: &str, revalidation: &str) -> String {
        format!(
            "max_entries = 1024\ndefault_lifetime = \"{default_lifetime}\"\nhosted_revalidation_interval = \"{revalidation}\"\nhit_flush_interval = \"5s\"\n"
        )
    }

    #[test]
    fn the_default_revalidates_a_hosted_context_every_minute() {
        let defaults = JsonLdCacheConfig::default();
        assert_eq!(defaults.hosted_revalidation_interval.as_duration(), Duration::from_secs(60));
    }

    #[test]
    fn the_lifetimes_read_a_human_readable_duration() {
        let parsed: JsonLdCacheConfig = toml::from_str(&table("2h", "5m")).expect("a valid table");
        assert_eq!(parsed.default_lifetime.as_duration(), Duration::from_secs(7200));
        assert_eq!(parsed.hosted_revalidation_interval.as_duration(), Duration::from_secs(300));
    }

    /// A sub-second span would be truncated to zero wherever it is stated in
    /// whole seconds.
    #[test]
    fn a_zero_or_sub_second_lifetime_is_refused_for_both_settings() {
        for (default_lifetime, revalidation) in [("0s", "60s"), ("500ms", "60s"), ("1h", "0s"), ("1h", "500ms")] {
            assert!(
                toml::from_str::<JsonLdCacheConfig>(&table(default_lifetime, revalidation)).is_err(),
                "default_lifetime = {default_lifetime}, hosted_revalidation_interval = {revalidation} was accepted"
            );
        }
    }

    #[test]
    fn the_lifetimes_serialise_in_the_human_readable_form() {
        let rendered = toml::to_string(&JsonLdCacheConfig::default()).expect("the defaults serialise");
        assert!(rendered.contains("default_lifetime = \"1h\""), "{rendered}");
        assert!(rendered.contains("hosted_revalidation_interval = \"1m\""), "{rendered}");
    }

    #[test]
    fn a_zero_capacity_is_refused_as_it_is_read() {
        assert!(toml::from_str::<JsonLdCacheConfig>(&table("1h", "1m").replace("max_entries = 1024", "max_entries = 0")).is_err());
    }
}
