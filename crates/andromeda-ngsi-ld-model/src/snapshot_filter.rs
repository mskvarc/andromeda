//! Typed filter AST for `DELETE /snapshots?q=...`
//! (ETSI GS CIM 009 v1.9.1 clauses 5.16.7, 6.36.3.2, 4.9).
//!
//! The snapshot purge query is a restricted subset of the NGSI-LD query
//! language (clause 4.9) that filters over the fields of the `Snapshot`
//! resource itself (clause 5.2.41): `id`, `snapshotPriority`, `status`,
//! `endpoint`, `createdAt`, `modifiedAt`, `expiresAt`, `lastUsedAt`.
//!
//! The opaque `snapshotQueries` / `snapshotTemporalQueries` payloads are
//! intentionally not filterable.

use crate::snapshot::{SnapshotPriority, SnapshotPriorityParseError, SnapshotStatus};
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::{SnapshotId, SnapshotIdParseError};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use std::str::FromStr;
use strum::{Display as StrumDisplay, EnumString};
use thiserror::Error;
use url::Url;

/// Filterable member of the `Snapshot` resource (clause 5.2.41).
#[derive(Debug, Clone, Copy, PartialEq, Eq, StrumDisplay, EnumString)]
pub enum SnapshotField {
    /// The snapshot's identifier.
    #[strum(serialize = "id")]
    Id,
    /// The snapshot's execution priority.
    #[strum(serialize = "snapshotPriority")]
    SnapshotPriority,
    /// The snapshot's status.
    #[strum(serialize = "status")]
    Status,
    /// The endpoint notified on status changes.
    #[strum(serialize = "endpoint")]
    Endpoint,
    /// When the snapshot was requested.
    #[strum(serialize = "createdAt")]
    CreatedAt,
    /// When the snapshot was last changed.
    #[strum(serialize = "modifiedAt")]
    ModifiedAt,
    /// When the snapshot stops being usable.
    #[strum(serialize = "expiresAt")]
    ExpiresAt,
    /// When the snapshot was last read.
    #[strum(serialize = "lastUsedAt")]
    LastUsedAt,
}

impl SnapshotField {
    /// SQL column backing this field in the `snapshots` table.
    pub const fn sql_column(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::SnapshotPriority => "snapshot_priority",
            Self::Status => "status",
            Self::Endpoint => "endpoint",
            Self::CreatedAt => "created_at",
            Self::ModifiedAt => "modified_at",
            Self::ExpiresAt => "expires_at",
            Self::LastUsedAt => "last_used_at",
        }
    }
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

impl SnapshotOp {
    /// SQL operator text for parameterised queries.
    pub const fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Neq => "<>",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Gt => ">",
            Self::Gte => ">=",
        }
    }
}

/// Typed value of a filter term, constrained by its field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotValue {
    /// A snapshot identifier, for the `id` field.
    Id(SnapshotId),
    /// A priority, for the `snapshotPriority` field.
    Priority(SnapshotPriority),
    /// A status, for the `status` field.
    Status(SnapshotStatus),
    /// An endpoint URL, for the `endpoint` field.
    Endpoint(Url),
    /// A timestamp, for any of the four time fields.
    DateTime(DateTime<Utc>),
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
    #[error("term missing comparison operator: {0}")]
    MissingOperator(CompactString),
    /// A term named a member the `Snapshot` resource does not have.
    #[error("unknown snapshot field: {0}")]
    UnknownField(CompactString),
    /// The operator is not defined for that member's type.
    #[error("operator '{op}' is not supported for field '{field}'")]
    OperatorNotSupported {
        /// Member the term filters on.
        field: SnapshotField,
        /// Operator that member rejects.
        op: SnapshotOp,
    },
    /// The value did not parse as the member's type.
    #[error("invalid value for field '{field}': {detail}")]
    InvalidValue {
        /// Member the term filters on.
        field: SnapshotField,
        /// Underlying parse failure.
        detail: CompactString,
    },
    /// The expression used both `;` and `|`, which clause 4.9 does not define.
    #[error("q expression mixes ';' and '|' conjunctions; use one consistently")]
    MixedConjunctions,
}

// `#[from]` would need its own variant and would drop the field the parse was
// attempted for; both conversions instead fold into `InvalidValue`, which
// names that field so the client is told which term it got wrong.
impl From<SnapshotIdParseError> for SnapshotQParseError {
    fn from(err: SnapshotIdParseError) -> SnapshotQParseError {
        SnapshotQParseError::InvalidValue {
            field: SnapshotField::Id,
            detail: CompactString::from(err.to_string()),
        }
    }
}

// See the note on the `SnapshotId` conversion above.
impl From<SnapshotPriorityParseError> for SnapshotQParseError {
    fn from(err: SnapshotPriorityParseError) -> SnapshotQParseError {
        SnapshotQParseError::InvalidValue {
            field: SnapshotField::SnapshotPriority,
            detail: CompactString::from(err.to_string()),
        }
    }
}

/// Parse a `q` expression into a typed [`SnapshotPurgeFilter`].
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
fn split_top_level(input: &str) -> Result<(Vec<&str>, Conjunction), SnapshotQParseError> {
    let has_semi = input.contains(';');
    let has_pipe = input.contains('|');
    let conjunction = match (has_semi, has_pipe) {
        (true, true) => return Err(SnapshotQParseError::MixedConjunctions),
        (false, true) => Conjunction::Or,
        (true, false) | (false, false) => Conjunction::And,
    };
    let sep = match conjunction {
        Conjunction::And => ';',
        Conjunction::Or => '|',
    };
    let segments: Vec<&str> = input.split(sep).map(str::trim).filter(|s| !s.is_empty()).collect();
    Ok((segments, conjunction))
}

/// Parses one `<field><op><value>` term.
fn parse_term(input: &str) -> Result<SnapshotPurgeTerm, SnapshotQParseError> {
    // Longest operators first so `<=` matches before `<`.
    let ops: [(&str, SnapshotOp); 6] = [
        ("==", SnapshotOp::Eq),
        ("!=", SnapshotOp::Neq),
        ("<=", SnapshotOp::Lte),
        (">=", SnapshotOp::Gte),
        ("<", SnapshotOp::Lt),
        (">", SnapshotOp::Gt),
    ];

    let (field_str, op, value_str) = ops
        .iter()
        .find_map(|(tok, op)| input.find(tok).map(|pos| (&input[..pos], *op, &input[pos + tok.len()..])))
        .ok_or_else(|| SnapshotQParseError::MissingOperator(CompactString::from(input)))?;

    let field_str = field_str.trim();
    let value_str = value_str.trim();

    let field = SnapshotField::from_str(field_str).map_err(|_| SnapshotQParseError::UnknownField(CompactString::from(field_str)))?;
    let value = parse_value(field, op, value_str)?;
    Ok(SnapshotPurgeTerm { field, op, value })
}

/// Parses a term's right-hand side into the value type its field carries, and
/// rejects an operator the field does not admit.
fn parse_value(field: SnapshotField, op: SnapshotOp, raw: &str) -> Result<SnapshotValue, SnapshotQParseError> {
    let unquoted = strip_quotes(raw);
    match field {
        SnapshotField::Id => {
            require_eq_or_neq(field, op)?;
            let id: SnapshotId = unquoted.parse()?;
            Ok(SnapshotValue::Id(id))
        }
        SnapshotField::Status => {
            require_eq_or_neq(field, op)?;
            let status = SnapshotStatus::from_str(unquoted).map_err(|e| SnapshotQParseError::InvalidValue {
                field,
                detail: CompactString::from(e.to_string()),
            })?;
            Ok(SnapshotValue::Status(status))
        }
        SnapshotField::Endpoint => {
            require_eq_or_neq(field, op)?;
            let url = Url::parse(unquoted).map_err(|e| SnapshotQParseError::InvalidValue {
                field,
                detail: CompactString::from(e.to_string()),
            })?;
            Ok(SnapshotValue::Endpoint(url))
        }
        SnapshotField::SnapshotPriority => {
            let n: u8 = unquoted.parse::<u8>().map_err(|e| SnapshotQParseError::InvalidValue {
                field,
                detail: CompactString::from(e.to_string()),
            })?;
            let priority = SnapshotPriority::try_from(n)?;
            Ok(SnapshotValue::Priority(priority))
        }
        SnapshotField::CreatedAt | SnapshotField::ModifiedAt | SnapshotField::ExpiresAt | SnapshotField::LastUsedAt => {
            let dt = DateTime::parse_from_rfc3339(unquoted)
                .map(|d| d.with_timezone(&Utc))
                .map_err(|e| SnapshotQParseError::InvalidValue {
                    field,
                    detail: CompactString::from(e.to_string()),
                })?;
            Ok(SnapshotValue::DateTime(dt))
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
mod tests {
    use super::*;

    #[test]
    fn parse_q_status_eq_success() {
        // Clause 5.2.41 (snapshotStatus), 4.9 (equality)
        let filter = parse_snapshot_q("status==success").expect("parse");
        pretty_assertions::assert_eq!(filter.conjunction, Conjunction::And);
        pretty_assertions::assert_eq!(filter.terms.len(), 1);
        let term = &filter.terms[0];
        pretty_assertions::assert_eq!(term.field, SnapshotField::Status);
        pretty_assertions::assert_eq!(term.op, SnapshotOp::Eq);
        pretty_assertions::assert_eq!(term.value, SnapshotValue::Status(SnapshotStatus::Success));
    }

    #[test]
    fn parse_q_priority_gte_numeric() {
        // Clause 5.2.41 (snapshotPriority), 4.9 (numeric comparison)
        let filter = parse_snapshot_q("snapshotPriority>=7").expect("parse");
        pretty_assertions::assert_eq!(filter.terms.len(), 1);
        pretty_assertions::assert_eq!(filter.terms[0].op, SnapshotOp::Gte);
        pretty_assertions::assert_eq!(filter.terms[0].value, SnapshotValue::Priority(SnapshotPriority::try_from(7u8).unwrap()));
    }

    #[test]
    fn parse_q_expires_at_lt_datetime() {
        // Clause 5.2.41 (expiresAt), 4.9 (dateTime comparison)
        let filter = parse_snapshot_q("expiresAt<2026-01-01T00:00:00Z").expect("parse");
        pretty_assertions::assert_eq!(filter.terms[0].field, SnapshotField::ExpiresAt);
        pretty_assertions::assert_eq!(filter.terms[0].op, SnapshotOp::Lt);
        let dt = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap().with_timezone(&Utc);
        pretty_assertions::assert_eq!(filter.terms[0].value, SnapshotValue::DateTime(dt));
    }

    #[test]
    fn parse_q_conjunction_semicolon_means_and_pipe_means_or() {
        // Clause 4.9: ';' = AND, '|' = OR
        let and_filter = parse_snapshot_q("status==success;snapshotPriority>=5").expect("parse and");
        pretty_assertions::assert_eq!(and_filter.conjunction, Conjunction::And);
        pretty_assertions::assert_eq!(and_filter.terms.len(), 2);

        let or_filter = parse_snapshot_q("status==success|status==partial").expect("parse or");
        pretty_assertions::assert_eq!(or_filter.conjunction, Conjunction::Or);
        pretty_assertions::assert_eq!(or_filter.terms.len(), 2);

        let mixed = parse_snapshot_q("status==success;status==partial|status==empty");
        pretty_assertions::assert_eq!(mixed, Err(SnapshotQParseError::MixedConjunctions));
    }

    #[test]
    fn parse_q_rejects_unknown_field() {
        // Clause 6.3.2 BadRequestData: unknown member
        let err = parse_snapshot_q("bogusField==success").unwrap_err();
        assert!(matches!(err, SnapshotQParseError::UnknownField(ref s) if s == "bogusField"));
    }

    #[test]
    fn parse_q_rejects_malformed() {
        // Clause 6.3.2 BadRequestData: missing operator
        let err = parse_snapshot_q("statussuccess").unwrap_err();
        assert!(matches!(err, SnapshotQParseError::MissingOperator(_)));

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

        let err = parse_snapshot_q("status<success").unwrap_err();
        assert!(matches!(
            err,
            SnapshotQParseError::OperatorNotSupported {
                field: SnapshotField::Status,
                op: SnapshotOp::Lt
            }
        ));
    }

    #[test]
    fn parse_q_endpoint_eq_url() {
        let filter = parse_snapshot_q("endpoint==\"https://example.org/sink\"").expect("parse");
        match &filter.terms[0].value {
            SnapshotValue::Endpoint(url) => pretty_assertions::assert_eq!(url.as_str(), "https://example.org/sink"),
            other => panic!("unexpected value: {:?}", other),
        }
    }

    #[test]
    fn parse_q_id_eq_snapshot_urn() {
        let filter = parse_snapshot_q("id==urn:ngsi-ld:Snapshot:abc").expect("parse");
        match &filter.terms[0].value {
            SnapshotValue::Id(id) => pretty_assertions::assert_eq!(id.as_str(), "urn:ngsi-ld:Snapshot:abc"),
            other => panic!("unexpected value: {:?}", other),
        }
    }

    #[test]
    fn sql_column_mapping_matches_schema() {
        pretty_assertions::assert_eq!(SnapshotField::Id.sql_column(), "id");
        pretty_assertions::assert_eq!(SnapshotField::SnapshotPriority.sql_column(), "snapshot_priority");
        pretty_assertions::assert_eq!(SnapshotField::Status.sql_column(), "status");
        pretty_assertions::assert_eq!(SnapshotField::ExpiresAt.sql_column(), "expires_at");
        pretty_assertions::assert_eq!(SnapshotField::LastUsedAt.sql_column(), "last_used_at");
    }
}
