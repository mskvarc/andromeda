//! The selections an Entity passes by the Attribute names it carries: `attrs`
//! (clause 5.7.2.4) and the clause 4.21 projection read as a selection.
//!
//! An Entity carries an Attribute while it holds a standing instance of it
//! (clause 4.22), which is what the existence of the Attribute in `q` reads,
//! so both selections hold where the current instances say so. The names an
//! Entity carries are also recorded as dictionary keys on its search row,
//! which the GIN index over them answers, but a name stays listed there after
//! its last instance's `expiresAt` passes, until the sweep rewrites the
//! Entity: the search row offers the candidates of a selection, and each
//! selection conjoins its test of the search row, which PostgreSQL evaluates
//! before the instance probe it prices higher, with the probe that decides.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    filter::reading::filter_reading::FilterReading,
    projection::projected_members::{ProjectedMembers, ScopeRetention},
};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    q_predicate::{instance_scan::InstanceScan, projection::compile, projection_probe::exists_besides_on_projection},
    query_names::QueryNames,
};
use andromeda_postgresql_schema::current::EntitySearch;
use andromeda_postgresql_seaquery::{
    literal::false_literal,
    operator::{array_contained_by_keys, array_contains_key},
};
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use iri_rs::IriBuf;
use sea_query::{Condition, ExprTrait};

/// The `attrs` selection: the Entity carries at least one of the named
/// Attributes, read as the existence terms `attrs` stands for
/// ([`FilterReading::carrying_any`]), beside its candidates. `None` when
/// `attrs` names none.
pub(crate) fn attrs_condition(selection: &EntitySelection, names: &QueryNames, rows: &EntityRowRefs) -> Option<Condition> {
    let carriers = attrs_carriers(selection, names, rows)?;
    Some(carriers.add(carrying_any(selection.attrs.iter().map(IriBuf::as_str), names, rows)))
}

/// The candidates of the `attrs` selection: the Entities whose search row
/// names one of the Attributes, which every Entity the selection keeps is,
/// and which the page rechecks. `None` when `attrs` names none.
pub(crate) fn attrs_carriers(selection: &EntitySelection, names: &QueryNames, rows: &EntityRowRefs) -> Option<Condition> {
    (!selection.attrs.is_empty()).then(|| {
        let listed = selection.attrs.iter().fold(Condition::any(), |carried, attr| {
            carried.add(match names.attributes.get_str(attr.as_str()) {
                Some(key) => array_contains_key(rows.search_column(EntitySearch::AttributeNameKeys), key.get()),
                None => false_literal(),
            })
        });
        Condition::all().add(listed)
    })
}

/// The clause 4.21 projection read as a selection: the entity keeps a member
/// the projection leaves it ([`ProjectedMembers::admits`]). `None` when every
/// entity does.
///
/// A `pick` keeps an entity carrying one of the Attributes it lists, read as
/// their existence terms, and an `omit` one carrying an Attribute outside
/// those it sets apart, each beside the search row's test that names its
/// candidates: a name the pick lists, or one outside the omit's, whose member
/// keys are then not contained by them. A name the dictionary never recorded
/// is carried by no entity, so it neither keeps one nor has to be set apart.
/// The `scope` the projection keeps is read off the search row's scope paths.
pub(crate) fn projected_member_condition(projected: &ProjectedMembers, names: &QueryNames, rows: &EntityRowRefs) -> Option<Condition> {
    let attribute_keys = || rows.search_column(EntitySearch::AttributeNameKeys);
    let (attribute, scope) = match projected {
        ProjectedMembers::Unconditional => return None,
        ProjectedMembers::AnyOf { scope, .. } => {
            let listed = projected
                .names()
                .filter_map(|name| names.attributes.get_str(name))
                .map(|key| array_contains_key(attribute_keys(), key.get()))
                .fold(Condition::any().add(false_literal()), Condition::add);
            (Condition::all().add(listed).add(carrying_any(projected.names(), names, rows)), *scope)
        }
        ProjectedMembers::AnyBesides { scope, .. } => {
            let set_apart: Vec<AttributeNameKey> = projected.names().filter_map(|name| names.attributes.get_str(name)).collect();
            let keys: Vec<i32> = set_apart.iter().map(|key| key.get()).collect();
            let listed = array_contained_by_keys(attribute_keys(), &keys).not();
            (Condition::all().add(listed).add(exists_besides_on_projection(&set_apart, rows)), *scope)
        }
    };
    Some(match scope {
        ScopeRetention::Kept => Condition::any().add(attribute).add(rows.search_column(EntitySearch::ScopePaths).is_not_null()),
        ScopeRetention::Removed => attribute,
    })
}

/// Whether the Entity `rows` names carries a standing instance of one of the
/// Attributes `attributes` names, an IRI that is no Attribute name naming
/// none, as `q` reads their existence.
fn carrying_any<'n>(attributes: impl Iterator<Item = &'n str>, names: &QueryNames, rows: &EntityRowRefs) -> Condition {
    let reading = FilterReading::carrying_any(attributes.filter_map(|name| name.parse::<AttributeName>().ok()));
    compile(&reading, &InstanceScan::Current(rows), names)
}
