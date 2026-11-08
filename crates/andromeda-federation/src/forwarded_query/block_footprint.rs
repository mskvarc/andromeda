//! Clause 4.3.6.1 makes the broker respect every constraint a registration declares when it forwards a request, and clause 4.3.6.3 makes each information block a self-contained coverage statement (an exclusive block names a single entity and its attributes, and a registration may carry several such blocks). A forwarded request therefore respects one block at a time, naming that block's entities, type and attributes, rather than the union of every block, which would ask a source for a cross-product of entities and attributes it never registered together.
//!
//! Registered type and attribute names are stored expanded (as IRIs). Two decisions turn on those names, and both go through the JSON-LD engine so they agree with the registration selection that reached here:
//!
//! - Coverage: a client's term is kept when its expanded IRI equals a block selector's stored type IRI or a declared attribute IRI, the same comparison the registration selection makes, so a selected block is narrowed consistently.
//! - Spelling: every name the forwarded request carries is written in the registration's forwarding context (the client's own terms reworded when that context is another one, clause 4.3.6.6), and a block's stored IRIs compacted to the term that context spells them with. A name the context abbreviates none of is forwarded as its full IRI, which a source still resolves (clause 4.5.1).

use crate::{error::FederationError, federation_invariant::FederationInvariant, forwarded_names::ForwardedNames};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    registration::info::RegistrationInfo,
    registration_matching::entity_specification::{EntitySpecification, TypeConstraint},
};
use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
use std::slice;

/// The entity selection one block contributes to a forwarded request: the
/// identifiers, id pattern and type it narrows the client's selection to, or
/// supplies when the client constrained none of them.
#[derive(Debug, Default)]
pub(super) struct BlockSelection {
    /// The identifiers to write as `id`.
    pub(super) ids: Vec<Identifier>,
    /// The pattern to write as `idPattern`.
    pub(super) id_pattern: Option<String>,
    /// The type selector to write as `type`, spelled in the forwarding context.
    pub(super) entity_type: Option<TypeExprRaw>,
}

impl BlockSelection {
    /// The entity selection `block` contributes for a client that named `ids`,
    /// `id_pattern` and `entity_type`, or `None` when the block covers none of
    /// the entities the client selected.
    ///
    /// The client's identifiers and type are narrowed to what the block covers,
    /// and a client pattern is kept for a block it may match (clause 5.12);
    /// where the client constrained neither, the block supplies its own
    /// identifiers (or its id pattern) and its declared types, so a forwarded
    /// request always names the block's footprint even when the client's query
    /// was broader (clause 4.3.6.1).
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when a client type cannot be expanded
    /// against the request context, and [`FederationError::Internal`] when a
    /// type does not spell as a name in the forwarding context.
    pub(super) async fn of(
        block: &RegistrationInfo,
        ids: Option<&[Identifier]>,
        id_pattern: Option<&Pattern>,
        entity_type: Option<&TypeExprRaw>,
        names: &ForwardedNames<'_>,
    ) -> Result<Option<BlockSelection>, FederationError> {
        let mut selection = BlockSelection::default();
        if let Some(ids) = ids.filter(|ids| !ids.is_empty()) {
            let Some(covered) = covered_ids(ids, block) else {
                return Ok(None);
            };
            selection.ids = covered;
            selection.id_pattern = id_pattern.map(ToString::to_string);
        } else if let Some(pattern) = id_pattern {
            // Clause 5.12: a pattern on either side, or none registered, is
            // taken as matching; a registered `id` must match the pattern.
            let specification = EntitySpecification {
                types: TypeConstraint::Any,
                ids: &[],
                id_pattern: Some(pattern),
            };
            if !specification.matches_entity_axis(block) {
                return Ok(None);
            }
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
            Some(entity_type) => match covered_type(entity_type, block, names).await? {
                Some(covered) => Some(covered),
                None => return Ok(None),
            },
            None => block_types(block, names)?,
        };
        Ok(Some(selection))
    }
}

/// The attribute list a block contributes to a forwarded request.
pub(super) enum AttrCoverage {
    /// Write these attribute names as `attrs`, spelled in the forwarding
    /// context.
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
/// or, when the client asked for none, the block's declared names are sent
/// on their own, because they are a registration constraint rather than a
/// client filter (clause 4.3.6.1). A block declaring no names forwards the
/// client's attributes, or nothing when the client named none. Every name is
/// spelled in the forwarding context (clause 4.3.6.6).
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a client attribute cannot be
/// expanded against the request context, and [`FederationError::Internal`]
/// when an attribute does not spell as a name in the forwarding context.
pub(super) async fn block_attrs(
    block: &RegistrationInfo,
    client_attrs: Option<&[AttributeName]>,
    names: &ForwardedNames<'_>,
) -> Result<AttrCoverage, FederationError> {
    let declared = block.attribute_names().collect::<Vec<_>>();
    match client_attrs.filter(|attrs| !attrs.is_empty()) {
        Some(attrs) => {
            let kept = if declared.is_empty() {
                attrs.to_vec()
            } else {
                covered_attrs(attrs, &declared, names).await?
            };
            if kept.is_empty() {
                return Ok(AttrCoverage::Uncovered);
            }
            let mut spelled = Vec::with_capacity(kept.len());
            for attribute in &kept {
                spelled.push(names.attr(attribute).await?);
            }
            Ok(AttrCoverage::Write(spelled))
        }
        None if declared.is_empty() => Ok(AttrCoverage::Omit),
        // The block's declared names are stored expanded; a source expects the
        // compact term the forwarding context uses, falling back to the full
        // IRI a context cannot shorten.
        None => {
            let mut spelled = Vec::with_capacity(declared.len());
            for name in declared {
                spelled.push(names.compact_attr(name)?);
            }
            Ok(AttrCoverage::Write(spelled))
        }
    }
}

/// The identifiers among `ids` this block covers, or `None` when it covers none.
fn covered_ids(ids: &[Identifier], block: &RegistrationInfo) -> Option<Vec<Identifier>> {
    let covered = ids
        .iter()
        .filter(|id| EntitySpecification::of_entity(&[], id).matches_entity_axis(block))
        .cloned()
        .collect::<Vec<_>>();
    (!covered.is_empty()).then_some(covered)
}

/// The attributes the client asked for that this block declares, kept in the
/// client's own spelling.
///
/// Each client attribute is expanded to its IRI through the request context and
/// kept when a declared attribute IRI equals it. The declared names are stored
/// expanded, so this is an exact IRI comparison (the same one that selected
/// the registration) rather than a guess at the client's term (clause
/// 4.3.6.1).
async fn covered_attrs(client_attrs: &[AttributeName], declared: &[&AttributeName], names: &ForwardedNames<'_>) -> Result<Vec<AttributeName>, FederationError> {
    let mut covered = Vec::new();
    for attribute in client_attrs {
        let iri = names.resolver().expand_attr(attribute.as_str()).await?;
        if declared.iter().any(|name| name.as_str() == iri.as_str()) {
            covered.push(attribute.clone());
        }
    }
    Ok(covered)
}

/// The part of the client's type selector the block covers, spelled in the
/// forwarding context, or `None` when it covers none of it.
///
/// Only a flat alternation of names can be narrowed; the wildcard and a
/// conjunction are forwarded whole, because removing a member would change what
/// they select rather than restrict it. Each leaf is expanded to its IRI and
/// kept when a block selector admits that IRI, matching the registration
/// selection exactly.
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a client type cannot be expanded
/// against the request context, and [`FederationError::Internal`] when a type
/// does not spell as a name in the forwarding context.
pub(super) async fn covered_type(
    entity_type: &TypeExprRaw,
    block: &RegistrationInfo,
    names: &ForwardedNames<'_>,
) -> Result<Option<TypeExprRaw>, FederationError> {
    if block.entities.is_none() {
        return Ok(Some(names.type_expr(entity_type).await?));
    }
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
        TypeExprRaw::Wildcard | TypeExprRaw::And(_) => return Ok(Some(names.type_expr(entity_type).await?)),
    };
    let mut covered = Vec::new();
    for name in leaves {
        let iri = names.resolver().expand_type(name.as_str()).await?;
        let leaf = iri
            .as_str()
            .parse::<EntityTypeName>()
            .map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))?;
        let specification = EntitySpecification {
            types: TypeConstraint::OneOf(slice::from_ref(&leaf)),
            ids: &[],
            id_pattern: None,
        };
        if specification.matches_entity_axis(block) {
            covered.push(TypeExprRaw::Type(names.entity_type(name).await?));
        }
    }
    Ok(alternation(covered))
}

/// One selector for `leaves`: none for no leaf, the leaf itself for one, and
/// their alternation (clause 4.17) for several.
fn alternation(leaves: Vec<TypeExprRaw>) -> Option<TypeExprRaw> {
    let mut leaves = leaves.into_iter();
    match (leaves.next(), leaves.next()) {
        (None, None | Some(_)) => None,
        (Some(only), None) => Some(only),
        (Some(first), Some(second)) => Some(TypeExprRaw::Or([first, second].into_iter().chain(leaves).collect())),
    }
}

/// The block's explicitly named identifiers, or, when it names none, the
/// single id pattern its entity selectors constrain identifiers by.
fn block_identifiers(block: &RegistrationInfo) -> (Vec<Identifier>, Option<String>) {
    let Some(selectors) = block.entities.as_deref() else {
        return (Vec::new(), None);
    };
    let ids = selectors.iter().filter_map(|selector| selector.id.clone()).collect::<Vec<_>>();
    if !ids.is_empty() {
        return (ids, None);
    }
    let pattern = selectors.iter().find_map(|selector| selector.id_pattern.as_ref().map(ToString::to_string));
    (Vec::new(), pattern)
}

/// The block's declared types as one selector: the single type, or the
/// alternation of every distinct type its entity selectors name (clause 4.17).
///
/// Registered types are stored expanded; each is compacted through the
/// forwarding context to the term a source recognises, falling back to the full
/// IRI a context cannot shorten.
///
/// # Errors
///
/// Returns [`FederationError::Internal`] when a type does not spell as a name
/// in the forwarding context.
fn block_types(block: &RegistrationInfo, names: &ForwardedNames<'_>) -> Result<Option<TypeExprRaw>, FederationError> {
    let Some(selectors) = block.entities.as_deref() else {
        return Ok(None);
    };
    let mut spelled: Vec<EntityTypeName> = Vec::new();
    for selector in selectors {
        for name in selector.type_.names() {
            let compact = names.compact_type(name)?;
            if !spelled.contains(&compact) {
                spelled.push(compact);
            }
        }
    }
    Ok(alternation(spelled.into_iter().map(TypeExprRaw::Type).collect()))
}

#[cfg(test)]
mod tests {
    use crate::forwarded_query::block_footprint::covered_ids;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::registration::info::RegistrationInfo;
    use serde_json::json;

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    /// Clause 5.2.8: the registered `id` decides, so the pattern beside it does
    /// not widen the identifiers a forwarded request names.
    #[test]
    fn a_block_naming_id_and_pattern_covers_only_that_id() {
        let block: RegistrationInfo = serde_json::from_value(json!({
            "entities": [{"type": "https://example.org/Vehicle", "id": "urn:ngsi-ld:Vehicle:7", "idPattern": "urn:ngsi-ld:Vehicle:.*"}]
        }))
        .expect("information parses");
        let seven = uri("urn:ngsi-ld:Vehicle:7");
        pretty_assertions::assert_eq!(covered_ids(&[uri("urn:ngsi-ld:Vehicle:1"), seven.clone()], &block), Some(vec![seven]));
        pretty_assertions::assert_eq!(covered_ids(&[uri("urn:ngsi-ld:Vehicle:1")], &block), None);
    }
}
