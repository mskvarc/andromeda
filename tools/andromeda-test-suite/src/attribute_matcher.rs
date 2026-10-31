//! Matchers on one attribute a broker answers, read through the typed model: its kind, its payload and its members.
//!
//! An attribute matcher states only what the test requires of the attribute; every member it does not name may hold
//! anything. Payloads, instants and numbers are compared by [`Equivalent`](crate::equivalence::Equivalent), so a spelling the specification reads the
//! same matches. An attribute with one instance is matched by it; a matcher naming a `datasetId` picks the instance of
//! that dataset (clause 4.5.5).

use crate::{
    attribute_check::{Check, check_mismatch},
    attribute_member::Member,
    equivalence::Comparison,
};
use cefact_units::UnitCode;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{
    Attribute,
    AttributeKind,
    Attributes,
    DatasetId,
    DateTime,
    EntityId,
    EntityTypeName,
    InstanceId,
    Instances,
    OneOrMany,
    Output,
    Value,
    VocabTerm,
    geometry::Geometry,
};

/// Why an attribute holds no instance a matcher can read.
#[derive(Debug, thiserror::Error)]
enum MissingInstance<'a> {
    /// No instance is the default one.
    #[error("has no default instance")]
    Default,
    /// No instance carries the dataset the matcher reads.
    #[error("has no instance of dataset {0}")]
    Dataset(&'a DatasetId),
    /// The matcher names no dataset and the attribute holds this many instances.
    #[error("has {0} instances")]
    Several(usize),
}

/// The requirements on one attribute.
#[derive(Debug, Clone, Default, MatcherBase)]
pub struct AttributeMatcher {
    /// The kind, when the test requires one.
    kind: Option<AttributeKind>,
    /// The dataset whose instance is matched; `None` for an attribute's one instance.
    dataset_id: Option<DatasetId>,
    /// The requirements on the instance.
    checks: Vec<Check>,
}

/// An attribute of any kind.
#[must_use]
pub fn any_attribute() -> AttributeMatcher {
    AttributeMatcher::default()
}

/// An attribute of `kind`.
#[must_use]
pub fn attribute_of(kind: AttributeKind) -> AttributeMatcher {
    AttributeMatcher {
        kind: Some(kind),
        ..AttributeMatcher::default()
    }
}

/// A Property.
#[must_use]
pub fn property() -> AttributeMatcher {
    attribute_of(AttributeKind::Property)
}

/// A Relationship.
#[must_use]
pub fn relationship() -> AttributeMatcher {
    attribute_of(AttributeKind::Relationship)
}

/// A `GeoProperty`.
#[must_use]
pub fn geo_property() -> AttributeMatcher {
    attribute_of(AttributeKind::GeoProperty)
}

impl AttributeMatcher {
    /// Adds `check`.
    fn with(mut self, check: Check) -> AttributeMatcher {
        self.checks.push(check);
        self
    }

    /// Picks the instance of `dataset_id`, and requires it to exist.
    #[must_use]
    pub fn dataset_id(mut self, dataset_id: DatasetId) -> AttributeMatcher {
        self.dataset_id = Some(dataset_id);
        self
    }

    /// Requires the whole `json` of a `JsonProperty` to equal `json` as parsed (clause 4.5.24 keeps the json opaque;
    /// the whole-object comparison is stricter than a member-by-member assert, since no member may sit beside the
    /// expected ones).
    #[must_use]
    pub fn json(self, json: serde_json::Value) -> AttributeMatcher {
        self.with(Check::Json(json))
    }

    /// Requires the whole `previousJson` of a `JsonProperty` to equal `json` as parsed (clause 5.8.1).
    #[must_use]
    pub fn previous_json(self, json: serde_json::Value) -> AttributeMatcher {
        self.with(Check::PreviousJson(json))
    }

    /// Requires the value of a Property, or the items of a `ListProperty`, to be `value`.
    #[must_use]
    pub fn value(self, value: impl Into<Value>) -> AttributeMatcher {
        self.with(Check::Value(value.into()))
    }

    /// Requires the member at `path` inside the attribute's object value to be `value`.
    #[must_use]
    pub fn value_member<const N: usize>(self, path: [&str; N], value: impl Into<Value>) -> AttributeMatcher {
        self.with(Check::ValueMember(
            path.iter().map(|segment| (*segment).to_owned()).collect(),
            value.into(),
            Comparison::Equivalent,
        ))
    }

    /// Requires the member at `path` inside the attribute's value, the value itself for an empty path, to be `value`
    /// as written: a typed value must carry the same datatype and lexical form, so the spelling a broker answers a
    /// `DateTime`, `Date` or `Time` in is pinned, where [`value_member`](Self::value_member) reads an instant.
    #[must_use]
    pub fn value_member_as_written<const N: usize>(self, path: [&str; N], value: impl Into<Value>) -> AttributeMatcher {
        self.with(Check::ValueMember(
            path.iter().map(|segment| (*segment).to_owned()).collect(),
            value.into(),
            Comparison::AsWritten,
        ))
    }

    /// Requires the array or object at `path` inside the attribute's value to hold `length` items or members.
    #[must_use]
    pub fn length_at<const N: usize>(self, path: [&str; N], length: usize) -> AttributeMatcher {
        self.with(Check::Length(path.iter().map(|segment| (*segment).to_owned()).collect(), length))
    }

    /// Requires the `valueList` of a `ListProperty`, the `objectList` of a `ListRelationship` or the array of terms of
    /// a `VocabProperty` to hold `length` items.
    #[must_use]
    pub fn list_length(self, length: usize) -> AttributeMatcher {
        self.with(Check::ListLength(length))
    }

    /// Requires the item at `index` of the `valueList` of a `ListProperty` to be `value`.
    #[must_use]
    pub fn list_value(self, index: usize, value: impl Into<Value>) -> AttributeMatcher {
        self.with(Check::ListValue(index, value.into()))
    }

    /// Requires the object at `index` of the `objectList` of a `ListRelationship` to be `object`.
    #[must_use]
    pub fn list_object(self, index: usize, object: EntityId) -> AttributeMatcher {
        self.with(Check::ListObject(index, object))
    }

    /// Requires a `VocabProperty` to hold `term`, written alone or in its array.
    #[must_use]
    pub fn vocab_holding(self, term: VocabTerm) -> AttributeMatcher {
        self.with(Check::VocabHolding(term))
    }

    /// Requires the object of a Relationship to be `object`.
    #[must_use]
    pub fn object(self, object: impl Into<OneOrMany<EntityId>>) -> AttributeMatcher {
        self.with(Check::Object(object.into()))
    }

    /// Requires the `objectType` of a Relationship to be `object_type`.
    #[must_use]
    pub fn object_type(self, object_type: impl Into<OneOrMany<VocabTerm>>) -> AttributeMatcher {
        self.with(Check::ObjectType(object_type.into()))
    }

    /// Requires the geometry of a `GeoProperty` to be `geometry`.
    #[must_use]
    pub fn geometry(self, geometry: impl Into<Geometry>) -> AttributeMatcher {
        self.with(Check::Geometry(geometry.into()))
    }

    /// Requires the vocabulary of a `VocabProperty` to be `vocab`.
    #[must_use]
    pub fn vocab(self, vocab: impl Into<OneOrMany<VocabTerm>>) -> AttributeMatcher {
        self.with(Check::Vocab(vocab.into()))
    }

    /// Requires the text of `language` in a `LanguageProperty` to be `text`.
    #[must_use]
    pub fn language(self, language: &str, text: &str) -> AttributeMatcher {
        self.with(Check::Language(language.to_owned(), text.to_owned()))
    }

    /// Requires the Property to name `language` as the language a language filter picked (`lang`, clause 4.15).
    #[must_use]
    pub fn lang(self, language: &str) -> AttributeMatcher {
        self.with(Check::Lang(language.to_owned()))
    }

    /// Requires the instance read to carry `datasetId` `dataset_id`; unlike [`dataset_id`](Self::dataset_id), it picks
    /// no instance, so it applies to the one instance of an attribute and to each instance of an evolution.
    #[must_use]
    pub fn with_dataset_id(self, dataset_id: DatasetId) -> AttributeMatcher {
        self.with(Check::DatasetId(dataset_id))
    }

    /// Requires `unitCode` to be `unit_code`.
    #[must_use]
    pub fn unit_code(self, unit_code: UnitCode) -> AttributeMatcher {
        self.with(Check::UnitCode(unit_code))
    }

    /// Requires `valueType` to be `value_type`.
    #[must_use]
    pub fn value_type(self, value_type: VocabTerm) -> AttributeMatcher {
        self.with(Check::ValueType(value_type))
    }

    /// Requires `observedAt` to be the instant `observed_at`.
    #[must_use]
    pub fn observed_at(self, observed_at: DateTime) -> AttributeMatcher {
        self.with(Check::ObservedAt(observed_at))
    }

    /// Requires `expiresAt` to be the instant `expires_at`.
    #[must_use]
    pub fn expires_at(self, expires_at: DateTime) -> AttributeMatcher {
        self.with(Check::ExpiresAt(expires_at))
    }

    /// Requires `instanceId` to be `instance_id` (clause 4.5.7).
    #[must_use]
    pub fn instance_id(self, instance_id: &InstanceId) -> AttributeMatcher {
        // The matcher keeps its own copy of the id it compares with.
        self.with(Check::InstanceId(instance_id.clone()))
    }

    /// Requires `member` to be absent.
    #[must_use]
    pub fn without(self, member: Member) -> AttributeMatcher {
        self.with(Check::Absent(member))
    }

    /// Requires `member` to be present.
    #[must_use]
    pub fn with_member(self, member: Member) -> AttributeMatcher {
        self.with(Check::Present(member))
    }

    /// Requires the value to hold no member at `path`.
    #[must_use]
    pub fn without_value_member<const N: usize>(self, path: [&str; N]) -> AttributeMatcher {
        self.with(Check::NoValueMember(path.iter().map(|segment| (*segment).to_owned()).collect()))
    }

    /// Requires the sub-attribute `name` to meet `matcher`.
    #[must_use]
    pub fn sub_attribute(self, name: &str, matcher: AttributeMatcher) -> AttributeMatcher {
        self.with(Check::SubAttribute(name.to_owned(), Box::new(matcher)))
    }

    /// Requires the sub-attribute `name` to be absent.
    #[must_use]
    pub fn without_sub_attribute(self, name: &str) -> AttributeMatcher {
        self.with(Check::NoSubAttribute(name.to_owned()))
    }

    /// Requires the attribute to carry an `ngsildproof` (clause 4.5.2.2).
    #[must_use]
    pub fn with_proof(self) -> AttributeMatcher {
        self.with(Check::Proof)
    }

    /// Requires the attribute's `ngsildproof` to be sealed to the entity `id` (`entityIdSealed`).
    #[must_use]
    pub fn sealed_to(self, id: EntityId) -> AttributeMatcher {
        self.with(Check::SealedId(id))
    }

    /// Requires the attribute's `ngsildproof` to be sealed to the one entity type `entity_type` (`entityTypeSealed`).
    #[must_use]
    pub fn sealed_type(self, entity_type: EntityTypeName) -> AttributeMatcher {
        self.with(Check::SealedType(entity_type))
    }

    /// Requires the member at `path` inside the proof object of the attribute's `ngsildproof` to be `value`.
    #[must_use]
    pub fn proof_member<const N: usize>(self, path: [&str; N], value: impl Into<Value>) -> AttributeMatcher {
        self.with(Check::ProofMember(path.iter().map(|segment| (*segment).to_owned()).collect(), value.into()))
    }

    /// Requires the sub-attribute `name` to hold `count` instances, one per dataset (clause 4.5.5).
    #[must_use]
    pub fn sub_attribute_instances(self, name: &str, count: usize) -> AttributeMatcher {
        self.with(Check::SubAttributeInstances(name.to_owned(), count))
    }

    /// Requires the payload to be the NGSI-LD Null, as a broker writes a deleted Property, `GeoProperty` or
    /// Relationship (clause 4.5.0).
    #[must_use]
    pub fn null_value(self) -> AttributeMatcher {
        self.with(Check::NullValue)
    }

    /// Requires the Property's `previousValue` to be `value` (clause 5.8.1).
    #[must_use]
    pub fn previous_value(self, value: impl Into<Value>) -> AttributeMatcher {
        self.with(Check::PreviousValue(value.into()))
    }

    /// Requires the Relationship's `previousObject` to be `object` (clause 5.8.1).
    #[must_use]
    pub fn previous_object(self, object: impl Into<OneOrMany<EntityId>>) -> AttributeMatcher {
        self.with(Check::PreviousObject(object.into()))
    }

    /// Requires the attribute to hold `count` sub-attributes.
    #[must_use]
    pub fn sub_attribute_count(self, count: usize) -> AttributeMatcher {
        self.with(Check::SubAttributeCount(count))
    }

    /// The instance this matcher reads in `instances`, or why there is none.
    fn instance<'a>(&'a self, instances: &'a Instances<Attribute<Output>>) -> Result<&'a Attribute<Output>, MissingInstance<'a>> {
        match &self.dataset_id {
            // The default instance is written without a `datasetId` (clause 4.5.5).
            Some(DatasetId::DefaultInstance) => instances
                .iter()
                .find(|instance| {
                    instance
                        .meta()
                        .dataset_id
                        .as_ref()
                        .is_none_or(|dataset_id| *dataset_id == DatasetId::DefaultInstance)
                })
                .ok_or(MissingInstance::Default),
            Some(dataset_id) => instances
                .iter()
                .find(|instance| instance.meta().dataset_id.as_ref() == Some(dataset_id))
                .ok_or(MissingInstance::Dataset(dataset_id)),
            None if instances.len() == 1 => Ok(instances.first()),
            None => Err(MissingInstance::Several(instances.len())),
        }
    }

    /// The first requirement `attribute` breaks, if any.
    fn mismatch(&self, attribute: &Attribute<Output>) -> Option<String> {
        if let Some(kind) = self.kind
            && attribute.kind() != kind
        {
            return Some(format!("is a {:?}", attribute.kind()));
        }
        self.checks.iter().find_map(|check| check_mismatch(check, attribute))
    }

    /// The first requirement the attribute `name` of `attributes` breaks, if any.
    pub(crate) fn mismatch_in(&self, attributes: &Attributes<Output>, name: &str) -> Option<String> {
        let Some(instances) = attributes
            .iter()
            .find_map(|(candidate, instances)| (candidate.as_str() == name).then_some(instances))
        else {
            return Some(format!("has no attribute {name}"));
        };
        match self.instance(instances) {
            Ok(instance) => self.mismatch(instance).map(|reason| format!("whose attribute {name} {reason}")),
            Err(reason) => Some(format!("whose attribute {name} {reason}")),
        }
    }
}

impl Matcher<&Attribute<Output>> for AttributeMatcher {
    fn matches(&self, actual: &Attribute<Output>) -> MatcherResult {
        self.mismatch(actual).is_none().into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("meets {self:?}").into(),
            MatcherResult::NoMatch => format!("doesn't meet {self:?}").into(),
        }
    }

    fn explain_match(&self, actual: &Attribute<Output>) -> Description {
        self.mismatch(actual).unwrap_or_else(|| "which meets it".to_owned()).into()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_matcher::{any_attribute, property, relationship},
        attribute_member::Member,
        entity_matcher::has_attribute,
    };
    use cefact_units::UnitCode;
    use googletest::prelude::*;
    use ngsi_ld::model::{Attribute, DatasetId, DateTime, Entity, EntityId, EntityTypeName, Lossless, Output, Time, Value};
    use serde_json::json;

    /// The one Property of an entity as a broker answers it.
    fn level() -> Result<Attribute<Output>> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","level":{"type":"Property","value":4,"unitCode":"C62"}}"#,
            Lossless::Normalized,
        )?;
        let (_, instances) = entity.attributes.iter().next().or_fail()?;
        Ok(instances.first().clone())
    }

    /// A Property carrying a proof sealed to its entity, as a broker answers it.
    fn sealed() -> Result<Attribute<Output>> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","name":{"type":"Property","value":"A",
            "ngsildproof":{"type":"Property","value":{"type":"DataIntegrityProof","cryptosuite":"eddsa-rdfc-2022"},
            "entityIdSealed":"urn:ngsi-ld:Building:1","entityTypeSealed":"Building"}}}"#,
            Lossless::Normalized,
        )?;
        let (_, instances) = entity.attributes.iter().next().or_fail()?;
        Ok(instances.first().clone())
    }

    #[gtest]
    fn a_typed_value_is_matched_as_its_instant_or_as_written() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","opening":{"type":"Property",
            "value":{"at":{"@value":"2024-01-01T00:00:00.500Z","type":"DateTime"},"daily":{"type":"Time","@value":"12:00:00.500Z"}}}}"#,
            Lossless::Normalized,
        )?;
        let (_, instances) = entity.attributes.iter().next().or_fail()?;
        let opening = instances.first();
        let shorter = Value::from("2024-01-01T00:00:00.5Z".parse::<DateTime>()?);
        expect_that!(opening, property().value_member(["at"], shorter.clone()));
        expect_that!(
            opening,
            property()
                .value_member_as_written(["at"], Value::from("2024-01-01T00:00:00.500Z".parse::<DateTime>()?))
                .value_member_as_written(["daily"], Value::from("12:00:00.500Z".parse::<Time>()?))
        );
        verify_that!(opening, not(property().value_member_as_written(["at"], shorter)))
    }

    #[gtest]
    fn a_proof_is_matched_by_its_seals_and_the_members_of_its_object() -> Result<()> {
        let sealed = sealed()?;
        let building: EntityId = "urn:ngsi-ld:Building:1".parse()?;
        let other: EntityId = "urn:ngsi-ld:Building:2".parse()?;
        let building_type: EntityTypeName = "Building".parse()?;
        expect_that!(
            &sealed,
            property()
                .with_proof()
                .sealed_to(building)
                .sealed_type(building_type)
                .proof_member(["cryptosuite"], "eddsa-rdfc-2022")
        );
        expect_that!(&sealed, not(property().sealed_to(other)));
        expect_that!(&sealed, not(property().proof_member(["cryptosuite"], "eddsa-jcs-2022")));
        verify_that!(&level()?, not(property().with_proof()))
    }

    #[gtest]
    fn an_attribute_meets_the_members_it_names_and_not_others() -> Result<()> {
        let attribute = level()?;
        expect_that!(&attribute, property().value(4).unit_code(UnitCode::C62));
        expect_that!(&attribute, not(property().value(5)));
        expect_that!(&attribute, not(relationship()));
        verify_that!(&attribute, not(property().without(Member::UnitCode)))
    }

    #[gtest]
    fn the_previous_member_of_each_kind_is_present_only_where_written() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","speed":{"type":"Property","value":2,"previousValue":1},
            "owner":{"type":"Relationship","object":"urn:ngsi-ld:P:2","previousObject":"urn:ngsi-ld:P:1"},
            "label":{"type":"LanguageProperty","languageMap":{"en":"b"},"previousLanguageMap":{"en":"a"}},
            "plain":{"type":"Property","value":3},"link":{"type":"Relationship","object":"urn:ngsi-ld:P:3"}}"#,
            Lossless::Normalized,
        )?;
        expect_that!(&entity, has_attribute("speed", any_attribute().with_member(Member::Previous)));
        expect_that!(&entity, has_attribute("owner", any_attribute().with_member(Member::Previous)));
        expect_that!(&entity, has_attribute("label", any_attribute().with_member(Member::Previous)));
        expect_that!(&entity, has_attribute("plain", any_attribute().without(Member::Previous)));
        verify_that!(&entity, has_attribute("link", any_attribute().without(Member::Previous)))
    }

    #[gtest]
    fn a_deleted_relationship_holds_the_null_and_its_previous_object() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","owner":{"type":"Relationship","object":"urn:ngsi-ld:null","previousObject":"urn:ngsi-ld:P:1"},
            "link":{"type":"Relationship","object":"urn:ngsi-ld:P:3"}}"#,
            Lossless::Normalized,
        )?;
        let person: EntityId = "urn:ngsi-ld:P:1".parse()?;
        let other: EntityId = "urn:ngsi-ld:P:3".parse()?;
        expect_that!(&entity, has_attribute("owner", relationship().null_value().previous_object(person.clone())));
        expect_that!(&entity, not(has_attribute("owner", relationship().previous_object(other))));
        expect_that!(&entity, not(has_attribute("link", relationship().null_value())));
        verify_that!(&entity, not(has_attribute("link", relationship().previous_object(person))))
    }

    #[gtest]
    fn the_dataset_of_the_instance_read_is_checked() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","speed":{"type":"Property","value":1,"datasetId":"urn:ngsi-ld:Dataset:a"}}"#,
            Lossless::Normalized,
        )?;
        let (_, instances) = entity.attributes.iter().next().or_fail()?;
        expect_that!(instances.first(), property().with_dataset_id("urn:ngsi-ld:Dataset:a".parse()?));
        verify_that!(instances.first(), not(property().with_dataset_id("urn:ngsi-ld:Dataset:b".parse()?)))
    }

    #[gtest]
    fn the_instances_of_a_sub_attribute_are_counted_and_read_by_dataset() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","speed":{"type":"Property","value":1,"accuracy":[
            {"type":"Property","value":0.1,"datasetId":"urn:ngsi-ld:Dataset:a"},{"type":"Property","value":0.2}]}}"#,
            Lossless::Normalized,
        )?;
        let (_, instances) = entity.attributes.iter().next().or_fail()?;
        let speed = instances.first();
        expect_that!(speed, property().sub_attribute_instances("accuracy", 2));
        expect_that!(speed, not(property().sub_attribute_instances("accuracy", 1)));
        expect_that!(
            speed,
            property().sub_attribute(
                "accuracy",
                any_attribute()
                    .dataset_id("urn:ngsi-ld:Dataset:a".parse()?)
                    .value(Value::Number("0.1".parse()?))
            )
        );
        expect_that!(
            speed,
            property().sub_attribute(
                "accuracy",
                any_attribute().dataset_id(DatasetId::DefaultInstance).value(Value::Number("0.2".parse()?))
            )
        );
        verify_that!(
            speed,
            not(property().sub_attribute(
                "accuracy",
                any_attribute().dataset_id(DatasetId::DefaultInstance).value(Value::Number("0.1".parse()?))
            ))
        )
    }

    #[gtest]
    fn a_missing_value_member_and_language_members_are_checked() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:B:1","type":"B","address":{"type":"Property","value":{"city":"Paris"}},
            "street":{"type":"LanguageProperty","languageMap":{"fr":"Grand Place"}}}"#,
            Lossless::Normalized,
        )?;
        let mut attributes = entity.attributes.iter();
        let (_, address) = attributes.next().or_fail()?;
        let (_, street) = attributes.next().or_fail()?;
        expect_that!(address.first(), property().without_value_member(["country"]));
        expect_that!(address.first(), not(property().without_value_member(["city"])));
        expect_that!(address.first(), property().without(Member::LanguageMap).without(Member::Lang));
        verify_that!(street.first(), any_attribute().with_member(Member::LanguageMap))
    }

    #[gtest]
    fn a_property_a_language_filter_picked_names_its_language() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","street":{"type":"Property","value":"Grand Place","lang":"fr"}}"#,
            Lossless::Normalized,
        )?;
        let (_, instances) = entity.attributes.iter().next().or_fail()?;
        expect_that!(instances.first(), property().lang("fr").value("Grand Place"));
        verify_that!(instances.first(), not(property().lang("nl")))
    }

    #[gtest]
    fn the_items_of_a_list_are_counted() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","tags":{"type":"ListProperty","valueList":["a","b"]},"rooms":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:R:1"}]}}"#,
            Lossless::Normalized,
        )?;
        let mut attributes = entity.attributes.iter();
        let (_, tags) = attributes.next().or_fail()?;
        let (_, rooms) = attributes.next().or_fail()?;
        expect_that!(tags.first(), any_attribute().list_length(2));
        expect_that!(tags.first(), not(any_attribute().list_length(3)));
        expect_that!(rooms.first(), any_attribute().list_length(1));
        expect_that!(tags.first(), any_attribute().list_value(1, "b"));
        expect_that!(tags.first(), not(any_attribute().list_value(1, "a")));
        expect_that!(rooms.first(), any_attribute().list_object(0, "urn:ngsi-ld:R:1".parse()?));
        expect_that!(rooms.first(), not(any_attribute().list_object(0, "urn:ngsi-ld:R:2".parse()?)));
        verify_that!(level()?, not(any_attribute().list_length(1)))
    }

    #[gtest]
    fn a_vocab_property_holds_each_of_its_terms() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","usage":{"type":"VocabProperty","vocab":["commercial","residential"]}}"#,
            Lossless::Normalized,
        )?;
        let (_, usage) = entity.attributes.iter().next().or_fail()?;
        expect_that!(usage.first(), any_attribute().vocab_holding("residential".parse()?).list_length(2));
        verify_that!(usage.first(), not(any_attribute().vocab_holding("mixed".parse()?)))
    }

    #[gtest]
    fn the_instance_id_of_the_instance_read_is_checked() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","speed":{"type":"Property","value":1,"instanceId":"urn:ngsi-ld:Instance:a"}}"#,
            Lossless::Normalized,
        )?;
        let (_, instances) = entity.attributes.iter().next().or_fail()?;
        expect_that!(instances.first(), property().instance_id(&"urn:ngsi-ld:Instance:a".parse()?));
        verify_that!(instances.first(), not(property().instance_id(&"urn:ngsi-ld:Instance:b".parse()?)))
    }

    #[gtest]
    fn the_whole_json_of_a_json_property_is_checked() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","config":{"type":"JsonProperty","json":{"@context":"kept","count":3}}}"#,
            Lossless::Normalized,
        )?;
        let (_, instances) = entity.attributes.iter().next().or_fail()?;
        expect_that!(instances.first(), any_attribute().json(json!({"count": 3, "@context": "kept"})));
        verify_that!(instances.first(), not(any_attribute().json(json!({"count": 3}))))
    }

    #[gtest]
    fn the_previous_json_of_a_json_property_is_checked() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","config":{"type":"JsonProperty","json":[{"code":"b"}],"previousJson":[{"code":"a"}]}}"#,
            Lossless::Normalized,
        )?;
        let (_, instances) = entity.attributes.iter().next().or_fail()?;
        expect_that!(instances.first(), any_attribute().previous_json(json!([{"code": "a"}])));
        verify_that!(instances.first(), not(any_attribute().previous_json(json!([{"code": "b"}]))))
    }
}
