//! Matchers on the entities a broker answers, read through the typed model: the attributes an entity has and lacks,
//! and the members its document holds. The attribute matchers take a projected entity too, whose `pick` or `omit`
//! dropped its `id` or `type` (clause 4.21).

use crate::{
    attribute_matcher::{AttributeMatcher, Member},
    attribute_reading::has_member,
};
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{Completeness, Entity, Output, Projected, Representation};

/// Matches an entity whose attribute `name` meets an attribute matcher.
#[derive(Debug, Clone, MatcherBase)]
pub struct HasAttribute {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The requirements on the attribute.
    matcher: AttributeMatcher,
}

/// An entity whose attribute `name` meets `matcher`.
#[must_use]
pub fn has_attribute(name: &str, matcher: AttributeMatcher) -> HasAttribute {
    HasAttribute {
        name: name.to_owned(),
        matcher,
    }
}

impl<C: Completeness> Matcher<&Entity<Output, C>> for HasAttribute {
    fn matches(&self, actual: &Entity<Output, C>) -> MatcherResult {
        self.matcher.mismatch_in(&actual.attributes, &self.name).is_none().into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has an attribute {} meeting {:?}", self.name, self.matcher).into(),
            MatcherResult::NoMatch => format!("hasn't an attribute {} meeting {:?}", self.name, self.matcher).into(),
        }
    }

    fn explain_match(&self, actual: &Entity<Output, C>) -> Description {
        self.matcher
            .mismatch_in(&actual.attributes, &self.name)
            .unwrap_or_else(|| "which meets it".to_owned())
            .into()
    }
}

/// Matches an entity without the attribute `name`.
#[derive(Debug, Clone, MatcherBase)]
pub struct LacksAttribute {
    /// The attribute name, as the answer's terms write it.
    name: String,
}

/// An entity without the attribute `name`.
#[must_use]
pub fn lacks_attribute(name: &str) -> LacksAttribute {
    LacksAttribute { name: name.to_owned() }
}

impl<C: Completeness> Matcher<&Entity<Output, C>> for LacksAttribute {
    fn matches(&self, actual: &Entity<Output, C>) -> MatcherResult {
        actual.attributes.names().all(|name| name.to_string() != self.name).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has no attribute {}", self.name).into(),
            MatcherResult::NoMatch => format!("has an attribute {}", self.name).into(),
        }
    }
}

/// Matches an entity one of whose instances of the attribute `name` meets an attribute matcher (clause 4.5.5),
/// whatever its `datasetId`.
#[derive(Debug, Clone, MatcherBase)]
pub struct HasInstance {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The requirements on the instance.
    matcher: AttributeMatcher,
}

/// An entity one of whose instances of the attribute `name` meets `matcher`.
#[must_use]
pub fn has_instance(name: &str, matcher: AttributeMatcher) -> HasInstance {
    HasInstance {
        name: name.to_owned(),
        matcher,
    }
}

impl<C: Completeness> Matcher<&Entity<Output, C>> for HasInstance {
    fn matches(&self, actual: &Entity<Output, C>) -> MatcherResult {
        actual
            .attributes
            .iter()
            .filter(|(name, _)| name.to_string() == self.name)
            .any(|(_, instances)| instances.iter().any(|instance| self.matcher.matches(instance).is_match()))
            .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has an instance of {} meeting {:?}", self.name, self.matcher).into(),
            MatcherResult::NoMatch => format!("hasn't an instance of {} meeting {:?}", self.name, self.matcher).into(),
        }
    }
}

/// Matches an entity whose attribute `name` holds `count` instances (clause 4.5.5).
#[derive(Debug, Clone, MatcherBase)]
pub struct HasInstances {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The number of instances.
    count: usize,
}

/// An entity whose attribute `name` holds `count` instances, one per `datasetId`.
#[must_use]
pub fn has_instances(name: &str, count: usize) -> HasInstances {
    HasInstances { name: name.to_owned(), count }
}

impl<C: Completeness> Matcher<&Entity<Output, C>> for HasInstances {
    fn matches(&self, actual: &Entity<Output, C>) -> MatcherResult {
        actual
            .attributes
            .iter()
            .any(|(name, instances)| name.to_string() == self.name && instances.len() == self.count)
            .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has an attribute {} of {} instances", self.name, self.count).into(),
            MatcherResult::NoMatch => format!("hasn't an attribute {} of {} instances", self.name, self.count).into(),
        }
    }
}

/// The number of members an entity's normalized or concise document holds: `id`, `type`, every attribute, and
/// `scope`, `expiresAt`, `@context` and the system members when present.
#[must_use]
pub fn member_count(entity: &Entity<Output>) -> usize {
    let optional = [
        entity.scope.is_some(),
        entity.expires_at.is_some(),
        entity.context.is_some(),
        entity.output.created_at.is_some(),
        entity.output.modified_at.is_some(),
        entity.output.deleted_at.is_some(),
    ];
    2 + entity.attributes.len() + optional.iter().filter(|present| **present).count()
}

/// The number of members a projected entity's document holds: `id` and `type` when the projection kept them, and
/// every member [`member_count`] counts besides them.
#[must_use]
pub fn projected_member_count(entity: &Entity<Output, Projected>) -> usize {
    let optional = [
        entity.id.is_some(),
        entity.entity_type.is_some(),
        entity.scope.is_some(),
        entity.expires_at.is_some(),
        entity.context.is_some(),
        entity.output.created_at.is_some(),
        entity.output.modified_at.is_some(),
        entity.output.deleted_at.is_some(),
    ];
    entity.attributes.len() + optional.iter().filter(|present| **present).count()
}

/// The number of members the attribute `name` of an entity holds written normalized: the members of its one
/// instance, or the number of its instances when it has several, which the document writes as an array.
#[must_use]
pub fn attribute_members(entity: &Entity<Output>, name: &str) -> Option<usize> {
    let rendered = serde_json::from_str::<serde_json::Value>(&entity.render(Representation::Normalized).to_string()).ok()?;
    match rendered.get(name)? {
        serde_json::Value::Object(members) => Some(members.len()),
        serde_json::Value::Array(instances) => Some(instances.len()),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => None,
    }
}

/// The number of instances of the attribute `name` of an entity that write `member`, or `None` when the entity has no
/// such attribute; the default instance writes no `datasetId` (clause 4.5.5).
#[must_use]
pub fn member_instances(entity: &Entity<Output>, name: &str, member: Member) -> Option<usize> {
    entity
        .attributes
        .iter()
        .find(|(attribute, _)| attribute.to_string() == name)
        .map(|(_, instances)| instances.iter().filter(|instance| has_member(instance, instance.meta(), member)).count())
}

/// Matches an entity having the attribute `name` every instance of which meets an attribute matcher (clause 4.5.5).
#[derive(Debug, Clone, MatcherBase)]
pub struct EveryInstance {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The requirements on each instance.
    matcher: AttributeMatcher,
}

/// An entity having the attribute `name`, every instance of which meets `matcher`.
#[must_use]
pub fn every_instance(name: &str, matcher: AttributeMatcher) -> EveryInstance {
    EveryInstance {
        name: name.to_owned(),
        matcher,
    }
}

impl<C: Completeness> Matcher<&Entity<Output, C>> for EveryInstance {
    fn matches(&self, actual: &Entity<Output, C>) -> MatcherResult {
        actual
            .attributes
            .iter()
            .find(|(name, _)| name.to_string() == self.name)
            .is_some_and(|(_, instances)| instances.iter().all(|instance| self.matcher.matches(instance).is_match()))
            .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has an attribute {} every instance of which meets {:?}", self.name, self.matcher).into(),
            MatcherResult::NoMatch => format!("hasn't an attribute {} every instance of which meets {:?}", self.name, self.matcher).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_matcher::{Member, any_attribute, property, relationship},
        entity_matcher::{
            attribute_members,
            every_instance,
            has_attribute,
            has_instance,
            has_instances,
            lacks_attribute,
            member_count,
            member_instances,
            projected_member_count,
        },
    };
    use cefact_units::UnitCode;
    use googletest::prelude::*;
    use ngsi_ld::model::{Entity, EntityId, Lossless, Output, Projected};

    /// A Building as a broker answers it.
    fn building() -> Result<Entity<Output>> {
        Ok(Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","airQualityLevel":{"type":"Property","value":4,"unitCode":"C62",
            "observedAt":"2020-09-09T16:40:00Z","hasSensor":{"type":"Relationship","object":"urn:ngsi-ld:Sensor:1"}},
            "name":{"type":"Property","value":{"en":"Eiffel"}}}"#,
            Lossless::Normalized,
        )?)
    }

    #[gtest]
    fn an_instance_is_found_among_the_datasets_of_an_attribute() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","speed":[{"type":"Property","value":10},{"type":"Property","value":2,"datasetId":"urn:ngsi-ld:Dataset:gps"}]}"#,
            Lossless::Normalized,
        )?;
        expect_that!(&entity, has_instance("speed", property().value(10).without(Member::DatasetId)));
        expect_that!(
            &entity,
            has_instance("speed", property().value(2).with_dataset_id("urn:ngsi-ld:Dataset:gps".parse()?))
        );
        verify_that!(&entity, not(has_instance("speed", property().value(2).without(Member::DatasetId))))
    }

    #[gtest]
    fn a_projected_entity_without_id_or_type_is_matched_by_its_attributes() -> Result<()> {
        let picked = Entity::<Output, Projected>::parse(r#"{"speed":{"type":"Property","value":5}}"#, Lossless::Normalized)?;
        expect_that!(projected_member_count(&picked), eq(1));
        expect_that!(&picked, has_attribute("speed", property().value(5)));
        verify_that!(&picked, lacks_attribute("brandName"))
    }

    #[gtest]
    fn a_property_meets_the_members_it_names() -> Result<()> {
        let entity = building()?;
        let level = property()
            .value(4)
            .unit_code(UnitCode::C62)
            .observed_at("2020-09-09T16:40:00.000Z".parse()?)
            .sub_attribute("hasSensor", relationship().object("urn:ngsi-ld:Sensor:1".parse::<EntityId>()?));
        verify_that!(&entity, has_attribute("airQualityLevel", level))
    }

    #[gtest]
    fn another_value_or_kind_does_not_match() -> Result<()> {
        let entity = building()?;
        expect_that!(&entity, not(has_attribute("airQualityLevel", property().value(5))));
        expect_that!(&entity, not(has_attribute("airQualityLevel", relationship())));
        verify_that!(&entity, not(has_attribute("airQualityLevel", property().without(Member::UnitCode))))
    }

    #[gtest]
    fn a_member_inside_an_object_value_is_read_by_its_path() -> Result<()> {
        let entity = building()?;
        verify_that!(&entity, has_attribute("name", property().value_member(["en"], "Eiffel")))
    }

    #[gtest]
    fn a_missing_attribute_is_lacked_and_members_are_counted() -> Result<()> {
        let entity = building()?;
        expect_that!(&entity, lacks_attribute("speed"));
        expect_that!(&entity, not(lacks_attribute("name")));
        verify_that!(member_count(&entity), eq(4))
    }

    #[gtest]
    fn an_attribute_is_matched_by_its_number_of_instances() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","speed":[{"type":"Property","value":1},{"type":"Property","value":2,"datasetId":"urn:ngsi-ld:Dataset:a"}]}"#,
            Lossless::Normalized,
        )?;
        expect_that!(&entity, has_instances("speed", 2));
        verify_that!(&entity, not(has_instances("speed", 1)))
    }

    #[gtest]
    fn the_members_of_an_attribute_are_counted_as_written_normalized() -> Result<()> {
        let entity = building()?;
        expect_that!(attribute_members(&entity, "airQualityLevel"), some(eq(5)));
        expect_that!(attribute_members(&entity, "name"), some(eq(2)));
        verify_that!(attribute_members(&entity, "speed"), none())
    }

    #[gtest]
    fn the_instances_writing_a_member_are_counted() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","speed":[{"type":"Property","value":1},{"type":"Property","value":2,"datasetId":"urn:ngsi-ld:Dataset:a"},{"type":"Property","value":3,"datasetId":"urn:ngsi-ld:Dataset:b"}]}"#,
            Lossless::Normalized,
        )?;
        expect_that!(member_instances(&entity, "speed", Member::DatasetId), some(eq(2)));
        expect_that!(member_instances(&entity, "speed", Member::DeletedAt), some(eq(0)));
        verify_that!(member_instances(&entity, "name", Member::DatasetId), none())
    }

    #[gtest]
    fn every_instance_of_a_deleted_attribute_holds_the_null() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","label":[{"type":"Property","value":"urn:ngsi-ld:null","previousValue":"Main","deletedAt":"2024-01-01T00:00:00Z"},{"type":"Property","value":"urn:ngsi-ld:null","datasetId":"urn:ngsi-ld:Dataset:a","deletedAt":"2024-01-01T00:00:00Z"}],"speed":[{"type":"Property","value":1},{"type":"Property","value":"urn:ngsi-ld:null","datasetId":"urn:ngsi-ld:Dataset:a"}]}"#,
            Lossless::Normalized,
        )?;
        expect_that!(&entity, every_instance("label", any_attribute().null_value()));
        expect_that!(&entity, every_instance("label", property()));
        expect_that!(&entity, has_instance("label", any_attribute().previous_value("Main")));
        expect_that!(member_instances(&entity, "label", Member::DeletedAt), some(eq(2)));
        expect_that!(&entity, not(every_instance("speed", any_attribute().null_value())));
        verify_that!(&entity, not(every_instance("name", property())))
    }
}
