//! How a deployment configures context resolution.
//!
//! The settings live with the crate that reads them rather than in a central
//! configuration module, so adding one here cannot be forgotten there.
//! Composing this into a broker-wide configuration, and deciding where the
//! configuration file lives, remain the binary's.

use crate::{
    context_cache::ContextCache,
    engine::{builder::JsonLdEngineBuilder, processor::JsonLdEngine},
    error::JsonLdError,
    hosted_context_recognizer::HostedContextRecognizer,
    loader::cache_lifetime::CacheLifetime,
};
use andromeda_configuration_scalars::{human_byte_size::HumanByteSize, human_duration::HumanDuration};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use toml_comment::TomlComment;
use url::Url;

/// A setting that must be positive and was not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum JsonLdConfigError {
    /// `timeout` was below one second, which would abandon nearly every fetch
    /// before a remote origin server could answer.
    #[error("jsonld.timeout must be at least 1 second")]
    TimeoutBelowOneSecond,
    /// `max_context` was zero, which would refuse every context.
    #[error("jsonld.max_context must be at least 1 byte")]
    ZeroMaxContext,
    /// `cache_capacity` was zero, which would cache nothing.
    #[error("jsonld.cache_capacity must be at least 1")]
    ZeroCacheCapacity,
}

/// Context resolution settings.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone)]
pub struct JsonLdConfig {
    /// Whether a context that is neither embedded nor cached may be fetched.
    pub allow_remote_contexts: bool,

    /// How long a context fetch may take, in human-readable form (`"30s"`).
    /// A span below one second is refused when the configuration is validated.
    #[toml_comment(inline)]
    pub timeout: HumanDuration,

    /// How many redirects a context fetch may follow.
    pub max_redirects: u32,

    /// The size beyond which a fetched context is refused, in human-readable
    /// form (`"2 MB"`).
    #[toml_comment(inline)]
    pub max_context: HumanByteSize,

    /// How many entries the in-process caches hold.
    pub cache_capacity: u64,

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
    /// refused when the configuration is read.
    #[toml_comment(inline)]
    pub cache_ttl: CacheLifetime,

    /// How long this process uses its in-memory copy of an `@context` the
    /// broker stores itself before reading the stored row again, in
    /// human-readable form (`"60s"`).
    ///
    /// Covers the `"Hosted"` and `"ImplicitlyCreated"` entries of clause
    /// 5.13.1, which no origin server can make stale but another broker process
    /// sharing the database can delete. The same span is the `max-age` the
    /// Serve response advertises for them, so a remote party caching the
    /// served document is held to it too. Shorter spans see such a change
    /// sooner at the cost of one primary-key read per entry per span. A span of
    /// zero or below one second is refused when the configuration is read,
    /// since `max-age` states whole seconds.
    #[toml_comment(inline)]
    pub hosted_context_revalidation: CacheLifetime,

    /// Contexts to read from disk at startup, as URL to file path.
    pub preload_contexts: HashMap<Url, PathBuf>,
}

impl Default for JsonLdConfig {
    fn default() -> JsonLdConfig {
        JsonLdConfig {
            allow_remote_contexts: true,
            timeout: HumanDuration::from_secs(30),
            max_redirects: 5,
            max_context: HumanByteSize::b(2_000_000),
            cache_capacity: 1024,
            cache_ttl: CacheLifetime::DEFAULT,
            hosted_context_revalidation: CacheLifetime::HOSTED_REVALIDATION_DEFAULT,
            preload_contexts: HashMap::new(),
        }
    }
}

impl JsonLdConfig {
    /// Check the settings that must be positive.
    ///
    /// The two lifetimes need no check here: [`CacheLifetime`] cannot hold a
    /// zero or sub-second span, so reading the configuration already refused
    /// one.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdConfigError`] naming the first setting that is
    /// zero, or the fetch timeout when it is below one second.
    pub fn validate(&self) -> Result<(), JsonLdConfigError> {
        if self.timeout.duration() < Duration::from_secs(1) {
            return Err(JsonLdConfigError::TimeoutBelowOneSecond);
        }
        if self.max_context.as_u64() == 0 {
            return Err(JsonLdConfigError::ZeroMaxContext);
        }
        if self.cache_capacity == 0 {
            return Err(JsonLdConfigError::ZeroCacheCapacity);
        }
        Ok(())
    }

    /// Resolve preloaded context paths against the directory the
    /// configuration file lives in.
    ///
    /// A path that is already absolute, or that resolves from the working
    /// directory, is left alone; anything else is read relative to the
    /// configuration file, which is where an operator writing it would expect
    /// a relative path to point.
    pub fn resolve_preload_paths(&mut self, config_dir: &Path) {
        for path in self.preload_contexts.values_mut() {
            let candidate = path.clone(); // clone: the map entry is replaced only for a relative path
            if candidate.is_absolute() || candidate.exists() {
                continue;
            }
            *path = config_dir.join(candidate);
        }
    }

    /// Build the engine these settings describe for the tenant whose stored
    /// `@context`s `hosted` recognises.
    ///
    /// # Errors
    ///
    /// Returns whatever [`JsonLdEngineBuilder::build`] returns: an embedded
    /// context that will not parse, a preloaded file that cannot be read or
    /// parsed, or an HTTP client that cannot be built.
    pub fn build_engine(&self, l2_cache: Arc<impl ContextCache + 'static>, hosted: Arc<dyn HostedContextRecognizer>) -> Result<JsonLdEngine, JsonLdError> {
        let mut builder = JsonLdEngineBuilder::new()
            .allow_remote(self.allow_remote_contexts)
            .timeout(self.timeout.duration())
            .max_redirects(self.max_redirects)
            .cache_capacity(self.cache_capacity)
            .cache_ttl(self.cache_ttl)
            .hosted_context_revalidation(self.hosted_context_revalidation)
            .max_context_bytes(self.max_context.as_u64())
            .with_l2_cache(l2_cache)
            .hosted_contexts(hosted);
        if !self.preload_contexts.is_empty() {
            builder = builder.preload_contexts(self.preload_contexts.clone()); // clone: the configuration is borrowed and stays valid
        }
        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::{JsonLdConfig, JsonLdConfigError},
        loader::cache_lifetime::CacheLifetime,
    };
    use andromeda_configuration_scalars::{human_byte_size::HumanByteSize, human_duration::HumanDuration};
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
    };
    use url::Url;

    fn context_url() -> Url {
        Url::parse("https://example.com/context.jsonld").expect("valid context URL")
    }

    /// Settings that pass validation, for the tests that vary one field.
    fn valid_config() -> JsonLdConfig {
        JsonLdConfig {
            allow_remote_contexts: true,
            timeout: HumanDuration::from_secs(30),
            max_redirects: 5,
            max_context: HumanByteSize::b(2_000_000),
            cache_capacity: 1024,
            cache_ttl: CacheLifetime::DEFAULT,
            hosted_context_revalidation: CacheLifetime::HOSTED_REVALIDATION_DEFAULT,
            preload_contexts: HashMap::new(),
        }
    }

    /// A `[jsonld]` section with the two lifetimes set to `cache_ttl` and
    /// `revalidation`.
    fn section(cache_ttl: &str, revalidation: &str) -> String {
        format!(
            r#"
            allow_remote_contexts = true
            timeout = "30s"
            max_redirects = 5
            max_context = "2 MB"
            cache_capacity = 1024
            cache_ttl = "{cache_ttl}"
            hosted_context_revalidation = "{revalidation}"
            preload_contexts = {{}}
            "#
        )
    }

    #[test]
    fn a_valid_configuration_passes() {
        assert!(valid_config().validate().is_ok());
    }

    /// A stored `@context` is re-read once a minute unless configured
    /// otherwise, and the default configuration passes its own validation.
    #[test]
    fn the_default_revalidates_a_hosted_context_every_minute() {
        let defaults = JsonLdConfig::default();

        pretty_assertions::assert_eq!(
            defaults.hosted_context_revalidation,
            CacheLifetime::from_seconds(60).expect("a minute is a positive lifetime")
        );
        assert!(defaults.validate().is_ok());
    }

    /// Both lifetimes read the human-readable duration form every other
    /// duration in this section uses.
    #[test]
    fn the_lifetimes_read_a_human_readable_duration() {
        let parsed: JsonLdConfig = toml::from_str(&section("2h", "5m")).expect("a valid [jsonld] section");

        pretty_assertions::assert_eq!(parsed.cache_ttl, CacheLifetime::from_seconds(7200).expect("two hours"));
        pretty_assertions::assert_eq!(parsed.hosted_context_revalidation, CacheLifetime::from_seconds(300).expect("five minutes"));
    }

    /// A zero or sub-second lifetime is refused when the configuration is
    /// read, for both settings: a sub-second span would be truncated to zero
    /// wherever it is stated in whole seconds.
    #[test]
    fn a_zero_or_sub_second_lifetime_is_refused_for_both_settings() {
        for (cache_ttl, revalidation) in [("0s", "60s"), ("500ms", "60s"), ("1h", "0s"), ("1h", "500ms")] {
            let parsed = toml::from_str::<JsonLdConfig>(&section(cache_ttl, revalidation));
            assert!(
                parsed.is_err(),
                "cache_ttl = {cache_ttl}, hosted_context_revalidation = {revalidation} was accepted"
            );
        }
    }

    /// A configured lifetime serialises back to the human-readable form, so
    /// the effective-configuration dump reads as the operator wrote it.
    #[test]
    fn the_lifetimes_serialise_in_the_human_readable_form() {
        let rendered = toml::to_string(&JsonLdConfig::default()).expect("the defaults serialise");

        assert!(rendered.contains("cache_ttl = \"1h\""), "{rendered}");
        assert!(rendered.contains("hosted_context_revalidation = \"1m\""), "{rendered}");
    }

    /// One way to zero out a setting, paired with the failure that must
    /// follow.
    type ZeroedSetting = (fn(&mut JsonLdConfig), JsonLdConfigError);

    #[test]
    fn each_setting_that_must_be_positive_reports_itself_by_name() {
        let cases: [ZeroedSetting; 3] = [
            (|config| config.timeout = HumanDuration::from_secs(0), JsonLdConfigError::TimeoutBelowOneSecond),
            (|config| config.max_context = HumanByteSize::b(0), JsonLdConfigError::ZeroMaxContext),
            (|config| config.cache_capacity = 0, JsonLdConfigError::ZeroCacheCapacity),
        ];
        for (zero_out, expected) in cases {
            let mut config = valid_config();
            zero_out(&mut config);
            pretty_assertions::assert_eq!(config.validate(), Err(expected));
        }
    }

    /// A fetch timeout below one second is refused rather than truncated to
    /// the zero seconds a whole-second reading would give it.
    #[test]
    fn a_sub_second_fetch_timeout_is_refused() {
        let parsed: JsonLdConfig =
            toml::from_str(&section("1h", "60s").replace("timeout = \"30s\"", "timeout = \"500ms\"")).expect("a readable [jsonld] section");

        pretty_assertions::assert_eq!(parsed.validate(), Err(JsonLdConfigError::TimeoutBelowOneSecond));
    }

    #[test]
    fn a_relative_preload_path_resolves_against_the_configuration_directory() {
        let mut config = valid_config();
        config.preload_contexts.insert(context_url(), PathBuf::from("contexts/test.jsonld"));

        config.resolve_preload_paths(Path::new("/etc/andromeda"));

        pretty_assertions::assert_eq!(
            config.preload_contexts.get(&context_url()),
            Some(&PathBuf::from("/etc/andromeda/contexts/test.jsonld"))
        );
    }

    #[test]
    fn an_absolute_preload_path_is_left_alone() {
        let mut config = valid_config();
        config.preload_contexts.insert(context_url(), PathBuf::from("/opt/contexts/test.jsonld"));

        config.resolve_preload_paths(Path::new("/etc/andromeda"));

        pretty_assertions::assert_eq!(config.preload_contexts.get(&context_url()), Some(&PathBuf::from("/opt/contexts/test.jsonld")));
    }
}
