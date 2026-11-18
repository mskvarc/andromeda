//! What the back-index records of an object: where each of its entries sits.

use crate::key::SpatialKey;
use std::{iter::Chain, mem, option, sync::Arc, vec};

/// Where one entry of an object sits: enough to find it in its tree again.
#[derive(Debug, PartialEq)]
pub(super) struct Placement {
    /// The partition the entry was indexed under.
    pub(super) key: Arc<SpatialKey>,
    /// The lower corner of the entry's box, which the removal descends by.
    ///
    /// Every box on the way down to the entry holds this corner, and an entry
    /// of the object whose box starts anywhere else is passed over. Two
    /// entries of the object starting at one corner are interchangeable: an
    /// object's entries in a partition always leave together. The corner
    /// rather than the whole box keeps a placement at 24 bytes, so a record
    /// of one fits in the room of a vector's header.
    pub(super) corner: [f64; 2],
}

/// Where every entry of one object sits, held at exactly their number.
///
/// Almost every object has one entry: the entity index holds one geometry of
/// an entity, and an entity of one type sits in one partition. One placement
/// is held in place, in the back-index's own map slot, and only an object of
/// several entries holds an allocation, of exactly their length. The key's
/// handle is never null, which leaves room to tell the two forms apart without
/// a tag of their own.
#[derive(Debug, PartialEq)]
pub(super) enum Placements {
    /// The object's one placement.
    One(Placement),
    /// Any other number of placements: several, or none in a record about to
    /// be dropped.
    Several(Box<[Placement]>),
}

// Manual because a derived `Default` takes only a unit variant; no placements,
// which allocates nothing, is what is actually wanted.
impl Default for Placements {
    fn default() -> Placements {
        Placements::Several(Box::default())
    }
}

// Written by hand: a derived `From` would wrap the list whole, where one
// placement is held in place and any other number at exactly its length.
impl From<Vec<Placement>> for Placements {
    fn from(placements: Vec<Placement>) -> Placements {
        match <[Placement; 1]>::try_from(placements) {
            Ok([placement]) => Placements::One(placement),
            Err(placements) => Placements::Several(placements.into_boxed_slice()),
        }
    }
}

// Written by hand: a derived iterator walks one wrapped collection, where the
// placements are either one in place or a list.
impl IntoIterator for Placements {
    type Item = Placement;
    type IntoIter = Chain<option::IntoIter<Placement>, vec::IntoIter<Placement>>;

    fn into_iter(self) -> Chain<option::IntoIter<Placement>, vec::IntoIter<Placement>> {
        let (one, several) = match self {
            Placements::One(placement) => (Some(placement), Vec::new()),
            Placements::Several(placements) => (None, placements.into_vec()),
        };
        one.into_iter().chain(several)
    }
}

impl Placements {
    /// Whether no placement is left, so the record is to be dropped.
    pub(super) const fn is_empty(&self) -> bool {
        match self {
            Placements::One(_) => false,
            Placements::Several(placements) => placements.is_empty(),
        }
    }

    /// Detach the placements `leaving` selects, keeping the rest at exactly
    /// their number.
    #[must_use]
    pub(super) fn split_off(&mut self, leaving: impl Fn(&Placement) -> bool) -> Placements {
        match mem::take(self) {
            Placements::One(placement) if leaving(&placement) => Placements::One(placement),
            Placements::One(placement) => {
                *self = Placements::One(placement);
                Placements::default()
            }
            Placements::Several(placements) => {
                let (left, kept): (Vec<Placement>, Vec<Placement>) = placements.into_iter().partition(|placement| leaving(placement));
                *self = Placements::from(kept);
                Placements::from(left)
            }
        }
    }

    /// Keep only the placements `keep` selects.
    pub(super) fn retain(&mut self, keep: impl Fn(&Placement) -> bool) {
        drop(self.split_off(|placement| !keep(placement)));
    }

    /// Add one placement.
    pub(super) fn push(&mut self, placement: Placement) {
        self.append(Placements::One(placement));
    }

    /// Add every placement of `arriving`, at exactly the number held.
    pub(super) fn append(&mut self, arriving: Placements) {
        if arriving.is_empty() {
            return;
        }
        *self = if self.is_empty() {
            arriving
        } else {
            Placements::from(mem::take(self).into_iter().chain(arriving).collect::<Vec<Placement>>())
        };
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        key::SpatialKey,
        partitioned_rtree::placement::{Placement, Placements},
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    /// The partition of entities of type `name`.
    fn key(name: &str) -> Arc<SpatialKey> {
        Arc::new(SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::new(format!("https://example.org/{name}")).expect("a well-formed IRI"),
            "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        ))
    }

    fn placement(name: &str, longitude: f64) -> Placement {
        Placement {
            key: key(name),
            corner: [longitude, 0.0],
        }
    }

    fn several(placements: Vec<Placement>) -> Placements {
        Placements::Several(placements.into_boxed_slice())
    }

    #[test]
    fn a_record_of_one_placement_takes_the_room_of_a_vector_header() {
        assert_eq!(size_of::<Placement>(), 24);
        assert_eq!(size_of::<Placements>(), size_of::<Vec<Placement>>());
    }

    #[test]
    fn one_placement_is_held_in_place_and_any_other_number_as_a_list() {
        assert_eq!(Placements::from(Vec::new()), Placements::default());
        assert_eq!(Placements::from(vec![placement("A", 1.0)]), Placements::One(placement("A", 1.0)));
        assert_eq!(
            Placements::from(vec![placement("A", 1.0), placement("B", 2.0)]),
            several(vec![placement("A", 1.0), placement("B", 2.0)])
        );
    }

    #[test]
    fn a_list_is_held_at_exactly_its_length() {
        let mut placements = Vec::with_capacity(8);
        placements.extend([placement("A", 1.0), placement("B", 2.0), placement("C", 3.0)]);
        let Placements::Several(held) = Placements::from(placements) else {
            panic!("three placements are a list");
        };
        assert_eq!(held.len(), 3);
    }

    #[test]
    fn the_placements_come_out_in_order() {
        assert_eq!(Placements::One(placement("A", 1.0)).into_iter().collect::<Vec<_>>(), vec![placement("A", 1.0)]);
        assert_eq!(
            several(vec![placement("A", 1.0), placement("B", 2.0)]).into_iter().collect::<Vec<_>>(),
            vec![placement("A", 1.0), placement("B", 2.0)]
        );
        assert_eq!(Placements::default().into_iter().count(), 0);
    }

    #[test]
    fn splitting_off_one_placement_leaves_none() {
        let mut placements = Placements::One(placement("A", 1.0));
        assert_eq!(placements.split_off(|_| true), Placements::One(placement("A", 1.0)));
        assert!(placements.is_empty());
    }

    #[test]
    fn splitting_off_nothing_keeps_one_placement_in_place() {
        let mut placements = Placements::One(placement("A", 1.0));
        assert!(placements.split_off(|_| false).is_empty());
        assert_eq!(placements, Placements::One(placement("A", 1.0)));
    }

    #[test]
    fn splitting_a_list_down_to_one_placement_holds_it_in_place() {
        let mut placements = several(vec![placement("A", 1.0), placement("B", 2.0), placement("C", 3.0)]);
        let left = placements.split_off(|placement| placement.corner[0] > 1.5);
        assert_eq!(left, several(vec![placement("B", 2.0), placement("C", 3.0)]));
        assert_eq!(placements, Placements::One(placement("A", 1.0)));
    }

    #[test]
    fn retaining_keeps_only_the_selected_placements() {
        let mut placements = several(vec![placement("A", 1.0), placement("B", 2.0), placement("C", 3.0)]);
        placements.retain(|placement| placement.corner[0] < 2.5);
        assert_eq!(placements, several(vec![placement("A", 1.0), placement("B", 2.0)]));
        placements.retain(|_| false);
        assert!(placements.is_empty());
    }

    #[test]
    fn pushing_onto_none_holds_the_placement_in_place_and_onto_one_makes_a_list() {
        let mut placements = Placements::default();
        placements.push(placement("A", 1.0));
        assert_eq!(placements, Placements::One(placement("A", 1.0)));
        placements.push(placement("B", 2.0));
        assert_eq!(placements, several(vec![placement("A", 1.0), placement("B", 2.0)]));
        placements.push(placement("C", 3.0));
        assert_eq!(placements, several(vec![placement("A", 1.0), placement("B", 2.0), placement("C", 3.0)]));
    }

    #[test]
    fn appending_none_changes_nothing() {
        let mut placements = Placements::One(placement("A", 1.0));
        placements.append(Placements::default());
        assert_eq!(placements, Placements::One(placement("A", 1.0)));
        let mut placements = Placements::default();
        placements.append(Placements::default());
        assert!(placements.is_empty());
    }

    #[test]
    fn appending_a_list_to_one_placement_keeps_every_placement() {
        let mut placements = Placements::One(placement("A", 1.0));
        placements.append(several(vec![placement("B", 2.0), placement("C", 3.0)]));
        assert_eq!(placements, several(vec![placement("A", 1.0), placement("B", 2.0), placement("C", 3.0)]));
    }
}
