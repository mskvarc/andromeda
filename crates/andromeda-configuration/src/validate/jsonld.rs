//! The `[jsonld]` rules.

use crate::{
    schema::{configured_lifetime::ConfiguredLifetime, jsonld::JsonLdConfig},
    validate::{
        bound::{Quantity, Relation},
        config_key::ConfigKey,
        rule::{CompoundRule, Limit, Rule},
        violation::ConfigurationViolation,
        violation_log::ViolationLog,
    },
};
use std::{collections::HashSet, time::Duration};

/// The shortest download deadline: below it nearly every fetch is abandoned
/// before a remote origin server could answer.
const MIN_FETCH_TIMEOUT: Duration = Duration::from_secs(1);

/// A download needs a deadline of a second and room for one byte, and the hit
/// counters a positive flush cadence. A cache lifetime is advertised as
/// `max-age` in whole seconds, so it is at least one.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [
        Rule::Ordered {
            key: ConfigKey::JsonLdFetchTimeout,
            relation: Relation::AtLeast,
            limit: Limit::Fixed(Quantity::Duration(MIN_FETCH_TIMEOUT)),
        },
        Rule::Positive(ConfigKey::JsonLdFetchMaxSize),
        Rule::Positive(ConfigKey::JsonLdCacheHitFlushInterval),
        Rule::Ordered {
            key: ConfigKey::JsonLdCacheDefaultLifetime,
            relation: Relation::AtLeast,
            limit: Limit::Fixed(Quantity::Duration(ConfiguredLifetime::MINIMUM)),
        },
        Rule::Ordered {
            key: ConfigKey::JsonLdCacheHostedRevalidationInterval,
            relation: Relation::AtLeast,
            limit: Limit::Fixed(Quantity::Duration(ConfiguredLifetime::MINIMUM)),
        },
    ]
    .into_iter()
}

/// What [`check_jsonld`] asks.
pub(crate) const COMPOUND_RULES: [CompoundRule; 1] = [CompoundRule {
    keys: &[ConfigKey::JsonLdPreloadedContextUrl],
    statement: "No two preloaded contexts name the same URL.",
}];

/// A preloaded URL has one file to serve it.
pub(crate) fn check_jsonld(jsonld: &JsonLdConfig, log: &mut ViolationLog) {
    let mut named = HashSet::with_capacity(jsonld.preloaded_contexts.len());
    let mut reported = HashSet::new();
    jsonld
        .preloaded_contexts
        .iter()
        .map(|context| &context.url)
        .filter(|url| !named.insert(*url) && reported.insert(*url))
        .for_each(|url| log.record(ConfigurationViolation::DuplicatePreloadedContext { url: url.clone() })); // clone: the violation outlives the configuration it reports on
}

#[cfg(test)]
mod tests {
    use crate::{
        broker_config::BrokerConfig,
        human_byte_size::HumanByteSize,
        human_duration::HumanDuration,
        schema::jsonld::PreloadedContext,
        validate::{
            bound::{Bound, Quantity, Relation},
            config_key::ConfigKey,
            rule_tests::{assert_accepted, assert_positive_duration, refusals},
            violation::ConfigurationViolation,
        },
    };
    use pretty_assertions::assert_eq;
    use std::{path::PathBuf, time::Duration};
    use url::Url;
    use url_macro::url;

    #[test]
    fn a_fetch_timeout_below_one_second_is_refused_and_one_second_accepted() {
        let mut config = BrokerConfig::local();
        config.jsonld.fetch.timeout = HumanDuration::from_millis(999);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::OutOfRange {
                key: ConfigKey::JsonLdFetchTimeout,
                value: Quantity::Duration(Duration::from_millis(999)),
                relation: Relation::AtLeast,
                bound: Bound::Fixed(Quantity::Duration(Duration::from_secs(1))),
            }]
        );
        config.jsonld.fetch.timeout = HumanDuration::from_secs(1);
        assert_accepted(&config);
    }

    #[test]
    fn a_zero_fetch_size_is_refused_and_one_byte_accepted() {
        let mut config = BrokerConfig::local();
        config.jsonld.fetch.max_size = HumanByteSize::b(0);
        assert_eq!(
            refusals(&config),
            [ConfigurationViolation::Zero {
                key: ConfigKey::JsonLdFetchMaxSize
            }]
        );
        config.jsonld.fetch.max_size = HumanByteSize::b(1);
        assert_accepted(&config);
    }

    #[test]
    fn a_zero_hit_flush_interval_is_refused() {
        assert_positive_duration(
            |config, value| config.jsonld.cache.hit_flush_interval = value,
            ConfigKey::JsonLdCacheHitFlushInterval,
        );
    }

    /// A URL named three times is one violation.
    #[test]
    fn a_preloaded_url_named_twice_is_refused_once_and_distinct_urls_accepted() {
        let url = url!("https://example.org/context.jsonld");
        let other = url!("https://example.org/other.jsonld");
        let context = |url: &Url| PreloadedContext {
            url: url.clone(),
            path: PathBuf::from("context.jsonld"),
        };
        let mut config = BrokerConfig::local();
        config.jsonld.preloaded_contexts = vec![context(&url), context(&url), context(&url)];
        assert_eq!(refusals(&config), [ConfigurationViolation::DuplicatePreloadedContext { url: url.clone() }]);
        config.jsonld.preloaded_contexts = vec![context(&url), context(&other)];
        assert_accepted(&config);
    }
}
