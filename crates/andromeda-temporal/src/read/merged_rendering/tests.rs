use crate::{
    merged_parts::{EntityParts, LocalPart},
    query_reading::QueryReading,
    stub_broker::{StubBroker, live_target, service_over, uri},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
use andromeda_ngsi_ld_lexicon::{
    aggr_method::AggrMethod,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    temporal_format::TemporalFormat,
    time_rel::TimeRel,
};
use andromeda_ngsi_ld_model::{
    received::{ReceivedElement, ReceivedEntities},
    temporal::{
        AggregationAverage,
        StreamExtent,
        TemporalAggregationRow,
        TemporalValue,
        aggregated_entity::{AggregateValue, AggregatedValues},
        temporal_body::TemporalBody,
        temporal_document::TemporalDocument,
        temporal_history::TemporalHistory,
    },
};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::filter::language_targeting::LanguageTargeting;
use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
use andromeda_ngsi_ld_request::{query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Arc;

const SPEED: &str = "https://example.com/speed";

fn context() -> ContextSpec {
    ContextSpec::from_body(JsonLiteral::admit(&json!({"speed": SPEED}).to_string()).expect("an admissible literal"))
}

/// The history one source answered the entity with: one `speed` observation of
/// `value`.
async fn answered(value: i64) -> TemporalDocument {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
    let answer = json!({
        "id": "urn:ngsi-ld:T:A",
        "type": "Thing",
        "speed": [{"type": "Property", "value": value, "observedAt": "2024-01-01T00:30:00Z"}]
    });
    let ReceivedEntities(mut elements) = ReceivedEntities::from_slice(answer.to_string().as_bytes()).expect("an answer");
    let Some(ReceivedElement::Read(contextual)) = elements.pop() else {
        panic!("the answer is a temporal entity");
    };
    let TemporalBody::Evolution(entity) = contextual.temporal(ContextReading::keyword_spellings()).expect("the answer reads") else {
        panic!("the answer is a Temporal Evolution");
    };
    let expansion = EntityExpansion::prepare(&engine, &context()).await.expect("the context resolves");
    expansion.temporal(entity).await.expect("the answer expands")
}

/// The one period the store aggregated to, over every instance it was given.
fn aggregate_of_the_merged_instances(average: i64) -> TemporalAggregationRow {
    TemporalAggregationRow {
        attribute_name: SPEED.parse().expect("an attribute name"),
        attribute_type: AttributeType::Property,
        dataset_id: None,
        bucket_start: None,
        bucket_end: None,
        avg: Some(AggregationAverage::Number(JsonNumber::from(average))),
        min: None,
        max: None,
        sum: None,
        sumsq: None,
        stddev: None,
        total_count: None,
        distinct_count: None,
        group_keys: Vec::new(),
        stream_extent: StreamExtent::Whole,
    }
}

fn aggregated_query() -> TemporalQueryParameters {
    TemporalQueryParameters {
        timerel: Some(TimeRel::After),
        time_at: Some("2024-01-01T00:00:00Z".parse().expect("a valid instant")),
        format: Some(WrittenValue::Supported(TemporalFormat::AggregatedValues)),
        aggr_methods: Some(vec![AggrMethod::Avg]),
        ..TemporalQueryParameters::default()
    }
}

/// Clause 5.7.4.4: two aggregates of one period are no aggregate of their
/// union, so the instances a source answered are handed to the store, which
/// aggregates them beside the local ones in one statement; what the entity is
/// rendered with is what the store answered, never an aggregate this service
/// computed or a source's own appended.
#[tokio::test]
async fn aggregation_over_sources_uses_the_merged_instances() {
    // Local instances 1 and 3 and the answered 5 average to 3.
    let broker = Arc::new(StubBroker::new([(uri("urn:ngsi-ld:T:A"), Vec::new())]).aggregating(vec![aggregate_of_the_merged_instances(3)]));
    let service = service_over(Arc::clone(&broker), None);

    let rendering = service
        .render_merged_aggregations(
            &live_target(),
            &aggregated_query(),
            QueryReading {
                context: &context(),
                language_targeting: LanguageTargeting::Specification,
            },
            vec![EntityParts {
                entity_id: uri("urn:ngsi-ld:T:A"),
                local: LocalPart::Included,
                answered: vec![answered(5).await],
            }],
        )
        .await
        .expect("the merged aggregation renders");

    let handed = broker.merged_answers();
    assert_eq!(handed.len(), 1, "the answered instance reaches the store");
    assert_eq!(
        handed[0].value,
        TemporalValue::Number {
            number: JsonNumber::from(5),
            datatype: None
        }
    );
    let [TemporalHistory::Aggregates(entity)] = rendering.entities.as_slice() else {
        panic!("one aggregated entity: {rendering:?}");
    };
    let AggregatedValues::Triples(triples) = &entity.attrs["speed"][0].aggregates[0].values else {
        panic!("the standard format is not grouped");
    };
    assert_eq!(triples.len(), 1);
    assert!(
        matches!(&triples[0].value, AggregateValue::Number(number) if *number == JsonNumber::from(3)),
        "{triples:?}"
    );
    assert!(rendering.warnings.is_empty(), "{:?}", rendering.warnings);
}

/// Clause 5.7.4.4: without split entities a local part that failed the filters
/// is no part of the entity merged into S4, so the store is told to leave this
/// broker's instances out and the entity is named by the types its answered
/// parts carry, not by the local catalogue.
#[tokio::test]
async fn an_excluded_local_part_is_left_to_the_store_and_the_answered_types_name_the_entity() {
    let broker = Arc::new(StubBroker::new([(uri("urn:ngsi-ld:T:A"), Vec::new())]).aggregating(vec![aggregate_of_the_merged_instances(5)]));
    let service = service_over(Arc::clone(&broker), None);

    service
        .render_merged_aggregations(
            &live_target(),
            &aggregated_query(),
            QueryReading {
                context: &context(),
                language_targeting: LanguageTargeting::Specification,
            },
            vec![EntityParts {
                entity_id: uri("urn:ngsi-ld:T:A"),
                local: LocalPart::Excluded,
                answered: vec![answered(5).await],
            }],
        )
        .await
        .expect("the merged aggregation renders");

    let answered_type: EntityTypeName = "https://uri.etsi.org/ngsi-ld/default-context/Thing".parse().expect("a type IRI");
    assert_eq!(
        broker.merged_entities(),
        vec![(uri("urn:ngsi-ld:T:A"), LocalPart::Excluded, vec![answered_type])]
    );
}

/// An included local part keeps the types the local catalogue holds for the
/// entity, whatever its answered parts call it.
#[tokio::test]
async fn an_included_local_part_is_named_by_the_local_catalogue() {
    let broker = Arc::new(StubBroker::new([(uri("urn:ngsi-ld:T:A"), Vec::new())]).aggregating(vec![aggregate_of_the_merged_instances(5)]));
    let service = service_over(Arc::clone(&broker), None);

    service
        .render_merged_aggregations(
            &live_target(),
            &aggregated_query(),
            QueryReading {
                context: &context(),
                language_targeting: LanguageTargeting::Specification,
            },
            vec![EntityParts {
                entity_id: uri("urn:ngsi-ld:T:A"),
                local: LocalPart::Included,
                answered: vec![answered(5).await],
            }],
        )
        .await
        .expect("the merged aggregation renders");

    let catalogued_type: EntityTypeName = "https://example.com/Thing".parse().expect("a type IRI");
    assert_eq!(
        broker.merged_entities(),
        vec![(uri("urn:ngsi-ld:T:A"), LocalPart::Included, vec![catalogued_type])]
    );
}
