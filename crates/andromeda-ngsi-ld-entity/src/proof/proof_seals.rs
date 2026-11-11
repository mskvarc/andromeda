//! The two seals of an `ngsildproof`: the identifier and the types of the
//! Atomic Entity its proof signs (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2, ETSI
//! TS 104 179 V1.1.1 clauses 5.2.2 to 5.2.4).
//!
//! TS 104 179 clause 5.2.2 binds "the information that the Attribute is part
//! of a specific Entity, with its type" into the signature, and clause 5.2.4
//! copies that head into the seals. A client rebuilds the signed Atomic Entity
//! from them (clause 5.2.5), so a seal holds both, always.
//!
//! Expanded, `entityIdSealed` is a plain string, the core `@context` giving the
//! term no type, and `entityTypeSealed` is one node reference per type, the
//! term being coerced with `@vocab`.

use crate::{
    entity_types::EntityTypes,
    expanded_value::{ExpandedValue, ValueNode},
    literal::{Literal, LiteralValue},
    member_role::ProofSeal,
    node_id::NodeId,
    proof::proof_fault::ProofFault,
};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier, uri::Uri};
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::is_ngsi_ld_null;
use compact_str::CompactString;
use indexmap::IndexMap;

/// The identifier and types of the entity a proof is sealed to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProofSeals {
    /// `entityIdSealed`.
    entity_id: Identifier,
    /// `entityTypeSealed`.
    entity_types: EntityTypes,
}

impl ProofSeals {
    /// The seals of the entity `entity_id` of types `entity_types`.
    #[must_use]
    pub const fn new(entity_id: Identifier, entity_types: EntityTypes) -> ProofSeals {
        ProofSeals { entity_id, entity_types }
    }

    /// `entityIdSealed`.
    #[must_use]
    pub const fn entity_id(&self) -> &Identifier {
        &self.entity_id
    }

    /// `entityTypeSealed`.
    #[must_use]
    pub const fn entity_types(&self) -> &EntityTypes {
        &self.entity_types
    }

    /// The expanded values of `entityIdSealed`: one plain string.
    #[must_use]
    pub fn entity_id_values(&self) -> Vec<ExpandedValue> {
        vec![ExpandedValue::Literal(Literal::plain(LiteralValue::String(CompactString::from(
            self.entity_id.as_str(),
        ))))]
    }

    /// The expanded values of `entityTypeSealed`: one node reference per type.
    #[must_use]
    pub fn entity_type_values(&self) -> Vec<ExpandedValue> {
        self.entity_types
            .as_slice()
            .iter()
            .map(|name| ExpandedValue::Node(Box::new(ValueNode::new(Some(NodeId::from(name.as_str())), Vec::new(), IndexMap::new()))))
            .collect()
    }
}

/// The identifier the expanded values of `entityIdSealed` hold: exactly one
/// plain string that is an identifier.
///
/// A seal is never deleted, so the NGSI-LD Null (clause 4.5.0) seals nothing.
///
/// # Errors
///
/// Returns [`ProofFault::MalformedSeal`] for anything else.
pub fn sealed_entity_id(values: &[ExpandedValue]) -> Result<Identifier, ProofFault> {
    let malformed = ProofFault::MalformedSeal(ProofSeal::EntityId);
    let [ExpandedValue::Literal(literal)] = values else {
        return Err(malformed);
    };
    match (literal.value(), literal.datatype(), literal.language(), literal.direction()) {
        (LiteralValue::String(text), None, None, None) if !is_ngsi_ld_null(text) => text.parse().map_err(|_| malformed),
        (LiteralValue::String(_) | LiteralValue::Number(_) | LiteralValue::Boolean(_), _, _, _) => Err(malformed),
    }
}

/// The entity types the expanded values of `entityTypeSealed` hold: one or
/// more node references, each an entity type name.
///
/// # Errors
///
/// Returns [`ProofFault::MalformedSeal`] for anything else.
pub fn sealed_entity_types(values: &[ExpandedValue]) -> Result<EntityTypes, ProofFault> {
    let malformed = ProofFault::MalformedSeal(ProofSeal::EntityType);
    let names = values
        .iter()
        .map(|value| match value {
            ExpandedValue::Node(node) if node.types().is_empty() && node.members().is_empty() => match node.id() {
                Some(NodeId::Uri(uri)) => type_name(uri).ok_or(malformed),
                Some(NodeId::Blank(_) | NodeId::Verbatim(_)) | None => Err(malformed),
            },
            ExpandedValue::Node(_) | ExpandedValue::Literal(_) | ExpandedValue::Json(_) | ExpandedValue::List(_) => Err(malformed),
        })
        .collect::<Result<Vec<_>, _>>()?;
    EntityTypes::try_from_names(names).map_err(|_| malformed)
}

/// The entity type name an expanded type IRI is; the NGSI-LD Null is none.
fn type_name(uri: &Uri) -> Option<EntityTypeName> {
    (!is_ngsi_ld_null(uri.as_str())).then(|| uri.as_str().parse().ok()).flatten()
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_types::EntityTypes,
        expanded_value::{ExpandedValue, ValueNode},
        literal::{Literal, LiteralValue},
        member_role::ProofSeal,
        node_id::NodeId,
        proof::{
            proof_fault::ProofFault,
            proof_seals::{ProofSeals, sealed_entity_id, sealed_entity_types},
        },
    };
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use indexmap::IndexMap;

    fn type_name(raw: &str) -> EntityTypeName {
        raw.parse().expect("a type name")
    }

    fn seals() -> ProofSeals {
        let types = EntityTypes::try_from_names([type_name("https://example.org/Store"), type_name("https://example.org/Shop")]).expect("types");
        ProofSeals::new("urn:ngsi-ld:Store:002".parse().expect("an identifier"), types)
    }

    #[test]
    fn the_seals_read_back_from_their_expanded_values() {
        let seals = seals();
        pretty_assertions::assert_eq!(sealed_entity_id(&seals.entity_id_values()).as_ref(), Ok(seals.entity_id()));
        pretty_assertions::assert_eq!(sealed_entity_types(&seals.entity_type_values()).as_ref(), Ok(seals.entity_types()));
    }

    #[test]
    fn an_identifier_seal_that_is_no_identifier_is_refused() {
        for values in [
            vec![ExpandedValue::Literal(Literal::plain(LiteralValue::String("not an identifier".into())))],
            vec![ExpandedValue::Literal(Literal::plain(LiteralValue::Boolean(true)))],
            vec![ExpandedValue::Literal(Literal::plain(LiteralValue::String("urn:ngsi-ld:null".into())))],
            Vec::new(),
        ] {
            pretty_assertions::assert_eq!(sealed_entity_id(&values), Err(ProofFault::MalformedSeal(ProofSeal::EntityId)));
        }
    }

    #[test]
    fn a_type_seal_that_is_no_node_reference_is_refused() {
        let node = |id: &str| ExpandedValue::Node(Box::new(ValueNode::new(Some(NodeId::from(id)), Vec::new(), IndexMap::new())));
        for values in [
            vec![ExpandedValue::Literal(Literal::plain(LiteralValue::String("Store".into())))],
            vec![node("_:b0")],
            vec![node("urn:ngsi-ld:null")],
            Vec::new(),
        ] {
            pretty_assertions::assert_eq!(sealed_entity_types(&values), Err(ProofFault::MalformedSeal(ProofSeal::EntityType)));
        }
    }
}
