//! The normal form of a filter holds for exactly the Entities the filter as
//! lowered holds for, over generated Entities and filters, the corpus Entities
//! and every corpus filter, under both language matchings.
//!
//! The filters are generated over a few paths each, so their terms repeat,
//! share a path under several operands and nest groups of one connective in
//! another: the shapes the normal form rewrites.

use crate::{
    q::filter_run::filter_holds,
    queried_entity::{
        corpus_fixture::corpus_entities,
        expanded_fixture::{FixtureEntity, FixtureLinked},
    },
};
use andromeda_ngsi_ld_query::filter::{Filter, language_matching::LanguageMatching, reading::filter_reading::FilterReading};
use andromeda_q_corpus::{case_filter::expanded_filter, corpus::CORPUS, corpus_vocabulary::CorpusVocabulary};
use proptest::{
    collection::vec,
    option,
    prelude::{BoxedStrategy, Just, Strategy, prop_assert_eq, prop_oneof, proptest},
    sample::select,
    strategy::ValueTree,
    test_runner::{Config, TestRunner},
};
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

/// The attributes a generated Entity draws from.
const NAMES: [&str; 8] = ["a", "b", "c", "d", "e", "f", "g", "h"];

/// The identifiers of the two generated Entities, which their Relationships
/// name, so a hop crosses from one into the other.
const IDENTIFIERS: [&str; 2] = ["urn:ngsi-ld:T:G1", "urn:ngsi-ld:T:G2"];

/// The type both generated Entities carry.
const ENTITY_TYPE: &str = "https://example.com/T";

/// The sub-Property a generated instance may carry.
const SUB_PROPERTY: &str = "s";

/// The expanded name of a generated attribute.
pub(crate) fn generated_iri(term: &str) -> String {
    format!("https://example.com/{term}")
}

/// The expanded name of a value member.
fn core_iri(member: &str) -> String {
    format!("https://uri.etsi.org/ngsi-ld/{member}")
}

/// A literal value node, the scalars clause 4.9 compares.
fn literal() -> BoxedStrategy<Value> {
    prop_oneof![
        select(vec![json!(1), json!(5), json!(10), json!(-3), json!(2.5)]).prop_map(|number| json!({"@value": number})),
        select(vec!["abc", "red", "10", "urn:x:1"]).prop_map(|text| json!({"@value": text})),
        select(vec![true, false]).prop_map(|flag| json!({"@value": flag})),
        select(vec!["2024-01-15", "2024-06-01"]).prop_map(|date| json!({"@value": date, "@type": "https://uri.etsi.org/ngsi-ld/Date"})),
        select(vec!["2024-01-15T12:00:00Z", "2024-06-01T00:00:00Z"])
            .prop_map(|instant| json!({"@value": instant, "@type": "https://uri.etsi.org/ngsi-ld/DateTime"})),
        select(vec!["NaN", "INF", "-INF", "2.5"]).prop_map(|double| json!({"@value": double, "@type": "http://www.w3.org/2001/XMLSchema#double"})),
    ]
    .boxed()
}

/// The object of a Relationship, one of the generated Entities or an
/// identifier no Entity holds.
fn object() -> BoxedStrategy<Value> {
    select(vec![IDENTIFIERS[0], IDENTIFIERS[1], "urn:x:1"])
        .prop_map(|id| json!({"@id": id}))
        .boxed()
}

/// The value member of one attribute instance and the nodes it holds: a
/// Property with one or two values, a Relationship, a `ListRelationship`, a
/// `ListProperty`, a `VocabProperty` or a `LanguageProperty`.
fn content() -> BoxedStrategy<(String, Value)> {
    let languages = select(vec![
        json!([{"@language": "en", "@value": "red"}, {"@language": "fr", "@value": "rouge"}]),
        json!([{"@language": "en-GB", "@value": "lift"}]),
        json!([{"@language": "en", "@value": "cat"}, {"@language": "de", "@value": "Katze"}]),
    ]);
    prop_oneof![
        3 => vec(literal(), 1..=2).prop_map(|values| (core_iri("hasValue"), Value::Array(values))),
        1 => object().prop_map(|object| (core_iri("hasObject"), json!([object]))),
        1 => vec(object(), 1..=2).prop_map(|objects| (core_iri("hasObjectList"), json!([{"@list": objects}]))),
        1 => vec(literal(), 1..=2).prop_map(|values| (core_iri("hasValueList"), json!([{"@list": values}]))),
        1 => select(vec!["https://example.com/Male", "https://example.com/Female"]).prop_map(|term| (core_iri("hasVocab"), json!([{"@id": term}]))),
        1 => languages.prop_map(|map| (core_iri("hasLanguageMap"), map)),
    ]
    .boxed()
}

/// One attribute instance: its content, an `observedAt` it may carry and a
/// sub-Property it may carry.
fn instance() -> BoxedStrategy<Value> {
    let observed = option::of(select(vec!["2024-01-15T12:00:00Z", "2024-06-01T00:00:00Z"]));
    let sub_property = option::of(select(vec![json!(1), json!(5), json!("abc")]));
    (content(), observed, sub_property)
        .prop_map(|((member, nodes), observed, sub_property)| {
            let mut members = Map::new();
            members.insert(member, nodes);
            if let Some(instant) = observed {
                members.insert(
                    core_iri("observedAt"),
                    json!([{"@value": instant, "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]),
                );
            }
            if let Some(value) = sub_property {
                members.insert(generated_iri(SUB_PROPERTY), json!([{core_iri("hasValue"): [{"@value": value}]}]));
            }
            Value::Object(members)
        })
        .boxed()
}

/// One attribute's instances: one, or one more carrying a `datasetId`.
fn instances() -> BoxedStrategy<Value> {
    (instance(), option::of(instance()))
        .prop_map(|(default, dataset)| {
            let mut instances = vec![default];
            if let Some(Value::Object(mut members)) = dataset {
                members.insert(core_iri("datasetId"), json!([{"@id": "urn:ngsi-ld:Dataset:d"}]));
                instances.push(Value::Object(members));
            }
            Value::Array(instances)
        })
        .boxed()
}

/// One generated Entity with one to four of the attributes, as an expanded
/// document.
fn entity(id: &'static str) -> BoxedStrategy<Value> {
    vec((select(NAMES.to_vec()), instances()), 1..=4)
        .prop_map(move |attributes| {
            let mut document = Map::new();
            document.insert("@id".to_owned(), json!(id));
            document.insert("@type".to_owned(), json!([ENTITY_TYPE]));
            for (name, instances) in attributes {
                document.insert(generated_iri(name), instances);
            }
            Value::Array(vec![Value::Object(document)])
        })
        .boxed()
}

/// The two generated Entities as expanded documents, and a filter over the
/// attributes they hold.
pub(crate) fn generated_case() -> BoxedStrategy<(Vec<Value>, String)> {
    (entity(IDENTIFIERS[0]), entity(IDENTIFIERS[1]))
        .prop_flat_map(|(first, second)| {
            let held: Vec<&'static str> = NAMES
                .into_iter()
                .filter(|name| [&first, &second].iter().any(|document| document[0].get(generated_iri(name)).is_some()))
                .collect();
            (Just(vec![first, second]), filter_over(generated_path(held)))
        })
        .boxed()
}

/// The Entities `documents` hold, decoded.
pub(crate) fn decoded(documents: &[Value]) -> Vec<FixtureEntity> {
    documents.iter().map(FixtureEntity::from_expanded).collect()
}

/// A quoted expanded name.
fn quoted(iri: &str) -> String {
    format!("\"{iri}\"")
}

/// A path over the generated attributes `held`: an attribute, then possibly
/// the sub-Property or an instance member, then possibly a trailing group, all
/// of it possibly behind a hop.
fn generated_path(held: Vec<&'static str>) -> BoxedStrategy<String> {
    let segment = prop_oneof![
        6 => Just(String::new()),
        1 => select(vec![
            format!(".{}", quoted(&generated_iri(SUB_PROPERTY))),
            format!(".{}", quoted(&core_iri("observedAt"))),
            format!(".{}", quoted(&core_iri("datasetId"))),
        ]),
    ];
    let trailing = prop_oneof![
        6 => Just(""),
        1 => select(vec!["[*]", "[en]", "[fr]", "[en-GB]", "[de-*-DE]"]),
    ];
    let local = (select(held.clone()), segment, trailing).prop_map(|(name, segment, trailing)| format!("{}{segment}{trailing}", quoted(&generated_iri(name))));
    let guard = select(vec![
        String::new(),
        format!("{}:", quoted(ENTITY_TYPE)),
        format!("{},{}:", quoted("https://example.com/U"), quoted(ENTITY_TYPE)),
    ]);
    prop_oneof![
        5 => local.clone(),
        1 => (select(held), guard, local).prop_map(|(relationship, guard, inner)| format!("{}{{{guard}{inner}}}", quoted(&generated_iri(relationship)))),
    ]
    .boxed()
}

/// A scalar operand over the values the generated Entities hold.
fn scalar() -> BoxedStrategy<String> {
    select(vec![
        "1",
        "5",
        "10",
        "10.0",
        "1e1",
        "-3",
        "2.5",
        "\"abc\"",
        "\"red\"",
        "\"rouge\"",
        "\"10\"",
        "\"urn:x:1\"",
        "urn:ngsi-ld:T:G1",
        "true",
        "false",
        "2024-01-15",
        "2024-01-15T12:00:00Z",
        "\"https://example.com/Male\"",
    ])
    .prop_map(str::to_owned)
    .boxed()
}

/// An operand an ordering operator or a range bound compares.
fn comparable() -> BoxedStrategy<String> {
    select(vec!["1", "5", "10", "2.5", "\"abc\"", "\"m\"", "2024-01-15", "2024-01-15T12:00:00Z"])
        .prop_map(str::to_owned)
        .boxed()
}

/// The test a term applies after `path`: existence, an equality with a value,
/// a value list or a range, an ordering or a pattern.
fn term_over(path: String) -> BoxedStrategy<String> {
    let equality = prop_oneof![
        3 => scalar(),
        1 => vec(scalar(), 2..=3).prop_map(|values| values.join(",")),
        1 => (comparable(), comparable()).prop_map(|(min, max)| format!("{min}..{max}")),
    ];
    let equality_operator = prop_oneof![2 => Just("=="), 1 => Just("!=")];
    prop_oneof![
        1 => Just(path.clone()),
        4 => (equality_operator, equality).prop_map({
            let path = path.clone();
            move |(operator, operand)| format!("{path}{operator}{operand}")
        }),
        1 => (select(vec![">", ">=", "<", "<="]), comparable()).prop_map({
            let path = path.clone();
            move |(operator, operand)| format!("{path}{operator}{operand}")
        }),
        1 => (select(vec!["~=", "!~="]), select(vec!["\"ab.*\"", "\"^r\"", "\"1\""])).prop_map(move |(operator, pattern)| format!("{path}{operator}{pattern}")),
    ]
    .boxed()
}

/// A filter over a few terms, each over one of a few paths, joined by `;` and
/// `|` in groups nested up to three deep, the same term free to appear more
/// than once.
pub(crate) fn filter_over(paths: BoxedStrategy<String>) -> BoxedStrategy<String> {
    vec(paths, 1..=3)
        .prop_flat_map(|paths| vec(select(paths).prop_flat_map(term_over), 1..=5))
        .prop_flat_map(|terms| {
            select(terms).prop_recursive(2, 8, 3, |inner| {
                prop_oneof![
                    vec(inner.clone(), 2..=4).prop_map(|members| format!("({})", members.join(";"))),
                    vec(inner, 2..=4).prop_map(|members| format!("({})", members.join("|"))),
                ]
            })
        })
        .boxed()
}

/// A path over the corpus vocabulary: an attribute, then possibly a
/// sub-attribute or an instance member, then possibly a trailing group.
pub(crate) fn corpus_path() -> BoxedStrategy<String> {
    let name = |term: &str| quoted(&CorpusVocabulary::attribute_iri(term));
    let terms = vec![
        "n", "s", "b", "r", "rl", "l", "d", "tm", "t", "f", "m", "addr", "city", "zip", "vp", "lp", "jp", "link", "src",
    ];
    let segment = prop_oneof![
        6 => Just(String::new()),
        1 => select(terms.clone()).prop_map(move |term| format!(".{}", name(term))),
        1 => select(vec!["observedAt", "unitCode", "datasetId"]).prop_map(|member| format!(".{}", quoted(&core_iri(member)))),
    ];
    let trailing = prop_oneof![
        6 => Just(""),
        1 => select(vec!["[*]", "[en]", "[en-GB]", "[\"https://example.com/city\"]", "[city]"]),
    ];
    (select(terms), segment, trailing)
        .prop_map(move |(term, segment, trailing)| format!("{}{segment}{trailing}", name(term)))
        .boxed()
}

/// The corpus Entities, decoded once.
pub(crate) static CORPUS_ENTITIES: LazyLock<Vec<FixtureEntity>> = LazyLock::new(corpus_entities);

/// The first Entity and matching for which the normal form of `expr` and
/// `expr` as lowered disagree, over `entities`.
fn disagreement(expr: &Filter, entities: &[FixtureEntity]) -> Option<String> {
    [LanguageMatching::TagEquality, LanguageMatching::ExtendedFiltering]
        .into_iter()
        .find_map(|matching| {
            let lowered = FilterReading::lowered(expr, matching);
            let normal = FilterReading::of(expr, matching);
            entities.iter().enumerate().find_map(|(index, entity)| {
                let as_lowered = filter_holds(&lowered, entity, FixtureLinked(entities));
                let as_normal = filter_holds(&normal, entity, FixtureLinked(entities));
                (as_lowered != as_normal).then(|| format!("entity {index} under {matching:?}: as lowered {as_lowered}, normal form {as_normal}"))
            })
        })
}

proptest! {
    #![proptest_config(Config::with_cases(4096))]

    /// The normal form agrees with the lowered filter over generated Entities.
    #[test]
    fn the_normal_form_holds_for_the_generated_entities_the_filter_holds_for((documents, text) in generated_case()) {
        let expr = text.parse::<Filter>().unwrap_or_else(|error| panic!("the generator writes grammatical filters, and {text} is refused: {error}"));
        prop_assert_eq!(disagreement(&expr, &decoded(&documents)), None, "{}", text);
    }

    /// The normal form agrees with the lowered filter over the corpus Entities.
    #[test]
    fn the_normal_form_holds_for_the_corpus_entities_the_filter_holds_for(text in filter_over(corpus_path())) {
        let expr = text.parse::<Filter>().unwrap_or_else(|error| panic!("the generator writes grammatical filters, and {text} is refused: {error}"));
        prop_assert_eq!(disagreement(&expr, &CORPUS_ENTITIES), None, "{}", text);
    }
}

/// The normal form agrees with the lowered filter for every corpus filter over the corpus Entities.
#[tokio::test]
async fn the_normal_form_of_every_corpus_filter_holds_for_the_entities_it_holds_for() {
    for case in CORPUS {
        let expr = expanded_filter(case)
            .await
            .unwrap_or_else(|refusal| panic!("{} {} is refused: {refusal}", case.id, case.q));
        pretty_assertions::assert_eq!(disagreement(&expr, &CORPUS_ENTITIES), None, "{}", case.q);
    }
}

/// How many of 1000 filters `cases` writes have a normal form other than
/// their lowering, and how many select some of their Entities and not others.
fn exercise(cases: &BoxedStrategy<(Vec<FixtureEntity>, String)>) -> (u32, u32) {
    let mut runner = TestRunner::deterministic();
    let (mut rewritten, mut selective) = (0, 0);
    for _ in 0..1000 {
        let (entities, text) = cases.new_tree(&mut runner).expect("a case").current();
        let expr = text.parse::<Filter>().unwrap_or_else(|error| panic!("{text} is refused: {error}"));
        let normal = FilterReading::of(&expr, LanguageMatching::TagEquality);
        if normal != FilterReading::lowered(&expr, LanguageMatching::TagEquality) {
            rewritten += 1;
        }
        let held = entities
            .iter()
            .filter(|entity| filter_holds(&normal, *entity, FixtureLinked(&entities)))
            .count();
        if held > 0 && held < entities.len() {
            selective += 1;
        }
    }
    (rewritten, selective)
}

/// The generators reach what the property is about: most filters they write
/// have a normal form other than their lowering, and a good share select some
/// of the Entities and not others.
#[test]
fn the_generated_filters_exercise_the_normal_form() {
    let generated = exercise(&generated_case().prop_map(|(documents, text)| (decoded(&documents), text)).boxed());
    let corpus = exercise(&filter_over(corpus_path()).prop_map(|text| (corpus_entities(), text)).boxed());
    assert!(
        generated.0 >= 800 && generated.1 >= 200,
        "over generated Entities, {generated:?} of 1000 filters are rewritten and selective"
    );
    assert!(
        corpus.0 >= 800 && corpus.1 >= 200,
        "over the corpus Entities, {corpus:?} of 1000 filters are rewritten and selective"
    );
}
