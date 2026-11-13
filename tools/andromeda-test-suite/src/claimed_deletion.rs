//! The `speed` attribute a Context Producer writes claiming a deletion, a `deletedAt` on its instance and on its
//! `accuracy` sub-attribute, and the matcher on the `speed` an entity holds once the write ignored both. Table 5.2.5-2
//! makes `deletedAt` output-only, and clause 5.2.5 has implementations ignore it "In the event that they are provided
//! (in update or create operations)".

use crate::{
    attribute_matcher::{AttributeMatcher, any_attribute},
    attribute_member::Member,
    entity_matcher::{HasAttribute, has_attribute},
};
use ngsi_ld::model::{InvalidJson, RawJson};

/// The `deletedAt` every written instance claims.
const CLAIMED: &str = "2026-01-01T00:00:00Z";

/// The fragment of a `speed` instance holding `speed`, with an `accuracy` sub-attribute holding `accuracy`, each
/// instance claiming a `deletedAt`, as text: the model refuses output-only members on input. It names no `type`, as a
/// Partial Attribute Update fragment may (clause 5.6.4.4).
///
/// # Errors
///
/// Returns the reason the text is not JSON.
pub fn speed_fragment_claiming_deletion(speed: i64, accuracy: i64) -> Result<RawJson, InvalidJson> {
    format!(
        r#"{{"value": {speed}, "deletedAt": "{CLAIMED}", "accuracy": {}}}"#,
        accuracy_claiming_deletion(accuracy)
    )
    .parse()
}

/// The Property `speed` holding `speed`, with an `accuracy` sub-attribute holding `accuracy`, each instance claiming a
/// `deletedAt`, as text: the model refuses output-only members on input.
///
/// # Errors
///
/// Returns the reason the text is not JSON.
pub fn speed_claiming_deletion(speed: i64, accuracy: i64) -> Result<RawJson, InvalidJson> {
    format!(
        r#"{{"type": "Property", "value": {speed}, "deletedAt": "{CLAIMED}", "accuracy": {}}}"#,
        accuracy_claiming_deletion(accuracy)
    )
    .parse()
}

/// The Property `accuracy` holding `accuracy` and claiming a `deletedAt`, as JSON text.
fn accuracy_claiming_deletion(accuracy: i64) -> String {
    format!(r#"{{"type": "Property", "value": {accuracy}, "deletedAt": "{CLAIMED}"}}"#)
}

/// An entity whose `speed` holds `speed` with an `accuracy` sub-attribute holding `accuracy`, neither instance carrying
/// a `deletedAt`.
#[must_use]
pub fn has_speed_without_deletion(speed: i64, accuracy: i64) -> HasAttribute {
    has_attribute("speed", undeleted(speed).sub_attribute("accuracy", undeleted(accuracy)))
}

/// An instance holding `value` and no `deletedAt`.
fn undeleted(value: i64) -> AttributeMatcher {
    any_attribute().value(value).without(Member::DeletedAt)
}

#[cfg(test)]
mod tests {
    use crate::claimed_deletion::{has_speed_without_deletion, speed_claiming_deletion, speed_fragment_claiming_deletion};
    use googletest::prelude::*;
    use ngsi_ld::model::{Entity, Lossless, Output};

    /// The Vehicle holding `speed` as written.
    fn vehicle(speed: &str) -> Result<Entity<Output>> {
        Ok(Entity::<Output>::parse(
            &format!(r#"{{"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {speed}}}"#),
            Lossless::Normalized,
        )?)
    }

    #[gtest]
    fn the_written_speed_claims_a_deletion_at_both_depths() -> Result<()> {
        let written = speed_claiming_deletion(60, 2)?.to_string();
        expect_that!(written.matches(r#""deletedAt":"2026-01-01T00:00:00Z""#).count(), eq(2));
        let fragment = speed_fragment_claiming_deletion(60, 2)?.to_string();
        expect_that!(fragment.matches(r#""deletedAt":"2026-01-01T00:00:00Z""#).count(), eq(2));
        verify_that!(fragment.contains(r#""type":"Property","value":60"#), eq(false))
    }

    #[gtest]
    fn a_speed_without_deletion_matches_and_one_claiming_it_does_not() -> Result<()> {
        let kept = vehicle(r#"{"type": "Property", "value": 60, "accuracy": {"type": "Property", "value": 2}}"#)?;
        expect_that!(&kept, has_speed_without_deletion(60, 2));
        let claimed = vehicle(&speed_claiming_deletion(60, 2)?.to_string())?;
        expect_that!(&claimed, not(has_speed_without_deletion(60, 2)));
        verify_that!(&kept, not(has_speed_without_deletion(61, 2)))
    }
}
