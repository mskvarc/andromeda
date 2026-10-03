//! The order the stage-one identifiers of stored and pending entities are
//! merged in (clauses 5.5.9 and 5.7.4.5).

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{ordering::sort_direction::SortDirection, temporal::ordering::TemporalEntityOrdering};

/// Which way the merged stage-one identifiers run.
///
/// Clause 5.7.4.3 admits no ordering member but `id`, so the whole of what an
/// ordering can ask of this merge is a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MergedIdentifierOrder {
    /// Identifiers ascending: what an `id;asc` ordering asks for, and also the
    /// store's own stable order when the request asked for no ordering at all
    /// (clause 5.5.9).
    Ascending,
    /// Identifiers descending, which only an explicit `id;desc` asks for.
    Descending,
}

impl MergedIdentifierOrder {
    /// The order the identifiers of a compiled temporal query are merged in.
    pub(super) const fn of_query(ordering: Option<&TemporalEntityOrdering>) -> MergedIdentifierOrder {
        match ordering {
            Some(TemporalEntityOrdering {
                direction: SortDirection::Descending,
                ..
            }) => MergedIdentifierOrder::Descending,
            Some(TemporalEntityOrdering {
                direction: SortDirection::Ascending,
                ..
            })
            | None => MergedIdentifierOrder::Ascending,
        }
    }
}

/// Put the merged stage-one identifiers in the order the response renders in.
///
/// Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then the
/// JSON-LD array returned shall be ordered according to the member names
/// specified." The clause makes no exception for an Entity whose history is
/// still an unapplied intent, so the pending identifiers are ordered together
/// with the stored ones rather than appended after them. With no ordering
/// requested the same ascending sequence is what clause 5.5.9 needs for a
/// stable page.
///
/// The comparison is over the identifier text alone. An ordering may name an
/// ICU collation (clause 4.23.1), which refines the comparison inside the store
/// because the store holds a comparator for it; this process holds none, so a
/// collation cannot change where an identifier lands here. That refinement is
/// the one thing the overlay does not reproduce, and it is a difference between
/// two distinct URIs rather than between an ordered and an unordered result.
pub(super) fn order_merged_identifiers(identifiers: &mut [Identifier], order: MergedIdentifierOrder) {
    match order {
        MergedIdentifierOrder::Ascending => identifiers.sort_by(|left, right| left.as_str().cmp(right.as_str())),
        MergedIdentifierOrder::Descending => identifiers.sort_by(|left, right| right.as_str().cmp(left.as_str())),
    }
}

#[cfg(test)]
mod tests {
    use crate::pending_overlay::merged_identifier_order::{MergedIdentifierOrder, order_merged_identifiers};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        ordering::{collation::IcuCollation, sort_direction::SortDirection},
        temporal::ordering::TemporalEntityOrdering,
    };

    fn ids(raw: &[&str]) -> Vec<Identifier> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn spelled(identifiers: &[Identifier]) -> Vec<&str> {
        identifiers.iter().map(Identifier::as_str).collect()
    }

    /// With no clause-4.23 ordering the store returns `id` ascending under the
    /// identifier column's byte-wise collation, and the merged union has to sit
    /// in that same order for clause 5.5.9 pagination to be stable.
    #[test]
    fn an_unordered_query_merges_the_pending_identifiers_into_the_ascending_order() {
        let mut identifiers = ids(&["urn:ngsi-ld:T:c", "urn:ngsi-ld:T:a", "urn:ngsi-ld:T:b"]);
        order_merged_identifiers(&mut identifiers, MergedIdentifierOrder::of_query(None));
        assert_eq!(spelled(&identifiers), vec!["urn:ngsi-ld:T:a", "urn:ngsi-ld:T:b", "urn:ngsi-ld:T:c"]);
    }

    /// Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then
    /// the JSON-LD array returned shall be ordered according to the member
    /// names specified." An Entity whose history is still an unapplied intent
    /// is no exception, so it takes its place in the sequence rather than being
    /// appended after it.
    #[test]
    fn an_ascending_ordering_places_the_pending_identifier_rather_than_appending_it() {
        let ordering = TemporalEntityOrdering::builder()
            .direction(SortDirection::Ascending)
            .collation(Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation")))
            .build();
        let mut identifiers = ids(&["urn:ngsi-ld:T:a", "urn:ngsi-ld:T:c", "urn:ngsi-ld:T:b"]);
        order_merged_identifiers(&mut identifiers, MergedIdentifierOrder::of_query(Some(&ordering)));
        assert_eq!(spelled(&identifiers), vec!["urn:ngsi-ld:T:a", "urn:ngsi-ld:T:b", "urn:ngsi-ld:T:c"]);
    }

    /// A descending ordering runs the other way, and the pending identifier is
    /// placed by the same comparison rather than left at the end.
    #[test]
    fn a_descending_ordering_places_the_pending_identifier_by_the_same_comparison() {
        let ordering = TemporalEntityOrdering::builder().direction(SortDirection::Descending).build();
        let mut identifiers = ids(&["urn:ngsi-ld:T:c", "urn:ngsi-ld:T:a", "urn:ngsi-ld:T:b"]);
        order_merged_identifiers(&mut identifiers, MergedIdentifierOrder::of_query(Some(&ordering)));
        assert_eq!(spelled(&identifiers), vec!["urn:ngsi-ld:T:c", "urn:ngsi-ld:T:b", "urn:ngsi-ld:T:a"]);
    }

    /// Only the direction reaches the merge: clause 5.7.4.3 admits no ordering
    /// member but `id`, and the ICU collation refines a comparison the store
    /// makes rather than this one.
    #[test]
    fn the_merge_order_reads_the_direction_alone() {
        let collated = TemporalEntityOrdering::builder()
            .direction(SortDirection::Ascending)
            .collation(Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation")))
            .build();
        pretty_assertions::assert_eq!(MergedIdentifierOrder::of_query(Some(&collated)), MergedIdentifierOrder::Ascending);
        pretty_assertions::assert_eq!(MergedIdentifierOrder::of_query(None), MergedIdentifierOrder::Ascending);
    }
}
