//! When an `idPattern` (clause 5.7.2) is read from the trigram index over
//! identifiers rather than tested along identifier order, and how far.
//!
//! A pattern that fixes the identifier's start is a range of identifier
//! order, which a walk reads directly. Otherwise the walk tests the pattern
//! on every identifier before the first match past a rare one, while the
//! trigram index yields the identifiers that match, each rechecked against
//! the pattern. The planner cannot price the pattern, so a statement walks
//! the first [`ID_PATTERN_SHORT_WALK`] identifiers, then reads at most
//! [`ID_PATTERN_CANDIDATE_BOUND`] + 1 matches, then walks every identifier.
//! The Entity query and stage 1 of the temporal query read the same index
//! shape over their own catalogue, so they share the decision and the bounds.

use andromeda_ngsi_ld_query::{identifier_axis::IdentifierAxis, pattern::Pattern, subject_start::SubjectStart};

/// How many identifiers the short walk tests before the index is read.
///
/// A visit reads one entry of a primary key and tests the pattern on the
/// identifier it holds, with no other relation, so the walk is far longer
/// than a keyed page's: a pattern one identifier in fifty matches fills a
/// page here, and walking it costs a few milliseconds.
pub const ID_PATTERN_SHORT_WALK: u32 = 5_000;

/// The most matches of the trigram index a page reads.
///
/// The index is a GIN, whose matches are gathered in full before the first
/// is returned, so a bound of more matches saves no read; past it, the
/// pattern is dense enough that the walk fills the page sooner than the
/// matches are sorted.
pub const ID_PATTERN_CANDIDATE_BOUND: u64 = 1_000;

/// The pattern of a selector's identifier `axis` when the trigram index
/// answers it: a pattern that leaves the identifier's start open. `None` for
/// a pattern that fixes the start, and for an axis that is no pattern.
#[must_use]
pub const fn trigram_pattern(axis: IdentifierAxis<'_>) -> Option<&Pattern> {
    match axis {
        IdentifierAxis::Pattern(pattern) => match pattern.subject_start() {
            SubjectStart::Open => Some(pattern),
            SubjectStart::Fixed => None,
        },
        IdentifierAxis::Ids(_) | IdentifierAxis::Any => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::id_pattern_trigrams::trigram_pattern;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{identifier_axis::IdentifierAxis, pattern::Pattern};

    fn pattern(raw: &str) -> Pattern {
        Pattern::parse(raw).expect("valid pattern")
    }

    #[test]
    fn a_pattern_leaving_the_start_open_is_read_from_the_trigrams() {
        let open = pattern(".*:0123.*");
        assert!(trigram_pattern(IdentifierAxis::Pattern(&open)).is_some());
    }

    #[test]
    fn a_pattern_fixing_the_start_is_read_along_identifier_order() {
        for raw in ["^urn:ngsi-ld:Sensor:0123.*", "^urn:ngsi-ld:Sensor:012345$"] {
            let fixed = pattern(raw);
            assert!(trigram_pattern(IdentifierAxis::Pattern(&fixed)).is_none(), "{raw}");
        }
    }

    #[test]
    fn an_axis_without_a_pattern_reads_no_trigrams() {
        let ids: [Identifier; 1] = ["urn:ngsi-ld:Sensor:1".parse().expect("valid identifier")];
        assert!(trigram_pattern(IdentifierAxis::Ids(&ids)).is_none());
        assert!(trigram_pattern(IdentifierAxis::Any).is_none());
    }
}
