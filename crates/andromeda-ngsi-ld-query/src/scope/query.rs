//! The `scopeQ` query parameter value (clause 4.19).

use crate::{
    error::QueryError,
    queried_entity::scoped_entity::ScopedEntity,
    scope::{pattern::ScopePattern, pattern_fault::ScopePatternFault},
};
use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Write},
    str::FromStr,
};

/// The separator rendered between alternatives of a scope query: `,` is OR.
const ALTERNATIVE_SEPARATOR: char = ',';
/// The separator rendered between requirements of one alternative: `;` is AND.
const REQUIREMENT_SEPARATOR: char = ';';

/// One lexical token of a `scopeQ` expression.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ScopeToken {
    /// A scope path pattern such as `/A/#`.
    Pattern(String),
    /// An OR operator (`,` or `|`, clause 4.19 `orOp = %x7C / %x2C`).
    Or,
    /// An AND operator (`;`).
    And,
    /// `(`, opening a grouped sub-expression.
    Open,
    /// `)`, closing a grouped sub-expression.
    Close,
}

/// NGSI-LD scope query expression, parsed.
///
/// The clause 4.19 grammar is a disjunction (`,` or `|`) of alternatives, each
/// either one pattern or a parenthesised conjunction (`;`) of patterns:
/// `ScopesQ = OrScopeQ *(orOp OrScopeQ)`, `OrScopeQ = "(" ScopeQ *(andOp
/// ScopeQ) ")" / ScopeQ`. It is already in disjunctive normal form, so every
/// rendering walks one list of alternatives, and a malformed expression is a
/// rejected request rather than an `lquery` cast that fails inside PostgreSQL.
///
/// The wire form is the clause 4.19 expression, so a Subscription or Snapshot
/// query that stores one gives back an equivalent expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ScopeQuery {
    /// The alternatives in disjunctive normal form; at least one, each a
    /// non-empty conjunction of patterns.
    alternatives: Vec<Vec<ScopePattern>>,
}

impl ScopeQuery {
    /// Parse a raw `scopeQ` parameter value (clause 4.19).
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ScopePatternInvalid`] when the expression is
    /// unbalanced, ends on an operator, conjoins outside parentheses, nests a
    /// group or a disjunction inside one, or contains a pattern outside the
    /// clause 4.19 grammar.
    pub fn parse(raw: &str) -> Result<ScopeQuery, QueryError> {
        let tokens = tokenize_scope_query(raw);
        let mut parser = ScopeQueryParser { tokens: &tokens, position: 0 };
        let alternatives = parser.parse_or()?;
        if !parser.is_exhausted() {
            // A dangling `)` or trailing operator: the whole input is malformed.
            return Err(QueryError::ScopePatternInvalid(ScopePatternFault::TrailingInput));
        }
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
    /// Clause 4.19: an alternative holds when every one of its requirements is
    /// satisfied by at least one of the entity's scopes, and the query holds
    /// when any alternative does. This is the in-memory reading used by
    /// subscription matching and by the post-merge filtering of a distributed
    /// query; the backend renderings walk the same tree. The scopes are walked
    /// once per requirement, so their iterator has to be cheap to clone.
    pub fn matches<'a, I>(&self, scopes: I) -> bool
    where
        I: IntoIterator<Item = &'a ScopePath>,
        I::IntoIter: Clone,
    {
        let scopes = scopes.into_iter();
        self.alternatives
            .iter()
            .any(|alternative| alternative.iter().all(|pattern| scopes.clone().any(|scope| pattern.matches(scope))))
    }

    /// Whether an entity's scopes (clause 4.18) satisfy this query.
    pub fn matches_entity<'a, E: ScopedEntity<'a>>(&self, entity: E) -> bool {
        self.matches(entity.scopes())
    }
}

/// Split a `scopeQ` string into tokens.
///
/// A scope path contains only `/`, level labels, `+` and `#`, none of which is
/// an operator, so the operators `, | ; ( )` cleanly delimit the patterns
/// between them. Surrounding whitespace is trimmed from each pattern.
fn tokenize_scope_query(raw: &str) -> Vec<ScopeToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, tokens: &mut Vec<ScopeToken>| {
        let trimmed = current.trim();
        if !trimmed.is_empty() {
            tokens.push(ScopeToken::Pattern(trimmed.to_string()));
        }
        current.clear();
    };
    for character in raw.chars() {
        match character {
            ',' | '|' => {
                flush(&mut current, &mut tokens);
                tokens.push(ScopeToken::Or);
            }
            ';' => {
                flush(&mut current, &mut tokens);
                tokens.push(ScopeToken::And);
            }
            '(' => {
                flush(&mut current, &mut tokens);
                tokens.push(ScopeToken::Open);
            }
            ')' => {
                flush(&mut current, &mut tokens);
                tokens.push(ScopeToken::Close);
            }
            other => current.push(other),
        }
    }
    flush(&mut current, &mut tokens);
    tokens
}

/// A parser over a tokenised `scopeQ`, following the clause 4.19 grammar.
struct ScopeQueryParser<'a> {
    /// The tokens to consume.
    tokens: &'a [ScopeToken],
    /// The next token to read.
    position: usize,
}

impl ScopeQueryParser<'_> {
    /// The next token without consuming it.
    fn peek(&self) -> Option<&ScopeToken> {
        self.tokens.get(self.position)
    }

    /// Whether every token has been consumed.
    const fn is_exhausted(&self) -> bool {
        self.position >= self.tokens.len()
    }

    /// Parse `OrScopeQ *(orOp OrScopeQ)`.
    fn parse_or(&mut self) -> Result<Vec<Vec<ScopePattern>>, QueryError> {
        let mut alternatives = vec![self.parse_alternative()?];
        while matches!(self.peek(), Some(ScopeToken::Or)) {
            self.position += 1;
            alternatives.push(self.parse_alternative()?);
        }
        Ok(alternatives)
    }

    /// Parse one `OrScopeQ`: a parenthesised conjunction or a single pattern.
    /// A `;` after either is a conjunction outside parentheses.
    fn parse_alternative(&mut self) -> Result<Vec<ScopePattern>, QueryError> {
        let alternative = match self.peek() {
            Some(ScopeToken::Open) => {
                self.position += 1;
                self.parse_group()?
            }
            Some(ScopeToken::Pattern(_) | ScopeToken::Or | ScopeToken::And | ScopeToken::Close) | None => vec![self.parse_pattern()?],
        };
        match self.peek() {
            Some(ScopeToken::And) => Err(QueryError::ScopePatternInvalid(ScopePatternFault::UngroupedConjunction)),
            Some(ScopeToken::Pattern(_) | ScopeToken::Or | ScopeToken::Open | ScopeToken::Close) | None => Ok(alternative),
        }
    }

    /// Parse `ScopeQ *(andOp ScopeQ) ")"` after its opening parenthesis.
    fn parse_group(&mut self) -> Result<Vec<ScopePattern>, QueryError> {
        let mut conjunction = vec![self.parse_pattern()?];
        loop {
            match self.peek() {
                Some(ScopeToken::And) => {
                    self.position += 1;
                    conjunction.push(self.parse_pattern()?);
                }
                Some(ScopeToken::Close) => {
                    self.position += 1;
                    return Ok(conjunction);
                }
                Some(ScopeToken::Or | ScopeToken::Open) => return Err(QueryError::ScopePatternInvalid(ScopePatternFault::NestedGroup)),
                Some(ScopeToken::Pattern(_)) | None => return Err(QueryError::ScopePatternInvalid(ScopePatternFault::UnclosedGroup)),
            }
        }
    }

    /// Parse one `ScopeQ` pattern.
    fn parse_pattern(&mut self) -> Result<ScopePattern, QueryError> {
        match self.peek() {
            Some(ScopeToken::Pattern(pattern)) => {
                let pattern = ScopePattern::parse(pattern)?;
                self.position += 1;
                Ok(pattern)
            }
            Some(ScopeToken::Open) => Err(QueryError::ScopePatternInvalid(ScopePatternFault::NestedGroup)),
            // An operator, a `)`, or the end of input where a pattern was expected.
            Some(ScopeToken::Or | ScopeToken::And | ScopeToken::Close) | None => Err(QueryError::ScopePatternInvalid(ScopePatternFault::MissingPattern)),
        }
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;

    fn paths(raw: &[&str]) -> Vec<ScopePath> {
        raw.iter().map(|path| path.parse().expect("a scope path")).collect()
    }

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
            assert!(
                matches!(
                    ScopeQuery::parse(raw),
                    Err(QueryError::ScopePatternInvalid(ScopePatternFault::UngroupedConjunction))
                ),
                "{raw}"
            );
        }
    }

    /// Clause 4.19: a parenthesised `OrScopeQ` holds `ScopeQ *(andOp ScopeQ)`
    /// only, never a disjunction or another group.
    #[test]
    fn a_disjunction_or_a_group_inside_a_group_is_rejected() {
        for raw in ["(/A,/B)", "(/A|/B);/C", "((/A;/B))", "(/A;(/B;/C))"] {
            assert!(
                matches!(ScopeQuery::parse(raw), Err(QueryError::ScopePatternInvalid(ScopePatternFault::NestedGroup))),
                "{raw}"
            );
        }
    }

    #[test]
    fn a_malformed_pattern_is_rejected_when_the_scope_query_is_parsed() {
        let error = ScopeQuery::parse("/A,/1Floor").expect_err("must reject");
        assert!(
            matches!(error, QueryError::ScopePatternInvalid(ScopePatternFault::InvalidLevel)),
            "got {error:?}"
        );
    }

    #[test]
    fn an_empty_alternative_is_rejected() {
        assert!(matches!(
            ScopeQuery::parse("/A,"),
            Err(QueryError::ScopePatternInvalid(ScopePatternFault::MissingPattern))
        ));
        assert!(ScopeQuery::parse("(/A;;/B)").is_err());
    }

    #[test]
    fn an_unclosed_group_and_a_dangling_parenthesis_are_told_apart() {
        assert!(matches!(
            ScopeQuery::parse("(/A;/B"),
            Err(QueryError::ScopePatternInvalid(ScopePatternFault::UnclosedGroup))
        ));
        assert!(matches!(
            ScopeQuery::parse("/A)"),
            Err(QueryError::ScopePatternInvalid(ScopePatternFault::TrailingInput))
        ));
    }

    #[test]
    fn a_conjunction_needs_every_requirement_met_by_some_scope() {
        // Clause 4.19: `;` is AND across the entity's whole scope set, not
        // within one scope.
        let query = ScopeQuery::parse("(/Spain;/Madrid/Gardens)").expect("valid scope query");
        assert!(query.matches(&paths(&["/Spain", "/Madrid/Gardens"])));
        assert!(!query.matches(&paths(&["/Spain"])));
    }

    #[test]
    fn a_disjunction_needs_only_one_alternative_met() {
        let query = ScopeQuery::parse("/France,/Spain").expect("valid scope query");
        assert!(query.matches(&paths(&["/Spain"])));
        assert!(query.matches(&paths(&["/France"])));
        assert!(!query.matches(&paths(&["/Italy"])));
    }

    #[test]
    fn a_pipe_is_an_alternative_separator_like_a_comma() {
        // Clause 4.19 `orOp = %x7C / %x2C`: `|` and `,` are both OR.
        assert_eq!(shape("/A|/B"), vec![vec!["/A".to_string()], vec!["/B".to_string()]]);
    }

    /// Clause 4.19 EXAMPLE 5: `(/Madrid/Districts;/CompanyA)|/CompanyB`.
    #[test]
    fn a_grouped_conjunction_or_a_pattern_matches_either() {
        let query = ScopeQuery::parse("(/A;/B)|/C").expect("valid scope query");
        assert!(query.matches(&paths(&["/A", "/B"])));
        assert!(query.matches(&paths(&["/C"])));
        assert!(!query.matches(&paths(&["/A"])));
    }

    /// Clause 4.19 spells rooted patterns only; a clause 4.18 Scope written
    /// without its leading `/` meets them by its levels.
    #[test]
    fn a_scope_query_matches_bare_and_rooted_scopes_alike() {
        let query = ScopeQuery::parse("(/A;/B)").expect("valid scope query");
        assert!(query.matches(&paths(&["A", "/B"])));
    }

    #[test]
    fn an_unbalanced_parenthesis_is_rejected() {
        assert!(ScopeQuery::parse("(/A;/B").is_err());
        assert!(ScopeQuery::parse("/A,/B)").is_err());
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
            let rendered = ScopeQuery::parse(input).expect("valid scope query").to_string();
            pretty_assertions::assert_eq!(rendered, expected, "rendering of {input}");
            let reparsed = ScopeQuery::parse(&rendered).expect("re-parses");
            pretty_assertions::assert_eq!(reparsed, ScopeQuery::parse(input).expect("valid scope query"), "round trip of {input}");
        }
    }

    #[test]
    fn a_scope_query_round_trips_through_its_clause_4_19_text() {
        let raw = "(/A;/B),/C/#";
        let query = ScopeQuery::parse(raw).expect("valid scope query");
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

    /// An entity carrying the given scopes (clause 4.18).
    struct ScopedFixture(Vec<ScopePath>);

    impl<'a> ScopedEntity<'a> for &'a ScopedFixture {
        fn scopes(self) -> impl Iterator<Item = &'a ScopePath> + Clone {
            self.0.iter()
        }
    }

    fn scoped(paths: &[&str]) -> ScopedFixture {
        ScopedFixture(paths.iter().map(|path| path.parse().expect("a scope path")).collect())
    }

    /// Clause 4.19 read against an entity's scopes: every requirement of one
    /// alternative met by some scope, and no scope at all meeting nothing.
    #[test]
    fn an_entity_is_matched_by_the_scopes_it_carries() {
        let query = ScopeQuery::parse("(/Madrid/#;/Spain)").expect("parses");
        assert!(query.matches_entity(&scoped(&["/Spain", "/Madrid/Gardens"])));
        assert!(!query.matches_entity(&scoped(&["/Spain"])));
        assert!(!query.matches_entity(&scoped(&[])));
    }
}
