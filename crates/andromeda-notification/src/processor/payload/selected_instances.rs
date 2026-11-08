//! The attribute instances a narrowed selection carries, by attribute
//! (clause 4.5.5 identity).
//!
//! An attribute is either carried whole or by the instances listed of it.
//! Asking whether an attribute is carried, and whether one of its instances
//! is, takes one lookup each, so narrowing an attribute of `n` instances costs
//! `n` and an attribute nothing is selected of is never walked. An attribute
//! every instance of which is selected is recorded whole: a deletion of every
//! instance of a large attribute selects it in one entry, and the narrowed
//! state shares the attribute rather than copying it.
//!
//! The selection is a cache key, so its form is canonical: the attributes are
//! ordered by name, a listed attribute lists its instances in the stored order
//! of the state the selection was made of, and never lists every one.

use andromeda_ngsi_ld_entity::attribute_instances::AttributeInstances;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use derive_more::From;
use std::collections::{BTreeMap, HashSet};

/// The instances of one attribute a selection carries.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum AttributeSelection {
    /// Every instance the attribute holds.
    Whole,
    /// The instances with these `datasetId`s (`None` for the default
    /// instance), in stored order: some of the attribute's instances, never
    /// all of them.
    Instances(Vec<Option<Identifier>>),
}

impl AttributeSelection {
    /// The instances this selection keeps of `held`, `None` when it keeps
    /// none of them.
    pub(crate) fn narrow(&self, held: &AttributeInstances) -> Narrowed {
        match self {
            AttributeSelection::Whole => Narrowed::Whole,
            AttributeSelection::Instances(listed) => {
                let listed: HashSet<Option<&Identifier>> = listed.iter().map(Option::as_ref).collect();
                let mut narrowed = held.clone();
                if narrowed.retain(|dataset_id, _instance| listed.contains(&dataset_id)) {
                    Narrowed::Part(narrowed)
                } else {
                    Narrowed::Nothing
                }
            }
        }
    }
}

/// What a selection keeps of one attribute's instances.
#[derive(Debug)]
pub(crate) enum Narrowed {
    /// Every instance: the attribute is shared as it is held.
    Whole,
    /// Some of them, as a narrowed copy.
    Part(AttributeInstances),
    /// None of them.
    Nothing,
}

/// How many of an attribute's instances [`SelectedInstances::keep`] kept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kept {
    /// Every instance the attribute holds.
    Every,
    /// Some of them, not all.
    Some,
    /// None of them.
    None,
}

/// The attribute instances a selection carries, by attribute.
#[derive(Clone, Debug, Eq, From, Hash, PartialEq)]
pub(crate) struct SelectedInstances(BTreeMap<AttributeName, AttributeSelection>);

impl SelectedInstances {
    /// A selection of no instance.
    pub(crate) const fn new() -> SelectedInstances {
        SelectedInstances(BTreeMap::new())
    }

    /// Select the instances of the attribute `name` holding `held` that `keep`
    /// accepts by `datasetId`: the attribute whole when it accepts every one,
    /// nothing when it accepts none.
    pub(crate) fn keep(&mut self, name: &AttributeName, held: &AttributeInstances, mut keep: impl FnMut(Option<&Identifier>) -> bool) -> Kept {
        let listed: Vec<Option<Identifier>> = held
            .iter()
            .filter(|(dataset_id, _instance)| keep(*dataset_id))
            .map(|(dataset_id, _instance)| dataset_id.cloned())
            .collect();
        let (selection, kept) = match listed.len() {
            0 => return Kept::None,
            count if count == held.len() => (AttributeSelection::Whole, Kept::Every),
            _ => (AttributeSelection::Instances(listed), Kept::Some),
        };
        self.0.insert(name.clone(), selection);
        kept
    }

    /// What is selected of the attribute `name`, `None` when nothing is.
    pub(crate) fn get(&self, name: &AttributeName) -> Option<&AttributeSelection> {
        self.0.get(name)
    }

    /// Whether no instance is selected.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::processor::payload::selected_instances::{AttributeSelection, Kept, Narrowed, SelectedInstances};
    use andromeda_ngsi_ld_entity::{attribute_instances::AttributeInstances, entity_state::EntityState};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{
        collections::BTreeMap,
        hash::{BuildHasher, RandomState},
        iter::once,
    };

    const SPEED: &str = "https://example.com/speed";

    fn dataset(index: usize) -> String {
        format!("urn:ngsi-ld:Dataset:{index}")
    }

    /// An attribute holding a default instance and `named` named ones.
    fn speed(named: usize) -> (AttributeName, AttributeInstances) {
        let instance = |dataset_id: Option<String>| {
            let mut instance = json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]
            });
            if let Some(dataset_id) = dataset_id {
                instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset_id}]);
            }
            instance
        };
        let instances: Vec<Value> = once(instance(None)).chain((1..=named).map(|index| instance(Some(dataset(index))))).collect();
        let document = json!([{"@id": "urn:ngsi-ld:Vehicle:1", "@type": ["https://example.com/Vehicle"], SPEED: instances}]);
        let state = EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes");
        let name: AttributeName = SPEED.parse().expect("valid attribute name");
        let held = state.attributes().get(SPEED).expect("the speed is held").clone();
        (name, held)
    }

    fn identifier(index: usize) -> Identifier {
        dataset(index).parse().expect("valid dataset URI")
    }

    #[test]
    fn an_attribute_every_instance_of_which_is_kept_is_selected_whole() {
        let (name, held) = speed(3);
        let mut selected = SelectedInstances::new();
        assert_eq!(selected.keep(&name, &held, |_| true), Kept::Every);
        assert_eq!(selected.get(&name), Some(&AttributeSelection::Whole));
        assert!(matches!(AttributeSelection::Whole.narrow(&held), Narrowed::Whole));
    }

    #[test]
    fn an_attribute_none_of_whose_instances_is_kept_is_not_selected() {
        let (name, held) = speed(3);
        let mut selected = SelectedInstances::new();
        assert_eq!(selected.keep(&name, &held, |_| false), Kept::None);
        assert!(selected.is_empty());
    }

    /// Clause 4.5.5: the default instance and a named one are told apart.
    #[test]
    fn the_default_instance_and_a_named_one_are_told_apart() {
        let (name, held) = speed(2);
        let mut selected = SelectedInstances::new();
        assert_eq!(selected.keep(&name, &held, |dataset_id| dataset_id.is_none()), Kept::Some);
        assert_eq!(selected.get(&name), Some(&AttributeSelection::Instances(vec![None])));
        let Narrowed::Part(narrowed) = AttributeSelection::Instances(vec![None]).narrow(&held) else {
            panic!("the default instance is kept alone");
        };
        assert_eq!(narrowed.iter().map(|(dataset_id, _)| dataset_id.cloned()).collect::<Vec<_>>(), vec![None]);
        let named = AttributeSelection::Instances(vec![Some(identifier(2))]);
        let Narrowed::Part(narrowed) = named.narrow(&held) else {
            panic!("the named instance is kept alone");
        };
        assert_eq!(
            narrowed.iter().map(|(dataset_id, _)| dataset_id.cloned()).collect::<Vec<_>>(),
            vec![Some(identifier(2))]
        );
    }

    #[test]
    fn listed_instances_the_attribute_no_longer_holds_narrow_it_to_nothing() {
        let (_, held) = speed(1);
        assert!(matches!(
            AttributeSelection::Instances(vec![Some(identifier(9))]).narrow(&held),
            Narrowed::Nothing
        ));
    }

    /// Two selections of the same instances are one cache key.
    #[test]
    fn equal_selections_hash_alike() {
        let (name, held) = speed(4);
        let odd = |dataset_id: Option<&Identifier>| dataset_id.is_some_and(|dataset_id| dataset_id.as_str().ends_with(['1', '3']));
        let (mut first, mut second) = (SelectedInstances::new(), SelectedInstances::new());
        first.keep(&name, &held, odd);
        second.keep(&name, &held, odd);
        assert_eq!(first, second);
        let hasher = RandomState::new();
        assert_eq!(hasher.hash_one(&first), hasher.hash_one(&second));
        let built = SelectedInstances::from(BTreeMap::from([(
            name,
            AttributeSelection::Instances(vec![Some(identifier(1)), Some(identifier(3))]),
        )]));
        assert_eq!(first, built);
    }

    /// 50,000 instances, the selection of every one is a single whole entry
    /// and a selection of half of them narrows to that half.
    #[test]
    fn an_attribute_of_fifty_thousand_instances_is_selected_and_narrowed() {
        let (name, held) = speed(49_999);
        let mut every = SelectedInstances::new();
        assert_eq!(every.keep(&name, &held, |_| true), Kept::Every);
        assert_eq!(every.get(&name), Some(&AttributeSelection::Whole));
        let even = |dataset_id: Option<&Identifier>| {
            dataset_id.is_some_and(|dataset_id| {
                dataset_id
                    .as_str()
                    .rsplit(':')
                    .next()
                    .and_then(|index| index.parse::<usize>().ok())
                    .is_some_and(|index| index % 2 == 0)
            })
        };
        let mut half = SelectedInstances::new();
        assert_eq!(half.keep(&name, &held, even), Kept::Some);
        let Some(selection) = half.get(&name) else {
            panic!("half of the speed is selected");
        };
        let Narrowed::Part(narrowed) = selection.narrow(&held) else {
            panic!("the even instances are kept");
        };
        assert_eq!(narrowed.len(), 24_999);
    }
}
