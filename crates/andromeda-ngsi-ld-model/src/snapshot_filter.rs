//! The filter of Purge Snapshots, `DELETE /snapshots?q=...` (ETSI GS CIM 009
//! v1.9.1 clauses 5.16.7 and 6.36.3.2).
//!
//! Clause 5.16.7.3 selects Snapshots by "an NGSI-LD Query ... based on members
//! of the Snapshot data type (see Table 5.2.41-1), as per clause 4.9". The
//! expression is therefore a clause 4.9 one in full: the query language's own
//! parser reads it, with its operators, value forms, `;` binding tighter than
//! `|` and parenthesised associations, and the query language's own engine
//! decides it against each Snapshot. What is narrowed is the paths it names:
//! each Query Term names one member of Table 5.2.41-1 by its plain name.
//! Anything else, an output-only member of Table 5.2.41-2 such as `expiresAt`
//! included, makes the query "not valid as per clause 4.9, restricted to
//! members of the Snapshot data type" (clause 5.16.7.4).
//!
//! `snapshotQueries`, `snapshotTemporalQueries` and `receiverInfo` are members
//! of Table 5.2.41-1 as well, but hold arrays of structured values the purge
//! filter does not compare, so naming them is refused too.

use crate::snapshot::{SnapshotPriority, SnapshotType};
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::{
    error::QueryError,
    filter::{
        eval::evaluate_members,
        expression::Expr,
        path::QueryPath,
        target_value::{MultiValueReading, ResolvedTarget, TargetOrigin, TargetValue},
    },
};
use std::{borrow::Cow, str::FromStr};
use strum::{Display as StrumDisplay, EnumString};
use thiserror::Error;
use typed_builder::TypedBuilder;
use url::Url;

/// A member of the Snapshot data type (Table 5.2.41-1) a purge filter compares.
///
/// Each member's target value is of the data type Table 5.2.41-1 gives it:
/// `snapshotPriority` is a Number, every other one a String.
#[derive(Debug, Clone, Copy, PartialEq, Eq, StrumDisplay, EnumString)]
pub enum SnapshotMember {
    /// The Snapshot's URI.
    #[strum(serialize = "id")]
    Id,
    /// The resource type, which is `Snapshot` for every Snapshot.
    #[strum(serialize = "type")]
    Type,
    /// The lifetime the Snapshot was given, as the ISO 8601 duration it reads
    /// back with.
    #[strum(serialize = "snapshotLifetime")]
    SnapshotLifetime,
    /// The priority, from 1 to 10.
    #[strum(serialize = "snapshotPriority")]
    SnapshotPriority,
    /// The endpoint notified of status changes.
    #[strum(serialize = "endpoint")]
    Endpoint,
}

/// What a purge filter reads of one Snapshot.
#[derive(Debug, Clone, PartialEq, TypedBuilder)]
pub struct PurgeCandidate {
    /// The Snapshot's URI.
    pub id: Identifier,
    /// The lifetime the Snapshot was given, `None` when it holds none.
    #[builder(default = None)]
    pub snapshot_lifetime: Option<Iso8601Duration>,
    /// The Snapshot's priority.
    pub snapshot_priority: SnapshotPriority,
    /// The endpoint notified of status changes, `None` when it has none.
    #[builder(default = None)]
    pub endpoint: Option<Url>,
}

impl PurgeCandidate {
    /// The clause 4.9 target value `member` holds, `None` when the Snapshot
    /// holds no such member.
    ///
    /// The lifetime and the endpoint compare in the spelling the Snapshot reads
    /// back with, so a filter matches the text a client retrieves.
    fn target(&self, member: SnapshotMember) -> Option<TargetValue<'_>> {
        match member {
            SnapshotMember::Id => Some(TargetValue::Text(Cow::Borrowed(self.id.as_str()))),
            SnapshotMember::Type => Some(TargetValue::Text(Cow::Owned(SnapshotType.to_string()))),
            SnapshotMember::SnapshotLifetime => self.snapshot_lifetime.map(|lifetime| TargetValue::Text(Cow::Owned(lifetime.to_string()))),
            SnapshotMember::SnapshotPriority => Some(TargetValue::Number(Cow::Owned(JsonNumber::from(self.snapshot_priority.as_u8())))),
            SnapshotMember::Endpoint => self.endpoint.as_ref().map(|endpoint| TargetValue::Text(Cow::Borrowed(endpoint.as_str()))),
        }
    }
}

/// Why a `q` expression is not a purge filter.
#[derive(Debug, Error)]
pub enum SnapshotQParseError {
    /// The expression is not one clause 4.9's grammar derives.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// A Query Term names something other than a member the purge filter
    /// compares (clause 5.16.7.4).
    #[error("a Query Term names a member the Snapshot data type does not have or the purge filter does not compare")]
    UnknownMember,
}

/// A parsed `q` of Purge Snapshots whose every Query Term names a
/// [`SnapshotMember`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPurgeFilter(Expr);

impl SnapshotPurgeFilter {
    /// Whether the filter selects `candidate` (clause 5.16.7.4).
    #[must_use]
    pub fn selects(&self, candidate: &PurgeCandidate) -> bool {
        evaluate_members(&self.0, &|path| {
            // Parsing admitted only paths naming a member, so every path here
            // names one.
            let member = filtered_member(path).ok()?;
            Some(ResolvedTarget {
                value: candidate.target(member)?,
                origin: TargetOrigin::AttributeValue,
                reading: MultiValueReading::WholeArray,
            })
        })
    }
}

impl FromStr for SnapshotPurgeFilter {
    type Err = SnapshotQParseError;

    fn from_str(raw: &str) -> Result<SnapshotPurgeFilter, SnapshotQParseError> {
        let expr = raw.parse::<Expr>()?;
        every_path_names_a_member(&expr)?;
        Ok(SnapshotPurgeFilter(expr))
    }
}

/// Refuse an expression one of whose Query Terms names no [`SnapshotMember`].
fn every_path_names_a_member(expr: &Expr) -> Result<(), SnapshotQParseError> {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => terms.iter().try_for_each(every_path_names_a_member),
        Expr::Pred(predicate) => filtered_member(predicate.path()).map(|_member| ()),
    }
}

/// The member a Query Term's path names.
///
/// A member is named by its plain name: a dotted path, a trailing `[...]`
/// group or a `{...}` hop addresses something inside or beyond a member, which
/// the Snapshot data type does not have.
fn filtered_member(path: &QueryPath) -> Result<SnapshotMember, SnapshotQParseError> {
    match path {
        QueryPath::Local(local) if local.is_simple() => local.attr.as_str().parse().map_err(|_unknown| SnapshotQParseError::UnknownMember),
        QueryPath::Local(_) | QueryPath::Linked { .. } => Err(SnapshotQParseError::UnknownMember),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        snapshot::SnapshotPriority,
        snapshot_filter::{PurgeCandidate, SnapshotPurgeFilter, SnapshotQParseError},
    };
    use andromeda_ngsi_ld_query::error::QueryError;
    use pretty_assertions::assert_eq;

    /// A Snapshot holding every member the purge filter compares.
    fn full(id: &str, priority: u8, lifetime: &str) -> PurgeCandidate {
        PurgeCandidate::builder()
            .id(id.parse().expect("a Snapshot URI"))
            .snapshot_lifetime(Some(lifetime.parse().expect("an ISO 8601 duration")))
            .snapshot_priority(SnapshotPriority::try_from(priority).expect("a priority"))
            .endpoint(Some("https://sink.example.org/purge".parse().expect("an endpoint")))
            .build()
    }

    /// A Snapshot holding no optional member, at the default priority.
    fn bare(id: &str) -> PurgeCandidate {
        PurgeCandidate::builder()
            .id(id.parse().expect("a Snapshot URI"))
            .snapshot_priority(SnapshotPriority::default())
            .build()
    }

    fn filter(q: &str) -> SnapshotPurgeFilter {
        q.parse().expect("a purge filter")
    }

    /// The URIs of the candidates `q` selects, in their order.
    fn selected<'c>(q: &str, candidates: &'c [PurgeCandidate]) -> Vec<&'c str> {
        let filter = filter(q);
        candidates
            .iter()
            .filter(|candidate| filter.selects(candidate))
            .map(|candidate| candidate.id.as_str())
            .collect()
    }

    fn priorities() -> Vec<PurgeCandidate> {
        (1..=10)
            .map(|priority| full(&format!("urn:ngsi-ld:Snapshot:p{priority}"), priority, "PT1H"))
            .collect()
    }

    /// Clause 4.9 `Operator = equal / unequal / greaterEq / greater / lessEq /
    /// less`, and `snapshotPriority` is a Number (Table 5.2.41-1), so every
    /// operator compares it numerically.
    #[test]
    fn every_operator_compares_the_priority_as_a_number() {
        let candidates = priorities();
        for (q, expected) in [
            ("snapshotPriority>8", vec!["p9", "p10"]),
            ("snapshotPriority>=9", vec!["p9", "p10"]),
            ("snapshotPriority<2", vec!["p1"]),
            ("snapshotPriority<=2", vec!["p1", "p2"]),
            ("snapshotPriority==5", vec!["p5"]),
            ("snapshotPriority>7.5", vec!["p8", "p9", "p10"]),
            ("snapshotPriority==2..4", vec!["p2", "p3", "p4"]),
            ("snapshotPriority==1,10", vec!["p1", "p10"]),
        ] {
            let expected: Vec<String> = expected.iter().map(|suffix| format!("urn:ngsi-ld:Snapshot:{suffix}")).collect();
            assert_eq!(selected(q, &candidates), expected, "{q}");
        }
        assert_eq!(selected("snapshotPriority!=5", &candidates).len(), 9);
    }

    /// Clause 4.9 makes AND take precedence over OR, and an association of
    /// Query Terms in parentheses take precedence over both.
    #[test]
    fn a_conjunction_binds_tighter_than_a_disjunction_and_parentheses_tighter_still() {
        let candidates = priorities();
        assert_eq!(
            selected("id==urn:ngsi-ld:Snapshot:p2;snapshotPriority==2|id==urn:ngsi-ld:Snapshot:p3", &candidates),
            vec!["urn:ngsi-ld:Snapshot:p2", "urn:ngsi-ld:Snapshot:p3"]
        );
        assert_eq!(
            selected("snapshotPriority==9|snapshotPriority==10;id==urn:ngsi-ld:Snapshot:p9", &candidates),
            vec!["urn:ngsi-ld:Snapshot:p9"]
        );
        assert_eq!(
            selected("(snapshotPriority==9|snapshotPriority==10);id==urn:ngsi-ld:Snapshot:p9", &candidates),
            vec!["urn:ngsi-ld:Snapshot:p9"]
        );
        assert_eq!(
            selected("(snapshotPriority<2|snapshotPriority>9);type==\"Snapshot\"", &candidates),
            vec!["urn:ngsi-ld:Snapshot:p1", "urn:ngsi-ld:Snapshot:p10"]
        );
    }

    /// Clause 4.9: a value of another data type than the target's is unequal
    /// to it and matches no ordering, so it selects nothing under `==` and
    /// every Snapshot holding the member under `!=`. A priority outside 1 to 10
    /// is a well-formed Number no Snapshot holds.
    #[test]
    fn a_value_no_snapshot_can_hold_is_compared_rather_than_refused() {
        let candidates = priorities();
        for q in [
            "snapshotPriority==11",
            "snapshotPriority==0",
            "snapshotPriority==\"high\"",
            "snapshotPriority==true",
            "id==5",
            "endpoint==2024-01-01T00:00:00Z",
            "snapshotLifetime==\"one hour\"",
        ] {
            assert_eq!(selected(q, &candidates), Vec::<&str>::new(), "{q}");
        }
        assert_eq!(selected("snapshotPriority!=\"high\"", &candidates).len(), 10);
        assert_eq!(selected("id!=5", &candidates).len(), 10);
    }

    /// Table 5.2.41-1 gives `snapshotLifetime` the data type String, so it
    /// compares as the text the Snapshot reads back with.
    #[test]
    fn the_lifetime_compares_as_the_duration_text_it_reads_back_with() {
        let candidates = [full("urn:ngsi-ld:Snapshot:a", 5, "PT1H"), full("urn:ngsi-ld:Snapshot:b", 5, "PT2H")];
        assert_eq!(selected("snapshotLifetime==\"PT2H\"", &candidates), vec!["urn:ngsi-ld:Snapshot:b"]);
        assert_eq!(selected("snapshotLifetime!=\"PT2H\"", &candidates), vec!["urn:ngsi-ld:Snapshot:a"]);
        assert_eq!(selected("snapshotLifetime~=\"^PT1\"", &candidates), vec!["urn:ngsi-ld:Snapshot:a"]);
    }

    /// Clause 4.9 orders strings, so the String members take the ordering
    /// operators against a quoted operand.
    #[test]
    fn a_string_member_takes_the_ordering_operators() {
        let candidates = [full("urn:ngsi-ld:Snapshot:a", 5, "PT1H"), full("urn:ngsi-ld:Snapshot:b", 5, "PT1H")];
        assert_eq!(selected("id<\"urn:ngsi-ld:Snapshot:b\"", &candidates), vec!["urn:ngsi-ld:Snapshot:a"]);
        assert_eq!(selected("id>=\"urn:ngsi-ld:Snapshot:b\"", &candidates), vec!["urn:ngsi-ld:Snapshot:b"]);
        assert_eq!(selected("type>=\"Snapshot\"", &candidates).len(), 2);
        assert_eq!(selected("type>\"Snapshot\"", &candidates).len(), 0);
        assert_eq!(selected("endpoint~=\"^https://sink\\.example\\.org/\"", &candidates).len(), 2);
    }

    /// Clause 4.9 requires every operator to find the target element, so a
    /// Snapshot without an optional member matches neither `==` nor `!=` on
    /// it, and existence alone tells the two apart.
    #[test]
    fn a_snapshot_without_an_optional_member_matches_no_comparison_on_it() {
        let candidates = [full("urn:ngsi-ld:Snapshot:full", 5, "PT1H"), bare("urn:ngsi-ld:Snapshot:bare")];
        for q in [
            "endpoint!=\"https://other.example.org/\"",
            "snapshotLifetime!=\"PT9H\"",
            "endpoint!~=\"^x\"",
            "endpoint",
            "snapshotLifetime",
        ] {
            assert_eq!(selected(q, &candidates), vec!["urn:ngsi-ld:Snapshot:full"], "{q}");
        }
        assert_eq!(
            selected("snapshotPriority==5", &candidates).len(),
            2,
            "every Snapshot has a priority, 5 unless given"
        );
        assert_eq!(selected("id;type;snapshotPriority", &candidates).len(), 2);
    }

    /// Clause 5.16.7.4: a Query Term naming anything but a compared member of
    /// Table 5.2.41-1 makes the query invalid.
    #[test]
    fn a_term_on_anything_but_a_compared_member_is_refused() {
        for q in [
            "snapshotStatus==\"success\"",
            "expiresAt==2026-01-01T00:00:00Z",
            "createdAt==2026-01-01T00:00:00Z",
            "lastUsedAt==2026-01-01T00:00:00Z",
            "receiverInfo",
            "snapshotQueries",
            "endpoint.host==\"sink\"",
            "endpoint[host]==\"sink\"",
            "endpoint{host}==\"sink\"",
            "snapshotPriority==5;unknown==\"value\"",
        ] {
            let refused = q.parse::<SnapshotPurgeFilter>();
            assert!(matches!(refused, Err(SnapshotQParseError::UnknownMember)), "{q}: {refused:?}");
        }
    }

    /// Clause 4.9's grammar admits a `URI` and a bare word only after `==` and
    /// `!=`, and a bare word only when it is a URI.
    #[test]
    fn an_expression_outside_the_grammar_is_refused() {
        for q in [
            "",
            "typeSnapshot==",
            "snapshotPriority==abc",
            "id<urn:ngsi-ld:Snapshot:z",
            "type>=Snapshot",
            "endpoint!=not-a-url",
            "snapshotLifetime>PT1H",
            "snapshotQueries==[]",
            "(snapshotPriority==1",
        ] {
            let refused = q.parse::<SnapshotPurgeFilter>();
            assert!(matches!(refused, Err(SnapshotQParseError::Query(_))), "{q}: {refused:?}");
        }
    }

    /// The refusal names no member the client wrote.
    #[test]
    fn an_unknown_member_refusal_does_not_echo_the_term() {
        let error = "bogusField==\"success\"".parse::<SnapshotPurgeFilter>().expect_err("an unknown member");
        assert!(!error.to_string().contains("bogusField"), "{error}");
    }

    /// The grammar refusal is the query language's own.
    #[test]
    fn a_grammar_refusal_carries_the_query_error() {
        let error = "snapshotPriority>".parse::<SnapshotPurgeFilter>().expect_err("no operand");
        assert!(matches!(error, SnapshotQParseError::Query(QueryError::QExpected { .. })), "{error:?}");
    }
}
