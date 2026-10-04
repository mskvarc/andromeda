//! Why an attribute written in the simplified temporal representation (clause
//! 4.5.9) is not one the representation admits.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use thiserror::Error;

/// An attribute object holding its family's plural member that clause 4.5.9
/// does not give a reading of.
#[derive(Debug, Error)]
pub enum PluralMemberRefusal {
    /// The object carries a member beside its plural member, its `type` and
    /// its `datasetId`: clause 4.5.9 has it "only contain" the plural member.
    #[error("{family} '{attribute}' holds its instances in '{}' and cannot also carry '{member}'", .family.temporal_values_key())]
    MemberBesidePairs {
        /// The attribute, by its compact name.
        attribute: AttributeName,
        /// The family the object declares.
        family: AttributeType,
        /// The member it cannot carry, by its compact name.
        member: AttributeName,
    },
    /// The plural member is not an array.
    #[error("{family} '{attribute}': '{}' is not an array of [value, instant] pairs", .family.temporal_values_key())]
    NotAnArray {
        /// The attribute, by its compact name.
        attribute: AttributeName,
        /// The family the object declares.
        family: AttributeType,
    },
    /// An element of the plural member is not an array of exactly two
    /// elements.
    #[error("{family} '{attribute}': an element of '{}' is not a [value, instant] pair", .family.temporal_values_key())]
    NotAPair {
        /// The attribute, by its compact name.
        attribute: AttributeName,
        /// The family the object declares.
        family: AttributeType,
    },
    /// The first element of a `languageMaps`, `jsons` or `vocabs` pair is not
    /// the one-member object clause 4.5.9 wraps those families' values in.
    #[error("{family} '{attribute}': the first element of a pair is not an object holding '{}' alone", .family.compact_value_key())]
    UnwrappedValue {
        /// The attribute, by its compact name.
        attribute: AttributeName,
        /// The family the object declares.
        family: AttributeType,
    },
    /// The second element of a pair, its `observedAt`, is not a string.
    #[error("{family} '{attribute}': the second element of a pair is not an observedAt date-time")]
    InstantNotAString {
        /// The attribute, by its compact name.
        attribute: AttributeName,
        /// The family the object declares.
        family: AttributeType,
    },
}

#[cfg(test)]
mod tests {
    use crate::plural_member_refusal::PluralMemberRefusal;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

    /// The refusal names the attribute, its family and the member clause 4.5.9
    /// holds the family's pairs in.
    #[test]
    fn a_refusal_names_the_attribute_and_the_plural_member() {
        let refusal = PluralMemberRefusal::MemberBesidePairs {
            attribute: "speed".parse().expect("an attribute name"),
            family: AttributeType::Property,
            member: "unitCode".parse().expect("a member name"),
        };
        pretty_assertions::assert_eq!(
            refusal.to_string(),
            "Property 'speed' holds its instances in 'values' and cannot also carry 'unitCode'"
        );
        let refusal = PluralMemberRefusal::UnwrappedValue {
            attribute: "says".parse().expect("an attribute name"),
            family: AttributeType::LanguageProperty,
        };
        pretty_assertions::assert_eq!(
            refusal.to_string(),
            "LanguageProperty 'says': the first element of a pair is not an object holding 'languageMap' alone"
        );
    }
}
