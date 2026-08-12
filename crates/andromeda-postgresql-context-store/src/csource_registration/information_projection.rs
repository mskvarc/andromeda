//! The routing projection derived from a registration's `information`.
//!
//! A registration's `information` (clause 5.2.9) is stored as one JSONB
//! document because that document is the authority for every response body.
//! Discovery asks the opposite question -- "which registrations could serve
//! this entity type, identifier or attribute" -- and no ordinary index answers
//! it over a nested document. `registration_information` is the flattened form
//! that does: one row per selective capability, rewritten by this module inside
//! the same transaction as every registration mutation. Nothing in the database
//! derives it; there is no trigger parsing the document, because a trigger would
//! own domain semantics that belong in the broker.
//!
//! # Superset preselection
//!
//! The projection narrows candidates and decides nothing. The typed evaluation
//! over the registrations it preselects remains the oracle for membership, so
//! every derivation choice below resolves ambiguity towards the *wider* row: a
//! row that admits registrations which turn out not to match costs residual CPU,
//! while a row that omits one that does silently drops a context source from
//! distributed operations.
//!
//! # The unconstrained registration
//!
//! `information` entries are disjunctive: a registration serves the union of
//! what its entries declare. An entry that constrains no axis therefore makes
//! the whole registration unconstrained, and so does an empty `information`
//! array. Such a registration is represented by *no rows at all*
//! ([`RegistrationRouting::Unconstrained`]), and a reader must treat a
//! registration without projection rows as a candidate for every request. This
//! is what lets the selector-presence constraint hold on every row that does
//! exist: a stored row always carries at least one selector dimension.
//!
//! # The table this module writes
//!
//! ```sql
//! CREATE TABLE registration_information (
//!     id                     BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
//!     registration_id        TEXT COLLATE "C" NOT NULL
//!                                REFERENCES context_source_registrations (id) ON DELETE CASCADE,
//!     entity_types           TEXT[] COLLATE "C",
//!     entity_id              TEXT COLLATE "C",
//!     entity_id_pattern      TEXT,
//!     property_name_ids      INTEGER[],
//!     relationship_name_ids  INTEGER[],
//!     operations             SMALLINT[],
//!     CHECK (entity_types IS NOT NULL OR entity_id IS NOT NULL OR entity_id_pattern IS NOT NULL
//!            OR property_name_ids IS NOT NULL OR relationship_name_ids IS NOT NULL)
//! );
//! ```
//!
//! Migration `0064_registration_information` declares it, with the storage
//! parameters, the per-column population checks and the index set that belong to
//! the schema rather than to this module; nothing here may name a column that
//! file does not.
//!
//! The row has no natural key. It is never referenced, never updated in place
//! and always replaced as a set, so a surrogate the database hands out is the
//! whole of its identity; carrying the registration's own `information` and
//! selector positions would add columns nothing reads. The parent key is the
//! registration URN itself rather than a surrogate: registrations are
//! catalogue-scale and each yields a handful of rows, so interning the URN would
//! buy no measurable width while forcing every mutation through an extra round
//! trip to fetch the key.
//!
//! Attribute names are interned through the `attribute_names` dictionary, whose
//! `INTEGER` keys the rest of the schema already indexes on. Entity identifiers
//! and identifier patterns stay text at the persistence boundary: an identifier
//! is matched for equality and a pattern is compiled by PostgreSQL, and neither
//! is a closed vocabulary worth a dictionary. Operations are stored as the
//! `SMALLINT[]` of their migration-owned registry numbers rather than as a
//! bitmask: discovery asks whether a registration declares an operation *or one
//! of the clause 4.20 groups covering it*, which is an array overlap (`&&`) a
//! GIN index serves directly, and an array keeps admitting members past the
//! sixty-four a bitmask word would cap the registry at.
//!
//! Operations are a registration-wide member (clause 5.2.9), replicated onto
//! every row of that registration so one indexed row carries every dimension a
//! preselect tests. A registration with no rows constrains no operation either,
//! which is consistent: it constrains nothing at all.

use crate::{csource_registration::operation_code::stored_operation, error::ContextStoreError};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    registration::{CsrOperation, RegistrationInfo},
    subscription::EntitySelector,
};
use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
use andromeda_postgresql_attribute_dictionary::attribute_name_lookup::AttributeNameLookup;
use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
use sqlx::{PgConnection, QueryBuilder, postgres::Postgres};
use std::collections::HashMap;

/// One row of the routing projection.
///
/// Every field is a dimension a discovery predicate may narrow on, and every
/// one of them is optional: `None` means the capability places no constraint on
/// that axis and the column is stored `NULL`. At least one is always `Some`,
/// which is the invariant the table's selector-presence constraint states.
#[derive(Debug)]
pub struct RegistrationInformationRow<'info> {
    /// Entity type names the selector admits, or `None` where it admits any.
    ///
    /// A clause 4.17 type expression is flattened to the set of names it
    /// mentions. That widens a conjunction such as `(Building;Tower)`, which
    /// requires both, into a row matched by either -- permitted, because
    /// preselection may only over-approximate, and the typed expression itself
    /// still decides.
    pub entity_types: Option<Vec<&'info EntityTypeName>>,
    /// The exact entity identifier the selector names, if it names one.
    pub entity_id: Option<&'info Uri>,
    /// The identifier pattern the selector carries, if it carries one.
    pub entity_id_pattern: Option<&'info Pattern>,
    /// Dictionary keys of the entry's `propertyNames`, if it declares any.
    pub property_name_ids: Option<Vec<AttributeNameId>>,
    /// Dictionary keys of the entry's `relationshipNames`, if it declares any.
    pub relationship_name_ids: Option<Vec<AttributeNameId>>,
}

/// The projection of one registration's whole `information` document.
/// A [`Pattern`] compiles to a regular expression, which has no equality, so
/// this pair of types cannot derive `PartialEq`; tests compare the dimensions
/// they are about.
#[derive(Debug)]
pub enum RegistrationRouting<'info> {
    /// The registration constrains nothing and is a candidate for every
    /// request, so it is represented by no rows.
    Unconstrained,
    /// The registration's selective capabilities, one row each.
    ///
    /// Never empty: an entry that produced no constraint would have made the
    /// whole registration [`RegistrationRouting::Unconstrained`].
    Selective(Vec<RegistrationInformationRow<'info>>),
}

impl<'info> RegistrationRouting<'info> {
    /// The rows to store, which is nothing for an unconstrained registration.
    pub fn rows(&self) -> &[RegistrationInformationRow<'info>] {
        match self {
            RegistrationRouting::Unconstrained => &[],
            RegistrationRouting::Selective(rows) => rows,
        }
    }
}

/// The attribute names a registration's `information` mentions, deduplicated.
///
/// These are the names the dictionary has to hold keys for before the
/// projection can be written. They arrive already expanded to IRIs: JSON-LD
/// expansion happens in the service layer, above the store.
pub fn projected_attribute_names(information: &[RegistrationInfo]) -> Vec<AttributeName> {
    let mut names: Vec<AttributeName> = Vec::new();
    for entry in information {
        let declared = entry.property_names.iter().chain(entry.relationship_names.iter()).flatten();
        for name in declared {
            if !names.contains(name) {
                names.push(name.clone()); // clone: the resolver takes an owned, validated name.
            }
        }
    }
    names
}

/// Derive the routing rows of a registration from its typed `information`.
///
/// `ids` is the dictionary resolution of [`projected_attribute_names`]. A name
/// missing from it widens the entry's whole attribute constraint away -- both
/// the property and the relationship axis -- rather than narrowing a row to the
/// names that did resolve, which would drop a registration that genuinely serves
/// the missing one.
pub fn derive_routing<'info>(information: &'info [RegistrationInfo], ids: &HashMap<AttributeName, AttributeNameId>) -> RegistrationRouting<'info> {
    let mut rows = Vec::new();
    for entry in information {
        match entry_rows(entry, ids) {
            Some(entry_rows) => rows.extend(entry_rows),
            None => return RegistrationRouting::Unconstrained,
        }
    }
    if rows.is_empty() {
        return RegistrationRouting::Unconstrained;
    }
    RegistrationRouting::Selective(rows)
}

/// The rows one `information` entry contributes, or `None` where the entry
/// constrains no axis and so makes the whole registration unconstrained.
fn entry_rows<'info>(entry: &'info RegistrationInfo, ids: &HashMap<AttributeName, AttributeNameId>) -> Option<Vec<RegistrationInformationRow<'info>>> {
    let (properties, relationships) = entry_names(entry, ids);
    let selectors = entry.entities.as_deref().filter(|selectors| !selectors.is_empty());

    match selectors {
        Some(selectors) => {
            let rows: Vec<RegistrationInformationRow<'info>> = selectors
                .iter()
                .map(|selector| selector_row(selector, properties.clone(), relationships.clone())) // clone: each row owns its own copy of the entry-wide key set.
                .collect();
            // A selector always carries a `type` (clause 5.2.33), so a row built
            // from one constrains at least the type axis unless that type is the
            // clause 4.17 wildcard. The selectors of an entry are alternatives,
            // so one of them constraining nothing makes the whole entry -- and
            // with it the registration -- unconstrained; it is also what would
            // otherwise offer the table a row carrying no selector at all.
            if rows.iter().any(|row| !row.is_selective()) { None } else { Some(rows) }
        }
        None => {
            let row = RegistrationInformationRow {
                entity_types: None,
                entity_id: None,
                entity_id_pattern: None,
                property_name_ids: properties,
                relationship_name_ids: relationships,
            };
            if row.is_selective() { Some(vec![row]) } else { None }
        }
    }
}

/// One row for one entity selector, carrying its entry's attribute constraints.
fn selector_row<'info>(
    selector: &'info EntitySelector,
    property_name_ids: Option<Vec<AttributeNameId>>,
    relationship_name_ids: Option<Vec<AttributeNameId>>,
) -> RegistrationInformationRow<'info> {
    RegistrationInformationRow {
        entity_types: selected_type_names(&selector.type_),
        entity_id: selector.id.as_ref(),
        entity_id_pattern: selector.id_pattern.as_ref(),
        property_name_ids,
        relationship_name_ids,
    }
}

impl RegistrationInformationRow<'_> {
    /// Whether the row constrains any axis, which is what makes it storable.
    const fn is_selective(&self) -> bool {
        self.entity_types.is_some()
            || self.entity_id.is_some()
            || self.entity_id_pattern.is_some()
            || self.property_name_ids.is_some()
            || self.relationship_name_ids.is_some()
    }
}

/// The dictionary resolution of one declared attribute-name list.
#[derive(Debug)]
enum DeclaredNames {
    /// The entry declared no name on this axis.
    Absent,
    /// Every declared name resolved to a dictionary key.
    Interned(Vec<AttributeNameId>),
    /// At least one declared name is not in the dictionary.
    Unresolved,
}

/// The key arrays of an entry's `propertyNames` and `relationshipNames`.
///
/// A name that does not resolve widens *both* axes to unconstrained rather than
/// only its own. A reader tells "declared nothing here" from "declared something
/// that could not be stored" by nothing but these two columns, so leaving one of
/// them populated beside a widened one would make the pair read as a complete,
/// narrower declaration than the registration made -- and a preselect over it
/// would drop a registration that genuinely serves the unresolved name.
fn entry_names(entry: &RegistrationInfo, ids: &HashMap<AttributeName, AttributeNameId>) -> (Option<Vec<AttributeNameId>>, Option<Vec<AttributeNameId>>) {
    let properties = interned_names(entry.property_names.as_deref(), ids);
    let relationships = interned_names(entry.relationship_names.as_deref(), ids);
    match (properties, relationships) {
        (DeclaredNames::Unresolved, _) | (_, DeclaredNames::Unresolved) => (None, None),
        (properties, relationships) => (stored_keys(properties), stored_keys(relationships)),
    }
}

/// The column value one resolved axis stores.
fn stored_keys(names: DeclaredNames) -> Option<Vec<AttributeNameId>> {
    match names {
        DeclaredNames::Absent | DeclaredNames::Unresolved => None,
        DeclaredNames::Interned(keys) => Some(keys),
    }
}

/// The dictionary keys of a declared attribute-name list.
fn interned_names(names: Option<&[AttributeName]>, ids: &HashMap<AttributeName, AttributeNameId>) -> DeclaredNames {
    match names.filter(|names| !names.is_empty()) {
        None => DeclaredNames::Absent,
        Some(names) => match names.iter().map(|name| ids.get(name).copied()).collect() {
            Some(keys) => DeclaredNames::Interned(keys),
            None => DeclaredNames::Unresolved,
        },
    }
}

/// The entity type names a clause 4.17 expression mentions, or `None` where it
/// admits every type.
///
/// A wildcard anywhere in the expression widens the whole row to unconstrained.
/// For a disjunction that is exact; for a conjunction it is wider than the
/// expression, which preselection permits.
fn selected_type_names(expression: &TypeExprRaw) -> Option<Vec<&EntityTypeName>> {
    match expression {
        TypeExprRaw::Wildcard => None,
        TypeExprRaw::Type(name) => Some(vec![name]),
        TypeExprRaw::And(children) | TypeExprRaw::Or(children) => {
            let mut names = Vec::new();
            for child in children {
                names.extend(selected_type_names(child)?);
            }
            if names.is_empty() { None } else { Some(names) }
        }
    }
}

/// Replace a registration's routing rows with the ones its `information` yields.
///
/// Runs on the caller's connection so that it joins the transaction that writes
/// the registration payload: the projection and the document it is derived from
/// become visible together or not at all. A failure here aborts the mutation.
///
/// The rewrite is wholesale -- one delete of the registration's rows followed by
/// one insert of the derived set -- because an update carries a resolved value
/// set rather than a fragment, so rows for capabilities the update dropped must
/// not survive it.
pub async fn replace_registration_information(
    connection: &mut PgConnection,
    registration: &Uri,
    information: &[RegistrationInfo],
    operations: Option<&[CsrOperation]>,
    attribute_names: &dyn AttributeNameLookup,
) -> Result<(), ContextStoreError> {
    // Resolved on the caller's connection, so the dictionary rows this mints
    // belong to the transaction that writes the registration payload: the
    // routing projection and the names it is keyed by land together or not at
    // all.
    let ids = attribute_names
        .resolve_or_create(&mut *connection, &projected_attribute_names(information))
        .await?;
    let routing = derive_routing(information, &ids);

    sqlx::query("DELETE FROM registration_information WHERE registration_id = $1")
        .bind(registration.as_str())
        .execute(&mut *connection)
        .await?;

    let rows = routing.rows();
    if rows.is_empty() {
        return Ok(());
    }

    let operation_codes: Option<Vec<i16>> = operations
        .filter(|operations| !operations.is_empty())
        .map(|operations| operations.iter().map(|operation| stored_operation(*operation).code()).collect());

    let mut insert: QueryBuilder<'_, Postgres> = QueryBuilder::new(
        "INSERT INTO registration_information \
         (registration_id, entity_types, entity_id, entity_id_pattern, property_name_ids, relationship_name_ids, operations) ",
    );
    insert.push_values(rows, |mut values, row| {
        let entity_types = row
            .entity_types
            .as_ref()
            .map(|names| names.iter().map(|name| name.as_str()).collect::<Vec<&str>>());
        let property_name_ids = row.property_name_ids.as_ref().map(|ids| ids.iter().map(|id| id.get()).collect::<Vec<i32>>());
        let relationship_name_ids = row
            .relationship_name_ids
            .as_ref()
            .map(|ids| ids.iter().map(|id| id.get()).collect::<Vec<i32>>());
        // Every array bind carries its cast: a `NULL` array parameter has no
        // inferable element type, and PostgreSQL refuses the statement without
        // one.
        values.push_bind(registration.as_str());
        values.push_bind(entity_types).push_unseparated("::text[]");
        values.push_bind(row.entity_id.map(Uri::as_str));
        values.push_bind(row.entity_id_pattern.map(Pattern::as_str));
        values.push_bind(property_name_ids).push_unseparated("::integer[]");
        values.push_bind(relationship_name_ids).push_unseparated("::integer[]");
        // clone: each row binds its own copy of the registration-wide set.
        values.push_bind(operation_codes.clone()).push_unseparated("::smallint[]");
    });
    insert.build().execute(connection).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::information_projection::{RegistrationInformationRow, RegistrationRouting, derive_routing, projected_attribute_names};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
    use andromeda_ngsi_ld_model::{registration::RegistrationInfo, subscription::EntitySelector};
    use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
    use std::collections::HashMap;

    /// An expanded attribute IRI, as the service layer hands one to the store.
    const SPEED: &str = "https://example.com/speed";
    /// A second expanded attribute IRI.
    const DRIVER: &str = "https://example.com/driver";

    fn name(iri: &str) -> AttributeName {
        iri.parse().expect("valid attribute name")
    }

    fn dictionary() -> HashMap<AttributeName, AttributeNameId> {
        HashMap::from([(name(SPEED), AttributeNameId::from(11)), (name(DRIVER), AttributeNameId::from(12))])
    }

    fn type_expression(raw: &str) -> TypeExprRaw {
        raw.parse().expect("valid type expression")
    }

    fn selector(raw_type: &str) -> EntitySelector {
        EntitySelector {
            id: None,
            id_pattern: None,
            type_: type_expression(raw_type),
        }
    }

    fn entry(entities: Option<Vec<EntitySelector>>, properties: Option<Vec<&str>>, relationships: Option<Vec<&str>>) -> RegistrationInfo {
        RegistrationInfo {
            entities,
            property_names: properties.map(|iris| iris.into_iter().map(name).collect()),
            relationship_names: relationships.map(|iris| iris.into_iter().map(name).collect()),
        }
    }

    /// Whether a derivation left the registration matching every request.
    fn unconstrained(routing: &RegistrationRouting<'_>) -> bool {
        routing.rows().is_empty()
    }

    fn rows_of<'info>(information: &'info [RegistrationInfo], ids: &HashMap<AttributeName, AttributeNameId>) -> Vec<RegistrationInformationRow<'info>> {
        match derive_routing(information, ids) {
            RegistrationRouting::Unconstrained => unreachable!("the fixture constrains at least one axis"),
            RegistrationRouting::Selective(rows) => rows,
        }
    }

    #[test]
    fn a_type_only_selector_projects_one_row_constraining_the_type_alone() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), None, None)];
        let rows = rows_of(&information, &dictionary());
        pretty_assertions::assert_eq!(rows.len(), 1);
        pretty_assertions::assert_eq!(rows[0].entity_types.as_ref().map(|names| names.len()), Some(1));
        assert!(rows[0].entity_id.is_none());
        assert!(rows[0].entity_id_pattern.is_none());
        assert!(rows[0].property_name_ids.is_none());
        assert!(rows[0].relationship_name_ids.is_none());
    }

    #[test]
    fn an_exact_identifier_selector_projects_the_identifier_alongside_its_type() {
        let identifier: Uri = "urn:ngsi-ld:Vehicle:A1".parse().expect("valid URI");
        let information = vec![entry(
            Some(vec![EntitySelector {
                id: Some(identifier),
                id_pattern: None,
                type_: type_expression("Vehicle"),
            }]),
            None,
            None,
        )];
        let rows = rows_of(&information, &dictionary());
        pretty_assertions::assert_eq!(rows[0].entity_id.map(Uri::as_str), Some("urn:ngsi-ld:Vehicle:A1"));
        assert!(rows[0].entity_types.is_some());
    }

    #[test]
    fn an_identifier_pattern_selector_projects_the_pattern_as_written() {
        let information = vec![entry(
            Some(vec![EntitySelector {
                id: None,
                id_pattern: Some(Pattern::parse("^urn:ngsi-ld:Vehicle:.*$").expect("valid pattern")),
                type_: type_expression("Vehicle"),
            }]),
            None,
            None,
        )];
        let rows = rows_of(&information, &dictionary());
        pretty_assertions::assert_eq!(rows[0].entity_id_pattern.map(Pattern::as_str), Some("^urn:ngsi-ld:Vehicle:.*$"));
    }

    #[test]
    fn declared_property_and_relationship_names_project_as_dictionary_keys() {
        let information = vec![entry(None, Some(vec![SPEED]), Some(vec![DRIVER]))];
        let rows = rows_of(&information, &dictionary());
        pretty_assertions::assert_eq!(rows.len(), 1);
        pretty_assertions::assert_eq!(rows[0].property_name_ids, Some(vec![AttributeNameId::from(11)]));
        pretty_assertions::assert_eq!(rows[0].relationship_name_ids, Some(vec![AttributeNameId::from(12)]));
        assert!(rows[0].entity_types.is_none(), "an entry without entities constrains no entity axis");
    }

    #[test]
    fn an_entry_combining_selectors_and_names_repeats_the_names_on_every_selector_row() {
        let information = vec![entry(Some(vec![selector("Vehicle"), selector("Building")]), Some(vec![SPEED]), None)];
        let rows = rows_of(&information, &dictionary());
        pretty_assertions::assert_eq!(rows.len(), 2);
        for row in &rows {
            pretty_assertions::assert_eq!(row.property_name_ids, Some(vec![AttributeNameId::from(11)]));
        }
    }

    #[test]
    fn several_information_entries_each_contribute_their_own_rows() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), None, None), entry(None, Some(vec![SPEED, DRIVER]), None)];
        let rows = rows_of(&information, &dictionary());
        pretty_assertions::assert_eq!(rows.len(), 2);
        pretty_assertions::assert_eq!(rows[1].property_name_ids, Some(vec![AttributeNameId::from(11), AttributeNameId::from(12)]));
    }

    /// Clause 4.17: an alternation selects any of its members, so the row lists
    /// them all and a request for either type preselects the registration.
    #[test]
    fn a_type_alternation_projects_every_name_it_mentions() {
        let information = vec![entry(Some(vec![selector("(Building,Tower)")]), None, None)];
        let rows = rows_of(&information, &dictionary());
        let names = rows[0].entity_types.as_ref().expect("an alternation constrains the type axis");
        pretty_assertions::assert_eq!(names.iter().map(|name| name.as_str()).collect::<Vec<&str>>(), vec!["Building", "Tower"]);
    }

    /// The clause 4.17 wildcard admits every type, so the type axis carries no
    /// constraint and the row keeps only what the rest of the entry declares.
    #[test]
    fn a_wildcard_type_leaves_the_type_axis_unconstrained() {
        let information = vec![entry(Some(vec![selector("*")]), Some(vec![SPEED]), None)];
        let rows = rows_of(&information, &dictionary());
        assert!(rows[0].entity_types.is_none());
        pretty_assertions::assert_eq!(rows[0].property_name_ids, Some(vec![AttributeNameId::from(11)]));
    }

    /// A wildcard selector with nothing else to constrain leaves the entry, and
    /// with it the registration, matching everything.
    #[test]
    fn a_wildcard_selector_alone_makes_the_registration_unconstrained() {
        let information = vec![entry(Some(vec![selector("*")]), None, None)];
        assert!(unconstrained(&derive_routing(&information, &dictionary())));
    }

    /// Entries are disjunctive: one entry that constrains nothing makes the
    /// whole registration a candidate for every request, so the selective
    /// entries beside it must not be projected as if they bounded it.
    #[test]
    fn an_unconstrained_entry_suppresses_the_rows_of_its_selective_siblings() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), None, None), entry(None, None, None)];
        assert!(unconstrained(&derive_routing(&information, &dictionary())));
    }

    #[test]
    fn a_registration_without_information_is_unconstrained() {
        assert!(unconstrained(&derive_routing(&[], &dictionary())));
    }

    /// An empty `entities` array bounds nothing, exactly as an absent one does.
    #[test]
    fn an_empty_entity_selector_list_constrains_no_entity_axis() {
        let information = vec![entry(Some(Vec::new()), Some(vec![SPEED]), None)];
        let rows = rows_of(&information, &dictionary());
        pretty_assertions::assert_eq!(rows.len(), 1);
        assert!(rows[0].entity_types.is_none());
        pretty_assertions::assert_eq!(rows[0].property_name_ids, Some(vec![AttributeNameId::from(11)]));
    }

    /// A name the dictionary does not hold widens the attribute axis instead of
    /// narrowing it to the names that did resolve: preselection may return a
    /// registration that does not match, never omit one that does.
    #[test]
    fn an_unresolved_attribute_name_widens_the_attribute_axis_rather_than_narrowing_it() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), Some(vec![SPEED, "https://example.com/unknown"]), None)];
        let rows = rows_of(&information, &dictionary());
        assert!(rows[0].property_name_ids.is_none());
        assert!(rows[0].entity_types.is_some(), "the entity axis is unaffected");
    }

    /// An unresolved name on one axis widens the other one with it, so the pair
    /// of columns never reads as a complete declaration narrower than the one
    /// the registration made.
    #[test]
    fn an_unresolved_property_name_widens_the_relationship_axis_as_well() {
        let information = vec![entry(
            Some(vec![selector("Vehicle")]),
            Some(vec![SPEED, "https://example.com/unknown"]),
            Some(vec![DRIVER]),
        )];
        let rows = rows_of(&information, &dictionary());
        assert!(rows[0].property_name_ids.is_none());
        assert!(rows[0].relationship_name_ids.is_none());
        assert!(rows[0].entity_types.is_some(), "the entity axis is unaffected");
    }

    /// The selectors of one entry are alternatives, so a wildcard among them
    /// admits every type whatever its siblings name -- and a row carrying no
    /// selector at all is not a row the table would accept.
    #[test]
    fn a_wildcard_selector_beside_a_named_one_makes_the_registration_unconstrained() {
        let information = vec![entry(Some(vec![selector("*"), selector("Vehicle")]), None, None)];
        assert!(unconstrained(&derive_routing(&information, &dictionary())));
    }

    /// The same entry with declared names constrains the attribute axis on every
    /// one of its rows, so the wildcard selector still projects a storable row.
    #[test]
    fn a_wildcard_selector_beside_declared_names_projects_a_row_constrained_by_the_names() {
        let information = vec![entry(Some(vec![selector("*"), selector("Vehicle")]), Some(vec![SPEED]), None)];
        let rows = rows_of(&information, &dictionary());
        pretty_assertions::assert_eq!(rows.len(), 2);
        assert!(rows[0].entity_types.is_none());
        pretty_assertions::assert_eq!(rows[0].property_name_ids, Some(vec![AttributeNameId::from(11)]));
    }

    /// The same widening, applied to an entry whose only constraint was the
    /// unresolved name.
    #[test]
    fn an_entry_constrained_only_by_an_unresolved_name_makes_the_registration_unconstrained() {
        let information = vec![entry(None, Some(vec!["https://example.com/unknown"]), None)];
        assert!(unconstrained(&derive_routing(&information, &dictionary())));
    }

    #[test]
    fn the_names_to_intern_are_every_declared_name_without_repetition() {
        let information = vec![entry(None, Some(vec![SPEED]), Some(vec![DRIVER])), entry(None, Some(vec![SPEED]), None)];
        pretty_assertions::assert_eq!(projected_attribute_names(&information), vec![name(SPEED), name(DRIVER)]);
    }

    #[test]
    fn a_registration_without_declared_names_interns_nothing() {
        let information = vec![entry(Some(vec![selector("Vehicle")]), None, None)];
        assert!(projected_attribute_names(&information).is_empty());
    }
}
