//! Whether the attribute family a written instance declares is the one it is
//! stored as.
//!
//! Table 6.5.3.4-1 lets a Merge Entity fragment be written in the simplified
//! representation of clause 4.5.4, where every attribute is a bare value, and
//! keeps the type of every attribute the target already holds. Such a fragment
//! is rewritten against the stored entity before it is merged, so the family
//! the concise reading of clause 4.5.2.3 gives a bare value beforehand may
//! still change. A rule on families is held to the rewritten fragment.

use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentation;

/// Whether the family each written instance declares, or its value member
/// implies, is the family the write stores it as.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WrittenFamilies {
    /// It is: every write but a simplified merge, and a simplified merge once
    /// its fragment is rewritten against the target.
    Final,
    /// It is not yet: a simplified merge fragment before it is rewritten
    /// against the target, whose held types decide the families.
    TargetHeld,
}

// Hand-written: maps one enum onto another, which no derive expresses.
impl From<MergeRepresentation> for WrittenFamilies {
    fn from(representation: MergeRepresentation) -> WrittenFamilies {
        match representation {
            MergeRepresentation::Normalized => WrittenFamilies::Final,
            MergeRepresentation::Simplified => WrittenFamilies::TargetHeld,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::admission::written_families::WrittenFamilies;
    use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentation;

    #[test]
    fn only_a_simplified_merge_leaves_its_families_to_the_target() {
        pretty_assertions::assert_eq!(WrittenFamilies::from(MergeRepresentation::Normalized), WrittenFamilies::Final);
        pretty_assertions::assert_eq!(WrittenFamilies::from(MergeRepresentation::Simplified), WrittenFamilies::TargetHeld);
    }
}
