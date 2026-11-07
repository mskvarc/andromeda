//! A registration's `information` (clause 5.2.10) as `registration_information`
//! rows, and the rows as the `information` they were written from.
//!
//! An item is one row per `EntityInfo`, numbered from 0 within the item, or
//! one row without entity columns at selector 0 when it has no `entities`. The
//! item's `propertyNames` and `relationshipNames` are repeated on each of its
//! rows, so the row naming a type also names the attributes clause 5.12 pairs
//! it with. Every declared name is a dictionary key: the write interns them
//! first, so a row never stands for a name it could not store.
//!
//! Assembly reads the rows in any order and refuses a set no write produces,
//! rather than reading back an `information` the client never wrote.

use crate::{csource_registration::stored_information_error::StoredInformationError, store_invariant::StoreInvariant};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity_info::{EntityInfo, EntityTypeSelection},
    registration::info::RegistrationInfo,
};
use andromeda_ngsi_ld_query::{
    pattern::Pattern,
    portable_pattern::{PortablePattern, RenderingRevision},
};
use andromeda_postgresql_codes::entity_type_shape::EntityTypeShapeCode;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use indexmap::IndexSet;
use std::{collections::HashMap, hash::BuildHasher};

/// One `registration_information` row, borrowing the text it stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InformationRow<'info> {
    /// The item's position in `information`.
    pub item_ordinal: i32,
    /// The `EntityInfo`'s position in the item's `entities`; 0 for an item
    /// without `entities`.
    pub selector_ordinal: i32,
    /// The `EntityInfo` this row stands for; `None` for an item without
    /// `entities`.
    pub selector: Option<SelectorColumns<'info>>,
    /// The item's `propertyNames`; `None` when absent.
    pub property_name_keys: Option<Vec<AttributeNameKey>>,
    /// The item's `relationshipNames`; `None` when absent.
    pub relationship_name_keys: Option<Vec<AttributeNameKey>>,
}

/// The entity columns of a row standing for one `EntityInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorColumns<'info> {
    /// The type names, in written order.
    pub types: Vec<&'info str>,
    /// Whether `type` was written as one name or as an array.
    pub shape: EntityTypeShapeCode,
    /// The `id`, if written.
    pub id: Option<&'info str>,
    /// The `idPattern` as written, if written.
    pub pattern: Option<&'info str>,
    /// The portable restatement of `pattern` the store evaluates in its place;
    /// present exactly when `pattern` is.
    pub pattern_form: Option<&'info str>,
}

/// The shape a type selection is stored under.
const fn shape_of(selection: &EntityTypeSelection) -> EntityTypeShapeCode {
    match selection {
        EntityTypeSelection::Single(_) => EntityTypeShapeCode::Single,
        EntityTypeSelection::Several(_) => EntityTypeShapeCode::Several,
    }
}

/// The attribute names `information` declares, each once, in first-seen
/// order: what the dictionary holds keys for before the rows are built.
#[must_use]
pub fn names_to_intern(information: &[RegistrationInfo]) -> Vec<AttributeName> {
    let names: IndexSet<&AttributeName> = information.iter().flat_map(RegistrationInfo::attribute_names).collect();
    names.into_iter().cloned().collect() // clone: the dictionary request owns the names it resolves
}

/// The rows `information` is stored as, in item and selector order.
///
/// # Errors
///
/// Returns [`StoreInvariant::InformationNameNotInterned`] when `keys` holds no
/// key for a declared name, which the caller interned beforehand, and
/// [`StoreInvariant::InformationPositionOutOfRange`] when a position does not
/// fit the ordinal column.
pub fn rows_of<'info, S: BuildHasher>(
    information: &'info [RegistrationInfo],
    keys: &HashMap<AttributeName, AttributeNameKey, S>,
) -> Result<Vec<InformationRow<'info>>, StoreInvariant> {
    let mut rows = Vec::new();
    for (item_position, item) in information.iter().enumerate() {
        let item_ordinal = ordinal(item_position)?;
        let property_name_keys = keys_of(item.property_names.as_deref(), keys)?;
        let relationship_name_keys = keys_of(item.relationship_names.as_deref(), keys)?;
        match item.entities.as_deref() {
            None => rows.push(InformationRow {
                item_ordinal,
                selector_ordinal: 0,
                selector: None,
                property_name_keys,
                relationship_name_keys,
            }),
            Some(selectors) => {
                for (selector_position, selector) in selectors.iter().enumerate() {
                    rows.push(InformationRow {
                        item_ordinal,
                        selector_ordinal: ordinal(selector_position)?,
                        selector: Some(selector_columns(selector)),
                        property_name_keys: property_name_keys.clone(), // clone: every row of the item repeats its names
                        relationship_name_keys: relationship_name_keys.clone(), // clone: same
                    });
                }
            }
        }
    }
    Ok(rows)
}

/// The rendering revision the rows' pattern restatements were produced by, or
/// `None` when no row carries a pattern.
#[must_use]
pub fn pattern_forms_revision(rows: &[InformationRow<'_>]) -> Option<RenderingRevision> {
    rows.iter()
        .any(|row| row.selector.as_ref().is_some_and(|selector| selector.pattern.is_some()))
        .then_some(PortablePattern::RENDERING_REVISION)
}

/// Every dictionary key `rows` hold, each once: what assembly needs the names
/// of.
#[must_use]
pub fn held_keys<'row, 'info: 'row>(rows: impl IntoIterator<Item = &'row InformationRow<'info>>) -> Vec<AttributeNameKey> {
    let keys: IndexSet<AttributeNameKey> = rows
        .into_iter()
        .flat_map(|row| row.property_name_keys.iter().chain(row.relationship_name_keys.iter()).flatten().copied())
        .collect();
    keys.into_iter().collect()
}

/// The `information` `rows` were written from, in item and selector order.
///
/// # Errors
///
/// Returns [`StoredInformationError`] when the rows are not a set a write
/// produces: a position missing or repeated, an item whose rows disagree on its
/// names or mix the two row shapes, an item declaring nothing or an empty name
/// list, entity types that do not fit their shape, a key `names` holds no name
/// for, or stored text that does not parse back into its type. No rows at all
/// is the gap at the first position: an `information` is never empty (Table
/// 5.2.9-1).
pub fn information_of<S: BuildHasher>(
    rows: &[InformationRow<'_>],
    names: &HashMap<AttributeNameKey, AttributeName, S>,
) -> Result<Vec<RegistrationInfo>, StoredInformationError> {
    if rows.is_empty() {
        return Err(StoredInformationError::OrdinalGap {
            item_ordinal: 0,
            selector_ordinal: 0,
        });
    }
    let mut ordered: Vec<&InformationRow<'_>> = rows.iter().collect();
    ordered.sort_by_key(|row| (row.item_ordinal, row.selector_ordinal));
    let ordered = ordered;
    ordered
        .chunk_by(|left, right| left.item_ordinal == right.item_ordinal)
        .enumerate()
        .map(|(position, item_rows)| item_of(position, item_rows, names))
        .collect()
}

/// The one item `rows` (all of one item ordinal, in selector order) stand for,
/// which must be the item at `position`.
fn item_of<S: BuildHasher>(
    position: usize,
    rows: &[&InformationRow<'_>],
    names: &HashMap<AttributeNameKey, AttributeName, S>,
) -> Result<RegistrationInfo, StoredInformationError> {
    let Some((first, _)) = rows.split_first() else {
        return Err(StoredInformationError::OrdinalGap {
            item_ordinal: position,
            selector_ordinal: 0,
        });
    };
    let item_ordinal = first.item_ordinal;
    for (selector_position, row) in rows.iter().enumerate() {
        if !(at_position(row.item_ordinal, position) && at_position(row.selector_ordinal, selector_position)) {
            return Err(StoredInformationError::OrdinalGap {
                item_ordinal: position,
                selector_ordinal: selector_position,
            });
        }
        if row.property_name_keys != first.property_name_keys || row.relationship_name_keys != first.relationship_name_keys {
            return Err(StoredInformationError::NamesDisagree { item_ordinal });
        }
    }
    let entities = match (&first.selector, rows.len()) {
        (None, 1) => None,
        (None, _) => return Err(StoredInformationError::MixedItem { item_ordinal }),
        (Some(_), _) => Some(
            rows.iter()
                .map(|row| match &row.selector {
                    Some(selector) => entity_info(selector, row),
                    None => Err(StoredInformationError::MixedItem { item_ordinal }),
                })
                .collect::<Result<Vec<EntityInfo>, StoredInformationError>>()?,
        ),
    };
    let property_names = names_of(first.property_name_keys.as_deref(), item_ordinal, names)?;
    let relationship_names = names_of(first.relationship_name_keys.as_deref(), item_ordinal, names)?;
    if entities.is_none() && property_names.is_none() && relationship_names.is_none() {
        return Err(StoredInformationError::EmptyItem { item_ordinal });
    }
    Ok(RegistrationInfo {
        entities,
        property_names,
        relationship_names,
    })
}

/// Whether a stored ordinal is the position `expected`.
fn at_position(stored: i32, expected: usize) -> bool {
    usize::try_from(stored).is_ok_and(|stored| stored == expected)
}

/// The `EntityInfo` one row's entity columns stand for.
fn entity_info(selector: &SelectorColumns<'_>, row: &InformationRow<'_>) -> Result<EntityInfo, StoredInformationError> {
    let misshapen = || StoredInformationError::TypeShape {
        item_ordinal: row.item_ordinal,
        selector_ordinal: row.selector_ordinal,
    };
    let names = selector
        .types
        .iter()
        .map(|raw| raw.parse::<EntityTypeName>().map_err(StoredInformationError::EntityType))
        .collect::<Result<Vec<_>, _>>()?;
    let selection = match selector.shape {
        EntityTypeShapeCode::Single => {
            let [only] = <[EntityTypeName; 1]>::try_from(names).map_err(|_| misshapen())?;
            EntityTypeSelection::Single(only)
        }
        EntityTypeShapeCode::Several if names.is_empty() => return Err(misshapen()),
        EntityTypeShapeCode::Several => EntityTypeSelection::Several(names),
    };
    let id = selector
        .id
        .map(str::parse::<Identifier>)
        .transpose()
        .map_err(StoredInformationError::EntityId)?;
    let id_pattern = selector
        .pattern
        .map(Pattern::parse)
        .transpose()
        .map_err(|error| StoredInformationError::IdPattern(Box::new(error)))?;
    Ok(EntityInfo {
        type_: selection,
        id,
        id_pattern,
    })
}

/// The names one stored axis holds, `None` when the axis is absent.
fn names_of<S: BuildHasher>(
    keys: Option<&[AttributeNameKey]>,
    item_ordinal: i32,
    names: &HashMap<AttributeNameKey, AttributeName, S>,
) -> Result<Option<Vec<AttributeName>>, StoredInformationError> {
    let Some(keys) = keys else {
        return Ok(None);
    };
    if keys.is_empty() {
        return Err(StoredInformationError::EmptyNames { item_ordinal });
    }
    keys.iter()
        .map(|key| names.get(key).cloned().ok_or(StoredInformationError::UnknownNameKey(*key))) // clone: the information owns its names
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// The entity columns of one `EntityInfo`.
fn selector_columns(selector: &EntityInfo) -> SelectorColumns<'_> {
    SelectorColumns {
        types: selector.type_.names().iter().map(EntityTypeName::as_str).collect(),
        shape: shape_of(&selector.type_),
        id: selector.id.as_ref().map(Identifier::as_str),
        pattern: selector.id_pattern.as_ref().map(Pattern::as_str),
        pattern_form: selector.id_pattern.as_ref().map(|pattern| pattern.portable().as_str()),
    }
}

/// The keys of one declared name list, `None` when the list is absent.
fn keys_of<S: BuildHasher>(
    names: Option<&[AttributeName]>,
    keys: &HashMap<AttributeName, AttributeNameKey, S>,
) -> Result<Option<Vec<AttributeNameKey>>, StoreInvariant> {
    names
        .map(|names| {
            names
                .iter()
                .map(|name| {
                    keys.get(name).copied().ok_or_else(|| StoreInvariant::InformationNameNotInterned {
                        name: name.clone(), // clone: the report owns the name it names
                    })
                })
                .collect()
        })
        .transpose()
}

/// A position as the ordinal column stores it.
fn ordinal(position: usize) -> Result<i32, StoreInvariant> {
    i32::try_from(position).map_err(|_| StoreInvariant::InformationPositionOutOfRange { position })
}

#[cfg(test)]
mod tests {
    use crate::{
        csource_registration::{
            information_rows::{InformationRow, SelectorColumns, held_keys, information_of, names_to_intern, pattern_forms_revision, rows_of},
            stored_information_error::StoredInformationError,
        },
        store_invariant::StoreInvariant,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
    use andromeda_ngsi_ld_query::portable_pattern::PortablePattern;
    use andromeda_postgresql_codes::entity_type_shape::EntityTypeShapeCode;
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::collections::HashMap;

    /// An expanded property IRI, as the service layer hands one to the store.
    const P: &str = "https://example.org/p";
    /// A second expanded property IRI.
    const Q: &str = "https://example.org/q";
    /// An expanded relationship IRI.
    const R: &str = "https://example.org/r";

    fn name(iri: &str) -> AttributeName {
        iri.parse().expect("a valid attribute name")
    }

    fn keys() -> HashMap<AttributeName, AttributeNameKey> {
        HashMap::from([
            (name(P), AttributeNameKey::from(11)),
            (name(Q), AttributeNameKey::from(12)),
            (name(R), AttributeNameKey::from(13)),
        ])
    }

    fn names() -> HashMap<AttributeNameKey, AttributeName> {
        keys().into_iter().map(|(name, key)| (key, name)).collect()
    }

    fn information(document: Value) -> Vec<RegistrationInfo> {
        serde_json::from_value(document).expect("a valid information")
    }

    fn key_list(keys: &[i32]) -> Vec<AttributeNameKey> {
        keys.iter().copied().map(AttributeNameKey::from).collect()
    }

    /// The JSON an `information` renders as, which is what reading it back
    /// must reproduce: `Pattern` has no equality of its own.
    fn rendered(information: &[RegistrationInfo]) -> Value {
        serde_json::to_value(information).expect("the information renders")
    }

    /// Every case the row tests below write, as one `information` each.
    fn every_case() -> Vec<Value> {
        vec![
            json!([{"entities": [{"type": "T"}]}]),
            json!([{"entities": [{"type": ["A", "B"]}]}]),
            json!([{"entities": [{"type": "T", "id": "urn:x", "idPattern": "urn:.*"}]}]),
            json!([{"entities": [{"type": "T"}, {"type": "U"}], "propertyNames": [P], "relationshipNames": [R]}]),
            json!([{"propertyNames": [P]}]),
            json!([{"propertyNames": [P]}, {"entities": [{"type": "T"}]}]),
            json!([{"entities": [{"type": "A"}]}, {"relationshipNames": [R]}, {"entities": [{"type": ["B"]}], "propertyNames": [Q, P]}]),
        ]
    }

    #[test]
    fn a_type_only_selector_is_one_row_with_its_type() {
        let information = information(json!([{"entities": [{"type": "T"}]}]));
        assert_eq!(
            rows_of(&information, &keys()).expect("the rows build"),
            vec![InformationRow {
                item_ordinal: 0,
                selector_ordinal: 0,
                selector: Some(SelectorColumns {
                    types: vec!["T"],
                    shape: EntityTypeShapeCode::Single,
                    id: None,
                    pattern: None,
                    pattern_form: None,
                }),
                property_name_keys: None,
                relationship_name_keys: None,
            }]
        );
    }

    /// Table 5.2.8-1: `type` is one name or an array; the array is kept as an
    /// array.
    #[test]
    fn a_type_array_keeps_its_array_shape() {
        let information = information(json!([{"entities": [{"type": ["A", "B"]}]}]));
        let rows = rows_of(&information, &keys()).expect("the rows build");
        let selector = rows[0].selector.as_ref().expect("a selector row");
        assert_eq!((selector.types.clone(), selector.shape), (vec!["A", "B"], EntityTypeShapeCode::Several));
    }

    /// Clause 5.2.8: `id` takes precedence over `idPattern` when matching, but
    /// both are what the client wrote, so both are stored.
    #[test]
    fn an_identifier_and_a_pattern_are_both_kept() {
        let information = information(json!([{"entities": [{"type": "T", "id": "urn:x", "idPattern": "urn:.*"}]}]));
        let rows = rows_of(&information, &keys()).expect("the rows build");
        let selector = rows[0].selector.as_ref().expect("a selector row");
        assert_eq!(
            (selector.id, selector.pattern, selector.pattern_form),
            (Some("urn:x"), Some("urn:.*"), Some("urn:.*"))
        );
        assert_eq!(pattern_forms_revision(&rows), Some(PortablePattern::RENDERING_REVISION));
    }

    /// Clause 5.12 matches the attributes against the same item as the
    /// entities, so every selector row carries its item's names.
    #[test]
    fn an_items_names_are_on_every_selector_row() {
        let information = information(json!([{"entities": [{"type": "T"}, {"type": "U"}], "propertyNames": [P], "relationshipNames": [R]}]));
        let rows = rows_of(&information, &keys()).expect("the rows build");
        assert_eq!(rows.len(), 2);
        for (selector, row) in rows.iter().enumerate() {
            assert_eq!(row.selector_ordinal, i32::try_from(selector).expect("a small position"));
            assert_eq!(
                (&row.property_name_keys, &row.relationship_name_keys),
                (&Some(key_list(&[11])), &Some(key_list(&[13])))
            );
        }
        assert_eq!(pattern_forms_revision(&rows), None, "no row carries a pattern");
    }

    #[test]
    fn an_item_without_entities_is_one_row_without_entity_columns() {
        let information = information(json!([{"propertyNames": [P]}]));
        assert_eq!(
            rows_of(&information, &keys()).expect("the rows build"),
            vec![InformationRow {
                item_ordinal: 0,
                selector_ordinal: 0,
                selector: None,
                property_name_keys: Some(key_list(&[11])),
                relationship_name_keys: None,
            }]
        );
    }

    /// Items are disjunctive (clause 5.12), and an item constraining no entity
    /// is stored like any other: it does not swallow its selective siblings.
    #[test]
    fn an_unconstrained_item_keeps_its_selective_siblings() {
        let information = information(json!([{"propertyNames": [P]}, {"entities": [{"type": "T"}]}]));
        let rows = rows_of(&information, &keys()).expect("the rows build");
        assert_eq!(rows.iter().map(|row| row.item_ordinal).collect::<Vec<_>>(), vec![0, 1]);
        assert!(rows[0].selector.is_none());
        assert!(rows[1].selector.is_some());
    }

    #[test]
    fn several_items_keep_their_order() {
        let information = information(json!([{"entities": [{"type": "A"}]}, {"entities": [{"type": "B"}]}, {"entities": [{"type": "C"}]}]));
        let rows = rows_of(&information, &keys()).expect("the rows build");
        let order: Vec<(i32, &str)> = rows
            .iter()
            .map(|row| (row.item_ordinal, row.selector.as_ref().expect("a selector row").types[0]))
            .collect();
        assert_eq!(order, vec![(0, "A"), (1, "B"), (2, "C")]);
    }

    #[test]
    fn rows_assemble_to_the_information_written() {
        for case in every_case() {
            let written = information(case);
            let rows = rows_of(&written, &keys()).expect("the rows build");
            let read = information_of(&rows, &names()).expect("the rows assemble");
            assert_eq!(rendered(&read), rendered(&written));
        }
    }

    #[test]
    fn rows_in_any_order_assemble_alike() {
        for case in every_case() {
            let written = information(case);
            let mut rows = rows_of(&written, &keys()).expect("the rows build");
            rows.reverse();
            let rows = rows;
            let read = information_of(&rows, &names()).expect("the rows assemble");
            assert_eq!(rendered(&read), rendered(&written));
        }
    }

    #[test]
    fn an_ordinal_gap_is_refused() {
        let information = information(json!([{"entities": [{"type": "A"}]}, {"entities": [{"type": "B"}]}, {"entities": [{"type": "C"}]}]));
        let rows: Vec<InformationRow<'_>> = rows_of(&information, &keys())
            .expect("the rows build")
            .into_iter()
            .filter(|row| row.item_ordinal != 1)
            .collect();
        assert!(matches!(
            information_of(&rows, &names()),
            Err(StoredInformationError::OrdinalGap {
                item_ordinal: 1,
                selector_ordinal: 0
            })
        ));
        assert!(matches!(
            information_of(&[], &names()),
            Err(StoredInformationError::OrdinalGap {
                item_ordinal: 0,
                selector_ordinal: 0
            })
        ));
    }

    #[test]
    fn an_item_whose_rows_disagree_on_names_is_refused() {
        let information = information(json!([{"entities": [{"type": "T"}, {"type": "U"}], "propertyNames": [P]}]));
        let mut rows = rows_of(&information, &keys()).expect("the rows build");
        rows[1].property_name_keys = Some(key_list(&[12]));
        let rows = rows;
        assert!(matches!(
            information_of(&rows, &names()),
            Err(StoredInformationError::NamesDisagree { item_ordinal: 0 })
        ));
    }

    #[test]
    fn a_name_key_without_a_name_is_refused() {
        let information = information(json!([{"propertyNames": [P]}]));
        let mut rows = rows_of(&information, &keys()).expect("the rows build");
        rows[0].property_name_keys = Some(key_list(&[99]));
        let rows = rows;
        assert!(matches!(
            information_of(&rows, &names()),
            Err(StoredInformationError::UnknownNameKey(key)) if key == AttributeNameKey::from(99)
        ));
    }

    #[test]
    fn the_names_to_intern_are_every_declared_name_once() {
        let information = information(json!([
            {"propertyNames": [P, Q], "relationshipNames": [R]},
            {"entities": [{"type": "T"}], "propertyNames": [Q]},
            {"relationshipNames": [R, P]}
        ]));
        assert_eq!(names_to_intern(&information), vec![name(P), name(Q), name(R)]);
        let rows = rows_of(&information, &keys()).expect("the rows build");
        assert_eq!(
            held_keys(&rows),
            vec![AttributeNameKey::from(11), AttributeNameKey::from(12), AttributeNameKey::from(13)]
        );
    }

    /// The write interns every declared name before it builds the rows, so a
    /// name without a key is the store's own contract broken, not a name to
    /// drop or widen away.
    #[test]
    fn a_name_the_write_did_not_intern_is_an_invariant_breach() {
        let information = information(json!([{"propertyNames": ["https://example.org/unknown"]}]));
        assert!(matches!(
            rows_of(&information, &keys()),
            Err(StoreInvariant::InformationNameNotInterned { name }) if name.as_str() == "https://example.org/unknown"
        ));
    }
}
