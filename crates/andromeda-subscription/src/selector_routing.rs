//! How one entity selector of a subscription is placed into the candidate index.
//!
//! A selector (clause 5.2.33) names a type expression and, optionally, an
//! entity id or an id pattern. Only a single concrete type and an exact id are
//! indexable; everything else falls back to a broader bucket, and the
//! authoritative matcher re-checks the full selector on every candidate.

use crate::compiled::CompiledSubscription;
use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};

/// Which index buckets a selector was placed in, and therefore which hits an
/// event must produce for the selector to route its subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectorShape {
    /// Indexed under one entity type only; a type hit routes it.
    TypeOnly,
    /// Indexed under one entity id only; an id hit routes it.
    IdOnly,
    /// Indexed under both a type and an id; the event must hit both.
    TypeAndId,
}

/// One indexed selector: the subscription it belongs to, its position among
/// that subscription's selectors, and the buckets it occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectorRef {
    /// The subscription the selector belongs to.
    pub subscription: SubscriptionKey,
    /// The selector's position in `CompiledSubscription::selectors`.
    pub ordinal: usize,
    /// The buckets the selector was placed in.
    pub shape: SelectorShape,
}

/// The bucket placements of every selector of one subscription.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SelectorRouting {
    /// The subscription must be routed for every event: it has no selectors,
    /// or at least one selector whose type expression cannot be reduced to a
    /// single name and which names no exact id.
    pub wildcard: bool,
    /// Type-bucket placements.
    pub by_type: Vec<(EntityTypeName, SelectorRef)>,
    /// Id-bucket placements.
    pub by_entity_id: Vec<(Identifier, SelectorRef)>,
}

impl SelectorRouting {
    /// Decide the placements for a compiled subscription.
    ///
    /// An id pattern is not indexable, so a selector carrying one is placed by
    /// its type alone (or in the wildcard when the type is not a single name);
    /// the matcher applies the pattern afterwards. A selector with a boolean
    /// type expression and an exact id is placed by id alone, since the id is
    /// the more selective half and the matcher re-checks the type.
    #[must_use]
    pub fn of(compiled: &CompiledSubscription) -> SelectorRouting {
        let mut routing = SelectorRouting::default();
        if compiled.selectors.is_empty() {
            routing.wildcard = true;
            return routing;
        }
        for (ordinal, selector) in compiled.selectors.iter().enumerate() {
            let placement = |shape| SelectorRef {
                subscription: compiled.key,
                ordinal,
                shape,
            };
            match (selector.type_.as_single_type(), selector.id.as_ref()) {
                (Some(type_name), Some(id)) => {
                    // clone: the index owns its bucket keys, which the compiled subscription keeps
                    routing.by_type.push((type_name.clone(), placement(SelectorShape::TypeAndId)));
                    routing.by_entity_id.push((id.clone(), placement(SelectorShape::TypeAndId)));
                }
                (Some(type_name), None) => {
                    routing.by_type.push((type_name.clone(), placement(SelectorShape::TypeOnly))); // clone: bucket key
                }
                (None, Some(id)) => {
                    routing.by_entity_id.push((id.clone(), placement(SelectorShape::IdOnly))); // clone: bucket key
                }
                (None, None) => routing.wildcard = true,
            }
        }
        routing
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compiled::CompiledSubscription,
        error::SubscriptionError,
        selector_routing::{SelectorRouting, SelectorShape},
    };
    use andromeda_context_commit::subscription_key::SubscriptionKey;
    use andromeda_ngsi_ld_model::subscription::{EntitySelector, SubscriptionStatus};
    use andromeda_ngsi_ld_query::projection::compiled::Projection;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;
    use url::Url;

    fn internal(error: &impl ToString) -> SubscriptionError {
        SubscriptionError::internal(error.to_string())
    }

    fn selector(type_expression: &str, id: Option<&str>, id_pattern: Option<&str>) -> Result<EntitySelector, SubscriptionError> {
        Ok(EntitySelector {
            id: id.map(str::parse).transpose().map_err(|error| internal(&error))?,
            id_pattern: id_pattern.map(str::parse).transpose().map_err(|error| internal(&error))?,
            type_: type_expression.parse().map_err(|error| internal(&error))?,
        })
    }

    fn subscription(selectors: Vec<EntitySelector>) -> Result<CompiledSubscription, SubscriptionError> {
        Ok(CompiledSubscription {
            id: "urn:ngsi-ld:Subscription:routing".parse().map_err(|error| internal(&error))?,
            key: SubscriptionKey::try_from(9).map_err(|error| internal(&error))?,
            status: SubscriptionStatus::Active,
            endpoint_uri: Url::parse("http://example.com/notify").map_err(|error| internal(&error))?,
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            triggers: Vec::new(),
            watched_attrs: None,
            selectors,
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: None,
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            expires_at: None,
            created_at: DateTime::<Utc>::MIN_UTC,
            compiled_q: None,
            compiled_geo_q: None,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
            ngsild_conformance: None,
            notification_projection: Projection::default(),
        })
    }

    #[test]
    fn a_subscription_without_selectors_is_placed_in_the_wildcard_bucket_only() -> Result<(), SubscriptionError> {
        let routing = SelectorRouting::of(&subscription(Vec::new())?);
        assert!(routing.wildcard);
        assert!(routing.by_type.is_empty() && routing.by_entity_id.is_empty());
        Ok(())
    }

    #[test]
    fn a_single_type_with_an_exact_id_is_placed_in_both_buckets_with_the_conjunctive_shape() -> Result<(), SubscriptionError> {
        let routing = SelectorRouting::of(&subscription(vec![selector("Building", Some("urn:ngsi-ld:E:1"), None)?])?);
        assert!(!routing.wildcard);
        assert_eq!(routing.by_type.len(), 1);
        assert_eq!(routing.by_entity_id.len(), 1);
        assert_eq!(routing.by_type[0].1.shape, SelectorShape::TypeAndId);
        assert_eq!(routing.by_type[0].1, routing.by_entity_id[0].1);
        Ok(())
    }

    #[test]
    fn an_id_pattern_does_not_index_and_a_boolean_type_falls_back_per_selector() -> Result<(), SubscriptionError> {
        let routing = SelectorRouting::of(&subscription(vec![
            selector("Building", None, Some("urn:ngsi-ld:E:.*"))?,
            selector("(Building|Vehicle)", Some("urn:ngsi-ld:E:2"), None)?,
            selector("(Building|Vehicle)", None, Some(".*"))?,
        ])?);
        assert!(routing.wildcard, "the boolean type without an id must route every event");
        assert_eq!(routing.by_type.len(), 1);
        assert_eq!(routing.by_type[0].1.shape, SelectorShape::TypeOnly);
        assert_eq!(routing.by_type[0].1.ordinal, 0);
        assert_eq!(routing.by_entity_id.len(), 1);
        assert_eq!(routing.by_entity_id[0].1.shape, SelectorShape::IdOnly);
        assert_eq!(routing.by_entity_id[0].1.ordinal, 1);
        Ok(())
    }
}
