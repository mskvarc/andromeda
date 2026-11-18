//! How the `datasetId`s a request names meet the ones a registration lists
//! (ETSI GS CIM 009 clauses 4.3.6.1 and 5.12, Table 5.2.9-1).
//!
//! The one rule decides both whether a registration is contacted and which
//! `datasetId`s its source is asked for. A registration's `datasetId` lists the
//! instances its Context Source can provide, so a source is asked only for the
//! listed ones the request selects, and for every listed one when the request
//! selects every instance.

use andromeda_ngsi_ld_query::dataset_selector::DatasetSelector;

/// How the `datasetId`s of a request meet the ones a registration lists. An
/// empty list names no `datasetId`, as an absent one does, and `@none` is a
/// value like any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetOverlap<'a> {
    /// Neither names a `datasetId`: the request selects every instance and the
    /// registration restricts none.
    Unnamed,
    /// Only the request names `datasetId`s, and the registration restricts none.
    Requested(&'a [DatasetSelector]),
    /// Only the registration lists `datasetId`s: the request selects every
    /// instance, and the source provides the listed ones.
    Listed(&'a [DatasetSelector]),
    /// Both name `datasetId`s.
    Both {
        /// The request's `datasetId`s, in the order it wrote them.
        requested: &'a [DatasetSelector],
        /// The registration's `datasetId`s.
        listed: &'a [DatasetSelector],
    },
}

impl<'a> DatasetOverlap<'a> {
    /// The overlap of the `requested` `datasetId`s with the `listed` ones.
    #[must_use]
    pub fn of(requested: Option<&'a [DatasetSelector]>, listed: Option<&'a [DatasetSelector]>) -> DatasetOverlap<'a> {
        match (requested.filter(|named| !named.is_empty()), listed.filter(|named| !named.is_empty())) {
            (None, None) => DatasetOverlap::Unnamed,
            (Some(requested), None) => DatasetOverlap::Requested(requested),
            (None, Some(listed)) => DatasetOverlap::Listed(listed),
            (Some(requested), Some(listed)) => DatasetOverlap::Both { requested, listed },
        }
    }

    /// Whether the registration matches the request (clause 5.12): when both
    /// name `datasetId`s, only if they share one; otherwise always.
    #[must_use]
    pub fn matches(self) -> bool {
        match self {
            DatasetOverlap::Unnamed => true,
            DatasetOverlap::Requested(_) | DatasetOverlap::Listed(_) | DatasetOverlap::Both { .. } => self.forwarded().next().is_some(),
        }
    }

    /// The `datasetId`s a request forwarded to the registration names (clause
    /// 4.3.6.1): the requested ones the registration lists, in the request's
    /// order; every listed one when the request names none; the requested ones
    /// when the registration lists none; and none when neither names any.
    pub fn forwarded(self) -> impl Iterator<Item = &'a DatasetSelector> {
        let (named, restriction) = match self {
            DatasetOverlap::Unnamed => (&[][..], None),
            DatasetOverlap::Requested(requested) => (requested, None),
            DatasetOverlap::Listed(listed) => (listed, None),
            DatasetOverlap::Both { requested, listed } => (requested, Some(listed)),
        };
        named.iter().filter(move |selector| restriction.is_none_or(|listed| listed.contains(selector)))
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::dataset_overlap::DatasetOverlap;
    use andromeda_ngsi_ld_query::dataset_selector::DatasetSelector;
    use pretty_assertions::assert_eq;

    fn datasets(raw: &[&str]) -> Vec<DatasetSelector> {
        raw.iter().map(|selector| selector.parse().expect("a valid datasetId")).collect()
    }

    fn overlap_of(requested: Option<&[&str]>, listed: Option<&[&str]>, read: impl FnOnce(DatasetOverlap<'_>)) {
        let requested = requested.map(datasets);
        let listed = listed.map(datasets);
        read(DatasetOverlap::of(requested.as_deref(), listed.as_deref()));
    }

    const A: &str = "urn:ngsi-ld:Dataset:a";
    const B: &str = "urn:ngsi-ld:Dataset:b";
    const NONE: &str = "@none";

    /// Clause 5.12: a registration listing `datasetId`s matches a request
    /// naming some only when they share one, `@none` included; one listing
    /// none, or a request naming none, matches.
    #[test]
    fn the_dataset_match_table() {
        for (requested, listed, matches) in [
            (Some(&[A][..]), Some(&[A][..]), true),
            (Some(&[A][..]), Some(&[B][..]), false),
            (Some(&[A, B][..]), Some(&[B][..]), true),
            (Some(&[NONE][..]), Some(&[B, NONE][..]), true),
            (Some(&[NONE][..]), Some(&[B][..]), false),
            (Some(&[A][..]), Some(&[NONE][..]), false),
            (Some(&[][..]), Some(&[B][..]), true),
            (None, Some(&[B][..]), true),
            (Some(&[A][..]), Some(&[][..]), true),
            (Some(&[A][..]), None, true),
            (Some(&[NONE][..]), None, true),
            (None, None, true),
        ] {
            overlap_of(requested, listed, |overlap| {
                assert_eq!(overlap.matches(), matches, "{requested:?} against {listed:?}");
            });
        }
    }

    /// Clause 4.3.6.1 and Table 5.2.9-1: the source is asked only for the
    /// `datasetId`s its registration lists, in the order the request wrote
    /// them, for every listed one when the request selects every instance, and
    /// for the request's own when the registration restricts none.
    #[test]
    fn the_forwarded_dataset_table() {
        for (requested, listed, forwarded) in [
            (Some(&[A, B][..]), Some(&[A][..]), vec![A]),
            (Some(&[B, A][..]), Some(&[A, B][..]), vec![B, A]),
            (Some(&[NONE, B][..]), Some(&[NONE, A][..]), vec![NONE]),
            (Some(&[A][..]), Some(&[B][..]), vec![]),
            (None, Some(&[A][..]), vec![A]),
            (None, Some(&[NONE, A][..]), vec![NONE, A]),
            (Some(&[][..]), Some(&[NONE, A][..]), vec![NONE, A]),
            (Some(&[A, B][..]), None, vec![A, B]),
            (Some(&[NONE][..]), Some(&[][..]), vec![NONE]),
            (None, None, vec![]),
            (Some(&[][..]), Some(&[][..]), vec![]),
        ] {
            overlap_of(requested, listed, |overlap| {
                let written: Vec<String> = overlap.forwarded().map(ToString::to_string).collect();
                assert_eq!(written, forwarded, "{requested:?} against {listed:?}");
            });
        }
    }

    #[test]
    fn an_empty_list_names_no_dataset_id_on_either_side() {
        overlap_of(Some(&[]), Some(&[]), |overlap| assert_eq!(overlap, DatasetOverlap::Unnamed));
        overlap_of(Some(&[A]), Some(&[]), |overlap| assert!(matches!(overlap, DatasetOverlap::Requested(_))));
        overlap_of(Some(&[]), Some(&[A]), |overlap| assert!(matches!(overlap, DatasetOverlap::Listed(_))));
    }
}
