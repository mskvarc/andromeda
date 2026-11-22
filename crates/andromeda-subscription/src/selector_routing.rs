//! How one entity selector of a subscription is placed into the candidate index.
//!
//! A selector (clause 5.2.33) names a type selection and, optionally, entity
//! ids or an id pattern. A type selection other than the wildcard is filed
//! under its covering types, one type of each of its clause 4.17 alternatives,
//! one of which every Entity it selects carries; an exact id is filed under
//! itself; an id pattern is not filed. A selector of the wildcard type naming
//! no id, and a subscription with no selector, fall back to the wildcard
//! bucket. The authoritative matcher re-checks the full selector on every
//! candidate, so a conjunction filed under one of its types still rejects an
//! Entity carrying that type alone.

use crate::compiled::CompiledSubscription;
use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_lexicon::{entity_type_iri::EntityTypeIri, identifier::Identifier};

/// Which index buckets a selector was placed in, and therefore which hits an
/// event must produce for the selector to route its subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectorShape {
    /// Indexed under entity types only; a hit on any of them routes it.
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
    /// or at least one selector of the wildcard type that names no exact id.
    pub wildcard: bool,
    /// Type-bucket placements.
    pub by_type: Vec<(EntityTypeIri, SelectorRef)>,
    /// Id-bucket placements.
    pub by_entity_id: Vec<(Identifier, SelectorRef)>,
}

impl SelectorRouting {
    /// Decide the placements for a compiled subscription.
    ///
    /// An id pattern is not indexable, so a selector carrying one is placed by
    /// its type alone (or in the wildcard when its type is the wildcard); the
    /// matcher applies the pattern afterwards. A selector with exact ids whose
    /// type selection one type covers is placed under that type and its ids
    /// together. One with exact ids and several covering types is placed by
    /// its ids alone, since they are the more selective half and the matcher
    /// re-checks the type.
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
            let covering = selector.type_.covering_types();
            // A selector listing several identifiers occupies the bucket of
            // each, and an event hits the bucket of its own identifier.
            match (covering.as_deref(), selector.id.as_deref().unwrap_or_default()) {
                (Some([type_name]), ids @ [_, ..]) => {
                    // clone: the index owns its bucket keys, which the compiled subscription keeps
                    routing.by_type.push((EntityTypeIri::clone(type_name), placement(SelectorShape::TypeAndId)));
                    routing
                        .by_entity_id
                        .extend(ids.iter().map(|id| (id.clone(), placement(SelectorShape::TypeAndId))));
                }
                (Some(types), []) => {
                    // clone: the index takes owned keys while the compiled subscription keeps its own
                    routing.by_type.extend(
                        types
                            .iter()
                            .map(|type_name| (EntityTypeIri::clone(type_name), placement(SelectorShape::TypeOnly))),
                    );
                }
                (Some([] | [_, _, ..]) | None, ids @ [_, ..]) => {
                    // clone: the index takes owned keys while the compiled subscription keeps its own
                    routing.by_entity_id.extend(ids.iter().map(|id| (id.clone(), placement(SelectorShape::IdOnly))));
                }
                (None, []) => routing.wildcard = true,
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
        geo_filter::SubscriptionGeoFilter,
        notifying_since::NotifyingSince,
        selector_routing::{SelectorRouting, SelectorShape},
    };
    use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
    use andromeda_ngsi_ld_model::{
        entity_selector::EntitySelector,
        subscription::{SubscriptionStatus, standing::SubscriptionStanding},
    };
    use andromeda_ngsi_ld_query::{name_stage::Expanded, projection::expanded_projection::ExpandedProjection};
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;
    use url::Url;

    fn internal(error: &impl ToString) -> SubscriptionError {
        panic!("invalid test fixture: {}", error.to_string())
    }

    fn selector(type_expression: &str, ids: &[&str], id_pattern: Option<&str>) -> Result<EntitySelector<Expanded>, SubscriptionError> {
        let ids = ids
            .iter()
            .map(|id| id.parse())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| internal(&error))?;
        Ok(EntitySelector {
            id: (!ids.is_empty()).then_some(ids),
            id_pattern: id_pattern.map(str::parse).transpose().map_err(|error| internal(&error))?,
            type_: type_expression.parse().map_err(|error| internal(&error))?,
        })
    }

    fn subscription(selectors: Vec<EntitySelector<Expanded>>) -> Result<CompiledSubscription, SubscriptionError> {
        Ok(CompiledSubscription {
            id: "urn:ngsi-ld:Subscription:routing".parse().map_err(|error| internal(&error))?,
            key: SubscriptionKey::try_from(9).map_err(|error| internal(&error))?,
            standing: SubscriptionStanding {
                status: SubscriptionStatus::Active,
                expires_at: None,
            },
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
            notification_attributes: Vec::new(),
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            notifying_since: NotifyingSince {
                active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
            },
            compiled_q: None,
            compiled_geo_q: SubscriptionGeoFilter::Unrestricted,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            local_only: false,
            ngsild_conformance: None,
            lang: None,
            notification_projection: ExpandedProjection::default(),
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
        let routing = SelectorRouting::of(&subscription(vec![selector("https://example.com/Building", &["urn:ngsi-ld:E:1"], None)?])?);
        assert!(!routing.wildcard);
        assert_eq!(routing.by_type.len(), 1);
        assert_eq!(routing.by_entity_id.len(), 1);
        assert_eq!(routing.by_type[0].1.shape, SelectorShape::TypeAndId);
        assert_eq!(routing.by_type[0].1, routing.by_entity_id[0].1);
        Ok(())
    }

    /// Table 5.2.33-1: a selector whose `id` lists several identifiers is
    /// reached through the bucket of each.
    #[test]
    fn a_selector_listing_several_ids_is_placed_in_the_bucket_of_each() -> Result<(), SubscriptionError> {
        let routing = SelectorRouting::of(&subscription(vec![
            selector("https://example.com/Building", &["urn:ngsi-ld:E:1", "urn:ngsi-ld:E:2"], None)?,
            selector(
                "https://example.com/Building|https://example.com/Vehicle",
                &["urn:ngsi-ld:E:3", "urn:ngsi-ld:E:4"],
                None,
            )?,
        ])?);
        assert!(!routing.wildcard);
        assert_eq!(routing.by_type.len(), 1);
        let placed: Vec<(&str, usize, SelectorShape)> = routing
            .by_entity_id
            .iter()
            .map(|(id, placement)| (id.as_str(), placement.ordinal, placement.shape))
            .collect();
        assert_eq!(
            placed,
            [
                ("urn:ngsi-ld:E:1", 0, SelectorShape::TypeAndId),
                ("urn:ngsi-ld:E:2", 0, SelectorShape::TypeAndId),
                ("urn:ngsi-ld:E:3", 1, SelectorShape::IdOnly),
                ("urn:ngsi-ld:E:4", 1, SelectorShape::IdOnly),
            ]
        );
        Ok(())
    }

    /// The type placements of `routing`: the type, the selector's position
    /// and its shape.
    fn by_type(routing: &SelectorRouting) -> Vec<(&str, usize, SelectorShape)> {
        routing
            .by_type
            .iter()
            .map(|(type_name, placement)| (type_name.as_str(), placement.ordinal, placement.shape))
            .collect()
    }

    /// The type placements of a subscription with the one selector of the
    /// type `type_expression` and no id.
    fn placed_types(type_expression: &str) -> Result<Vec<(String, usize, SelectorShape)>, SubscriptionError> {
        let routing = SelectorRouting::of(&subscription(vec![selector(type_expression, &[], None)?])?);
        assert!(!routing.wildcard, "{type_expression} names types, so it routes only the events on them");
        assert_eq!(routing.by_entity_id, Vec::new());
        Ok(by_type(&routing)
            .into_iter()
            .map(|(type_name, ordinal, shape)| (type_name.to_owned(), ordinal, shape))
            .collect())
    }

    /// Clause 4.17: an Entity matches `A|B` when it carries `A` or `B`, so a
    /// change of either reaches the selector and one of neither reaches none.
    #[test]
    fn a_disjunction_is_placed_under_each_of_its_types() -> Result<(), SubscriptionError> {
        assert_eq!(
            placed_types("https://example.com/Building|https://example.com/Tower")?,
            [
                ("https://example.com/Building".to_owned(), 0, SelectorShape::TypeOnly),
                ("https://example.com/Tower".to_owned(), 0, SelectorShape::TypeOnly),
            ]
        );
        Ok(())
    }

    /// Clause 4.17: an Entity matches `(A;B)` only when it carries both, so it
    /// carries `A`; the matcher rejects an Entity carrying `A` alone.
    #[test]
    fn a_conjunction_is_placed_under_one_of_its_types() -> Result<(), SubscriptionError> {
        assert_eq!(
            placed_types("(https://example.com/Home;https://example.com/Vehicle)")?,
            [("https://example.com/Home".to_owned(), 0, SelectorShape::TypeOnly)]
        );
        Ok(())
    }

    /// An alternative naming a type already taken from an earlier one is
    /// reached through it and adds no placement.
    #[test]
    fn a_disjunction_of_conjunctions_is_placed_under_one_type_of_each_alternative() -> Result<(), SubscriptionError> {
        assert_eq!(
            placed_types(
                "(https://example.com/Home;https://example.com/Vehicle)|https://example.com/Tower|(https://example.com/Boat;https://example.com/Home)"
            )?,
            [
                ("https://example.com/Home".to_owned(), 0, SelectorShape::TypeOnly),
                ("https://example.com/Tower".to_owned(), 0, SelectorShape::TypeOnly),
            ]
        );
        Ok(())
    }

    /// Clause 4.17: `*` selects every entity type, so only an id narrows it.
    #[test]
    fn the_wildcard_type_routes_every_event_unless_it_names_an_id() -> Result<(), SubscriptionError> {
        let routing = SelectorRouting::of(&subscription(vec![selector("*", &[], None)?])?);
        assert!(routing.wildcard);
        assert!(routing.by_type.is_empty() && routing.by_entity_id.is_empty());

        let routing = SelectorRouting::of(&subscription(vec![selector("*", &["urn:ngsi-ld:E:1"], None)?])?);
        assert!(!routing.wildcard);
        assert_eq!(routing.by_type, Vec::new());
        assert_eq!(routing.by_entity_id.len(), 1);
        assert_eq!(routing.by_entity_id[0].1.shape, SelectorShape::IdOnly);
        Ok(())
    }

    /// A conjunction is covered by one type, so with an exact id it is placed
    /// as a single type is, under the type and the id together.
    #[test]
    fn a_conjunction_with_an_id_is_placed_under_its_covering_type_and_the_id() -> Result<(), SubscriptionError> {
        let routing = SelectorRouting::of(&subscription(vec![selector(
            "(https://example.com/Home;https://example.com/Vehicle)",
            &["urn:ngsi-ld:E:1"],
            None,
        )?])?);
        assert!(!routing.wildcard);
        assert_eq!(by_type(&routing), [("https://example.com/Home", 0, SelectorShape::TypeAndId)]);
        assert_eq!(routing.by_entity_id.len(), 1);
        assert_eq!(routing.by_type[0].1, routing.by_entity_id[0].1);
        Ok(())
    }

    #[test]
    fn an_id_pattern_does_not_index_and_a_boolean_type_is_placed_under_its_covering_types() -> Result<(), SubscriptionError> {
        let routing = SelectorRouting::of(&subscription(vec![
            selector("https://example.com/Building", &[], Some("urn:ngsi-ld:E:.*"))?,
            selector("https://example.com/Building|https://example.com/Vehicle", &["urn:ngsi-ld:E:2"], None)?,
            selector("https://example.com/Building|https://example.com/Vehicle", &[], Some(".*"))?,
        ])?);
        assert!(!routing.wildcard, "no selector is of the wildcard type");
        assert_eq!(
            by_type(&routing),
            [
                ("https://example.com/Building", 0, SelectorShape::TypeOnly),
                ("https://example.com/Building", 2, SelectorShape::TypeOnly),
                ("https://example.com/Vehicle", 2, SelectorShape::TypeOnly),
            ]
        );
        assert_eq!(routing.by_entity_id.len(), 1);
        assert_eq!(routing.by_entity_id[0].1.shape, SelectorShape::IdOnly);
        assert_eq!(routing.by_entity_id[0].1.ordinal, 1);
        Ok(())
    }
}
