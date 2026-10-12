use crate::{
    candidate_filters::CandidateFilters,
    requested_identifiers::RequestedIdentifiers,
    stub_broker::{StubBroker, live_target, service_over},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_rel::TimeRel};
use andromeda_ngsi_ld_query::{entity_selection::EntitySelection, filter::parser::parse, page_window::PageWindow, pattern::Pattern, scope::query::ScopeQuery};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use serde_json::json;
use std::{iter, sync::Arc};

fn context() -> ContextSpec {
    ContextSpec::from_body(JsonLiteral::admit(&json!({"speed": "https://example.com/speed"}).to_string()).expect("an admissible literal"))
}

/// A temporal query selecting by id pattern and filtering by value and by
/// scope, over the instants before 2024.
fn filtered_query() -> TemporalQueryParameters {
    TemporalQueryParameters {
        id_pattern: Some(Pattern::parse("^urn:ngsi-ld:Vehicle:").expect("valid pattern")),
        q: Some(parse("speed>10").expect("q parses")),
        scope_q: Some(ScopeQuery::parse("/Madrid").expect("scopeQ parses")),
        timerel: Some(TimeRel::Before),
        time_at: Some("2024-01-01T00:00:00Z".parse().expect("a valid instant")),
        ..TemporalQueryParameters::default()
    }
}

/// The one selection the stubbed store listed the identifiers under.
async fn listed_under(filters: CandidateFilters) -> EntitySelection {
    let broker = Arc::new(StubBroker::new(iter::empty::<(Identifier, Vec<Identifier>)>()));
    let service = service_over(Arc::clone(&broker), None);
    service
        .query_ids(
            &live_target(),
            &filtered_query(),
            &PageWindow::unbounded(),
            &context(),
            RequestedIdentifiers::Narrow,
            filters,
        )
        .await
        .expect("the identifiers are listed");
    let [selection]: [EntitySelection; 1] = broker.listed_selections().try_into().expect("one listing ran");
    selection
}

/// Clause 5.14.5.4: under split entities the local candidates are the
/// Entities whose identifier, id pattern and type match, so the value filter
/// and the scope query are left to the requests that run under the map.
#[tokio::test]
async fn a_deferred_listing_leaves_the_value_and_scope_filters_out() {
    let selection = listed_under(CandidateFilters::Deferred).await;

    assert!(selection.q_expr.is_none(), "the value filter is left out");
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
    let selection = listed_under(CandidateFilters::Applied).await;

    assert!(selection.q_expr.is_some(), "the value filter applies");
    assert!(selection.scope_q.is_some(), "the scope query applies");
    assert!(selection.selectors.iter().any(|selector| selector.id_pattern.is_some()));
}
