//! Which members of a notified entity state one notification carries
//! (clause 5.8.6): the attributes `notification.attributes` lists, the
//! instances the subscription's `datasetId` selects, and the attributes its
//! `pick` and `omit` keep.
//!
//! A selection is decided on the typed state by expanded attribute name, before
//! anything is compacted, and it is a value of its own: two subscriptions that
//! select the same instances of the same state are served one compaction.

use crate::processor::payload::projection::projection_keeps;
use andromeda_ngsi_ld_entity::{attributes::Attributes, entity_state::EntityState, reads::attribute_instance_key::AttributeInstanceKey};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::entity_change::EntityChange;
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::compiled::Projection};
use andromeda_ngsi_ld_vocabulary::prefix;
use std::{borrow::Cow, collections::HashMap, sync::Arc};

/// What a subscription selects of every entity it is notified of.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SelectionRule<'r> {
    /// The `notification.attributes` list, by expanded name.
    pub(crate) attributes: Option<&'r [AttributeName]>,
    /// The subscription's `datasetId` selectors (Table 5.2.12-1).
    pub(crate) dataset_ids: Option<&'r [DatasetSelector]>,
    /// The compiled `notification.pick` / `notification.omit`.
    pub(crate) projection: &'r Projection,
}

impl SelectionRule<'_> {
    /// Whether the entity's own members -- `scope`, its `expiresAt` and the
    /// clause 4.8 system attributes -- travel with the selected attributes.
    ///
    /// A `notification.attributes` list names the members a notification
    /// carries beside `id` and `type`, and none of the entity's own members is
    /// an attribute it can list.
    pub(crate) const fn entity_members(self) -> EntityMembers {
        match self.attributes {
            Some(_) => EntityMembers::Dropped,
            None => EntityMembers::Kept,
        }
    }

    /// Whether the rule keeps the attribute `name` at all.
    fn keeps_attribute(self, name: &AttributeName) -> bool {
        self.attributes.is_none_or(|listed| listed.contains(name)) && projection_keeps(name, self.projection)
    }

    /// Whether the rule keeps the instance of `name` carrying `dataset_id`.
    ///
    /// Table 5.2.12-1: `@none` selects the default instance. The selectors
    /// apply to the attributes a client names; the attributes the NGSI-LD core
    /// vocabulary defines (`location` and its siblings) are carried whole.
    fn keeps_instance(self, name: &AttributeName, dataset_id: Option<&Identifier>) -> bool {
        match self.dataset_ids {
            Some(selectors) if is_dataset_selectable(name) => selectors.iter().any(|selector| selector.selects(dataset_id)),
            Some(_) | None => true,
        }
    }
}

/// Whether a `datasetId` selection reaches the attribute `name`: every
/// attribute outside the NGSI-LD core namespace, and the ones the core context's
/// `@vocab` coined for undeclared terms.
fn is_dataset_selectable(name: &AttributeName) -> bool {
    let core = prefix::NGSI_LD.as_str();
    let coined = name.as_str().strip_prefix(core).is_some_and(|rest| rest.starts_with("default-context/"));
    !name.as_str().starts_with(core) || coined
}

/// Whether the entity's own members are carried.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum EntityMembers {
    /// `scope`, `expiresAt` and the system attributes travel with the entity.
    Kept,
    /// Only `id`, `type` and the selected attributes travel.
    Dropped,
}

/// The members of one entity state a notification carries.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Selection {
    /// The state as it is held.
    Whole,
    /// The listed attribute instances, in the state's stored order.
    Narrowed {
        /// The instances carried.
        instances: Vec<AttributeInstanceKey>,
        /// Whether the entity's own members are carried.
        members: EntityMembers,
    },
}

impl Selection {
    /// What `rule` selects of `state`.
    pub(crate) fn of(state: &EntityState, rule: SelectionRule<'_>) -> Selection {
        let mut narrowed = rule.entity_members() == EntityMembers::Dropped;
        let mut instances = Vec::new();
        for (name, held) in state.attributes().iter() {
            let attribute_kept = rule.keeps_attribute(name);
            for (dataset_id, _instance) in held.iter() {
                if attribute_kept && rule.keeps_instance(name, dataset_id) {
                    instances.push(AttributeInstanceKey {
                        attr: name.clone(),              // clone: the selection names the instance it keeps
                        dataset_id: dataset_id.cloned(), // clone: same
                    });
                } else {
                    narrowed = true;
                }
            }
        }
        if narrowed {
            Selection::Narrowed {
                instances,
                members: rule.entity_members(),
            }
        } else {
            Selection::Whole
        }
    }

    /// The entity's identity alone: its `id` and its types.
    pub(crate) const fn identity() -> Selection {
        Selection::Narrowed {
            instances: Vec::new(),
            members: EntityMembers::Dropped,
        }
    }

    /// The instances of `previous` the change touched, for the `showChanges`
    /// previous values of a creation or modification (clause 5.8.6).
    pub(crate) fn touched(previous: &EntityState, change: &EntityChange) -> Selection {
        let instances = change
            .instances
            .iter()
            .filter(|touched| held_instance(previous, &touched.key))
            .map(|touched| touched.key.clone()) // clone: the selection names the instance it keeps
            .collect();
        Selection::Narrowed {
            instances,
            members: EntityMembers::Dropped,
        }
    }

    /// The instances of `previous` a creation or modification deleted that
    /// `rule` selects, or `None` when there are none to materialise.
    ///
    /// Clause 5.8.6 notifies a deleted attribute as `urn:ngsi-ld:null`, so the
    /// instance comes from the state before the change; one the write left in
    /// place is no deletion of it. A subscription with `watchedAttributes` is
    /// told about those attributes alone.
    pub(crate) fn deleted<'k>(
        previous: &EntityState,
        current: &EntityState,
        deleted: impl IntoIterator<Item = &'k AttributeInstanceKey>,
        rule: SelectionRule<'_>,
    ) -> Option<Selection> {
        let wanted: Vec<&AttributeInstanceKey> = deleted
            .into_iter()
            .filter(|key| rule.keeps_attribute(&key.attr) && rule.keeps_instance(&key.attr, key.dataset_id.as_ref()))
            .filter(|key| !held_instance(current, key))
            .collect();
        let instances: Vec<AttributeInstanceKey> = previous
            .attributes()
            .iter()
            .flat_map(|(name, held)| held.iter().map(move |(dataset_id, _instance)| (name, dataset_id)))
            .filter(|(name, dataset_id)| wanted.iter().any(|key| &key.attr == *name && key.dataset_id.as_ref() == *dataset_id))
            .map(|(name, dataset_id)| AttributeInstanceKey {
                attr: name.clone(),              // clone: the selection names the instance it keeps
                dataset_id: dataset_id.cloned(), // clone: same
            })
            .collect();
        (!instances.is_empty()).then_some(Selection::Narrowed {
            instances,
            members: EntityMembers::Dropped,
        })
    }

    /// The state this selection carries of `source`: `source` itself when the
    /// selection is whole, otherwise a state holding the selected instances,
    /// which shares every attribute it keeps whole.
    pub(crate) fn apply<'s>(&self, source: &'s EntityState) -> Cow<'s, EntityState> {
        let (instances, members) = match self {
            Selection::Whole => return Cow::Borrowed(source),
            Selection::Narrowed { instances, members } => (instances, *members),
        };
        let mut kept: HashMap<&AttributeName, Vec<Option<&Identifier>>> = HashMap::new();
        for key in instances {
            kept.entry(&key.attr).or_default().push(key.dataset_id.as_ref());
        }
        let mut attributes = Attributes::new();
        for (name, held) in source.attributes().iter_shared() {
            let Some(datasets) = kept.get(name) else { continue };
            if held.iter().all(|(dataset_id, _instance)| datasets.contains(&dataset_id)) {
                // clone: the narrowed state names the attribute it shares
                attributes.insert_shared(name.clone(), Arc::clone(held));
            } else {
                // clone: the narrowed state owns the kept instances of an attribute it does not keep whole
                let mut narrowed = (**held).clone();
                if narrowed.retain(|dataset_id, _instance| datasets.contains(&dataset_id)) {
                    attributes.insert(name.clone(), narrowed); // clone: the narrowed state names the attribute
                }
            }
        }
        let (scope, expires_at) = match members {
            EntityMembers::Kept => (source.scope().cloned(), source.expires_at().cloned()), // clone: the narrowed state owns the entity's own members
            EntityMembers::Dropped => (None, None),
        };
        // clone: the narrowed state is named and typed as its source
        Cow::Owned(EntityState::from_parts(
            source.id().clone(),
            source.types().clone(),
            scope,
            expires_at,
            attributes,
        ))
    }
}

/// Whether `state` holds the instance `key` names.
fn held_instance(state: &EntityState, key: &AttributeInstanceKey) -> bool {
    state
        .attributes()
        .get(key.attr.as_str())
        .is_some_and(|held| held.contains(key.dataset_id.as_ref()))
}

#[cfg(test)]
mod tests {
    use crate::processor::payload::selection::{EntityMembers, Selection, SelectionRule};
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::compiled::Projection};
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::{borrow::Cow, sync::Arc};

    const SPEED: &str = "https://example.com/speed";
    const NAME: &str = "https://example.com/name";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const NAMED: &str = "urn:ngsi-ld:Dataset:named";

    fn property(value: &Value, dataset: Option<&str>) -> Value {
        let mut instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
        });
        if let Some(dataset) = dataset {
            instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
        }
        instance
    }

    fn point(dataset: Option<&str>) -> Value {
        let mut instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 1}, {"@value": 2}]}]
            }]
        });
        if let Some(dataset) = dataset {
            instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
        }
        instance
    }

    /// A Vehicle whose `speed` holds a default and a named instance, with a
    /// `name`, a two-instance `location` and a scope.
    fn vehicle() -> EntityState {
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}],
            SPEED: [property(&json!(1), None), property(&json!(2), Some(NAMED))],
            NAME: [property(&json!("bus"), None)],
            LOCATION: [point(None), point(Some(NAMED))]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    fn selectors(raw: &[&str]) -> Vec<DatasetSelector> {
        raw.iter().map(|entry| entry.parse().expect("valid dataset selector")).collect()
    }

    fn rule<'r>(attributes: Option<&'r [AttributeName]>, dataset_ids: Option<&'r [DatasetSelector]>, projection: &'r Projection) -> SelectionRule<'r> {
        SelectionRule {
            attributes,
            dataset_ids,
            projection,
        }
    }

    fn key(attribute: &str, dataset: Option<&str>) -> AttributeInstanceKey {
        AttributeInstanceKey {
            attr: attribute.parse().expect("valid attribute name"),
            dataset_id: dataset.map(|dataset| dataset.parse().expect("valid dataset URI")),
        }
    }

    fn datasets_of<'s>(state: &'s EntityState, attribute: &str) -> Vec<Option<&'s str>> {
        state
            .attributes()
            .get(attribute)
            .into_iter()
            .flat_map(|held| held.iter().map(|(dataset_id, _instance)| dataset_id.map(Identifier::as_str)))
            .collect()
    }

    /// No selection keeps the state itself, borrowed rather than copied.
    #[test]
    fn a_subscription_selecting_nothing_carries_the_state_itself() {
        let state = vehicle();
        let projection = Projection::default();
        let selection = Selection::of(&state, rule(None, None, &projection));
        assert_eq!(selection, Selection::Whole);
        assert!(matches!(selection.apply(&state), Cow::Borrowed(_)));
    }

    /// Table 5.2.12-1: `@none` selects the default instance, and only it.
    #[test]
    fn the_none_selector_keeps_only_the_default_instance() {
        let state = vehicle();
        let projection = Projection::default();
        let none = selectors(&["@none"]);
        let selected = Selection::of(&state, rule(None, Some(&none), &projection)).apply(&state).into_owned();
        assert_eq!(datasets_of(&selected, SPEED), vec![None]);
        assert_eq!(datasets_of(&selected, NAME), vec![None]);
    }

    /// A named selector keeps the instance carrying that `datasetId` and drops
    /// the default one, and an attribute left with no instance leaves.
    #[test]
    fn a_named_selector_keeps_only_its_instance() {
        let state = vehicle();
        let projection = Projection::default();
        let named = selectors(&[NAMED]);
        let selected = Selection::of(&state, rule(None, Some(&named), &projection)).apply(&state).into_owned();
        assert_eq!(datasets_of(&selected, SPEED), vec![Some(NAMED)]);
        assert!(selected.attributes().get(NAME).is_none());
    }

    /// `@none` beside a named selector keeps every instance.
    #[test]
    fn the_none_and_a_named_selector_together_keep_both_instances() {
        let state = vehicle();
        let projection = Projection::default();
        let both = selectors(&["@none", NAMED]);
        assert_eq!(Selection::of(&state, rule(None, Some(&both), &projection)), Selection::Whole);
    }

    /// The NGSI-LD core attributes are carried whole by a `datasetId`
    /// selection, while a client's attributes are narrowed.
    #[test]
    fn a_dataset_selection_leaves_the_core_attributes_whole() {
        let state = vehicle();
        let projection = Projection::default();
        let other = selectors(&["urn:ngsi-ld:Dataset:other"]);
        let selected = Selection::of(&state, rule(None, Some(&other), &projection)).apply(&state).into_owned();
        assert_eq!(datasets_of(&selected, LOCATION), vec![None, Some(NAMED)]);
        assert!(selected.attributes().get(SPEED).is_none());
    }

    /// `notification.attributes` keeps the attributes it lists, `id` and the
    /// types, and none of the entity's own members.
    #[test]
    fn the_listed_attributes_travel_without_the_entity_members() {
        let state = vehicle();
        let projection = Projection::default();
        let listed: Vec<AttributeName> = vec![SPEED.parse().expect("valid attribute name")];
        let selection = Selection::of(&state, rule(Some(&listed), None, &projection));
        assert!(matches!(
            selection,
            Selection::Narrowed {
                members: EntityMembers::Dropped,
                ..
            }
        ));
        let selected = selection.apply(&state);
        assert_eq!(selected.attributes().names().map(AttributeName::as_str).collect::<Vec<_>>(), vec![SPEED]);
        assert!(selected.scope().is_none());
        assert_eq!(selected.id(), state.id());
    }

    /// An attribute kept whole is shared with the source, not copied.
    #[test]
    fn an_attribute_kept_whole_is_shared_with_the_source() {
        let state = vehicle();
        let projection = Projection::default();
        let listed: Vec<AttributeName> = vec![LOCATION.parse().expect("valid attribute name")];
        let selected = Selection::of(&state, rule(Some(&listed), None, &projection)).apply(&state).into_owned();
        let (Some(kept), Some(source)) = (selected.attributes().get_shared(LOCATION), state.attributes().get_shared(LOCATION)) else {
            panic!("the location is kept");
        };
        assert!(Arc::ptr_eq(kept, source));
    }

    /// Clause 5.8.6 with clause 5.5.11.0: the instances one change deleted are
    /// selected from the state before it, a dataset-specific deletion selecting
    /// that instance alone.
    #[test]
    fn a_dataset_specific_deletion_selects_only_that_instance() {
        let previous = vehicle();
        let current = EntityState::new(previous.id().clone(), previous.types().clone());
        let projection = Projection::default();
        let deleted = [key(SPEED, Some(NAMED))];
        let selection = Selection::deleted(&previous, &current, &deleted, rule(None, None, &projection)).expect("a deleted instance");
        let selected = selection.apply(&previous);
        assert_eq!(datasets_of(&selected, SPEED), vec![Some(NAMED)]);
        assert!(selected.attributes().get(NAME).is_none());
    }

    #[test]
    fn every_instance_one_change_deleted_is_selected() {
        let previous = vehicle();
        let current = EntityState::new(previous.id().clone(), previous.types().clone());
        let projection = Projection::default();
        let deleted = [key(SPEED, None), key(SPEED, Some(NAMED)), key(NAME, None)];
        let selection = Selection::deleted(&previous, &current, &deleted, rule(None, None, &projection)).expect("deleted instances");
        let selected = selection.apply(&previous);
        assert_eq!(datasets_of(&selected, SPEED), vec![None, Some(NAMED)]);
        assert_eq!(datasets_of(&selected, NAME), vec![None]);
    }

    /// An instance the write left in place was not deleted by it, and a
    /// deletion the subscription does not select is not materialised.
    #[test]
    fn nothing_is_materialised_for_an_instance_still_held_or_not_selected() {
        let previous = vehicle();
        let projection = Projection::default();
        let still_held = [key(SPEED, None)];
        assert_eq!(Selection::deleted(&previous, &previous, &still_held, rule(None, None, &projection)), None);

        let current = EntityState::new(previous.id().clone(), previous.types().clone());
        let listed: Vec<AttributeName> = vec![NAME.parse().expect("valid attribute name")];
        assert_eq!(
            Selection::deleted(&previous, &current, &still_held, rule(Some(&listed), None, &projection)),
            None
        );
    }

    /// The previous state a creation or modification reports is the instances
    /// it touched that the state before it held.
    #[test]
    fn a_touched_selection_names_the_touched_instances_held_before() {
        let previous = vehicle();
        let change = EntityChange::new(
            EntityLifecycle::Modified,
            vec![
                AttributeChange {
                    key: key(SPEED, Some(NAMED)),
                    kind: AttributeChangeKind::Updated,
                },
                AttributeChange {
                    key: key("https://example.com/fresh", None),
                    kind: AttributeChangeKind::Created,
                },
            ],
        );
        assert_eq!(
            Selection::touched(&previous, &change),
            Selection::Narrowed {
                instances: vec![key(SPEED, Some(NAMED))],
                members: EntityMembers::Dropped,
            }
        );
    }
}
