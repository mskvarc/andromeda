//! The `[jsonld]` table: how `@context` documents are resolved.

pub mod cache;
pub mod fetch;

use crate::schema::jsonld::{cache::JsonLdCacheConfig, fetch::JsonLdFetchConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

/// One `@context` read from a file at startup instead of being downloaded.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(description = "One `@context` read from a file at startup instead of being downloaded.")]
pub struct PreloadedContext {
    /// The URL the context is known by.
    #[schemars(description = "The URL the context is known by. A preloaded URL is never downloaded.")]
    pub url: Url,
    /// The file it is read from. A relative path is read relative to the
    /// configuration file.
    #[schemars(
        title = "path",
        description = "The file the context is read from, which must exist and hold a valid `@context` document. A relative path that does not resolve from the working directory is read relative to the configuration file's directory."
    )]
    pub path: PathBuf,
}

/// How `@context` documents are resolved.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
#[schemars(description = "How the broker downloads, caches and preloads the `@context` documents requests refer to.")]
pub struct JsonLdConfig {
    /// Contexts read from files at startup, one `[[jsonld.preloaded_contexts]]`
    /// table each, with its `url` and `path`. Set in the file only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(
        description = "Contexts read from files at startup, one `[[jsonld.preloaded_contexts]]` table each with its `url` and `path`. Set in the file only. Left out, no context is preloaded."
    )]
    pub preloaded_contexts: Vec<PreloadedContext>,
    /// Whether and how a remote `@context` is downloaded.
    #[schemars(description = "Whether and how a remote `@context` is downloaded.")]
    pub fetch: JsonLdFetchConfig,
    /// How long resolved contexts are kept in memory.
    #[schemars(description = "How long resolved contexts are kept in memory.")]
    pub cache: JsonLdCacheConfig,
}

impl JsonLdConfig {
    /// Resolve preloaded context paths against the directory the
    /// configuration file lives in.
    ///
    /// A path that is already absolute, or that resolves from the working
    /// directory, is left alone; anything else is read relative to the
    /// configuration file, which is where an operator writing it would expect
    /// a relative path to point.
    pub fn resolve_preload_paths(&mut self, config_dir: &Path) {
        self.preloaded_contexts
            .iter_mut()
            .filter(|context| !context.path.is_absolute() && !context.path.exists())
            .for_each(|context| context.path = config_dir.join(&context.path));
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::jsonld::{JsonLdConfig, PreloadedContext};
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};
    use url::Url;

    fn preloaded(path: &str) -> PreloadedContext {
        PreloadedContext {
            url: Url::parse("https://example.com/context.jsonld").expect("valid context URL"),
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn a_relative_preload_path_resolves_against_the_configuration_directory() {
        let mut config = JsonLdConfig {
            preloaded_contexts: vec![preloaded("contexts/test.jsonld")],
            ..JsonLdConfig::default()
        };
        config.resolve_preload_paths(Path::new("/etc/andromeda"));
        assert_eq!(config.preloaded_contexts[0].path, PathBuf::from("/etc/andromeda/contexts/test.jsonld"));
    }

    #[test]
    fn an_absolute_preload_path_is_left_alone() {
        let mut config = JsonLdConfig {
            preloaded_contexts: vec![preloaded("/opt/contexts/test.jsonld")],
            ..JsonLdConfig::default()
        };
        config.resolve_preload_paths(Path::new("/etc/andromeda"));
        assert_eq!(config.preloaded_contexts[0].path, PathBuf::from("/opt/contexts/test.jsonld"));
    }

    #[test]
    fn preloaded_contexts_are_an_array_of_tables() {
        let config: JsonLdConfig = toml::from_str(&format!(
            "{}\n[[preloaded_contexts]]\nurl = \"https://example.com/a.jsonld\"\npath = \"a.jsonld\"\n[[preloaded_contexts]]\nurl = \"https://example.com/b.jsonld\"\npath = \"b.jsonld\"\n",
            toml::to_string(&JsonLdConfig::default()).expect("the defaults render")
        ))
        .expect("an array of tables");
        assert_eq!(config.preloaded_contexts.len(), 2);
    }
}
