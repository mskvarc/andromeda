//! The `scopeQ` query parameter value (clause 4.19).

use crate::scope::{parser, scope_pattern::ScopePattern, scope_query_error::ScopeQueryError};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Write},
    str::FromStr,
};

/// The separator rendered between alternatives of a scope query: `,` is OR.
const ALTERNATIVE_SEPARATOR: char = ',';
/// The separator rendered between requirements of one alternative: `;` is AND.
const REQUIREMENT_SEPARATOR: char = ';';

/// NGSI-LD scope query expression, parsed.
///
/// The clause 4.19 grammar is a disjunction (`,` or `|`) of alternatives, each
/// either one pattern or a parenthesised conjunction (`;`) of patterns:
/// `ScopesQ = OrScopeQ *(orOp OrScopeQ)`, `OrScopeQ = "(" ScopeQ *(andOp
/// ScopeQ) ")" / ScopeQ`, every pattern optionally ending in `/#`. The bare
/// `/#` is an expression of its own, `ScopesQ =/ "/" "#"`, held as the one
/// alternative of the one pattern `#`. It is already in disjunctive normal
/// form, so every rendering walks one list of alternatives, and a malformed
/// expression is a rejected request rather than an `lquery` cast that fails
/// inside PostgreSQL.
///
/// The wire form is the clause 4.19 expression, so a Subscription or Snapshot
/// query that stores one gives back an equivalent expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ScopeQuery {
    /// The alternatives in disjunctive normal form; at least one, each a
    /// non-empty conjunction of patterns. A pattern of the one segment `#`
    /// stands only as the one pattern of the one alternative.
    alternatives: Vec<Vec<ScopePattern>>,
}

impl ScopeQuery {
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

impl FromStr for ScopeQuery {
    type Err = ScopeQueryError;

    /// Parse a raw `scopeQ` parameter value (clause 4.19).
    ///
    /// # Errors
    ///
    /// Returns [`ScopeQueryError`] when the expression is
    /// unbalanced, ends on an operator, conjoins outside parentheses, nests a
    /// group or a disjunction inside one, holds a bare `/#` beside another
    /// alternative or inside a group, or contains a pattern outside the clause
    /// 4.19 grammar.
    fn from_str(raw: &str) -> Result<ScopeQuery, ScopeQueryError> {
        parser::parse(raw).map(|alternatives| ScopeQuery { alternatives })
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate;
// this adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for ScopeQuery {
    type Error = ScopeQueryError;

    fn try_from(raw: String) -> Result<ScopeQuery, ScopeQueryError> {
        raw.parse()
    }
}

// The rendering walks two levels of variable-length list and re-inserts both
// separators, which `derive_more::Display`'s format string cannot express.
//
// Clause 4.19's grammar is `OrScopeQ = "(" ScopeQ *(andOp ScopeQ) ")" / ScopeQ`:
// a conjunction is legal only parenthesised, so an alternative of two or more
// requirements is wrapped, while a single-requirement alternative and the
// top-level `,` between alternatives stand unparenthesised.
impl fmt::Display for ScopeQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, alternative) in self.alternatives.iter().enumerate() {
            if index > 0 {
                formatter.write_char(ALTERNATIVE_SEPARATOR)?;
            }
            let conjunction = alternative.len() > 1;
            if conjunction {
                formatter.write_char('(')?;
            }
            for (position, pattern) in alternative.iter().enumerate() {
                if position > 0 {
                    formatter.write_char(REQUIREMENT_SEPARATOR)?;
                }
                write!(formatter, "{pattern}")?;
            }
            if conjunction {
                formatter.write_char(')')?;
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
    use crate::scope::scope_query_error::ScopeQueryError;

    fn shape(raw: &str) -> Vec<Vec<String>> {
        raw.parse::<ScopeQuery>()
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
    fn semicolons_separate_requirements_within_a_group() {
        assert_eq!(shape("(/A;/B)"), vec![vec!["/A".to_string(), "/B".to_string()]]);
    }

    #[test]
    fn a_group_and_a_pattern_are_alternatives() {
        assert_eq!(shape("(/A;/B),/C"), vec![vec!["/A".to_string(), "/B".to_string()], vec!["/C".to_string()]]);
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_from_every_pattern() {
        assert_eq!(shape(" ( /A ; /B ) "), vec![vec!["/A".to_string(), "/B".to_string()]]);
    }

    /// Clause 4.19: "For logical and grouping parenthesis are needed", and
    /// `OrScopeQ` admits `;` only inside `( )`.
    #[test]
    fn a_conjunction_outside_parentheses_is_rejected() {
        for raw in ["/A;/B", "/A;/B,/C", "/C,/A;/B", "/A;/B|/C", "(/A;/B);/C", "/A/#;/B"] {
            assert!(matches!(raw.parse::<ScopeQuery>(), Err(ScopeQueryError::UngroupedConjunction)), "{raw}");
        }
    }

    /// Clause 4.19: a parenthesised `OrScopeQ` holds `ScopeQ *(andOp ScopeQ)`
    /// only, never a disjunction or another group.
    #[test]
    fn a_disjunction_or_a_group_inside_a_group_is_rejected() {
        for raw in ["(/A,/B)", "(/A|/B);/C", "((/A;/B))", "(/A;(/B;/C))"] {
            assert!(matches!(raw.parse::<ScopeQuery>(), Err(ScopeQueryError::NestedGroup)), "{raw}");
        }
    }

    #[test]
    fn a_malformed_pattern_is_rejected_when_the_scope_query_is_parsed() {
        let error = "/A,/1Floor".parse::<ScopeQuery>().expect_err("must reject");
        assert!(matches!(error, ScopeQueryError::InvalidLevel), "got {error:?}");
    }

    #[test]
    fn an_empty_alternative_is_rejected() {
        assert!(matches!("/A,".parse::<ScopeQuery>(), Err(ScopeQueryError::MissingPattern)));
        assert!("(/A;;/B)".parse::<ScopeQuery>().is_err());
    }

    #[test]
    fn an_unclosed_group_and_a_dangling_parenthesis_are_told_apart() {
        assert!(matches!("(/A;/B".parse::<ScopeQuery>(), Err(ScopeQueryError::UnclosedGroup)));
        assert!(matches!("/A)".parse::<ScopeQuery>(), Err(ScopeQueryError::TrailingInput)));
    }

    #[test]
    fn a_pipe_is_an_alternative_separator_like_a_comma() {
        // Clause 4.19 `orOp = %x7C / %x2C`: `|` and `,` are both OR.
        assert_eq!(shape("/A|/B"), vec![vec!["/A".to_string()], vec!["/B".to_string()]]);
    }

    #[test]
    fn an_unbalanced_parenthesis_is_rejected() {
        assert!("(/A;/B".parse::<ScopeQuery>().is_err());
        assert!("/A,/B)".parse::<ScopeQuery>().is_err());
    }

    #[test]
    fn a_conjunction_always_renders_parenthesised_per_clause_4_19() {
        // `OrScopeQ = "(" ScopeQ *(andOp ScopeQ) ")" / ScopeQ`: a conjunction is
        // legal only parenthesised and a single requirement stands alone.
        let cases = [
            ("/A", "/A"),
            ("/A,/B", "/A,/B"),
            ("(/A)", "/A"),
            ("(/A;/B)", "(/A;/B)"),
            ("(/A;/B)|/C", "(/A;/B),/C"),
        ];
        for (input, expected) in cases {
            let rendered = input.parse::<ScopeQuery>().expect("valid scope query").to_string();
            pretty_assertions::assert_eq!(rendered, expected, "rendering of {input}");
            let reparsed = rendered.parse::<ScopeQuery>().expect("re-parses");
            pretty_assertions::assert_eq!(reparsed, input.parse::<ScopeQuery>().expect("valid scope query"), "round trip of {input}");
        }
    }

    #[test]
    fn a_scope_query_round_trips_through_its_clause_4_19_text() {
        let raw = "(/A;/B),/C/#";
        let query = raw.parse::<ScopeQuery>().expect("valid scope query");
        let json = serde_json::to_string(&query).expect("serialises");
        pretty_assertions::assert_eq!(json, "\"(/A;/B),/C/#\"");
        let decoded: ScopeQuery = serde_json::from_str(&json).expect("deserialises");
        pretty_assertions::assert_eq!(decoded, query);
    }

    #[test]
    fn a_malformed_scope_query_is_rejected_when_the_resource_is_deserialized() {
        // Clause 4.19 admits `#` only as the last segment.
        assert!(serde_json::from_str::<ScopeQuery>("\"/A/#/C\"").is_err());
    }

    /// Clause 4.19 `ScopesQ =/ "/" "#"`: a stored or posted expression holding
    /// the bare `/#` beside another Scope is refused like a request is.
    #[test]
    fn a_bare_any_scope_beside_another_scope_is_rejected_when_the_resource_is_deserialized() {
        for json in ["\"/#,/A\"", "\"(/#;/A)\"", "\"(/#;/A),/B\""] {
            assert!(serde_json::from_str::<ScopeQuery>(json).is_err(), "{json}");
        }
        let query = serde_json::from_str::<ScopeQuery>("\"/#\"").expect("deserialises");
        pretty_assertions::assert_eq!(query.to_string(), "/#");
    }

    #[test]
    fn a_group_of_patterns_ending_in_the_multi_level_wildcard_round_trips() {
        for raw in ["(/Madrid/#;/CompanyA)", "(/Madrid/#;/CompanyA),/CompanyB", "(/A/+/#;/B/#)"] {
            pretty_assertions::assert_eq!(raw.parse::<ScopeQuery>().expect("valid scope query").to_string(), raw);
        }
    }
}
