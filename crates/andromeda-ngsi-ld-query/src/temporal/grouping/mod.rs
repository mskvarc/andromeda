//! The `aggrGroupBy` grouping grammar (temporal aggregation grouping extension).
//!
//! A fourth small language beside `orderBy` (clause 4.23.3), `pick`/`omit`
//! (clause 4.21) and `q` (clause 4.9). It partitions a temporal aggregation into
//! groups that clause 4.5.19's fixed `(entity, attribute, datasetId, period)`
//! key cannot express: calendar and cyclic time buckets, per-entity-member keys,
//! and value-in-effect keys.
//!
//! The grammar is
//!
//! ```text
//! AggregationGrouping = GroupingTerm *("," GroupingTerm)
//! GroupingTerm        = GroupKey *1(";" BucketTerm)
//! GroupKey            = ValuePath / EntityMember
//! ```
//!
//! where `,` reads as "then" — each further term nests one level deeper inside
//! the previous term's groups — and `;` attaches a per-key bucket modifier. A
//! bucket is only accepted on a temporal key (a clause 4.8 temporal property).

pub mod bucket_term;
pub mod entity_member;
pub mod group_key;

use crate::{
    error::QueryError,
    temporal::grouping::{bucket_term::BucketTerm, group_key::GroupKey},
};
use std::str::FromStr;
use thiserror::Error;

/// A malformed `aggrGroupBy` value.
#[derive(Debug, Error)]
pub enum GroupingParseError {
    /// The whole parameter was empty or blank.
    #[error("aggrGroupBy must name at least one grouping term")]
    Empty,
    /// A term between commas was empty (a leading, trailing or doubled comma).
    #[error("aggrGroupBy has an empty grouping term")]
    EmptyTerm,
    /// A grouping key was not a well-formed clause 4.9 `ValuePath`.
    #[error("invalid aggrGroupBy key: {0}")]
    Key(#[source] QueryError),
    /// A bucket modifier used an unknown token.
    #[error("invalid aggrGroupBy bucket: {0}")]
    Bucket(#[source] strum::ParseError),
    /// A bucket modifier was attached to a key that is not a temporal property.
    ///
    /// A bucket truncates or folds a timestamp, so it is meaningful only on one
    /// of the clause 4.8 temporal properties; on any other key it names no
    /// value the bucket could apply to.
    #[error("aggrGroupBy bucket '{0}' is on a key that is not a temporal property: a bucket applies only to a temporal property")]
    BucketOnNonTemporalKey(BucketTerm),
}

/// One term of an `aggrGroupBy` value: what it keys on, and an optional bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupingTerm {
    /// The key this term groups on.
    pub key: GroupKey,
    /// The time bucket applied to the key, when the key is a temporal property.
    pub bucket: Option<BucketTerm>,
}

impl GroupingTerm {
    /// Parse one comma-delimited grouping term.
    fn parse(text: &str) -> Result<GroupingTerm, GroupingParseError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(GroupingParseError::EmptyTerm);
        }
        let (key_text, bucket) = match text.split_once(';') {
            Some((key_text, bucket_text)) => {
                let bucket = BucketTerm::from_str(bucket_text.trim()).map_err(GroupingParseError::Bucket)?;
                (key_text.trim(), Some(bucket))
            }
            None => (text, None),
        };
        let key = GroupKey::parse(key_text).map_err(GroupingParseError::Key)?;
        if let Some(bucket) = bucket
            && key.temporal_property().is_none()
        {
            return Err(GroupingParseError::BucketOnNonTemporalKey(bucket));
        }
        Ok(GroupingTerm { key, bucket })
    }
}

/// A parsed `aggrGroupBy` value: one or more nested grouping terms, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregationGrouping(pub Vec<GroupingTerm>);

impl AggregationGrouping {
    /// Parse a whole `aggrGroupBy` value.
    ///
    /// Splits on `,` into terms — outermost first — then each term on `;` into
    /// a key and an optional bucket.
    ///
    /// # Errors
    ///
    /// Returns [`GroupingParseError`] when the value is empty, has an empty
    /// term, names an unknown bucket, buckets a non-temporal key, or carries a
    /// malformed key path.
    pub fn parse(raw: &str) -> Result<AggregationGrouping, GroupingParseError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(GroupingParseError::Empty);
        }
        let terms = raw.split(',').map(GroupingTerm::parse).collect::<Result<Vec<_>, _>>()?;
        Ok(AggregationGrouping(terms))
    }

    /// The grouping terms, outermost first.
    #[must_use]
    pub fn terms(&self) -> &[GroupingTerm] {
        &self.0
    }
}

impl FromStr for AggregationGrouping {
    type Err = GroupingParseError;

    fn from_str(raw: &str) -> Result<AggregationGrouping, GroupingParseError> {
        AggregationGrouping::parse(raw)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal::grouping::{AggregationGrouping, GroupingParseError, bucket_term::BucketTerm, entity_member::EntityMember, group_key::GroupKey};

    fn parse(raw: &str) -> AggregationGrouping {
        AggregationGrouping::parse(raw).expect("a well-formed aggrGroupBy value")
    }

    #[test]
    fn a_bare_member_parses_to_one_member_term() {
        let grouping = parse("datasetId");
        assert_eq!(grouping.terms().len(), 1);
        assert_eq!(grouping.terms()[0].key, GroupKey::Member(EntityMember::DatasetId));
        assert_eq!(grouping.terms()[0].bucket, None);
    }

    #[test]
    fn a_temporal_key_carries_its_bucket() {
        let grouping = parse("observedAt;month");
        assert_eq!(grouping.terms()[0].bucket, Some(BucketTerm::Month));
        assert!(matches!(grouping.terms()[0].key, GroupKey::Path(_)));
    }

    #[test]
    fn a_comma_nests_terms_in_written_order() {
        let grouping = parse("zone,vehicleType");
        assert_eq!(grouping.terms().len(), 2);
        assert!(matches!(&grouping.terms()[0].key, GroupKey::Path(path) if path.attr.as_str() == "zone"));
        assert!(matches!(&grouping.terms()[1].key, GroupKey::Path(path) if path.attr.as_str() == "vehicleType"));
    }

    #[test]
    fn clause_4_9_value_paths_parse_as_value_keys() {
        for text in ["address[city]", "refDevice.category"] {
            let grouping = parse(text);
            assert!(matches!(grouping.terms()[0].key, GroupKey::Path(_)), "{text}");
        }
    }

    #[test]
    fn an_unknown_bucket_token_is_rejected() {
        let error = AggregationGrouping::parse("speed;bogus").expect_err("bogus is no bucket");
        assert!(matches!(error, GroupingParseError::Bucket(_)), "got {error:?}");
    }

    #[test]
    fn a_bucket_on_a_non_temporal_key_is_rejected() {
        let error = AggregationGrouping::parse("speed;month").expect_err("speed is not temporal");
        assert!(matches!(error, GroupingParseError::BucketOnNonTemporalKey(_)), "got {error:?}");
        let error = AggregationGrouping::parse("datasetId;month").expect_err("datasetId is not temporal");
        assert!(matches!(error, GroupingParseError::BucketOnNonTemporalKey(_)), "got {error:?}");
    }

    #[test]
    fn empty_and_trailing_comma_values_are_rejected() {
        assert!(matches!(AggregationGrouping::parse(""), Err(GroupingParseError::Empty)));
        assert!(matches!(AggregationGrouping::parse("   "), Err(GroupingParseError::Empty)));
        assert!(matches!(AggregationGrouping::parse("zone,"), Err(GroupingParseError::EmptyTerm)));
        assert!(matches!(AggregationGrouping::parse(",zone"), Err(GroupingParseError::EmptyTerm)));
        assert!(matches!(AggregationGrouping::parse("zone,,type"), Err(GroupingParseError::EmptyTerm)));
    }

    #[test]
    fn a_cyclic_and_a_calendar_bucket_are_distinguished() {
        assert_eq!(parse("observedAt;hourOfDay").terms()[0].bucket, Some(BucketTerm::HourOfDay));
        assert_eq!(parse("observedAt;day").terms()[0].bucket, Some(BucketTerm::Day));
    }
}
