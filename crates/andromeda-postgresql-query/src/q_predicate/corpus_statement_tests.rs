//! Every filter of the parity corpus, rendered over current rows and over the
//! rows of a temporal window, as the statements its snapshots fix.
//!
//! The corpus names the Entities each filter selects; these snapshots fix the
//! statement each execution over stored rows sends for it, so a refactor that
//! changes a statement shows as a reviewed diff. A filter the language refuses
//! is fixed as its refusal.

use crate::{
    attribute_name_key::AttributeNameKey,
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    entity_row_refs::EntityRowRefs,
    entity_type_name_key::EntityTypeNameKey,
    history_source::{HistoryBranch, HistoryOf},
    q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, projection::compile},
    window_reach::WindowReach,
};
use andromeda_architecture_checks::assert_named_snapshot;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{
    error::QueryError,
    filter::{
        expansion::{apply_expand_values, expand_attribute_names, expand_json_keys, expand_q_expr},
        expression::Expr,
        language_matching::LanguageMatching,
        parser,
    },
    temporal::window::TimeInterval,
    term_expander::{TermExpander, TermExpansionFailure},
};
use andromeda_ngsi_ld_vocabulary::compact_to_expanded;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::assert_statement_snapshot;
use andromeda_q_corpus::{
    corpus::{CORPUS, CorpusCase},
    corpus_entity::CORPUS_TYPE,
};
use iri_rs::IriBuf;
use sea_query::{Alias, Expr as SeaExpr, Query};
use std::{
    collections::HashSet,
    future::{Future, ready},
};

/// The vocabulary every corpus term that is no core term expands under.
const CORPUS_VOCABULARY: &str = "https://example.com/";

/// Expands corpus terms with no JSON-LD processor: a term the core `@context`
/// maps to an IRI expands to it, and every other term, an entity type
/// included, to [`CORPUS_VOCABULARY`] followed by the term.
struct CorpusVocabulary;

/// `text` as an IRI.
fn vocabulary_iri(text: String) -> Result<IriBuf, QueryError> {
    IriBuf::new(text).map_err(|_| QueryError::TermExpansion(TermExpansionFailure::NotAnIri))
}

impl TermExpander for CorpusVocabulary {
    fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        let core = compact_to_expanded(term).filter(|expanded| expanded.contains("://"));
        ready(vocabulary_iri(core.map_or_else(|| format!("{CORPUS_VOCABULARY}{term}"), str::to_owned)))
    }

    fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        ready(vocabulary_iri(format!("{CORPUS_VOCABULARY}{term}")))
    }
}

/// Every attribute term a corpus Entity writes, which the dictionary of a
/// tenant holding the corpus resolves. A name no Entity writes (`missing`) is
/// absent, as it is from such a dictionary.
const WRITTEN_ATTRIBUTES: &[&str] = &[
    "n", "s", "b", "other", "r", "rl", "l", "d", "tm", "f", "g", "m", "k", "src", "addr", "h", "hp", "location", "vp", "lp", "jp", "link", "link2", "n1", "t",
];

/// The names of a tenant holding the corpus, each with a key of its own.
async fn corpus_names() -> QueryNames {
    let mut attributes = Vec::with_capacity(WRITTEN_ATTRIBUTES.len());
    for (key, term) in (1..).zip(WRITTEN_ATTRIBUTES) {
        let iri = CorpusVocabulary.expand_term(term).await.expect("a corpus term expands");
        attributes.push((
            iri.as_str().parse().expect("an expanded term is an attribute name"),
            AttributeNameKey::from(key),
        ));
    }
    let corpus_type = format!("{CORPUS_VOCABULARY}{CORPUS_TYPE}").parse().expect("the corpus type is a name");
    QueryNames {
        attributes: ResolvedNames::from_pairs(attributes),
        types: ResolvedNames::from_pairs([(corpus_type, EntityTypeNameKey::from(1))]),
        ..QueryNames::default()
    }
}

/// The attribute names `terms`.
fn attribute_names(terms: &[&str]) -> Vec<AttributeName> {
    terms.iter().map(|term| term.parse().expect("a corpus term is an attribute name")).collect()
}

/// The filter of `case`, expanded as a request carrying its `jsonKeys` and
/// `expandValues` expands it.
async fn expanded(case: &CorpusCase) -> Result<Expr, QueryError> {
    let json_keys = expand_json_keys(Some(&attribute_names(case.json_keys)), &CorpusVocabulary).await?;
    let expr = expand_q_expr(parser::parse(case.q)?, &json_keys, &CorpusVocabulary).await?;
    let opted_in: HashSet<AttributeName> = expand_attribute_names(&attribute_names(case.expand_values), &CorpusVocabulary)
        .await?
        .into_iter()
        .collect();
    apply_expand_values(expr, &opted_in, &CorpusVocabulary).await
}

/// A window with a closed start, so the history reads both the instances
/// inside it and the instance in effect when it opens.
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
        .reach(WindowReach::InsideAndInEffect)
        .build()
}

#[tokio::test]
async fn every_corpus_filter_renders_the_statements_its_snapshots_fix() {
    let names = corpus_names().await;
    let rows = EntityRowRefs::unaliased(None);
    let window = window();
    for case in CORPUS {
        let name = case.id.to_lowercase();
        match expanded(case).await {
            Err(refusal) => assert_named_snapshot!(format!("refused-{name}"), format!("{}\n{refusal}", case.q)),
            Ok(expr) => {
                let current = compile(&expr, &InstanceScan::Current(&rows), &names, LanguageMatching::TagEquality);
                assert_statement_snapshot!(
                    format!("current-{name}"),
                    Query::select().column(Entities::Id).from(Entities::Table).cond_where(current).to_owned()
                );
                let history = compile(&expr, &InstanceScan::History(&window), &names, LanguageMatching::TagEquality);
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
