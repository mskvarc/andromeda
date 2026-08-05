//! The `scopeQ` query parameter value (clause 4.18).

use crate::{error::QueryError, scope::pattern::ScopePattern};

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
#[derive(Debug, Clone, PartialEq, Eq)]
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
}
