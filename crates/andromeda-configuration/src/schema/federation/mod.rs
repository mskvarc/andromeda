//! The `[federation]` tables: forwarding to Context Sources, caching their
//! answers, holding back the failing ones, and this broker's own identity.

pub mod cooldown;
pub mod forwarding;
pub mod identity;

use crate::schema::federation::{cooldown::CooldownConfig, forwarding::ForwardingConfig, identity::IdentityConfig};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use toml_comment::TomlComment;

/// The per-tenant cache of forwarded `GET` responses (clause 6.3.17).
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResponseCacheConfig {
    /// The most responses one tenant's cache holds.
    #[toml_comment(inline)]
    pub max_entries: NonZeroUsize,
}

impl Default for ResponseCacheConfig {
    fn default() -> ResponseCacheConfig {
        ResponseCacheConfig {
            max_entries: NonZeroUsize::MIN.saturating_add(1023),
        }
    }
}

/// Forwarding to Context Sources, caching their answers, holding back the
/// failing ones, and this broker's own identity.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct FederationConfig {
    /// How requests are forwarded to Context Sources.
    pub forwarding: ForwardingConfig,
    /// The per-tenant cache of forwarded responses.
    pub response_cache: ResponseCacheConfig,
    /// How a failing Context Source is held back. The cooldown schedule is
    /// checked by the forwarding layer that builds it.
    pub cooldown: CooldownConfig,
    /// The identity this broker reports as a Context Source.
    pub identity: IdentityConfig,
}
