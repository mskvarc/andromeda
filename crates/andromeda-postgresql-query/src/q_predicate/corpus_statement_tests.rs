//! Every filter of the parity corpus, rendered over current rows and over the
//! rows of a temporal window, as the statements its snapshots fix.
//!
//! The corpus names the Entities each filter selects; these snapshots fix the
//! statement each execution over stored rows sends for it, so a refactor that
//! changes a statement shows as a reviewed diff. A filter the language refuses
//! is fixed as its refusal.

use crate::{
    entity_row_refs::EntityRowRefs,
    history_source::{HistoryBranch, HistoryOf},
    q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, projection::compile},
    query_names::QueryNames,
};
use andromeda_architecture_checks::assert_named_snapshot;
use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{
    filter::{language_matching::LanguageMatching, reading::filter_reading::FilterReading},
    temporal::window::TimeInterval,
    term_expander::TermExpander,
};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::assert_statement_snapshot;
use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
use andromeda_q_corpus::{
    case_filter::expanded_filter,
    corpus::CORPUS,
    corpus_entity::CORPUS_TYPE,
    corpus_vocabulary::CorpusVocabulary,
    entity_attributes::written_terms,
};
use sea_query::{Alias, Expr as SeaExpr, Query};

/// The names of a tenant holding the corpus, each with a key of its own.
async fn corpus_names() -> QueryNames {
    let terms = written_terms();
    let mut attributes = Vec::with_capacity(terms.len());
    for (key, term) in (1..).zip(terms) {
        let iri = CorpusVocabulary.expand_term(term).await.expect("a corpus term expands");
        attributes.push((
            iri.as_str().parse().expect("an expanded term is an attribute name"),
            AttributeNameKey::from(key),
        ));
    }
    let corpus_type = CorpusVocabulary::vocabulary_iri(CORPUS_TYPE).parse().expect("the corpus type is a name");
    QueryNames {
        attributes: ResolvedNames::from_pairs(attributes),
        types: ResolvedNames::from_pairs([(corpus_type, EntityTypeNameKey::from(1))]),
        ..QueryNames::default()
    }
}

/// A window with a closed start.
fn window() -> HistoryWindow {
    let instant = |raw: &str| raw.parse::<NgsiLdDateTime>().expect("a valid instant").get();
    HistoryWindow::builder()
        .branch(HistoryBranch::live())
        .of(HistoryOf::Entity(SeaExpr::col((Alias::new("e"), Alias::new("id")))))
        .timeproperty(TimeProp::Observed)
        .interval(TimeInterval::Between {
            from: instant("2024-01-01T00:00:00Z"),
            to: instant("2025-01-01T00:00:00Z"),
        })
        .build()
}

#[tokio::test]
async fn every_corpus_filter_renders_the_statements_its_snapshots_fix() {
    let names = corpus_names().await;
    let rows = EntityRowRefs::unaliased(None);
    let window = window();
    for case in CORPUS {
        let name = case.id.to_lowercase();
        match expanded_filter(case).await {
            Err(refusal) => assert_named_snapshot!(format!("refused-{name}"), format!("{}\n{refusal}", case.q)),
            Ok(expr) => {
                let reading = FilterReading::of(&expr, LanguageMatching::TagEquality);
                let current = compile(&reading, &InstanceScan::Current(&rows), &names);
                assert_statement_snapshot!(
                    format!("current-{name}"),
                    Query::select().column(Entities::Id).from(Entities::Table).cond_where(current).to_owned()
                );
                let history = compile(&reading, &InstanceScan::History(&window), &names);
                assert_statement_snapshot!(
                    format!("history-{name}"),
                    Query::select()
                        .column((Alias::new("e"), Alias::new("id")))
                        .from_as(Entities::Table, Alias::new("e"))
                        .cond_where(history)
                        .to_owned()
                );
            }
        }
    }
}
