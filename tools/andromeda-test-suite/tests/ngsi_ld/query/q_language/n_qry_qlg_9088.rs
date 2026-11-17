//! A segment naming the value member of any attribute family addresses nothing in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, QRequest, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{
    Entity,
    EntityId,
    JsonProperty,
    LanguageMap,
    LanguageProperty,
    ListProperty,
    ListRelationship,
    RawJson,
    Relationship,
    Value,
    VocabProperty,
    VocabTerm,
};

/// The term `q` selecting `selects`, asked with no other request member.
const fn case<'c>(q: &'c str, selects: &'c [&'c EntityId]) -> QCase<'c> {
    QCase {
        q,
        selects,
        request: QRequest::PLAIN,
    }
}

/// Pins: for Things whose `p` is a Relationship to `other` carrying the sub-Property `s` 5, a `LanguageProperty` with
/// `en` "red" carrying the sub-Property `s` 5, a `ListProperty` 1, 2, a `ListRelationship` to `other`, a
/// `JsonProperty` `{"a": 1}` and a `VocabProperty` `Male`, Query Entities and a Subscription with q p select all six
/// and with q p.s==5 the first two, and with q p.object, p.object=="<other>", p.object!="<none>", p.languageMap,
/// p.valueList, p.objectList, p.json and p.vocab select nothing; each name after the first of an attribute path is a
/// sub-Property or sub-Relationship, and the value member of an attribute family is neither
/// Clause: 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9088_a_segment_naming_the_value_member_of_any_family_addresses_nothing_in_a_query_and_a_subscription_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let thing_type = namespace.entity_type("Thing")?;
    let other = namespace.urn("Thing", "other")?;
    let none = namespace.urn("Thing", "none")?;
    let link = namespace.urn("Thing", "link")?;
    let label = namespace.urn("Thing", "label")?;
    let list = namespace.urn("Thing", "list")?;
    let objects = namespace.urn("Thing", "objects")?;
    let json = namespace.urn("Thing", "json")?;
    let vocab = namespace.urn("Thing", "vocab")?;
    let thing = |id: &EntityId| Entity::builder().id(id.clone()).entity_type(thing_type.clone());
    let entities = [
        thing(&link).relationship("p", Relationship::builder(other.clone()).property("s", 5)).build()?,
        thing(&label)
            .language_property("p", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "red")])?).property("s", 5))
            .build()?,
        thing(&list)
            .list_property("p", ListProperty::builder(vec![Value::from(1), Value::from(2)]))
            .build()?,
        thing(&objects).list_relationship("p", ListRelationship::builder(vec![other.clone()])).build()?,
        thing(&json)
            .json_property("p", JsonProperty::builder(r#"{"a":1}"#.parse::<RawJson>()?))
            .build()?,
        thing(&vocab)
            .vocab_property("p", VocabProperty::builder("Male".parse::<VocabTerm>()?))
            .build()?,
    ];
    let object_equal = format!("p.object==\"{other}\"");
    let object_unequal = format!("p.object!=\"{none}\"");
    let every = [&link, &label, &list, &objects, &json, &vocab];
    let carrying_s = [&link, &label];
    let cases = [
        case("p", &every),
        case("p.s==5", &carrying_s),
        case("p.object", &[]),
        case(&object_equal, &[]),
        case(&object_unequal, &[]),
        case("p.languageMap", &[]),
        case("p.valueList", &[]),
        case("p.objectList", &[]),
        case("p.json", &[]),
        case("p.vocab", &[]),
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &thing_type, &entities, &cases).await
}
