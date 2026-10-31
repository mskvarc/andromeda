//! Fifty entities of every family upserted twice read back as the second write.

use andromeda_test_suite::{
    attribute_matcher::{AttributeMatcher, any_attribute, attribute_of, geo_property, property, relationship},
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::{Done, UpsertCreated},
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{UpsertOutcome, headers::Accept},
    model::{
        AttributeKind,
        Entity,
        EntityId,
        EntityTypeName,
        GeoProperty,
        Input,
        JsonProperty,
        LanguageMap,
        LanguageProperty,
        ListProperty,
        ListRelationship,
        NgsiLdMediaType,
        Output,
        Property,
        RawJson,
        Relationship,
        Value,
        VocabProperty,
        VocabTerm,
        geometry::{Position, Shape},
    },
};
use serde_json::json;

/// Pins: a Batch Entity Upsert creating fifty entities, one Attribute of each of the eight families among them
/// beside a Property with a sub-attribute, and a second one replacing all fifty with changed values, dropped and
/// added Attributes, reads back every entity as the second write stated it
/// Clause: 5.6.8.4, 6.15.3.1, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_bat_ups_0017_fifty_entities_of_every_family_upserted_twice_read_back_as_the_second_write() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let first = (0..ENTITIES)
        .map(|i| entity(&namespace, &building_type, i, Write::First))
        .collect::<Result<Vec<_>>>()?;
    let created = broker.entities().batch_upsert(&first).send().await?.created_entities()?;
    expect_that!(created, len(eq(COUNT)));
    let second = (0..ENTITIES)
        .map(|i| entity(&namespace, &building_type, i, Write::Second))
        .collect::<Result<Vec<_>>>()?;
    let updated = broker.entities().batch_upsert(&second).send().await?.done()?;
    expect_that!(updated, eq(&UpsertOutcome::Updated));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(100)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(COUNT)));
    for i in 0..ENTITIES {
        let id = building_id(&namespace, i)?;
        let stored = page.items().iter().find(|stored| stored.id == id).or_fail()?;
        expect_second_write(stored, &namespace, i)?;
    }
    Ok(())
}

/// The number of entities each write carries.
const ENTITIES: i32 = 50;

/// The number of entities each write carries, as a length.
const COUNT: usize = 50;

/// Which of the two writes an entity is built for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Write {
    /// The write that creates the entities.
    First,
    /// The write that replaces them.
    Second,
}

/// The eight families of attribute, one of which each entity carries beside `index`.
#[derive(Clone, Copy)]
enum Family {
    /// A Property with a unit code.
    Reading,
    /// A Relationship.
    Owner,
    /// A `GeoProperty`.
    Location,
    /// A `LanguageProperty`.
    Label,
    /// A `VocabProperty`.
    Category,
    /// A `JsonProperty`.
    Settings,
    /// A `ListProperty`.
    Series,
    /// A `ListRelationship`.
    Route,
}

impl Family {
    /// The family of entity `i`, one per residue of `i` modulo eight.
    const fn of(i: i32) -> Family {
        match i % 8 {
            0 => Family::Reading,
            1 => Family::Owner,
            2 => Family::Location,
            3 => Family::Label,
            4 => Family::Category,
            5 => Family::Settings,
            6 => Family::Series,
            _ => Family::Route,
        }
    }

    /// The attribute name of the family.
    const fn name(self) -> &'static str {
        match self {
            Family::Reading => "reading",
            Family::Owner => "owner",
            Family::Location => "location",
            Family::Label => "label",
            Family::Category => "category",
            Family::Settings => "settings",
            Family::Series => "series",
            Family::Route => "route",
        }
    }
}

/// The id of entity `i`.
fn building_id(namespace: &Namespace, i: i32) -> Result<EntityId> {
    Ok(namespace.urn("Building", format!("{i:02}"))?)
}

/// The number entity `i` carries in `write`: its own in the first, a hundred above it in the second.
const fn base(i: i32, write: Write) -> i32 {
    match write {
        Write::First => i,
        Write::Second => 100 + i,
    }
}

/// Whether the second write keeps the family attribute of entity `i`.
const fn keeps_family(i: i32) -> bool {
    i % 3 != 0
}

/// Whether the second write adds `extra` to entity `i`.
const fn adds_extra(i: i32) -> bool {
    i % 2 == 0
}

/// The parity member of entity `i`.
const fn parity(i: i32) -> &'static str {
    if i % 2 == 0 { "even" } else { "odd" }
}

/// The point of entity `i`: a tenth of a degree further per entity from 10.05 and 0.05; the second write repeats the
/// longitude as the latitude.
fn point(i: i32, write: Write) -> Result<Position> {
    let hundredths = i * 10 + 5;
    let latitude = format!("{}.{:02}", hundredths / 100, hundredths % 100).parse::<f64>()?;
    let longitude = format!("{}.{:02}", 10 + hundredths / 100, hundredths % 100).parse::<f64>()?;
    Ok(match write {
        Write::First => Position::try_new(longitude, latitude)?,
        Write::Second => Position::try_new(longitude, longitude)?,
    })
}

/// Entity `i` as `write` states it.
fn entity(namespace: &Namespace, entity_type: &EntityTypeName, i: i32, write: Write) -> Result<Entity<Input>> {
    let number = base(i, write);
    let mut builder = Entity::builder()
        .id(building_id(namespace, i)?)
        .entity_type(entity_type.clone())
        .property("index", Property::builder(number).property("parity", parity(i)));
    if write == Write::First || keeps_family(i) {
        builder = match Family::of(i) {
            Family::Reading => builder.property("reading", Property::builder(number).unit_code(UnitCode::Cel)),
            Family::Owner => builder.relationship("owner", Relationship::builder(namespace.urn("Person", number)?)),
            Family::Location => builder.geo_property("location", GeoProperty::builder(Shape::Point(point(i, write)?))),
            Family::Label => {
                let english = format!("label {number}");
                let german = format!("Etikett {number}");
                builder.language_property(
                    "label",
                    LanguageProperty::builder(LanguageMap::try_from_pairs([("en", english.as_str()), ("de", german.as_str())])?),
                )
            }
            Family::Category => builder.vocab_property("category", VocabProperty::builder(format!("category{number}").parse::<VocabTerm>()?)),
            Family::Settings => {
                let on = write == Write::Second;
                builder.json_property(
                    "settings",
                    JsonProperty::builder(format!(r#"{{"level":{number},"on":{on}}}"#).parse::<RawJson>()?),
                )
            }
            Family::Series => builder.list_property(
                "series",
                ListProperty::builder(vec![Value::from(number), Value::from(number + 1), Value::from(number + 2)]),
            ),
            Family::Route => builder.list_relationship(
                "route",
                ListRelationship::builder(vec![namespace.urn("Stop", number)?, namespace.urn("Stop", number + 1)?]),
            ),
        };
    }
    if write == Write::Second && adds_extra(i) {
        builder = builder.property("extra", format!("extra {i}").as_str());
    }
    Ok(builder.build()?)
}

/// What the second write states of the family attribute of entity `i`.
fn second_family(namespace: &Namespace, i: i32) -> Result<AttributeMatcher> {
    let number = base(i, Write::Second);
    Ok(match Family::of(i) {
        Family::Reading => property().value(number).unit_code(UnitCode::Cel),
        Family::Owner => relationship().object(namespace.urn("Person", number)?),
        Family::Location => geo_property().geometry(Shape::Point(point(i, Write::Second)?)),
        Family::Label => any_attribute()
            .language("en", &format!("label {number}"))
            .language("de", &format!("Etikett {number}")),
        Family::Category => attribute_of(AttributeKind::VocabProperty).vocab(format!("category{number}").parse::<VocabTerm>()?),
        Family::Settings => attribute_of(AttributeKind::JsonProperty).json(json!({"level": number, "on": true})),
        Family::Series => any_attribute()
            .list_length(3)
            .list_value(0, number)
            .list_value(1, number + 1)
            .list_value(2, number + 2),
        Family::Route => any_attribute()
            .list_length(2)
            .list_object(0, namespace.urn("Stop", number)?)
            .list_object(1, namespace.urn("Stop", number + 1)?),
    })
}

/// Requires `stored`, entity `i`, to read as the second write stated it.
fn expect_second_write(stored: &Entity<Output>, namespace: &Namespace, i: i32) -> Result<()> {
    let number = base(i, Write::Second);
    expect_that!(
        stored,
        has_attribute("index", any_attribute().value(number).sub_attribute("parity", any_attribute().value(parity(i))))
    );
    let name = Family::of(i).name();
    if keeps_family(i) {
        expect_that!(stored, has_attribute(name, second_family(namespace, i)?));
    } else {
        expect_that!(stored, lacks_attribute(name));
    }
    if adds_extra(i) {
        expect_that!(stored, has_attribute("extra", any_attribute().value(format!("extra {i}").as_str())));
    } else {
        expect_that!(stored, lacks_attribute("extra"));
    }
    Ok(())
}
