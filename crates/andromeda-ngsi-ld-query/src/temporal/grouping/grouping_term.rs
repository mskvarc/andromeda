//! One term of an `aggrGroupBy` value: the key it groups on and an optional time bucket.

use crate::{
    name_stage::{NameStage, Written},
    temporal::grouping::{bucket_term::BucketTerm, group_key::GroupKey, grouping_error::GroupingError},
};
use std::str::FromStr;

/// One term of an `aggrGroupBy` value: what it keys on, and an optional bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupingTerm<S: NameStage> {
    /// The key this term groups on.
    pub key: GroupKey<S>,
    /// The time bucket applied to the key, when the key is a temporal property.
    pub bucket: Option<BucketTerm>,
}

impl FromStr for GroupingTerm<Written> {
    type Err = GroupingError;

    /// Parse one comma-delimited grouping term.
    fn from_str(text: &str) -> Result<GroupingTerm<Written>, GroupingError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(GroupingError::EmptyTerm);
        }
        let (key_text, bucket) = match text.split_once(';') {
            Some((key_text, bucket_text)) => {
                let bucket = BucketTerm::from_str(bucket_text.trim()).map_err(GroupingError::InvalidBucket)?;
                (key_text.trim(), Some(bucket))
            }
            None => (text, None),
        };
        let key = key_text.parse::<GroupKey<Written>>().map_err(GroupingError::Key)?;
        if let Some(bucket) = bucket
            && key.temporal_property().is_none()
        {
            return Err(GroupingError::BucketOnNonTemporalKey(bucket));
        }
        Ok(GroupingTerm { key, bucket })
    }
}
