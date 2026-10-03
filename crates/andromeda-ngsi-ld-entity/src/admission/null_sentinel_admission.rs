//! Which of the clause 5.5.4 NGSI-LD Null prohibitions a write is exempt from.
//!
//! Clause 5.2.1 states the default: "in the context of a partial update or
//! merge operation (see clauses 5.5.8 and 5.5.12), an NGSI-LD Null shall be
//! used to indicate the removal of a target member, as explained in clause
//! 4.5.0. In all other cases, implementations shall raise an error of type
//! `BadRequestData` if an NGSI-LD Null value is encountered." Clause 5.5.4
//! turns that into three prohibitions that do not share one exception list:
//!
//! - the Null as a first-level member value, excepted for fragments "used in
//!   partial update and merge operations";
//! - the Null as the right-hand side of a Property `value`, a Relationship
//!   `object` or `{"@none": ..}` of a `languageMap` (and the list families'
//!   own spelling, clauses 4.5.21.2 and 4.5.22.2), excepted for fragments
//!   "used in update and merge operations";
//! - the Null as the value of a key within the JSON object on the right-hand
//!   side of a Property `value`, excepted only for "NGSI-LD Fragments used in
//!   merge operations (see clause 5.5.12)".
//!
//! Clauses 5.5.8 and 5.5.12 add that "A `datasetId` cannot be deleted by
//! setting it to the value `urn:ngsi-ld:null`", without exception.
//!
//! The second prohibition also excepts "the representation of deleted
//! Properties, Relationships or Language Properties in [...] the temporal
//! evolution"; the third excepts nothing but a merge fragment, so a Temporal
//! write may record a deleted instance but never a Null under a key.

use andromeda_ngsi_ld_representation::family_member_admission::MemberAdmission;

/// Which prohibitions the operation writing a payload is exempt from.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NullSentinelAdmission {
    /// None of them: Create Entity (clause 5.6.1), Append Attributes (clause
    /// 5.6.3), Replace Entity (clause 5.6.18), Replace Attribute (clause
    /// 5.6.19) and the batch create, upsert and update operations (clauses
    /// 5.6.7 to 5.6.9) submit a whole entity or attribute, not a fragment.
    Refused,
    /// The first two: Update Attributes (clause 5.6.2) and Partial Update
    /// Attribute (clause 5.6.4) delete a target member with the sentinel at the
    /// first level and in the value position (clause 5.5.8).
    PartialUpdateFragment,
    /// All three: Merge Entity (clause 5.6.17) and Batch Entity Merge (clause
    /// 5.6.20) merge "up to an arbitrary depth", so a sentinel nested in a
    /// Property value object deletes the key it stands on (clause 5.5.12).
    MergeFragment,
    /// The first two: a Temporal Evolution (clauses 5.6.11, 5.6.12 and
    /// 5.6.14) records a deleted instance with the sentinel in its value
    /// member, and a deleted Scope with the sentinel as its value (clause
    /// 4.18), but no write outside a merge nests it under a key.
    TemporalEvolution,
}

impl NullSentinelAdmission {
    /// Whether a first-level member or a value-position member may be the
    /// sentinel (the first two prohibitions).
    #[must_use]
    pub const fn deletion_admission(self) -> MemberAdmission {
        match self {
            NullSentinelAdmission::Refused => MemberAdmission::Forbidden,
            NullSentinelAdmission::PartialUpdateFragment | NullSentinelAdmission::MergeFragment | NullSentinelAdmission::TemporalEvolution => {
                MemberAdmission::Admitted
            }
        }
    }

    /// Whether a key inside a Property value object may be the sentinel (the
    /// third prohibition, excepted for merge operations alone).
    #[must_use]
    pub const fn nested_deletion_admission(self) -> MemberAdmission {
        match self {
            NullSentinelAdmission::Refused | NullSentinelAdmission::PartialUpdateFragment | NullSentinelAdmission::TemporalEvolution => {
                MemberAdmission::Forbidden
            }
            NullSentinelAdmission::MergeFragment => MemberAdmission::Admitted,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::admission::null_sentinel_admission::NullSentinelAdmission;
    use andromeda_ngsi_ld_representation::family_member_admission::MemberAdmission;

    /// Clause 5.5.4: each operation is exempt from the prohibitions its own
    /// clause supports the NGSI-LD Null for, and only a merge from the third.
    #[test]
    fn every_admission_states_which_prohibitions_it_is_exempt_from() {
        let answers = [
            NullSentinelAdmission::Refused,
            NullSentinelAdmission::PartialUpdateFragment,
            NullSentinelAdmission::MergeFragment,
            NullSentinelAdmission::TemporalEvolution,
        ]
        .map(|admission| (admission, admission.deletion_admission(), admission.nested_deletion_admission()));
        pretty_assertions::assert_eq!(
            answers,
            [
                (NullSentinelAdmission::Refused, MemberAdmission::Forbidden, MemberAdmission::Forbidden),
                (
                    NullSentinelAdmission::PartialUpdateFragment,
                    MemberAdmission::Admitted,
                    MemberAdmission::Forbidden
                ),
                (NullSentinelAdmission::MergeFragment, MemberAdmission::Admitted, MemberAdmission::Admitted),
                (NullSentinelAdmission::TemporalEvolution, MemberAdmission::Admitted, MemberAdmission::Forbidden),
            ]
        );
    }
}
