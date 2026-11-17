//! The fixed `q` corpus asked of Query Entities and of a Subscription.
//!
//! The corpus names, for each filter, the Entities clause 4.9 has it select among a fixed set. A case here creates
//! those Entities under the test's own namespace and expects Query Entities and a Subscription with the case's filter
//! to answer exactly the Entities the corpus names.
//!
//! The Entities are of the namespace's type `T_<token>`, which keeps a case's queries and notifications to its own
//! Entities. The corpus writes that type `T` where a hop guard names it, so a guard `{T:` is read as the namespace's
//! type.

use crate::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, QRequest, expect_query_and_subscription_alike},
    value::{object, xsd_typed},
};
use andromeda_q_corpus::{
    corpus::{CORPUS, CorpusCase},
    corpus_entity::{CORPUS_TYPE, CorpusEntity},
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::model::{
    CoreDatatype,
    DatasetId,
    Entity,
    EntityId,
    EntityTypeName,
    Input,
    JsonProperty,
    LanguageMap,
    ListProperty,
    ListRelationship,
    Number,
    Property,
    RawJson,
    Relationship,
    TypedValue,
    Value,
    VocabProperty,
    VocabTerm,
    geometry::{BoundingBox, Geometry, Position, Shape},
};
use std::collections::BTreeMap;
use strum::IntoEnumIterator;

/// The Entities of one case, by the name the corpus gives them, and the type they share.
struct CorpusEntities {
    /// The namespace's corpus type.
    entity_type: EntityTypeName,
    /// The identifier of each corpus Entity.
    ids: BTreeMap<CorpusEntity, EntityId>,
}

impl CorpusEntities {
    /// The Entities of the corpus under `namespace`.
    fn under(namespace: &Namespace) -> Result<CorpusEntities> {
        let mut ids = BTreeMap::new();
        for entity in CorpusEntity::iter() {
            ids.insert(entity, namespace.urn(CORPUS_TYPE, entity)?);
        }
        Ok(CorpusEntities {
            entity_type: namespace.entity_type(CORPUS_TYPE)?,
            ids,
        })
    }

    /// The identifier of `entity`.
    fn id(&self, entity: CorpusEntity) -> Result<&EntityId> {
        self.ids.get(&entity).or_fail()
    }

    /// The Entities of the corpus, each linked Entity before the Entity linking to it.
    fn documents(&self) -> Result<Vec<Entity<Input>>> {
        CorpusEntity::iter().map(|entity| self.document(entity)).collect()
    }

    /// The document of `entity`, as its variant describes it.
    fn document(&self, entity: CorpusEntity) -> Result<Entity<Input>> {
        let builder = Entity::builder().id(self.id(entity)?.clone()).entity_type(self.entity_type.clone());
        let builder = match entity {
            CorpusEntity::E1 => builder.property("n", 10).property("s", "abc").property("b", true),
            CorpusEntity::E2 => builder.property("n", 20).property("s", "abd").property("b", false),
            CorpusEntity::E3 => builder.property("n", "10").property("s", 10),
            CorpusEntity::E4 => builder.property("n", Value::Array(vec![number("10")?, number("30")?])),
            CorpusEntity::E5 => builder
                .property("n", 10)
                .property("n", Property::builder(60).dataset_id("urn:ngsi-ld:Dataset:corpus".parse::<DatasetId>()?)),
            CorpusEntity::E6 => builder.property("other", 1),
            CorpusEntity::E7 => builder
                .relationship("r", Relationship::builder("urn:x:1".parse::<EntityId>()?))
                .list_relationship("rl", ListRelationship::builder(vec!["urn:x:1".parse::<EntityId>()?, "urn:x:2".parse()?])),
            CorpusEntity::E8 => builder.language_property("l", LanguageMap::try_from_pairs([("en", "red"), ("fr", "rouge")])?),
            CorpusEntity::E9 => builder
                .property("d", Property::builder("2024-01-15").value_type("Date".parse::<VocabTerm>()?))
                .property("tm", Property::builder("2024-01-15T12:00:00Z").value_type("DateTime".parse::<VocabTerm>()?)),
            CorpusEntity::E10 => builder.property("f", xsd_typed("double", "NaN")?).property("g", xsd_typed("double", "INF")?),
            CorpusEntity::E11 => builder.property("m", Property::builder(5).observed_at("2024-06-01T00:00:00Z".parse()?).unit_code(UnitCode::Cel)),
            CorpusEntity::E12 => builder.property("k", Property::builder(1).property("src", "a")),
            CorpusEntity::E13 => builder.property(
                "addr",
                object([
                    ("city", Value::from("X")),
                    ("zip", number("1")?),
                    ("tags", Value::Array(Vec::new())),
                    ("geo", object([("lat", number("1")?)])),
                    ("mix", Value::Array(vec![number("1")?, object([("a", number("2")?)])])),
                    ("since", Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, "0000-02-29T12:00:00Z")?)),
                ]),
            ),
            CorpusEntity::E14 => builder
                .relationship("h", Relationship::builder("http://example.com/a".parse::<EntityId>()?))
                .property("hp", "http://example.com/a")
                .geo_property(
                    "location",
                    Geometry::from(Shape::Point(Position::try_new(13.4, 52.5)?))
                        .with_bbox(BoundingBox::try_new(Position::try_new(13.3, 52.4)?, Position::try_new(13.5, 52.6)?)?)?
                        .with_foreign_member("title".parse()?, r#""Gate""#.parse::<RawJson>()?)
                        .with_foreign_member("meta".parse()?, r#"{"source":"survey"}"#.parse::<RawJson>()?),
                ),
            CorpusEntity::E15 => builder.vocab_property("vp", VocabProperty::builder("Male".parse::<VocabTerm>()?)),
            CorpusEntity::E16 => builder
                .list_property("lp", ListProperty::builder(vec![number("1")?, number("2")?]))
                .json_property("jp", JsonProperty::builder(r#"{"value":"x"}"#.parse::<RawJson>()?))
                .json_property("jn", JsonProperty::builder(r#"{"a":[1,[2]]}"#.parse::<RawJson>()?)),
            CorpusEntity::E17 => builder
                .relationship("link", Relationship::builder(self.id(CorpusEntity::E1)?.clone()))
                .relationship("link2", Relationship::builder(self.id(CorpusEntity::E6)?.clone()))
                .list_relationship("llink", ListRelationship::builder(vec![self.id(CorpusEntity::E1)?.clone()]))
                .relationship(
                    "hm",
                    Relationship::builder(vec!["http://example.com/a".parse::<EntityId>()?, "urn:x:9".parse()?]),
                ),
            CorpusEntity::E18 => builder
                .property("n1", Value::Array(vec![number("10")?]))
                .property("e0", Value::Array(Vec::new())),
            CorpusEntity::E19 => builder
                .property("t", Property::builder("12:00:00Z").value_type("Time".parse::<VocabTerm>()?))
                .json_property("jw", JsonProperty::builder(r#"{"@type":"@json","@value":{"a":2}}"#.parse::<RawJson>()?))
                .geo_property("place", Shape::Point(Position::try_new(8.8, 52.6)?))
                .property("rows", Value::Array(vec![object([("a", number("5")?)]), object([("a", number("6")?)])])),
            CorpusEntity::E20 => builder.property("rows", Value::Array(vec![object([("a", number("5")?)])])),
        };
        Ok(builder.build()?)
    }
}

/// The number `text` as a Value.
fn number(text: &str) -> Result<Value> {
    Ok(Value::Number(text.parse::<Number>()?))
}

/// The corpus case named `id`.
fn corpus_case(id: &str) -> Result<&'static CorpusCase> {
    CORPUS.iter().find(|case| case.id == id).or_fail()
}

/// Creates the corpus Entities and expects Query Entities and a Subscription with the filter of the corpus case `id`
/// to select exactly the Entities the corpus names.
///
/// # Errors
///
/// Fails when the case is not in the corpus, a value cannot be built, a request is refused or an answer differs from
/// the corpus.
pub async fn expect_corpus_case(id: &str) -> Result<()> {
    let case = corpus_case(id)?;
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let entities = CorpusEntities::under(&namespace)?;
    let documents = entities.documents()?;
    let q = case.q.replace(&format!("{{{CORPUS_TYPE}:"), &format!("{{{}:", entities.entity_type.as_str()));
    let mut selects = Vec::with_capacity(case.selects.len());
    for entity in case.selects {
        selects.push(entities.id(*entity)?);
    }
    let cases = [QCase {
        q: &q,
        selects: &selects,
        request: QRequest {
            expand_values: case.expand_values,
            json_keys: case.json_keys,
            traversal: case.traversal,
        },
    }];
    expect_query_and_subscription_alike(&broker, &namespace, &entities.entity_type, &documents, &cases).await
}
