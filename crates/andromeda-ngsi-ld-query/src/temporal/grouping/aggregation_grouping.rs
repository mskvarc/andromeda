//! A whole `aggrGroupBy` value: its grouping terms, outermost first.

use crate::temporal::grouping::{grouping_error::GroupingError, grouping_term::GroupingTerm};
use std::str::FromStr;

/// A parsed `aggrGroupBy` value: one or more nested grouping terms, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregationGrouping(pub Vec<GroupingTerm>);

impl AggregationGrouping {
    /// Parse a whole `aggrGroupBy` value.
    ///
    /// Splits on `,` into terms (outermost first), then each term on `;` into
    /// a key and an optional bucket.
    ///
    /// # Errors
    ///
    /// Returns [`GroupingError`] when the value is empty, has an empty
    /// term, names an unknown bucket, buckets a non-temporal key, or carries a
    /// malformed key path.
    pub fn parse(raw: &str) -> Result<AggregationGrouping, GroupingError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(GroupingError::Empty);
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
    type Err = GroupingError;

    fn from_str(raw: &str) -> Result<AggregationGrouping, GroupingError> {
        AggregationGrouping::parse(raw)
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::grouping::{
        aggregation_grouping::AggregationGrouping,
        bucket_term::BucketTerm,
        entity_member::EntityMember,
        group_key::GroupKey,
        grouping_error::GroupingError,
    };

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
        assert!(matches!(&grouping.terms()[0].key, GroupKey::Path(path) if path.attribute.as_str() == "zone"));
        assert!(matches!(&grouping.terms()[1].key, GroupKey::Path(path) if path.attribute.as_str() == "vehicleType"));
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
        assert!(matches!(error, GroupingError::InvalidBucket(_)), "got {error:?}");
    }

    #[test]
    fn a_bucket_on_a_non_temporal_key_is_rejected() {
        let error = AggregationGrouping::parse("speed;month").expect_err("speed is not temporal");
        assert!(matches!(error, GroupingError::BucketOnNonTemporalKey(_)), "got {error:?}");
        let error = AggregationGrouping::parse("datasetId;month").expect_err("datasetId is not temporal");
        assert!(matches!(error, GroupingError::BucketOnNonTemporalKey(_)), "got {error:?}");
    }

    #[test]
    fn empty_and_trailing_comma_values_are_rejected() {
        assert!(matches!(AggregationGrouping::parse(""), Err(GroupingError::Empty)));
        assert!(matches!(AggregationGrouping::parse("   "), Err(GroupingError::Empty)));
        assert!(matches!(AggregationGrouping::parse("zone,"), Err(GroupingError::EmptyTerm)));
        assert!(matches!(AggregationGrouping::parse(",zone"), Err(GroupingError::EmptyTerm)));
        assert!(matches!(AggregationGrouping::parse("zone,,type"), Err(GroupingError::EmptyTerm)));
    }

    #[test]
    fn a_cyclic_and_a_calendar_bucket_are_distinguished() {
        assert_eq!(parse("observedAt;hourOfDay").terms()[0].bucket, Some(BucketTerm::HourOfDay));
        assert_eq!(parse("observedAt;day").terms()[0].bucket, Some(BucketTerm::Day));
    }
}
