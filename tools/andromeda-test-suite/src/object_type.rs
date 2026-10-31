//! The `objectType` of a Relationship or `ListRelationship` a broker answers (clause 5.2.6), read through the typed
//! model, with the spelling the document wrote it in: one type bare, several as an array.

use ngsi_ld::model::{Attribute, Entity, OneOrMany, Output, VocabTerm};

/// The `objectType` of the attribute `name` of `entity`, when its one instance is a Relationship or a
/// `ListRelationship` that has one.
#[must_use]
pub fn object_type<'e>(entity: &'e Entity<Output>, name: &str) -> Option<&'e OneOrMany<VocabTerm>> {
    entity
        .attributes
        .iter()
        .find(|(candidate, _)| candidate.as_str() == name)
        .and_then(|(_, instances)| match instances.first() {
            Attribute::Relationship(relationship) => relationship.object_type(),
            Attribute::ListRelationship(relationship) => relationship.object_type(),
            Attribute::Property(_)
            | Attribute::GeoProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::VocabProperty(_)
            | Attribute::ListProperty(_)
            | Attribute::JsonProperty(_) => None,
        })
}

#[cfg(test)]
mod tests {
    use crate::object_type::object_type;
    use googletest::prelude::*;
    use ngsi_ld::model::{Entity, Lossless, OneOrMany, Output, VocabTerm};

    fn entity() -> Result<Entity<Output>> {
        Ok(Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V",
            "one":{"type":"Relationship","object":"urn:ngsi-ld:W:1","objectType":"W"},
            "many":{"type":"Relationship","object":"urn:ngsi-ld:W:1","objectType":["W","X"]},
            "bare":{"type":"Relationship","object":"urn:ngsi-ld:W:1"},
            "speed":{"type":"Property","value":1}}"#,
            Lossless::Normalized,
        )?)
    }

    #[gtest]
    fn one_type_reads_bare_and_several_read_as_an_array() -> Result<()> {
        let entity = entity()?;
        expect_that!(object_type(&entity, "one"), some(eq(&OneOrMany::one("W".parse::<VocabTerm>()?))));
        verify_that!(
            object_type(&entity, "many"),
            some(eq(&OneOrMany::try_from(vec!["W".parse::<VocabTerm>()?, "X".parse::<VocabTerm>()?])?))
        )
    }

    #[gtest]
    fn a_relationship_without_a_type_and_other_families_have_none() -> Result<()> {
        let entity = entity()?;
        expect_that!(object_type(&entity, "bare"), none());
        verify_that!(object_type(&entity, "speed"), none())
    }
}
