//! How a deployment configures context resolution.
//!
//! The settings live with the crate that reads them rather than in a central
//! configuration module, so adding one here cannot be forgotten there.
//! Composing this into a broker-wide configuration, and deciding where the
//! configuration file lives, remain the binary's.

use crate::{
    context_cache::ContextCache,
    core_context::NGSI_LD_CORE_CONTEXT_V19_URL,
    engine::{builder::JsonLdEngineBuilder, processor::JsonLdEngine},
    error::JsonLdError,
};
use confique::Config;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use url::Url;

/// A setting that must be positive and was not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum JsonLdConfigError {
    /// `timeout_seconds` was zero, which would abandon every fetch instantly.
    #[error("jsonld.timeout_seconds must be at least 1")]
    ZeroTimeout,
    /// `max_context_bytes` was zero, which would refuse every context.
    #[error("jsonld.max_context_bytes must be at least 1")]
    ZeroMaxContextBytes,
    /// `cache_capacity` was zero, which would cache nothing.
    #[error("jsonld.cache_capacity must be at least 1")]
    ZeroCacheCapacity,
    /// `cache_ttl_seconds` was zero, which would expire every entry at once.
    #[error("jsonld.cache_ttl_seconds must be at least 1")]
    ZeroCacheTtl,
}

/// Context resolution settings.
#[derive(Config, Debug, Clone, Serialize)]
pub struct JsonLdConfig {
    /// Whether a context that is neither embedded nor cached may be fetched.
    #[config(env = "ANDROMEDA_JSONLD_ALLOW_REMOTE_CONTEXTS", default = true)]
    pub allow_remote_contexts: bool,

    /// How long a context fetch may take.
    #[config(env = "ANDROMEDA_JSONLD_TIMEOUT_SECONDS", default = 30)]
    pub timeout_seconds: u64,

    /// How many redirects a context fetch may follow.
    #[config(env = "ANDROMEDA_JSONLD_MAX_REDIRECTS", default = 5)]
    pub max_redirects: u32,

    /// The size beyond which a fetched context is refused.
    #[config(env = "ANDROMEDA_JSONLD_MAX_CONTEXT_BYTES", default = 2_000_000)]
    pub max_context_bytes: u64,

    /// How many entries the in-process caches hold.
    #[config(env = "ANDROMEDA_JSONLD_CACHE_CAPACITY", default = 1024)]
    pub cache_capacity: u64,

    /// How long an in-process cache entry lives.
    #[config(env = "ANDROMEDA_JSONLD_CACHE_TTL_SECONDS", default = 3600)]
    pub cache_ttl_seconds: u64,

    /// Contexts to read from disk at startup, as URL to file path.
    #[config(env = "ANDROMEDA_JSONLD_PRELOAD_CONTEXTS", parse_env = parse_url_to_path_map, default = {})]
    pub preload_contexts: HashMap<Url, PathBuf>,
}

impl JsonLdConfig {
    /// Check the settings that must be positive.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdConfigError`] naming the first setting that is
    /// zero.
    pub const fn validate(&self) -> Result<(), JsonLdConfigError> {
        if self.timeout_seconds == 0 {
            return Err(JsonLdConfigError::ZeroTimeout);
        }
        if self.max_context_bytes == 0 {
            return Err(JsonLdConfigError::ZeroMaxContextBytes);
        }
        if self.cache_capacity == 0 {
            return Err(JsonLdConfigError::ZeroCacheCapacity);
        }
        if self.cache_ttl_seconds == 0 {
            return Err(JsonLdConfigError::ZeroCacheTtl);
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

    /// Build the engine these settings describe.
    ///
    /// # Errors
    ///
    /// Returns whatever [`JsonLdEngineBuilder::build`] returns: an embedded
    /// context that will not parse, a preloaded file that cannot be read or
    /// parsed, or an HTTP client that cannot be built.
    pub fn build_engine(&self, l2_cache: Arc<impl ContextCache + 'static>) -> Result<JsonLdEngine, JsonLdError> {
        // Pin the deployment to the latest released core context (v1.9), the
        // version the compile-time vocabulary table is derived from, so the
        // fastpath term table and the deployment agree (clause 4.3.6.8). This
        // replaces the process-global the engine used to read; the pin now lives
        // on the engine each tenant builds.
        let mut builder = JsonLdEngineBuilder::new()
            .allow_remote(self.allow_remote_contexts)
            .timeout_seconds(self.timeout_seconds)
            .max_redirects(self.max_redirects)
            .cache_capacity(self.cache_capacity)
            .cache_ttl_seconds(self.cache_ttl_seconds)
            .max_context_bytes(self.max_context_bytes)
            .default_core_url(NGSI_LD_CORE_CONTEXT_V19_URL)
            .with_l2_cache(l2_cache);
        if !self.preload_contexts.is_empty() {
            builder = builder.preload_contexts(self.preload_contexts.clone()); // clone: the configuration is borrowed and stays valid
        }
        builder.build()
    }
}

/// Read the preload map from an environment variable holding JSON.
fn parse_url_to_path_map(input: &str) -> Result<HashMap<Url, PathBuf>, serde_json::Error> {
    serde_json::from_str(input)
}

#[cfg(test)]
mod tests {
    use crate::config::{JsonLdConfig, JsonLdConfigError, parse_url_to_path_map};
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
            timeout_seconds: 30,
            max_redirects: 5,
            max_context_bytes: 2_000_000,
            cache_capacity: 1024,
            cache_ttl_seconds: 3600,
            preload_contexts: HashMap::new(),
        }
    }

    #[test]
    fn a_valid_configuration_passes() {
        assert!(valid_config().validate().is_ok());
    }

    /// One way to zero out a setting, paired with the failure that must
    /// follow.
    type ZeroedSetting = (fn(&mut JsonLdConfig), JsonLdConfigError);

    #[test]
    fn each_setting_that_must_be_positive_reports_itself_by_name() {
        let cases: [ZeroedSetting; 4] = [
            (|config| config.timeout_seconds = 0, JsonLdConfigError::ZeroTimeout),
            (|config| config.max_context_bytes = 0, JsonLdConfigError::ZeroMaxContextBytes),
            (|config| config.cache_capacity = 0, JsonLdConfigError::ZeroCacheCapacity),
            (|config| config.cache_ttl_seconds = 0, JsonLdConfigError::ZeroCacheTtl),
        ];
        for (zero_out, expected) in cases {
            let mut config = valid_config();
            zero_out(&mut config);
            pretty_assertions::assert_eq!(config.validate(), Err(expected));
        }
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

    #[test]
    fn the_preload_map_is_read_from_json_in_the_environment() {
        let parsed = parse_url_to_path_map(r#"{"https://example.com/context.jsonld":"contexts/test.jsonld"}"#).expect("valid JSON");

        pretty_assertions::assert_eq!(parsed.get(&context_url()), Some(&PathBuf::from("contexts/test.jsonld")));
    }

    #[test]
    fn a_preload_map_with_a_malformed_context_url_is_rejected() {
        assert!(parse_url_to_path_map(r#"{"not a URL":"contexts/test.jsonld"}"#).is_err());
    }
}
