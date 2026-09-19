//! What one `RegistrationInfo` block covers of a client's query (clauses
//! 4.3.6.1, 4.3.6.3 and 5.2.10).
//!
//! Clause 4.3.6.1 makes the broker respect every constraint a registration
//! declares when it forwards a request, and clause 4.3.6.3 makes each
//! information block a self-contained coverage statement -- an exclusive block
//! names a single entity and its attributes, and a registration may carry
//! several such blocks. A forwarded request therefore respects one block at a
//! time, naming that block's entities, type and attributes, rather than the
//! union of every block, which would ask a source for a cross-product of
//! entities and attributes it never registered together.
//!
//! Registered type and attribute names are stored expanded; when a block
//! supplies its own names to a forwarded request (the client constrained none),
//! they are compacted with the same last-segment heuristic
//! (`compact_attribute_name`, `compact_type_name`) the codebase uses to match
//! a client's compact term against a stored IRI. That heuristic is exact
//! whenever a term equals the last fragment or path segment of its expanded IRI
//! -- the case for the NGSI-LD core context and typical vocabularies -- but a
//! custom `@context` that aliases a term to an unrelated IRI could compact it
//! differently than the request `@context` would. The retrieve-by-id path
//! (`crate::entity_read`) instead compacts the registered type through the
//! JSON-LD engine, which is exact; the query path stays engine-free so this
//! narrowing remains a pure function.

use crate::registration_selection::compact_attribute_name;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{entity_info::EntityTypeSelection, registration::info::RegistrationInfo};
use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};

/// The entity selection one block contributes to a forwarded request: the
/// identifiers, id pattern and type it narrows the client's selection to, or
/// supplies when the client constrained none of them.
#[derive(Debug, Default)]
pub(super) struct BlockSelection {
    /// The identifiers to write as `id`.
    pub(super) ids: Vec<Uri>,
    /// The pattern to write as `idPattern`.
    pub(super) id_pattern: Option<String>,
    /// The type selector to write as `type`.
    pub(super) entity_type: Option<TypeExprRaw>,
}

impl BlockSelection {
    /// The entity selection `block` contributes for a client that named `ids`,
    /// `id_pattern` and `entity_type`, or `None` when the block covers none of
    /// the entities the client selected.
    ///
    /// The client's identifiers and type are narrowed to what the block covers;
    /// where the client constrained neither, the block supplies its own
    /// identifiers (or its id pattern) and its declared types, so a forwarded
    /// request always names the block's footprint even when the client's query
    /// was broader (clause 4.3.6.1).
    pub(super) fn of(block: &RegistrationInfo, ids: Option<&[Uri]>, id_pattern: Option<&Pattern>, entity_type: Option<&TypeExprRaw>) -> Option<BlockSelection> {
        let mut selection = BlockSelection::default();
        if let Some(ids) = ids.filter(|ids| !ids.is_empty()) {
            selection.ids = covered_ids(ids, block)?;
            selection.id_pattern = id_pattern.map(ToString::to_string);
        } else if let Some(pattern) = id_pattern {
            selection.id_pattern = Some(pattern.to_string());
        } else {
            let (block_ids, block_pattern) = block_identifiers(block);
            if block_ids.is_empty() {
                selection.id_pattern = block_pattern;
            } else {
                selection.ids = block_ids;
            }
        }
        selection.entity_type = match entity_type {
            Some(entity_type) => Some(covered_type(entity_type, block)?),
            None => block_types(block),
        };
        Some(selection)
    }
}

/// The attribute list a block contributes to a forwarded request.
pub(super) enum AttrCoverage {
    /// Write these attribute names as `attrs`.
    Write(Vec<AttributeName>),
    /// Write no `attrs` parameter: the block constrains no attribute and the
    /// client asked for none, or its attribute filter was dropped for split
    /// entities (clause 5.7.2.4).
    Omit,
    /// The block declares attribute names and none of the ones the client asked
    /// for is among them, so the block covers nothing the client requested.
    Uncovered,
}

/// The attributes `block` contributes for a client that asked for `client_attrs`.
///
/// When the block declares names, the client's attributes are narrowed to them,
/// or -- when the client asked for none -- the block's declared names are sent
/// on their own, because they are a registration constraint rather than a
/// client filter (clause 4.3.6.1). A block declaring no names forwards the
/// client's attributes unchanged, or nothing when the client named none.
pub(super) fn block_attrs(block: &RegistrationInfo, client_attrs: Option<&[AttributeName]>) -> AttrCoverage {
    let declared = block.attribute_names().collect::<Vec<_>>();
    match client_attrs.filter(|attrs| !attrs.is_empty()) {
        Some(attrs) => {
            if declared.is_empty() {
                return AttrCoverage::Write(attrs.to_vec()); // clone: the forwarded query owns the names it writes
            }
            let covered = attrs
                .iter()
                .filter(|attribute| {
                    declared
                        .iter()
                        .any(|name| *name == *attribute || compact_attribute_name(name).as_ref() == Some(attribute))
                })
                .cloned() // clone: same
                .collect::<Vec<_>>();
            if covered.is_empty() {
                AttrCoverage::Uncovered
            } else {
                AttrCoverage::Write(covered)
            }
        }
        None if declared.is_empty() => AttrCoverage::Omit,
        // The block's declared names are stored expanded; a source expects the
        // compact term its own @context uses, so each is compacted the same way
        // a client's term is matched against it.
        None => AttrCoverage::Write(
            declared
                .into_iter()
                .map(|name| compact_attribute_name(name).unwrap_or_else(|| name.clone())) // clone: a name already compact is forwarded as written
                .collect(),
        ),
    }
}

/// Whether the block's entity selectors cover `id`; a block that names no
/// entity selectors covers every identifier (clause 5.2.10).
pub(super) fn block_covers_id(block: &RegistrationInfo, id: &Uri) -> bool {
    match block.entities.as_deref() {
        None => true,
        Some(selectors) => selectors.iter().any(|selector| selector.covers_id(id)),
    }
}

/// The identifiers among `ids` this block covers, or `None` when it covers none.
fn covered_ids(ids: &[Uri], block: &RegistrationInfo) -> Option<Vec<Uri>> {
    let covered = ids
        .iter()
        .filter(|id| block_covers_id(block, id))
        .cloned() // clone: the forwarded query owns the identifiers it writes
        .collect::<Vec<_>>();
    (!covered.is_empty()).then_some(covered)
}

/// The part of the client's type selector the block covers, or `None` when it
/// covers none of it.
///
/// Only a flat alternation of names can be narrowed; the wildcard and a
/// conjunction are forwarded whole, because removing a member would change what
/// they select rather than restrict it.
fn covered_type(entity_type: &TypeExprRaw, block: &RegistrationInfo) -> Option<TypeExprRaw> {
    let Some(selectors) = block.entities.as_deref().filter(|selectors| !selectors.is_empty()) else {
        return Some(entity_type.clone()); // clone: an unrestricted block forwards the client's selector as written
    };
    let leaves = match entity_type {
        TypeExprRaw::Type(name) => vec![name],
        TypeExprRaw::Or(children) => children
            .iter()
            .map(|child| match child {
                TypeExprRaw::Type(name) => Some(name),
                TypeExprRaw::Wildcard | TypeExprRaw::And(_) | TypeExprRaw::Or(_) => None,
            })
            .collect::<Option<Vec<_>>>()?,
        TypeExprRaw::Wildcard | TypeExprRaw::And(_) => return Some(entity_type.clone()), // clone: same
    };
    let mut covered = leaves
        .into_iter()
        .filter(|name| selectors.iter().any(|selector| selector_covers_type(&selector.type_, name)))
        .cloned() // clone: the narrowed selector owns its names
        .map(TypeExprRaw::Type)
        .collect::<Vec<_>>();
    match covered.len() {
        0 => None,
        1 => covered.pop(),
        _ => Some(TypeExprRaw::Or(covered)),
    }
}

/// The block's explicitly named identifiers, or -- when it names none -- the
/// single id pattern its entity selectors constrain identifiers by.
fn block_identifiers(block: &RegistrationInfo) -> (Vec<Uri>, Option<String>) {
    let Some(selectors) = block.entities.as_deref() else {
        return (Vec::new(), None);
    };
    let ids = selectors
        .iter()
        .filter_map(|selector| selector.id.clone()) // clone: the forwarded query owns the identifiers it writes
        .collect::<Vec<_>>();
    if !ids.is_empty() {
        return (ids, None);
    }
    let pattern = selectors.iter().find_map(|selector| selector.id_pattern.as_ref().map(ToString::to_string));
    (Vec::new(), pattern)
}

/// The block's declared types as one selector: the single type, or the
/// alternation of every distinct type its entity selectors name (clause 4.17).
///
/// Registered types are stored expanded; each is compacted to the term a source
/// recognises, the same way a client's type name is matched against them.
fn block_types(block: &RegistrationInfo) -> Option<TypeExprRaw> {
    let selectors = block.entities.as_deref()?;
    let mut names: Vec<EntityTypeName> = Vec::new();
    for selector in selectors {
        for name in selector.type_.names() {
            let compact = compact_type_name(name).unwrap_or_else(|| name.clone()); // clone: a type already compact is forwarded as written
            if !names.contains(&compact) {
                names.push(compact);
            }
        }
    }
    match names.len() {
        0 => None,
        1 => names.pop().map(TypeExprRaw::Type),
        _ => Some(TypeExprRaw::Or(names.into_iter().map(TypeExprRaw::Type).collect())),
    }
}

/// Whether a registered type selection (Table 5.2.8-1: one name or several)
/// admits the client's compact or expanded type name. Registered names are
/// stored expanded, so a compact client name is compared against the last
/// segment of the registered IRI as well.
fn selector_covers_type(selector: &EntityTypeSelection, name: &EntityTypeName) -> bool {
    selector
        .names()
        .iter()
        .any(|registered| registered == name || compact_type_name(registered).as_ref() == Some(name))
}

/// The last fragment or path segment of an expanded type IRI, as the compact
/// name a client most likely wrote for it.
fn compact_type_name(name: &EntityTypeName) -> Option<EntityTypeName> {
    let iri = name.as_ref();
    let compact = match iri.rfind('#') {
        Some(position) => &iri[position + 1..],
        None => iri.rfind('/').map(|position| &iri[position + 1..])?,
    };
    compact.parse().ok()
}
