//! The `[payload, time]` pairs of one attribute of a simplified temporal entity (clause 4.5.9, `format=temporalValues`),
//! by the kind of the attribute: `values`, `languageMaps`, `vocabs`, `valueLists`, `jsons`, `objects` and
//! `objectLists`. A name that is absent, or an attribute of another kind, has no pairs of that kind.

use ngsi_ld::model::{
    EntityId,
    GeoValue,
    LanguageMap,
    OneOrMany,
    RawJson,
    Value,
    VocabTerm,
    evolution::{SimplifiedTemporalEntity, TemporalValue, TemporalValues},
};

/// The temporal values of the attribute `name` of `entity`, of whatever kind.
#[must_use]
pub fn values_of<'e>(entity: &'e SimplifiedTemporalEntity, name: &str) -> Option<&'e TemporalValues> {
    entity
        .attributes
        .iter()
        .find_map(|(candidate, values)| (candidate.as_str() == name).then_some(values))
}

/// Defines the accessor of the pairs of one kind: the pairs of the variant, `None` for every other.
macro_rules! pairs_of_kind {
    ($(#[$doc:meta])* $name:ident, $variant:ident, $payload:ty) => {
        $(#[$doc])*
        #[must_use]
        pub fn $name<'e>(entity: &'e SimplifiedTemporalEntity, name: &str) -> Option<&'e [TemporalValue<$payload>]> {
            let TemporalValues::$variant(pairs) = values_of(entity, name)? else {
                return None;
            };
            Some(pairs.as_slice())
        }
    };
}

pairs_of_kind!(
    /// The `values` pairs of a Property.
    property_pairs,
    Property,
    Value
);
pairs_of_kind!(
    /// The `values` pairs of a `GeoProperty`.
    geo_pairs,
    GeoProperty,
    GeoValue
);
pairs_of_kind!(
    /// The `languageMaps` pairs of a `LanguageProperty`.
    language_pairs,
    LanguageProperty,
    LanguageMap
);
pairs_of_kind!(
    /// The `vocabs` pairs of a `VocabProperty`.
    vocab_pairs,
    VocabProperty,
    OneOrMany<VocabTerm>
);
pairs_of_kind!(
    /// The `valueLists` pairs of a `ListProperty`.
    list_pairs,
    ListProperty,
    Vec<Value>
);
pairs_of_kind!(
    /// The `jsons` pairs of a `JsonProperty`.
    json_pairs,
    JsonProperty,
    RawJson
);
pairs_of_kind!(
    /// The `objects` pairs of a Relationship.
    object_pairs,
    Relationship,
    OneOrMany<EntityId>
);
pairs_of_kind!(
    /// The `objectLists` pairs of a `ListRelationship`.
    object_list_pairs,
    ListRelationship,
    Vec<EntityId>
);

#[cfg(test)]
mod tests {
    use crate::simplified_pairs::{json_pairs, property_pairs, vocab_pairs};
    use googletest::prelude::*;
    use ngsi_ld::model::evolution::SimplifiedTemporalEntity;

    fn entity() -> Result<SimplifiedTemporalEntity> {
        Ok(SimplifiedTemporalEntity::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle",
            "speed":{"type":"Property","values":[[120,"2018-08-01T12:03:00Z"]]},
            "config":{"type":"JsonProperty","jsons":[[{"json":{"maxSpeed":200}},"2018-08-01T12:03:00Z"]]}}"#,
        )?)
    }

    #[gtest]
    fn the_pairs_of_the_kind_an_attribute_is_are_found() -> Result<()> {
        let entity = entity()?;
        expect_that!(property_pairs(&entity, "speed"), some(len(eq(1))));
        verify_that!(json_pairs(&entity, "config"), some(len(eq(1))))
    }

    #[gtest]
    fn an_attribute_of_another_kind_or_a_missing_one_has_no_pairs() -> Result<()> {
        let entity = entity()?;
        expect_that!(vocab_pairs(&entity, "speed"), none());
        verify_that!(property_pairs(&entity, "absent"), none())
    }
}
