//! Every spelling of one `q` filter renders one condition, since a statement
//! renders the filter's normal form: the prepared-statement cache holds one
//! statement per filter, whatever order or grouping a client writes it in.

use crate::{
    entity_row_refs::EntityRowRefs,
    q_predicate::{instance_scan::InstanceScan, projection::compile},
    query_names::QueryNames,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::{Filter, language_matching::LanguageMatching, reading::filter_reading::FilterReading};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
use sea_query::{PostgresQueryBuilder, Query};
use std::collections::BTreeSet;

/// Each filter, as every spelling of it a client may write. The names are
/// the expanded `https://example.com/` terms `a`, `b` and `c`.
const SPELLINGS: [&[&str]; 8] = [
    &["A==1;B==2", "B==2;A==1", "(A==1);B==2", "B==2;(A==1)"],
    &["A==1|A==2", "A==2|A==1", "A==1,2", "A==2,1", "(A==1)|(A==2)"],
    &["A>1;B<5;C", "A>1;C;B<5", "B<5;A>1;C", "B<5;C;A>1", "C;A>1;B<5", "C;B<5;A>1"],
    &["(A==1|B==2);C", "C;(B==2|A==1)", "(B==2|A==1);C", "C;(A==1|B==2)"],
    &["A~=\"x.*\";B", "B;A~=\"x.*\""],
    &["A==1;(B==2;C==3)", "(A==1;B==2);C==3", "A==1;B==2;C==3", "C==3;B==2;A==1"],
    &["A==\"x\"|A==\"y\"|B", "B|A==\"y\"|A==\"x\"", "A==\"x\",\"y\"|B", "B|A==\"y\",\"x\""],
    &["A==1;A==1", "A==1", "(A==1)"],
];

/// The filter `spelling` writes, its names expanded.
fn expanded(spelling: &str) -> String {
    ["A", "B", "C"].into_iter().fold(spelling.to_owned(), |text, name| {
        text.replace(name, &format!("\"https://example.com/{}\"", name.to_lowercase()))
    })
}

/// The statement the normal form of `spelling` renders.
fn rendered(spelling: &str) -> String {
    let names = QueryNames {
        attributes: ResolvedNames::from_pairs([("a", 1), ("b", 2), ("c", 3)].map(|(name, key)| {
            (
                format!("https://example.com/{name}").parse::<AttributeName>().expect("a valid attribute name"),
                AttributeNameKey::from(key),
            )
        })),
        ..QueryNames::default()
    };
    let reading = FilterReading::of(
        &expanded(spelling).parse::<Filter>().expect("the spelling parses"),
        LanguageMatching::TagEquality,
    );
    let rows = EntityRowRefs::unaliased(None);
    let condition = compile(&reading, &InstanceScan::Current(&rows), &names);
    Query::select()
        .column(Entities::Id)
        .from(Entities::Table)
        .cond_where(condition)
        .to_string(PostgresQueryBuilder)
}

#[test]
fn every_spelling_of_one_filter_renders_one_statement() {
    for spellings in SPELLINGS {
        let statements: BTreeSet<String> = spellings.iter().map(|spelling| rendered(spelling)).collect();
        assert_eq!(statements.len(), 1, "{spellings:?}");
    }
}

#[test]
fn distinct_filters_render_distinct_statements() {
    let statements: BTreeSet<String> = SPELLINGS.iter().map(|spellings| rendered(spellings[0])).collect();
    assert_eq!(statements.len(), SPELLINGS.len());
}
