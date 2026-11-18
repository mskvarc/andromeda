//! The expanded filter of a corpus case.

use crate::{corpus::CorpusCase, corpus_vocabulary::CorpusVocabulary};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_query::{
    filter::{
        Filter,
        expansion::{apply_expand_values, expand_attribute_names, expand_json_keys, expand_q_expr},
        parser,
    },
    query_error::QueryError,
};
use std::collections::HashSet;

/// Why the filter of a corpus case could not be expanded.
#[derive(Debug, thiserror::Error)]
pub enum CaseFilterError {
    /// The language refused the filter or one of its terms.
    #[error(transparent)]
    Filter(#[from] QueryError),
    /// An `expandValues` or `jsonKeys` term of the case is no attribute name.
    #[error(transparent)]
    Term(#[from] NgsiLdReferenceError),
}

/// The attribute names `terms`.
fn attribute_names(terms: &[&str]) -> Result<Vec<AttributeName>, CaseFilterError> {
    terms.iter().map(|term| Ok(term.parse()?)).collect()
}

/// The filter of `case`, expanded under [`CorpusVocabulary`] as a request
/// carrying the case's `jsonKeys` and `expandValues` expands it.
///
/// # Errors
///
/// Returns the language's refusal when the filter does not parse or a term
/// does not expand, and an error when a term of the case is no attribute name.
pub async fn expanded_filter(case: &CorpusCase) -> Result<Filter, CaseFilterError> {
    let json_keys = expand_json_keys(Some(&attribute_names(case.json_keys)?), &CorpusVocabulary).await?;
    let expr = expand_q_expr(parser::parse(case.q)?, &json_keys, &CorpusVocabulary).await?;
    let opted_in: HashSet<AttributeName> = expand_attribute_names(&attribute_names(case.expand_values)?, &CorpusVocabulary)
        .await?
        .into_iter()
        .collect();
    Ok(apply_expand_values(expr, &opted_in, &CorpusVocabulary).await?)
}
