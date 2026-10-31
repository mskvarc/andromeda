//! One context source's part of an entity's `speed` history in a federated aggregation test, written as the source
//! answers it: every instance, or the `totalCount` and `sum` the source computes over them (clause 4.5.19).
//!
//! Holder `h` holds instance `k` with the value `100 h + k` at `2020-08-01T10:00:00Z` plus `3 k + h` seconds, so the
//! parts of at most three holders never share an instant and the merged history counts every instance of every part.

use std::fmt;

/// The instant every series starts from.
const FIRST_INSTANT: &str = "2020-08-01T10:00:00Z";

/// The parts that can hold one entity without two of them sharing an instant.
const HOLDERS: u32 = 3;

/// One holder's `speed` instances of one entity.
#[derive(Debug, Clone)]
pub struct HeldSeries {
    /// The entity the series belongs to.
    entity_id: String,
    /// The entity's type, as the source spells it.
    entity_type: String,
    /// Which holder this is, below [`HOLDERS`].
    holder: u32,
    /// How many instances the holder holds.
    count: u32,
}

impl HeldSeries {
    /// The `count` instances holder `holder` (taken modulo three) holds of the entity `entity_id` of type
    /// `entity_type`.
    #[must_use]
    pub fn new(entity_id: impl fmt::Display, entity_type: impl fmt::Display, holder: u32, count: u32) -> HeldSeries {
        HeldSeries {
            entity_id: entity_id.to_string(),
            entity_type: entity_type.to_string(),
            holder: holder % HOLDERS,
            count,
        }
    }

    /// The value of every instance, in instant order.
    fn values(&self) -> impl Iterator<Item = u32> {
        let base = self.holder * 100;
        (0..self.count).map(move |instance| base + instance)
    }

    /// The sum of every instance's value.
    #[must_use]
    pub fn sum(&self) -> u32 {
        self.values().sum()
    }

    /// The instant of instance `instance`.
    fn instant(&self, instance: u32) -> String {
        let seconds = instance * HOLDERS + self.holder;
        format!("2020-08-01T10:{:02}:{:02}Z", seconds / 60, seconds % 60)
    }

    /// The entity holding every instance of the series, as one element of a temporal query answer.
    #[must_use]
    pub fn instances(&self) -> String {
        let instances: Vec<String> = self
            .values()
            .zip(0..)
            .map(|(value, instance)| format!(r#"{{"type": "Property", "value": {value}, "observedAt": "{}"}}"#, self.instant(instance)))
            .collect();
        format!(
            r#"{{"id": "{}", "type": "{}", "speed": [{}]}}"#,
            self.entity_id,
            self.entity_type,
            instances.join(", ")
        )
    }

    /// The entity holding the `totalCount` and `sum` of the series over one whole-range period, as one element of an
    /// aggregated temporal query answer.
    #[must_use]
    pub fn aggregates(&self) -> String {
        let end = self.instant(self.count.saturating_sub(1));
        format!(
            r#"{{"id": "{}", "type": "{}", "speed": {{"type": "Property", "totalCount": [[{}, "{FIRST_INSTANT}", "{end}"]], "sum": [[{}, "{FIRST_INSTANT}", "{end}"]]}}}}"#,
            self.entity_id,
            self.entity_type,
            self.count,
            self.sum()
        )
    }
}

/// The temporal query answer holding `elements`.
#[must_use]
pub fn answer_of(elements: &[String]) -> String {
    format!("[{}]", elements.join(", "))
}

#[cfg(test)]
mod tests {
    use crate::held_series::{HeldSeries, answer_of};
    use googletest::prelude::*;

    #[gtest]
    fn three_holders_of_thirty_instances_sum_to_every_value() -> Result<()> {
        let sums: u32 = (0..3).map(|holder| HeldSeries::new("urn:ngsi-ld:Vehicle:1", "Vehicle", holder, 30).sum()).sum();
        verify_that!(sums, eq(10_305))
    }

    #[gtest]
    fn the_instances_of_two_holders_never_share_an_instant() -> Result<()> {
        let first = HeldSeries::new("urn:ngsi-ld:Vehicle:1", "Vehicle", 0, 30).instances();
        let second = HeldSeries::new("urn:ngsi-ld:Vehicle:1", "Vehicle", 1, 30).instances();
        expect_that!(first.as_str(), contains_substring(r#""observedAt": "2020-08-01T10:01:27Z""#));
        expect_that!(second.as_str(), contains_substring(r#""observedAt": "2020-08-01T10:01:28Z""#));
        verify_that!(second.as_str(), not(contains_substring(r#""observedAt": "2020-08-01T10:01:27Z""#)))
    }

    #[gtest]
    fn the_aggregates_count_and_sum_the_series() -> Result<()> {
        let written = answer_of(&[HeldSeries::new("urn:ngsi-ld:Vehicle:2", "Vehicle", 0, 7).aggregates()]);
        verify_that!(
            written.as_str(),
            eq(
                r#"[{"id": "urn:ngsi-ld:Vehicle:2", "type": "Vehicle", "speed": {"type": "Property", "totalCount": [[7, "2020-08-01T10:00:00Z", "2020-08-01T10:00:18Z"]], "sum": [[21, "2020-08-01T10:00:00Z", "2020-08-01T10:00:18Z"]]}}]"#
            )
        )
    }
}
