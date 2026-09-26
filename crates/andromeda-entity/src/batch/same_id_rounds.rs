//! Splitting a batch entity array into the successive rounds clause 5.5.11
//! requires when one identifier appears more than once.
//!
//! Clause 5.5.11.0 fixes the semantics: instances sharing an identifier are
//! processed in the order they have in the array, the higher index later, and
//! "implementations shall treat the entity instances as if they had all arrived
//! in separate requests". Round `k` collects the `k`-th occurrence of every
//! identifier the array carries, so within one round every identifier is
//! distinct and the rounds run one after another.
//!
//! That is what makes the fold clauses 5.5.11.2 and 5.5.11.5 ask for fall out of
//! the ordinary write path rather than needing a payload-level merge: each round
//! reads the state the previous round committed and writes on top of it, so the
//! current state ends up as "the entity resulting from merging all of the entity
//! instances, in the correct order". Because every round commits separately,
//! every instance also lands its own temporal-history observation — clause
//! 5.5.11.2's "all entity instances shall be taken into account, in the correct
//! order" — and its own notification, which is what clause 5.5.11.3 means by
//! "separate notifications for each individual update".
//!
//! Separate commits give the repeated instances distinct `modifiedAt` values.
//! Clause 5.5.11.0's remark that a batch's entities share one `modifiedAt`
//! describes instances that are written together; instances of the same entity
//! are not, because the normative sentence in the same clause requires them to
//! behave as separate requests, and a shared timestamp would make the second
//! instance's history row indistinguishable from the first's.
//!
//! An array without repeats yields exactly one round holding every entry, so the
//! ordinary batch keeps the chunking and write concurrency it has always had.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use std::collections::HashMap;

/// A batch entity array arranged into the rounds clause 5.5.11.0 processes it in.
///
/// Every round holds at most one instance per identifier, and rounds are ordered
/// oldest first: an identifier's instance in round `k` precedes its instance in
/// round `k + 1` in the submitted array.
pub(super) struct SameIdRounds<T> {
    /// The rounds, oldest first; each preserves the submitted array's order.
    rounds: Vec<Vec<T>>,
}

impl<T> SameIdRounds<T> {
    /// Arrange `entries` into rounds by how often the identifier `id_of` reads
    /// off each has been seen.
    ///
    /// The first instance of every identifier goes to round 0, the second to
    /// round 1, and so on, which keeps array order both within a round and
    /// between the rounds of one identifier (clause 5.5.11.0).
    pub(super) fn from_entries(entries: Vec<T>, id_of: impl Fn(&T) -> &Uri) -> SameIdRounds<T> {
        let round_indexes = {
            let mut occurrences: HashMap<&Uri, usize> = HashMap::with_capacity(entries.len());
            entries
                .iter()
                .map(|entry| {
                    let seen = occurrences.entry(id_of(entry)).or_default();
                    let round_index = *seen;
                    *seen += 1;
                    round_index
                })
                .collect::<Vec<_>>()
        };

        let mut rounds: Vec<Vec<T>> = Vec::new();
        for (entry, round_index) in entries.into_iter().zip(round_indexes) {
            match rounds.get_mut(round_index) {
                Some(round) => round.push(entry),
                // An identifier reaches index `n` only after `n` earlier
                // instances, so a missing round is always the next one.
                None => rounds.push(vec![entry]),
            }
        }

        SameIdRounds { rounds }
    }

    /// The rounds, oldest first.
    pub(super) fn into_rounds(self) -> Vec<Vec<T>> {
        self.rounds
    }
}

#[cfg(test)]
mod tests {
    use crate::batch::same_id_rounds::SameIdRounds;
    use andromeda_ngsi_ld_lexicon::uri::Uri;

    /// Submitted instances, each tagged with its array position.
    fn instances(ids: &[&str]) -> Vec<(usize, Uri)> {
        ids.iter()
            .enumerate()
            .map(|(position, id)| (position, id.parse().expect("the fixture is a valid identifier")))
            .collect()
    }

    /// The submitted-array positions of every round, oldest first.
    fn positions(ids: &[&str]) -> Vec<Vec<usize>> {
        SameIdRounds::from_entries(instances(ids), |(_, id)| id)
            .into_rounds()
            .into_iter()
            .map(|round| round.into_iter().map(|(position, _)| position).collect())
            .collect()
    }

    #[test]
    fn an_array_without_repeats_is_one_round_holding_every_entry() {
        pretty_assertions::assert_eq!(positions(&["urn:ngsi-ld:Building:a", "urn:ngsi-ld:Building:b"]), vec![vec![0, 1]]);
    }

    #[test]
    fn repeated_identifiers_are_spread_over_successive_rounds_in_array_order() {
        pretty_assertions::assert_eq!(
            positions(&[
                "urn:ngsi-ld:Building:a",
                "urn:ngsi-ld:Building:b",
                "urn:ngsi-ld:Building:a",
                "urn:ngsi-ld:Building:a",
            ]),
            vec![vec![0, 1], vec![2], vec![3]]
        );
    }

    #[test]
    fn every_round_holds_each_identifier_at_most_once() {
        let rounds = SameIdRounds::from_entries(
            instances(&[
                "urn:ngsi-ld:Building:a",
                "urn:ngsi-ld:Building:a",
                "urn:ngsi-ld:Building:b",
                "urn:ngsi-ld:Building:b",
            ]),
            |(_, id)| id,
        )
        .into_rounds();
        pretty_assertions::assert_eq!(rounds.len(), 2);
        for round in rounds {
            let mut identifiers: Vec<&str> = round.iter().map(|(_, id)| id.as_str()).collect();
            identifiers.sort_unstable();
            let distinct = {
                identifiers.dedup();
                identifiers.len()
            };
            pretty_assertions::assert_eq!(distinct, round.len(), "a round must never repeat an identifier");
        }
    }

    #[test]
    fn every_instance_of_a_repeated_identifier_is_kept_rather_than_discarded() {
        let kept: usize = positions(&["urn:ngsi-ld:Building:a", "urn:ngsi-ld:Building:a", "urn:ngsi-ld:Building:a"])
            .iter()
            .map(Vec::len)
            .sum();
        pretty_assertions::assert_eq!(kept, 3);
    }

    #[test]
    fn an_empty_array_yields_no_rounds() {
        pretty_assertions::assert_eq!(positions(&[]), Vec::<Vec<usize>>::new());
    }
}
