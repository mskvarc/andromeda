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
//! Registered type and attribute names are stored expanded (as IRIs). Two
//! decisions turn on those names, and both go through the JSON-LD engine so
//! they agree with the registration selection that reached here:
//!
//! - Coverage: a client's term is kept when its expanded IRI equals a block
//!   selector's stored type IRI or a declared attribute IRI. This is the same
//!   comparison `crate::registration_selection::information_matches_entity`
//!   makes when it selects the registration, so a block that was selected is
//!   narrowed consistently rather than discarded by a divergent string guess.
//! - Supplied names: when a block contributes its own type or attribute names
//!   (the client constrained none), each stored IRI is compacted to the term
//!   the request context spells it with. A name the context abbreviates none of
//!   is forwarded as its full IRI, which a source still resolves (clause
//!   4.5.1), rather than a wrong last-segment token it could not.

use crate::{error::FederationError, term_resolver::TermResolver};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
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
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when a client type cannot be expanded
    /// against the request context.
    pub(super) async fn of(
        block: &RegistrationInfo,
        ids: Option<&[Uri]>,
        id_pattern: Option<&Pattern>,
        entity_type: Option<&TypeExprRaw>,
        resolver: &TermResolver<'_>,
    ) -> Result<Option<BlockSelection>, FederationError> {
        let mut selection = BlockSelection::default();
        if let Some(ids) = ids.filter(|ids| !ids.is_empty()) {
            let Some(covered) = covered_ids(ids, block) else {
                return Ok(None);
            };
            selection.ids = covered;
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
            Some(entity_type) => match covered_type(entity_type, block, resolver).await? {
                Some(covered) => Some(covered),
                None => return Ok(None),
            },
            None => block_types(block, resolver).await,
        };
        Ok(Some(selection))
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
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a client attribute cannot be
/// expanded against the request context.
pub(super) async fn block_attrs(
    block: &RegistrationInfo,
    client_attrs: Option<&[AttributeName]>,
    resolver: &TermResolver<'_>,
) -> Result<AttrCoverage, FederationError> {
    let declared = block.attribute_names().collect::<Vec<_>>();
    match client_attrs.filter(|attrs| !attrs.is_empty()) {
        Some(attrs) => {
            if declared.is_empty() {
                return Ok(AttrCoverage::Write(attrs.to_vec())); // clone: the forwarded query owns the names it writes
            }
            let covered = covered_attrs(attrs, &declared, resolver).await?;
            if covered.is_empty() {
                Ok(AttrCoverage::Uncovered)
            } else {
                Ok(AttrCoverage::Write(covered))
            }
        }
        None if declared.is_empty() => Ok(AttrCoverage::Omit),
        // The block's declared names are stored expanded; a source expects the
        // compact term its own @context uses, so each is compacted through the
        // request context, falling back to the full IRI a context cannot shorten.
        None => {
            let mut names = Vec::with_capacity(declared.len());
            for name in declared {
                let compact = resolver.compact_attr(name.as_str()).await;
                names.push(compact.parse().unwrap_or_else(|_| name.clone())); // clone: a name already compact is forwarded as written
            }
            Ok(AttrCoverage::Write(names))
        }
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

/// The attributes the client asked for that this block declares, kept in the
/// client's own spelling.
///
/// Each client attribute is expanded to its IRI through the request context and
/// kept when a declared attribute IRI equals it. The declared names are stored
/// expanded, so this is an exact IRI comparison -- the same one that selected
/// the registration -- rather than a guess at the client's term (clause
/// 4.3.6.1).
async fn covered_attrs(
    client_attrs: &[AttributeName],
    declared: &[&AttributeName],
    resolver: &TermResolver<'_>,
) -> Result<Vec<AttributeName>, FederationError> {
    let mut covered = Vec::new();
    for attribute in client_attrs {
        let iri = resolver.expand_attr(attribute.as_str()).await?;
        if declared.iter().any(|name| name.as_str() == iri.as_str()) {
            covered.push(attribute.clone()); // clone: the forwarded query owns the names it writes
        }
    }
    Ok(covered)
}

/// The part of the client's type selector the block covers, or `None` when it
/// covers none of it.
///
/// Only a flat alternation of names can be narrowed; the wildcard and a
/// conjunction are forwarded whole, because removing a member would change what
/// they select rather than restrict it. Each leaf is expanded to its IRI and
/// kept when a block selector admits that IRI, matching the registration
/// selection exactly.
pub(super) async fn covered_type(
    entity_type: &TypeExprRaw,
    block: &RegistrationInfo,
    resolver: &TermResolver<'_>,
) -> Result<Option<TypeExprRaw>, FederationError> {
    let Some(selectors) = block.entities.as_deref().filter(|selectors| !selectors.is_empty()) else {
        return Ok(Some(entity_type.clone())); // clone: an unrestricted block forwards the client's selector as written
    };
    let leaves = match entity_type {
        TypeExprRaw::Type(name) => vec![name],
        TypeExprRaw::Or(children) => {
            let mut leaves = Vec::with_capacity(children.len());
            for child in children {
                match child {
                    TypeExprRaw::Type(name) => leaves.push(name),
                    // A nested boolean group cannot be narrowed member by member,
                    // and an alternation only makes sense as a flat one, so a
                    // block covering none of its flat leaves covers nothing here.
                    TypeExprRaw::Wildcard | TypeExprRaw::And(_) | TypeExprRaw::Or(_) => return Ok(None),
                }
            }
            leaves
        }
        TypeExprRaw::Wildcard | TypeExprRaw::And(_) => return Ok(Some(entity_type.clone())), // clone: same
    };
    let mut covered = Vec::new();
    for name in leaves {
        let iri = resolver.expand_type(name.as_str()).await?;
        if selectors.iter().any(|selector| selector.type_.admits(iri.as_str())) {
            covered.push(TypeExprRaw::Type(name.clone())); // clone: the narrowed selector owns its names
        }
    }
    match covered.len() {
        0 => Ok(None),
        1 => Ok(covered.pop()),
        _ => Ok(Some(TypeExprRaw::Or(covered))),
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
/// Registered types are stored expanded; each is compacted through the request
/// context to the term a source recognises, falling back to the full IRI a
/// context cannot shorten.
async fn block_types(block: &RegistrationInfo, resolver: &TermResolver<'_>) -> Option<TypeExprRaw> {
    let selectors = block.entities.as_deref()?;
    let mut names: Vec<EntityTypeName> = Vec::new();
    for selector in selectors {
        for name in selector.type_.names() {
            let compact = resolver.compact_type(name.as_str()).await;
            let compact: EntityTypeName = compact.parse().unwrap_or_else(|_| name.clone()); // clone: a type already compact is forwarded as written
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
