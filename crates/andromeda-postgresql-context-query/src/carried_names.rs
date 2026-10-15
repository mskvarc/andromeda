//! The selections an Entity passes by the Attribute names it carries: `attrs`
//! (clause 5.7.2.4) and the clause 4.21 projection read as a selection.
//!
//! The names an Entity carries are recorded as dictionary keys on its search
//! row, so each name is a containment test the GIN index over them answers; a
//! name the dictionary has never recorded is carried by no Entity.

use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    projection::projected_members::{ProjectedMembers, ScopeRetention},
};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::query_names::QueryNames, entity_row_refs::EntityRowRefs};
use andromeda_postgresql_schema::current::EntitySearch;
use andromeda_postgresql_seaquery::{
    literal::false_literal,
    operator::{array_contained_by_keys, array_contains_key},
    pg_array::has_elements,
};
use sea_query::{Condition, ExprTrait};

/// The `attrs` selection: the Entity carries at least one of the named
/// Attributes. `None` when `attrs` names none.
pub(crate) fn attrs_condition(selection: &EntitySelection, names: &QueryNames, rows: &EntityRowRefs) -> Option<Condition> {
    (!selection.attrs.is_empty()).then(|| {
        selection.attrs.iter().fold(Condition::any(), |carried, attr| {
            carried.add(match names.attributes.get_str(attr.as_str()) {
                Some(key) => array_contains_key(rows.search_column(EntitySearch::AttributeNameKeys), key.get()),
                None => false_literal(),
            })
        })
    })
}

/// The clause 4.21 projection read as a selection: the entity keeps a member
/// the projection leaves it. `None` when every entity does.
///
/// The Attributes are read off the search row's interned member names, as
/// `attrs` is: a `pick` keeps an entity carrying one of the names it lists, and
/// an `omit` one carrying a name outside those it and clause 5.2.4 set apart,
/// which is the negation of the member keys being contained by them. A name the
/// dictionary never recorded is carried by no entity, so it neither keeps one
/// nor has to be set apart. The `scope` the projection keeps is read off the
/// search row's scope paths.
pub(crate) fn projected_member_condition(projected: &ProjectedMembers, names: &QueryNames, rows: &EntityRowRefs) -> Option<Condition> {
    let (attribute, scope) = match projected {
        ProjectedMembers::Unconditional => return None,
        ProjectedMembers::AnyOf { scope, .. } => {
            let carried = projected
                .names()
                .filter_map(|name| names.attributes.get_str(name))
                .map(|key| array_contains_key(rows.search_column(EntitySearch::AttributeNameKeys), key.get()))
                .fold(Condition::any().add(false_literal()), Condition::add);
            (carried, *scope)
        }
        ProjectedMembers::AnyBesides { scope, .. } => {
            let set_apart: Vec<i32> = projected
                .names()
                .filter_map(|name| names.attributes.get_str(name))
                .map(AttributeNameKey::get)
                .collect();
            let other = array_contained_by_keys(rows.search_column(EntitySearch::AttributeNameKeys), &set_apart).not();
            (Condition::all().add(other), *scope)
        }
    };
    Some(match scope {
        ScopeRetention::Kept => Condition::any().add(attribute).add(has_elements(rows.search_column(EntitySearch::ScopePaths))),
        ScopeRetention::Removed => attribute,
    })
}
