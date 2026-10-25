//! What a write does to an Entity's `current_attributes` rows.
//!
//! An update knows the state it replaces, so it writes only what differs from
//! it: an instance that is new or not equal to the one it replaces is written
//! whole, an equal one whose written position moved gets its new ordinal and
//! nothing else, and a grain (clause 4.5.5) the new state no longer holds is
//! removed. An attribute both states share by pointer is equal without a look
//! at its instances. Everything is compared, system times included, because
//! the rows hold every member: an instance a write left as it was keeps its
//! times, and one it restated equal took the times it had (clause 4.8). A
//! change anywhere in an instance's sub-attribute tree rewrites its row, the
//! elements included.
//!
//! A write that replaces no state its writer read writes every instance, and
//! every stored grain it does not write is removed.

use crate::current_attribute::{
    flatten::{flatten_instance, ordinal},
    row::CurrentAttributeRow,
};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, instance::Instance};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_postgresql_attribute_instance::{flatten::FlattenError, name_keys::NameKeys};
use std::{collections::HashMap, sync::Arc};

/// The clause 4.5.5 identity of one top-level attribute instance.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InstanceGrain<'a> {
    /// The attribute name.
    pub name: &'a AttributeName,
    /// The `datasetId`, absent on the default instance.
    pub dataset_id: Option<&'a Identifier>,
}

/// Which stored rows a write accounts for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditReach {
    /// The written rows are every row the Entity keeps: a stored grain none of
    /// them has is removed.
    EveryRow,
    /// The edit names every row it writes, moves or removes; any other stored
    /// row stays as it is.
    NamedRows,
}

/// One instance a write stores whole.
#[derive(Clone, Copy, Debug)]
pub struct WrittenInstance<'a> {
    /// Its identity.
    pub grain: InstanceGrain<'a>,
    /// The instance.
    pub instance: &'a Instance,
    /// Its written position among the Entity's instances.
    pub ordinal: i32,
}

/// One stored row a write keeps as it is but at another written position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MovedInstance<'a> {
    /// Its identity.
    pub grain: InstanceGrain<'a>,
    /// Its new written position among the Entity's instances.
    pub ordinal: i32,
}

/// What one write does to an Entity's rows.
#[derive(Debug)]
pub struct RowEdit<'a> {
    /// Which stored rows the edit accounts for.
    pub reach: EditReach,
    /// The instances written whole, in written order.
    pub written: Vec<WrittenInstance<'a>>,
    /// The rows that only take a new ordinal.
    pub moved: Vec<MovedInstance<'a>>,
    /// The grains removed.
    pub removed: Vec<InstanceGrain<'a>>,
}

impl<'a> RowEdit<'a> {
    /// The edit storing `current` in place of `replaced`, the state the write
    /// was computed from, or in place of whatever is stored when the writer
    /// read nothing.
    ///
    /// # Errors
    ///
    /// Returns [`FlattenError::TooManyInstances`] when `current` holds more
    /// instances than an ordinal counts.
    pub fn between(replaced: Option<&'a EntityState>, current: &'a EntityState) -> Result<RowEdit<'a>, FlattenError> {
        let Some(replaced) = replaced else {
            return Ok(RowEdit {
                reach: EditReach::EveryRow,
                written: placed(current)?
                    .into_iter()
                    .map(|(grain, instance, ordinal)| WrittenInstance { grain, instance, ordinal })
                    .collect(),
                moved: Vec::new(),
                removed: Vec::new(),
            });
        };
        let before = placed(replaced)?;
        let stored: HashMap<InstanceGrain<'a>, (i32, &'a Instance)> = before.iter().map(|&(grain, instance, ordinal)| (grain, (ordinal, instance))).collect();
        let mut edit = RowEdit {
            reach: EditReach::NamedRows,
            written: Vec::new(),
            moved: Vec::new(),
            removed: Vec::new(),
        };
        for (grain, instance, ordinal) in placed(current)? {
            let shared = replaced
                .attributes()
                .get_shared(grain.name)
                .zip(current.attributes().get_shared(grain.name))
                .is_some_and(|(before, after)| Arc::ptr_eq(before, after));
            match stored.get(&grain) {
                Some((before, held)) if shared || *held == instance => {
                    if *before != ordinal {
                        edit.moved.push(MovedInstance { grain, ordinal });
                    }
                }
                Some(_) | None => edit.written.push(WrittenInstance { grain, instance, ordinal }),
            }
        }
        edit.removed = before
            .into_iter()
            .map(|(grain, _, _)| grain)
            .filter(|grain| {
                current
                    .attributes()
                    .get(grain.name)
                    .is_none_or(|instances| !instances.contains(grain.dataset_id))
            })
            .collect();
        Ok(edit)
    }

    /// The rows of the instances written whole.
    ///
    /// # Errors
    ///
    /// Returns [`FlattenError`] when a name has no key in `keys`, or when an
    /// instance is not one a write commits.
    pub fn rows(&self, keys: &NameKeys) -> Result<Vec<CurrentAttributeRow>, FlattenError> {
        self.written
            .iter()
            .map(|written| flatten_instance(written.grain.name, written.grain.dataset_id, written.instance, written.ordinal, keys))
            .collect()
    }
}

/// Every top-level instance of `state` with its identity and written position.
fn placed(state: &EntityState) -> Result<Vec<(InstanceGrain<'_>, &Instance, i32)>, FlattenError> {
    let mut placed = Vec::new();
    for (name, instances) in state.attributes().iter() {
        for (dataset_id, instance) in instances.iter() {
            placed.push((InstanceGrain { name, dataset_id }, instance, ordinal(name, placed.len())?));
        }
    }
    Ok(placed)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::current_attribute::{
        round_trip_tests::{committed, dictionary, document, ex, ngsi, property, with},
        row_edit::{EditReach, InstanceGrain, MovedInstance, RowEdit},
    };
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, instance::Instance};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::system_instant::SystemInstant};
    use chrono::{TimeZone, Utc};
    use serde_json::{Map, Value, json};

    /// The instant an update is committed at, after every fixture's creation.
    fn later() -> SystemInstant {
        SystemInstant::new(Utc.with_ymd_and_hms(2026, 10, 9, 13, 0, 0).single().expect("a valid instant"))
    }

    fn name(term: &str) -> AttributeName {
        ex(term).parse().expect("a valid attribute name")
    }

    fn dataset(raw: &str) -> Identifier {
        raw.parse().expect("a valid dataset identifier")
    }

    /// A Property holding the number `value`.
    fn number(value: f64) -> Value {
        property(&json!([{"@value": value}]))
    }

    /// `temperature` at `value`, its `accuracy` sub-attribute at `accuracy`,
    /// beside 49 more attributes: 50 in all. The members are written in key
    /// order, so `temperature` is the last.
    fn sensor(value: f64, accuracy: f64) -> Value {
        let mut attributes = Map::new();
        attributes.insert(ex("temperature"), json!([with(number(value), &json!({ex("accuracy"): [number(accuracy)]}))]));
        for index in 0..49 {
            attributes.insert(ex(&format!("reading{index:02}")), json!([number(f64::from(index))]));
        }
        document(&Value::Object(attributes))
    }

    /// The state an update commits from `document` in place of `previous`.
    fn updated(previous: &EntityState, document: &Value) -> EntityState {
        let mut state = EntityState::from_slice(document.to_string().as_bytes()).expect("a stored entity");
        state.settle(Some(previous), later());
        state
    }

    fn grain<'a>(name: &'a AttributeName, dataset_id: Option<&'a Identifier>) -> InstanceGrain<'a> {
        InstanceGrain { name, dataset_id }
    }

    #[test]
    fn an_update_binds_only_the_changed_attribute() {
        let previous = committed(&sensor(20.0, 0.5));
        let current = updated(&previous, &sensor(21.0, 0.5));
        let edit = RowEdit::between(Some(&previous), &current).expect("an edit");
        let temperature = name("temperature");
        let written: Vec<InstanceGrain<'_>> = edit.written.iter().map(|written| written.grain).collect();
        pretty_assertions::assert_eq!(written, vec![grain(&temperature, None)]);
        pretty_assertions::assert_eq!(edit.written[0].ordinal, 49);
        assert!(edit.moved.is_empty() && edit.removed.is_empty(), "{edit:?}");
        pretty_assertions::assert_eq!(edit.reach, EditReach::NamedRows);
    }

    /// U2: a change anywhere in an instance's sub-attribute tree rewrites the
    /// row that holds it, with its whole element array.
    #[test]
    fn a_sub_attribute_change_rewrites_its_row_only() {
        let previous = committed(&sensor(20.0, 0.5));
        let current = updated(&previous, &sensor(20.0, 0.25));
        let edit = RowEdit::between(Some(&previous), &current).expect("an edit");
        let (keys, _) = dictionary(&current);
        let rows = edit.rows(&keys).expect("the written instances flatten");
        pretty_assertions::assert_eq!(rows.len(), 1);
        pretty_assertions::assert_eq!(edit.written[0].grain.name.as_str(), ex("temperature"));
        pretty_assertions::assert_eq!(rows[0].sub_attributes.len(), 1, "the accuracy element travels with its row");
        assert!(rows[0].sub_attributes[0].modified_at.is_some_and(|instant| instant == later().get()));
    }

    /// Clause 4.8: an instance restated with the content it had keeps its
    /// system times, so it equals the stored one and nothing is written.
    #[test]
    fn an_unchanged_restatement_binds_no_rows() {
        let previous = committed(&sensor(20.0, 0.5));
        let current = updated(&previous, &sensor(20.0, 0.5));
        let edit = RowEdit::between(Some(&previous), &current).expect("an edit");
        assert!(edit.written.is_empty() && edit.moved.is_empty() && edit.removed.is_empty(), "{edit:?}");
    }

    /// A changed instance keeps the `createdAt` it was created with; only its
    /// `modifiedAt` moves (clause 4.8).
    #[test]
    fn a_changed_instance_keeps_its_created_at_in_its_row() {
        let previous = committed(&sensor(20.0, 0.5));
        let current = updated(&previous, &sensor(21.0, 0.5));
        let edit = RowEdit::between(Some(&previous), &current).expect("an edit");
        let (keys, _) = dictionary(&current);
        let rows = edit.rows(&keys).expect("the written instances flatten");
        let created = previous
            .attributes()
            .get(&ex("temperature"))
            .and_then(|instances| instances.get(None))
            .and_then(Instance::created_at)
            .expect("a stamped instance");
        pretty_assertions::assert_eq!(rows[0].created_at, created.get());
        pretty_assertions::assert_eq!(rows[0].modified_at, later().get());
    }

    /// Clause 4.5.5: removing one dataset instance removes its grain alone;
    /// the rows after it keep their content and take their new written
    /// position.
    #[test]
    fn a_removed_dataset_instance_is_removed_and_the_rows_after_it_move() {
        let instances = |datasets: &[&str]| -> Value {
            let mut held = vec![number(1.0)];
            held.extend(datasets.iter().map(|id| with(number(2.0), &json!({ngsi("datasetId"): [{"@id": id}]}))));
            Value::Array(held)
        };
        let previous = committed(&document(
            &json!({ex("level"): instances(&["urn:ds:1", "urn:ds:2"]), ex("pressure"): [number(3.0)]}),
        ));
        let current = updated(
            &previous,
            &document(&json!({ex("level"): instances(&["urn:ds:2"]), ex("pressure"): [number(3.0)]})),
        );
        let edit = RowEdit::between(Some(&previous), &current).expect("an edit");
        let (level, pressure, first, second) = (name("level"), name("pressure"), dataset("urn:ds:1"), dataset("urn:ds:2"));
        assert!(edit.written.is_empty(), "{edit:?}");
        pretty_assertions::assert_eq!(edit.removed, vec![grain(&level, Some(&first))]);
        pretty_assertions::assert_eq!(
            edit.moved,
            vec![
                MovedInstance {
                    grain: grain(&level, Some(&second)),
                    ordinal: 1
                },
                MovedInstance {
                    grain: grain(&pressure, None),
                    ordinal: 2
                },
            ]
        );
    }

    /// A removed attribute takes every one of its grains with it, its
    /// sub-attributes inside them; removing the last attribute moves no other
    /// row.
    #[test]
    fn a_removed_attribute_removes_every_grain_it_held() {
        let previous = committed(&sensor(20.0, 0.5));
        let mut current = EntityState::clone(&previous); // clone: shares every attribute, as a read-modify-write does
        current.attributes_mut().remove(&ex("temperature"));
        let edit = RowEdit::between(Some(&previous), &current).expect("an edit");
        let removed = name("temperature");
        pretty_assertions::assert_eq!(edit.removed, vec![grain(&removed, None)]);
        assert!(edit.written.is_empty() && edit.moved.is_empty(), "the last attribute moves nothing: {edit:?}");
    }

    /// Removing the first attribute moves every row after it and writes none
    /// of them.
    #[test]
    fn removing_the_first_attribute_moves_the_rows_after_it() {
        let previous = committed(&sensor(20.0, 0.5));
        let mut current = EntityState::clone(&previous); // clone: shares every attribute, as a read-modify-write does
        current.attributes_mut().remove(&ex("reading00"));
        let edit = RowEdit::between(Some(&previous), &current).expect("an edit");
        assert!(edit.written.is_empty(), "{edit:?}");
        let ordinals: Vec<i32> = edit.moved.iter().map(|moved| moved.ordinal).collect();
        pretty_assertions::assert_eq!(ordinals, (0..49).collect::<Vec<i32>>());
    }

    /// A new attribute is written at the written position after every other,
    /// as a merge places it (clause 5.6.17), and moves nothing.
    #[test]
    fn a_new_attribute_is_written_after_the_others() {
        let previous = committed(&sensor(20.0, 0.5));
        let humidity_state = committed(&document(&json!({ex("humidity"): [number(40.0)]})));
        let mut current = EntityState::clone(&previous); // clone: shares every attribute, as a read-modify-write does
        let instances = humidity_state.attributes().get_shared(&ex("humidity")).expect("the attribute").clone(); // clone: Arc ref-count bump
        current.attributes_mut().insert_shared(name("humidity"), instances);
        let edit = RowEdit::between(Some(&previous), &current).expect("an edit");
        let humidity = name("humidity");
        let written: Vec<(InstanceGrain<'_>, i32)> = edit.written.iter().map(|written| (written.grain, written.ordinal)).collect();
        pretty_assertions::assert_eq!(written, vec![(grain(&humidity, None), 50)]);
        assert!(edit.moved.is_empty() && edit.removed.is_empty(), "{edit:?}");
    }

    /// An update whose writer read nothing writes every instance at its
    /// written position, and lets the statement remove every grain it does not
    /// write.
    #[test]
    fn an_update_that_read_nothing_writes_every_row() {
        let current = committed(&sensor(20.0, 0.5));
        let edit = RowEdit::between(None, &current).expect("an edit");
        pretty_assertions::assert_eq!(edit.reach, EditReach::EveryRow);
        let ordinals: Vec<i32> = edit.written.iter().map(|written| written.ordinal).collect();
        pretty_assertions::assert_eq!(ordinals, (0..50).collect::<Vec<i32>>());
        assert!(edit.moved.is_empty() && edit.removed.is_empty(), "{edit:?}");
    }
}
