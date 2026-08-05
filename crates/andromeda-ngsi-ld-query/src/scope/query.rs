//! The `scopeQ` query parameter value (clause 4.18).

use crate::{error::QueryError, scope::pattern::ScopePattern};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Write},
    str::FromStr,
};

/// The separator between alternatives of a scope query: `,` is OR.
const ALTERNATIVE_SEPARATOR: char = ',';
/// The separator between requirements of one alternative: `;` is AND.
const REQUIREMENT_SEPARATOR: char = ';';

/// NGSI-LD scope query expression, parsed.
///
/// The grammar is a disjunction of conjunctions: comma is OR, semicolon is AND,
/// and AND binds tighter (clause 4.18). Parsing it once means every rendering
/// walks the same tree rather than re-splitting the raw text, and a malformed
/// pattern is a rejected request rather than an `lquery` cast that fails inside
/// PostgreSQL.
///
/// The wire form is the clause 4.18 expression, so a Subscription or Snapshot
/// query that stores one gives back an equivalent expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ScopeQuery {
    /// The comma-separated alternatives; at least one, each non-empty.
    alternatives: Vec<Vec<ScopePattern>>,
}

impl ScopeQuery {
    /// Parse a raw `scopeQ` parameter value.
    pub fn parse(raw: &str) -> Result<ScopeQuery, QueryError> {
        let alternatives = raw
            .split(ALTERNATIVE_SEPARATOR)
            .map(|alternative| alternative.split(REQUIREMENT_SEPARATOR).map(ScopePattern::parse).collect())
            .collect::<Result<Vec<Vec<ScopePattern>>, QueryError>>()?;
        Ok(ScopeQuery { alternatives })
    }

    /// The expression as a disjunction of conjunctions of scope patterns.
    ///
    /// The outer iterator yields the alternatives; each inner iterator yields
    /// the requirements of one alternative. Every rendering of a scope query
    /// walks this, so the two separators are read in one place rather than once
    /// per backend.
    pub fn alternatives(&self) -> impl Iterator<Item = impl Iterator<Item = &ScopePattern>> {
        self.alternatives.iter().map(|alternative| alternative.iter())
    }

    /// Whether a set of entity scopes satisfies this query.
    ///
    /// Clause 4.18: an alternative holds when every one of its requirements is
    /// satisfied by at least one of the entity's scopes, and the query holds
    /// when any alternative does. This is the in-memory reading used by
    /// subscription matching and by the post-merge filtering of a distributed
    /// query; the backend renderings walk the same tree.
    pub fn matches<S: AsRef<str>>(&self, scopes: &[S]) -> bool {
        self.alternatives
            .iter()
            .any(|alternative| alternative.iter().all(|pattern| scopes.iter().any(|scope| pattern.matches(scope.as_ref()))))
    }
}

impl FromStr for ScopeQuery {
    type Err = QueryError;

    fn from_str(raw: &str) -> Result<ScopeQuery, QueryError> {
        ScopeQuery::parse(raw)
    }
}

// Parsing lives in `ScopeQuery::parse` so borrowed call sites do not allocate;
// this adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for ScopeQuery {
    type Error = QueryError;

    fn try_from(raw: String) -> Result<ScopeQuery, QueryError> {
        ScopeQuery::parse(&raw)
    }
}

// The rendering walks two levels of variable-length list and re-inserts both
// separators, which `derive_more::Display`'s format string cannot express.
impl fmt::Display for ScopeQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, alternative) in self.alternatives.iter().enumerate() {
            if index > 0 {
                formatter.write_char(ALTERNATIVE_SEPARATOR)?;
            }
            for (position, pattern) in alternative.iter().enumerate() {
                if position > 0 {
                    formatter.write_char(REQUIREMENT_SEPARATOR)?;
                }
                write!(formatter, "{pattern}")?;
            }
        }
        Ok(())
    }
}

// The wire form is produced by the `Display` above; this exists only because
// `#[serde(into = "String")]` demands an `Into<String>` and cannot use it.
impl From<ScopeQuery> for String {
    fn from(query: ScopeQuery) -> String {
        query.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(raw: &str) -> Vec<Vec<String>> {
        ScopeQuery::parse(raw)
            .expect("valid scope query")
            .alternatives()
            .map(|alternative| alternative.map(ScopePattern::to_string).collect())
            .collect()
    }

    #[test]
    fn a_single_pattern_is_one_alternative_of_one_requirement() {
        assert_eq!(shape("/A/B"), vec![vec!["/A/B".to_string()]]);
    }

    #[test]
    fn commas_separate_alternatives() {
        assert_eq!(shape("/A,/B"), vec![vec!["/A".to_string()], vec!["/B".to_string()]]);
    }

    #[test]
    fn semicolons_separate_requirements_within_an_alternative() {
        assert_eq!(shape("/A;/B"), vec![vec!["/A".to_string(), "/B".to_string()]]);
    }

    #[test]
    fn and_binds_tighter_than_or() {
        // Clause 4.18: `/A;/B,/C` is `(/A AND /B) OR /C`.
        assert_eq!(shape("/A;/B,/C"), vec![vec!["/A".to_string(), "/B".to_string()], vec!["/C".to_string()]]);
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_from_every_pattern() {
        assert_eq!(shape(" /A ; /B "), vec![vec!["/A".to_string(), "/B".to_string()]]);
    }

    #[test]
    fn a_malformed_pattern_is_rejected_when_the_scope_query_is_parsed() {
        let error = ScopeQuery::parse("/A,/1Floor").expect_err("must reject");
        assert!(matches!(error, QueryError::ScopePatternInvalid(_)), "got {error:?}");
    }

    #[test]
    fn an_empty_alternative_is_rejected() {
        assert!(ScopeQuery::parse("/A,").is_err());
        assert!(ScopeQuery::parse("/A;;/B").is_err());
    }

    #[test]
    fn a_conjunction_needs_every_requirement_met_by_some_scope() {
        // Clause 4.18: `;` is AND across the entity's whole scope set, not
        // within one scope.
        let query = ScopeQuery::parse("/Spain;/Madrid/Gardens").expect("valid scope query");
        assert!(query.matches(&["/Spain", "/Madrid/Gardens"]));
        assert!(!query.matches(&["/Spain"]));
    }

    #[test]
    fn a_disjunction_needs_only_one_alternative_met() {
        let query = ScopeQuery::parse("/France,/Spain").expect("valid scope query");
        assert!(query.matches(&["/Spain"]));
        assert!(query.matches(&["/France"]));
        assert!(!query.matches(&["/Italy"]));
    }

    #[test]
    fn a_scope_query_round_trips_through_its_clause_4_18_text() {
        let raw = "/A;/B,/C/#";
        let json = serde_json::to_string(&ScopeQuery::parse(raw).expect("valid scope query")).expect("serialises");
        pretty_assertions::assert_eq!(json, format!("{raw:?}"));
        let decoded: ScopeQuery = serde_json::from_str(&json).expect("deserialises");
        pretty_assertions::assert_eq!(decoded, ScopeQuery::parse(raw).expect("valid scope query"));
    }

    #[test]
    fn a_malformed_scope_query_is_rejected_when_the_resource_is_deserialized() {
        // Clause 4.18 admits `#` only as the last segment.
        assert!(serde_json::from_str::<ScopeQuery>("\"/A/#/C\"").is_err());
    }
}
