use crate::{
    candidate_listing::CandidateListing,
    query_reading::QueryReading,
    requested_identifiers::RequestedIdentifiers,
    stub_broker::{StubBroker, admitted, live_target, service_over},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_rel::TimeRel};
use andromeda_ngsi_ld_query::{
    candidate_filters::CandidateFilters,
    entity_selection::EntitySelection,
    filter::{Filter, language_targeting::LanguageTargeting},
    name_stage::Written,
    page_window::PageWindow,
    pattern::Pattern,
    scope::scope_query::ScopeQuery,
    temporal::contributed_entities::{ContributedEntities, ContributedRendering},
};
use andromeda_ngsi_ld_request::query::temporal::{TemporalQueryParameters, admitted::AdmittedTemporalRequest};
use serde_json::json;
use std::{iter, sync::Arc};

fn context() -> ContextSpec {
    ContextSpec::from_body(JsonLiteral::admit(&json!({"speed": "https://example.com/speed"}).to_string()).expect("an admissible literal"))
}

/// A temporal query selecting by id pattern and filtering by value and by
/// scope, over the instants before 2024.
fn filtered_query() -> AdmittedTemporalRequest {
    admitted(TemporalQueryParameters {
        id_pattern: Some("^urn:ngsi-ld:Vehicle:".parse::<Pattern>().expect("valid pattern")),
        q: Some("speed>10".parse::<Filter<Written>>().expect("q parses")),
        scope_q: Some("/Madrid".parse::<ScopeQuery>().expect("scopeQ parses")),
        timerel: Some(TimeRel::Before),
        time_at: Some("2024-01-01T00:00:00Z".parse().expect("a valid instant")),
        ..TemporalQueryParameters::default()
    })
}

/// The one selection the stubbed store listed the identifiers under, beside
/// the contributed entities it exempted.
async fn listed_under(filters: CandidateFilters, contributed: ContributedEntities) -> (EntitySelection, ContributedEntities) {
    let broker = Arc::new(StubBroker::new(iter::empty::<(Identifier, Vec<Identifier>)>()));
    let service = service_over(Arc::clone(&broker), None);
    service
        .query_ids(
            &live_target(),
            &filtered_query(),
            &PageWindow::unbounded(),
            QueryReading {
                context: &context(),
                language_targeting: LanguageTargeting::Specification,
            },
            CandidateListing {
                identifiers: RequestedIdentifiers::Narrow,
                filters,
                contributed,
            },
        )
        .await
        .expect("the identifiers are listed");
    let [listing]: [(EntitySelection, ContributedEntities); 1] = broker.listings().try_into().expect("one listing ran");
    listing
}

/// Clause 5.14.5.4: under split entities the local candidates are the
/// Entities whose identifier, id pattern and type match, so the value filter
/// and the scope query are left to the requests that run under the map.
#[tokio::test]
async fn a_deferred_listing_leaves_the_value_and_scope_filters_out() {
    let (selection, _) = listed_under(CandidateFilters::Deferred, ContributedEntities::default()).await;

    assert!(selection.q.is_none(), "the value filter is left out");
    assert!(selection.scope_q.is_none(), "the scope query is left out");
    assert!(
        selection.selectors.iter().any(|selector| selector.id_pattern.is_some()),
        "the id pattern still selects the candidates"
    );
}

/// Clause 5.14.5.4: without split entities the local candidates meet every
/// filter of the query.
#[tokio::test]
async fn an_applied_listing_holds_the_entities_to_every_filter() {
    let (selection, _) = listed_under(CandidateFilters::Applied, ContributedEntities::default()).await;

    assert!(selection.q.is_some(), "the value filter applies");
    assert!(selection.scope_q.is_some(), "the scope query applies");
    assert!(selection.selectors.iter().any(|selector| selector.id_pattern.is_some()));
}

/// Clause 5.7.4.4: under split entities the filters of an entity a Context
/// Source contributed to are decided over its gathered parts, so a listing
/// keeps every filter for the others and exempts that entity alone.
#[tokio::test]
async fn a_listing_exempts_the_contributed_entities_alone_from_the_filters() {
    let vehicle = "urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("a valid identifier");
    let contributed = ContributedEntities::new([vehicle], ContributedRendering::Local);
    let (selection, exempted) = listed_under(CandidateFilters::Applied, contributed.clone()).await;

    assert!(
        selection.q.is_some() && selection.scope_q.is_some(),
        "every filter still holds the other entities"
    );
    pretty_assertions::assert_eq!(exempted, contributed);
}
