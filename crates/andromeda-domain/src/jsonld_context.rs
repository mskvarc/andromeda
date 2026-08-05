//! The `@context` resources the broker caches, hosts, and serves (ETSI GS CIM 009 clause 5.13).

use crate::uri::Uri;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};
use typed_builder::TypedBuilder;
use url::Url;

/// Kind of JSON-LD @context managed by the broker (ETSI GS CIM 009 clause 5.13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Display, EnumString, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "PascalCase")]
pub enum ContextKind {
    /// Fetched from its origin and held in the broker's cache.
    Cached,
    /// Uploaded to the broker, which serves it under its own URL.
    Hosted,
    /// Created by the broker when a payload inlined a context body.
    ImplicitlyCreated,
}

/// Detailed @context metadata response (for details=true).
#[derive(Debug, Serialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct ContextMetadata {
    /// Origin URL, absent for contexts the broker itself created.
    #[serde(rename = "URL")]
    #[builder(default)]
    pub url: Option<Url>,
    /// Identifier under which the broker serves this context.
    pub local_id: Uri,
    /// How the broker came to hold this context.
    pub kind: ContextKind,
    /// When the broker first stored it.
    pub created_at: DateTime<Utc>,
    /// When the cached copy stops being valid, if it expires at all.
    #[builder(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// When it was last read.
    pub last_usage: DateTime<Utc>,
    /// How many times it has been read.
    pub number_of_hits: u64,
    /// Implementation-specific extra information (clause 5.13).
    #[builder(default)]
    pub extra_info: Option<Value>,
}

/// Service-layer DTO: list result as URLs or full metadata records.
#[derive(Debug)]
pub enum ContextListResult {
    /// List of context URLs or IDs.
    Urls(Vec<String>),
    /// List of context metadata records with full details.
    Detailed(Vec<ContextMetadata>),
}

/// Service-layer DTO: single-context fetch result (payload or metadata).
#[derive(Debug)]
pub enum ContextGetResult {
    /// The raw JSON-LD context payload.
    Payload(Value),
    /// Full context metadata including usage statistics.
    Detailed(Box<ContextMetadata>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    /// Labels of the PostgreSQL `jsonld_context_kind` enum, verbatim from
    /// `migrations/tenant/0003_types.sql`. Unlike the other broker enums this
    /// one is PascalCase, matching clause 5.13's own casing.
    const CONTEXT_KIND_LABELS: [(&str, ContextKind); 3] = [
        ("Cached", ContextKind::Cached),
        ("Hosted", ContextKind::Hosted),
        ("ImplicitlyCreated", ContextKind::ImplicitlyCreated),
    ];

    #[test]
    fn context_kind_parses_from_its_postgres_label() {
        for (label, expected) in CONTEXT_KIND_LABELS {
            pretty_assertions::assert_eq!(label.parse::<ContextKind>().expect(label), expected);
        }
    }

    #[test]
    fn context_kind_renders_the_label_postgres_expects() {
        for (label, variant) in CONTEXT_KIND_LABELS {
            pretty_assertions::assert_eq!(variant.to_string(), label);
            pretty_assertions::assert_eq!(<&str>::from(&variant), label);
        }
    }

    #[test]
    fn context_kind_round_trips_through_its_label_for_every_variant() {
        // Exhaustive over `EnumIter`, replacing the type-level guarantee
        // `sqlx::Type` used to give.
        for variant in ContextKind::iter() {
            pretty_assertions::assert_eq!(variant.to_string().parse::<ContextKind>().expect("round trip"), variant);
            assert!(
                CONTEXT_KIND_LABELS.iter().any(|(_, known)| *known == variant),
                "{variant:?} has no PostgreSQL label in jsonld_context_kind"
            );
        }
    }

    #[test]
    fn an_unknown_context_kind_label_is_a_parse_error_not_a_panic() {
        assert!("NotAKind".parse::<ContextKind>().is_err());
    }
}
