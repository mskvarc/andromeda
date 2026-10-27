//! Typed filter AST for `DELETE /snapshots?q=...`
//! (ETSI GS CIM 009 v1.9.1 clauses 5.16.7, 6.36.3.2, 4.9).
//!
//! The snapshot purge query is a restricted subset of the NGSI-LD query
//! language (clause 4.9) that filters over the fields of the `Snapshot`
//! resource itself (clause 5.2.41): `id`, `type`, `snapshotPriority`, and
//! `endpoint`.
//!
//! The opaque `snapshotQueries` / `snapshotTemporalQueries` payloads are
//! intentionally not filterable.

use crate::snapshot::{SnapshotPriority, SnapshotPriorityParseError};
use andromeda_ngsi_ld_lexicon::identifier::{Identifier, IdentifierParseError};
use andromeda_ngsi_ld_query::filter::token;
use compact_str::CompactString;
use std::{num::ParseIntError, str::FromStr};
use strum::{Display as StrumDisplay, EnumString};
use thiserror::Error;
use url::Url;

/// Filterable member of the `Snapshot` resource (clause 5.2.41).
#[derive(Debug, Clone, Copy, PartialEq, Eq, StrumDisplay, EnumString)]
pub enum SnapshotField {
    /// The snapshot's identifier.
    #[strum(serialize = "id")]
    Id,
    /// The resource type, whose value is always `Snapshot`.
    #[strum(serialize = "type")]
    Type,
    /// The snapshot's execution priority.
    #[strum(serialize = "snapshotPriority")]
    SnapshotPriority,
    /// The endpoint notified on status changes.
    #[strum(serialize = "endpoint")]
    Endpoint,
}

/// Comparison operator (NGSI-LD clause 4.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, StrumDisplay)]
pub enum SnapshotOp {
    /// Equality.
    #[strum(serialize = "==")]
    Eq,
    /// Inequality.
    #[strum(serialize = "!=")]
    Neq,
    /// Strictly less than.
    #[strum(serialize = "<")]
    Lt,
    /// Less than or equal.
    #[strum(serialize = "<=")]
    Lte,
    /// Strictly greater than.
    #[strum(serialize = ">")]
    Gt,
    /// Greater than or equal.
    #[strum(serialize = ">=")]
    Gte,
}

/// Typed value of a filter term, constrained by its field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotValue {
    /// A snapshot identifier, for the `id` field.
    Id(Identifier),
    /// A resource type, for the `type` field.
    Type(CompactString),
    /// A priority, for the `snapshotPriority` field.
    Priority(SnapshotPriority),
    /// An endpoint URL, for the `endpoint` field.
    Endpoint(Url),
}

/// Top-level conjunction between filter terms (clause 4.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, StrumDisplay)]
pub enum Conjunction {
    /// Every term must match.
    #[strum(serialize = ";")]
    And,
    /// At least one term must match.
    #[strum(serialize = "|")]
    Or,
}

/// A single `field op value` filter term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPurgeTerm {
    /// Snapshot member the term filters on.
    pub field: SnapshotField,
    /// Comparison the term applies.
    pub op: SnapshotOp,
    /// Value the member is compared against.
    pub value: SnapshotValue,
}

/// Parsed filter for `DELETE /snapshots?q=...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPurgeFilter {
    /// The individual `field op value` terms.
    pub terms: Vec<SnapshotPurgeTerm>,
    /// How the terms combine.
    pub conjunction: Conjunction,
}

/// Parsing failures for the snapshot purge `q` expression.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SnapshotQParseError {
    /// The expression held no terms.
    #[error("q expression must not be empty")]
    Empty,
    /// A term carried no recognised comparison operator.
    #[error("a term carries no comparison operator")]
    MissingOperator,
    /// A term named a member the `Snapshot` resource does not have.
    #[error("a term names a member the Snapshot resource does not have or does not filter on")]
    UnknownField,
    /// The operator is not defined for that member's type.
    #[error("operator '{op}' is not supported for field '{field}'")]
    OperatorNotSupported {
        /// Member the term filters on.
        field: SnapshotField,
        /// Operator that member rejects.
        op: SnapshotOp,
    },
    /// The value did not parse as the member's type.
    #[error("invalid value for field '{field}': {fault}")]
    InvalidValue {
        /// Member the term filters on.
        field: SnapshotField,
        /// Why the value is not one of the member's type.
        #[source]
        fault: SnapshotValueFault,
    },
    /// The expression used both `;` and `|`, which clause 4.9 does not define.
    #[error("q expression mixes ';' and '|' conjunctions; use one consistently")]
    MixedConjunctions,
}

/// Why a term's value is not a value of the member it filters on.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SnapshotValueFault {
    /// An `id` that is not an identifier.
    #[error(transparent)]
    Identifier(#[from] IdentifierParseError),
    /// An empty `type`.
    #[error("a type must not be empty")]
    EmptyType,
    /// An `endpoint` that is not an absolute URL.
    #[error(transparent)]
    Endpoint(#[from] url::ParseError),
    /// A `snapshotPriority` that is not an integer.
    #[error("a snapshotPriority is an integer")]
    NotAnInteger(#[source] ParseIntError),
    /// A `snapshotPriority` outside its range.
    #[error(transparent)]
    Priority(#[from] SnapshotPriorityParseError),
}

/// Parse a `q` expression into a typed [`SnapshotPurgeFilter`].
///
/// # Errors
///
/// Returns [`SnapshotQParseError::Empty`] when the expression is blank or holds
/// only separators, [`SnapshotQParseError::MixedConjunctions`] when it joins
/// terms with both `;` and `|`, [`SnapshotQParseError::MissingOperator`] for a
/// term carrying no recognised comparison,
/// [`SnapshotQParseError::UnknownField`] for a member the `Snapshot` resource
/// does not have, [`SnapshotQParseError::OperatorNotSupported`] for an
/// ordering comparison on a member that has no ordering, and
/// [`SnapshotQParseError::InvalidValue`] when the value does not parse as the
/// member's type.
pub fn parse_snapshot_q(input: &str) -> Result<SnapshotPurgeFilter, SnapshotQParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(SnapshotQParseError::Empty);
    }

    let (segments, conjunction) = split_top_level(trimmed)?;
    if segments.is_empty() {
        return Err(SnapshotQParseError::Empty);
    }
    let terms: Vec<SnapshotPurgeTerm> = segments.iter().map(|s| parse_term(s)).collect::<Result<_, _>>()?;
    Ok(SnapshotPurgeFilter { terms, conjunction })
}

/// Splits a purge filter into its terms and reports which conjunction joins
/// them. Mixing `;` and `|` in one filter is ambiguous and rejected.
///
/// The scan is quote-aware: a `;` or `|` inside a double-quoted value (e.g.
/// `endpoint=="https://sink/a;b"`) is a literal character, not a conjunction,
/// so both the detection and the split ignore separators inside quotes.
fn split_top_level(input: &str) -> Result<(Vec<&str>, Conjunction), SnapshotQParseError> {
    let has_semi = contains_unquoted(input, ';');
    let has_pipe = contains_unquoted(input, '|');
    let conjunction = match (has_semi, has_pipe) {
        (true, true) => return Err(SnapshotQParseError::MixedConjunctions),
        (false, true) => Conjunction::Or,
        (true | false, false) => Conjunction::And,
    };
    let sep = match conjunction {
        Conjunction::And => ';',
        Conjunction::Or => '|',
    };
    let segments: Vec<&str> = split_unquoted(input, sep).into_iter().map(str::trim).filter(|s| !s.is_empty()).collect();
    Ok((segments, conjunction))
}

/// Whether `needle` occurs outside any double-quoted span of `input`.
fn contains_unquoted(input: &str, needle: char) -> bool {
    let mut in_quotes = false;
    for c in input.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == needle && !in_quotes {
            return true;
        }
    }
    false
}

/// Splits `input` on `sep` wherever `sep` falls outside a double-quoted span,
/// leaving separators inside a quoted value as part of that value.
fn split_unquoted(input: &str, sep: char) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    for (index, c) in input.char_indices() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == sep && !in_quotes {
            segments.push(&input[start..index]);
            start = index + c.len_utf8();
        }
    }
    segments.push(&input[start..]);
    segments
}

/// Parses one `<field><op><value>` term.
fn parse_term(input: &str) -> Result<SnapshotPurgeTerm, SnapshotQParseError> {
    // Longest operators first so `<=` matches before `<`. The purge filter is a
    // restricted subset of the clause 4.9 operators.
    let ops: [(&str, SnapshotOp); 6] = [
        (token::EQUAL, SnapshotOp::Eq),
        (token::UNEQUAL, SnapshotOp::Neq),
        (token::LESS_EQ, SnapshotOp::Lte),
        (token::GREATER_EQ, SnapshotOp::Gte),
        (token::LESS, SnapshotOp::Lt),
        (token::GREATER, SnapshotOp::Gt),
    ];

    let (field_str, op, value_str) = ops
        .iter()
        .find_map(|(tok, op)| input.find(tok).map(|pos| (&input[..pos], *op, &input[pos + tok.len()..])))
        .ok_or(SnapshotQParseError::MissingOperator)?;

    let field_str = field_str.trim();
    let value_str = value_str.trim();

    let field = SnapshotField::from_str(field_str).map_err(|_unknown| SnapshotQParseError::UnknownField)?;
    let value = parse_value(field, op, value_str)?;
    Ok(SnapshotPurgeTerm { field, op, value })
}

/// Parses a term's right-hand side into the value type its field carries, and
/// rejects an operator the field does not admit.
fn parse_value(field: SnapshotField, op: SnapshotOp, raw: &str) -> Result<SnapshotValue, SnapshotQParseError> {
    require_eq_or_neq(field, op)?;
    typed_value(field, strip_quotes(raw)).map_err(|fault| SnapshotQParseError::InvalidValue { field, fault })
}

/// Reads a term's unquoted value as the type its field carries.
fn typed_value(field: SnapshotField, unquoted: &str) -> Result<SnapshotValue, SnapshotValueFault> {
    match field {
        SnapshotField::Id => Ok(SnapshotValue::Id(unquoted.parse::<Identifier>()?)),
        SnapshotField::Type if unquoted.is_empty() => Err(SnapshotValueFault::EmptyType),
        SnapshotField::Type => Ok(SnapshotValue::Type(CompactString::from(unquoted))),
        SnapshotField::Endpoint => Ok(SnapshotValue::Endpoint(Url::parse(unquoted)?)),
        SnapshotField::SnapshotPriority => {
            let n = unquoted.parse::<u8>().map_err(SnapshotValueFault::NotAnInteger)?;
            Ok(SnapshotValue::Priority(SnapshotPriority::try_from(n)?))
        }
    }
}

/// Rejects an ordering operator on a field whose values are not ordered.
const fn require_eq_or_neq(field: SnapshotField, op: SnapshotOp) -> Result<(), SnapshotQParseError> {
    match op {
        SnapshotOp::Eq | SnapshotOp::Neq => Ok(()),
        SnapshotOp::Lt | SnapshotOp::Lte | SnapshotOp::Gt | SnapshotOp::Gte => Err(SnapshotQParseError::OperatorNotSupported { field, op }),
    }
}

/// Removes one matched pair of surrounding double quotes, if present.
fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use super::*;

    /// Clause 4.9: the snapshot purge filter is a restricted subset of the
    /// NGSI-LD query language, so its operator and conjunction spellings must be
    /// exactly those of the query language. This pins the strum `Display` tokens
    /// to the shared ones.
    #[test]
    fn snapshot_operator_and_conjunction_tokens_match_the_shared_query_tokens() {
        pretty_assertions::assert_eq!(SnapshotOp::Eq.to_string(), token::EQUAL);
        pretty_assertions::assert_eq!(SnapshotOp::Neq.to_string(), token::UNEQUAL);
        pretty_assertions::assert_eq!(SnapshotOp::Lt.to_string(), token::LESS);
        pretty_assertions::assert_eq!(SnapshotOp::Lte.to_string(), token::LESS_EQ);
        pretty_assertions::assert_eq!(SnapshotOp::Gt.to_string(), token::GREATER);
        pretty_assertions::assert_eq!(SnapshotOp::Gte.to_string(), token::GREATER_EQ);
        pretty_assertions::assert_eq!(Conjunction::And.to_string(), token::AND);
        pretty_assertions::assert_eq!(Conjunction::Or.to_string(), token::OR);
    }

    /// Clause 5.16.7: a purge `q` may compare `endpoint` against a quoted URL
    /// that itself contains `;` or `|`. The top-level split must treat those as
    /// literal characters inside the quotes, not as conjunctions: a value
    /// carrying both is one term, not a mix of conjunctions.
    #[test]
    fn a_conjunction_character_inside_a_quoted_value_does_not_split_the_term() {
        let (segments, conjunction) = split_top_level(r#"endpoint=="https://sink/a;b""#).expect("a quoted semicolon parses");
        pretty_assertions::assert_eq!(segments.len(), 1);
        pretty_assertions::assert_eq!(conjunction, Conjunction::And);

        let (segments, _) = split_top_level(r#"endpoint=="https://sink/a|b""#).expect("a quoted pipe parses");
        pretty_assertions::assert_eq!(segments.len(), 1);

        // The whole parse must succeed too, not just the split.
        let filter = parse_snapshot_q(r#"endpoint=="https://sink/a;b""#).expect("a quoted semicolon parses end to end");
        pretty_assertions::assert_eq!(filter.terms.len(), 1);
    }

    #[test]
    fn parse_q_type_eq_snapshot() {
        let filter = parse_snapshot_q("type==Snapshot").expect("parse");
        pretty_assertions::assert_eq!(filter.conjunction, Conjunction::And);
        pretty_assertions::assert_eq!(filter.terms.len(), 1);
        let term = &filter.terms[0];
        pretty_assertions::assert_eq!(term.field, SnapshotField::Type);
        pretty_assertions::assert_eq!(term.op, SnapshotOp::Eq);
        pretty_assertions::assert_eq!(term.value, SnapshotValue::Type(CompactString::from("Snapshot")));
    }

    #[test]
    fn parse_q_priority_eq_numeric() {
        let filter = parse_snapshot_q("snapshotPriority==7").expect("parse");
        pretty_assertions::assert_eq!(filter.terms.len(), 1);
        pretty_assertions::assert_eq!(filter.terms[0].op, SnapshotOp::Eq);
        pretty_assertions::assert_eq!(filter.terms[0].value, SnapshotValue::Priority(SnapshotPriority::try_from(7u8).unwrap()));
    }

    #[test]
    fn parse_q_conjunction_semicolon_means_and_pipe_means_or() {
        // Clause 4.9: ';' = AND, '|' = OR
        let and_filter = parse_snapshot_q("type==Snapshot;snapshotPriority==5").expect("parse and");
        pretty_assertions::assert_eq!(and_filter.conjunction, Conjunction::And);
        pretty_assertions::assert_eq!(and_filter.terms.len(), 2);

        let or_filter = parse_snapshot_q("type==Snapshot|snapshotPriority==5").expect("parse or");
        pretty_assertions::assert_eq!(or_filter.conjunction, Conjunction::Or);
        pretty_assertions::assert_eq!(or_filter.terms.len(), 2);

        let mixed = parse_snapshot_q("type==Snapshot;snapshotPriority==5|snapshotPriority==6");
        pretty_assertions::assert_eq!(mixed, Err(SnapshotQParseError::MixedConjunctions));
    }

    #[test]
    fn parse_q_rejects_unknown_field() {
        // Clause 6.3.2 BadRequestData: unknown member
        let err = parse_snapshot_q("bogusField==success").unwrap_err();
        pretty_assertions::assert_eq!(err, SnapshotQParseError::UnknownField);
        assert!(!err.to_string().contains("bogusField"), "{err}");
    }

    #[test]
    fn parse_q_rejects_malformed() {
        // Clause 6.3.2 BadRequestData: missing operator
        let err = parse_snapshot_q("typeSnapshot").unwrap_err();
        pretty_assertions::assert_eq!(err, SnapshotQParseError::MissingOperator);

        let err = parse_snapshot_q("").unwrap_err();
        pretty_assertions::assert_eq!(err, SnapshotQParseError::Empty);

        let err = parse_snapshot_q("snapshotPriority==abc").unwrap_err();
        assert!(matches!(
            err,
            SnapshotQParseError::InvalidValue {
                field: SnapshotField::SnapshotPriority,
                ..
            }
        ));

        let err = parse_snapshot_q("snapshotPriority<5").unwrap_err();
        assert!(matches!(
            err,
            SnapshotQParseError::OperatorNotSupported {
                field: SnapshotField::SnapshotPriority,
                op: SnapshotOp::Lt
            }
        ));
    }

    #[test]
    fn parse_q_endpoint_eq_url() {
        let filter = parse_snapshot_q("endpoint==\"https://example.org/sink\"").expect("parse");
        match &filter.terms[0].value {
            SnapshotValue::Endpoint(url) => pretty_assertions::assert_eq!(url.as_str(), "https://example.org/sink"),
            other @ (SnapshotValue::Id(_) | SnapshotValue::Type(_) | SnapshotValue::Priority(_)) => {
                panic!("unexpected value: {other:?}")
            }
        }
    }

    #[test]
    fn parse_q_id_eq_snapshot_urn() {
        let filter = parse_snapshot_q("id==urn:ngsi-ld:Snapshot:abc").expect("parse");
        match &filter.terms[0].value {
            SnapshotValue::Id(id) => pretty_assertions::assert_eq!(id.as_str(), "urn:ngsi-ld:Snapshot:abc"),
            other @ (SnapshotValue::Type(_) | SnapshotValue::Priority(_) | SnapshotValue::Endpoint(_)) => {
                panic!("unexpected value: {other:?}")
            }
        }
    }
}
