//! The family value of a stored data-integrity seal (ETSI GS CIM 009 v1.9.1
//! clause 4.5.2.2): the `ngsildproof` Property the seal fields of its element
//! and the further members of its proof hold.

use crate::{assemble::AssemblyError, stored_family::StoredFamily, stored_json::StoredJson, stored_proof::StoredProof};
use andromeda_ngsi_ld_entity::{
    entity_types::EntityTypes,
    expanded_value::ExpandedValue,
    expanded_value_name::ExpandedValueName,
    family_value::FamilyValue,
    node_id::NodeId,
    proof::{attribute_proof::AttributeProof, data_integrity_proof::DataIntegrityProof, proof_seals::ProofSeals},
};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, reference::NgsiLdReference};
use andromeda_ngsi_ld_model::temporal::structured_value::StructuredValue;
use andromeda_postgresql_codes::value_kind::ValueKind;
use cefact_units::UnitCode;
use indexmap::IndexMap;

/// The `@id` and further members of a stored proof.
type FurtherMembers = (Option<NodeId>, IndexMap<ExpandedValueName, Vec<ExpandedValue>>);

impl StoredFamily<'_> {
    /// The seal `proof` holds, with the `unitCode` and `valueType` stored
    /// beside it.
    ///
    /// # Errors
    ///
    /// Returns [`AssemblyError::UnknownType`] for a sealed type key with no
    /// name, and [`AssemblyError::ValueOfAnotherFamily`] for a seal with no
    /// type or with further members that are no node of members.
    pub(crate) fn sealed(self, proof: StoredProof, unit_code: Option<UnitCode>, value_type: Option<NgsiLdReference>) -> Result<FamilyValue, AssemblyError> {
        let StoredProof {
            entity_id,
            entity_type_keys,
            created,
            verification_method,
            cryptosuite,
            proof_purpose,
            proof_value,
            members,
        } = proof;
        let names = entity_type_keys
            .into_iter()
            .map(|key| self.type_name(key).map(|name| EntityTypeName::from(&name)))
            .collect::<Result<Vec<_>, _>>()?;
        let entity_types = EntityTypes::try_from_names(names).map_err(|_| self.mismatch(ValueKind::DataIntegrityProof))?;
        let (id, further) = match members {
            Some(json) => self.further_members(&json)?,
            None => (None, IndexMap::new()),
        };
        let integrity = DataIntegrityProof::builder()
            .id(id)
            .created(created)
            .verification_method(verification_method)
            .cryptosuite(cryptosuite)
            .proof_purpose(proof_purpose)
            .proof_value(proof_value)
            .further(further)
            .build();
        let mut sealed = AttributeProof::new(ProofSeals::new(entity_id, entity_types), integrity);
        sealed.set_unit_code(unit_code);
        sealed.set_value_type(value_type);
        Ok(FamilyValue::Proof(Box::new(sealed)))
    }

    /// The `@id` and members of the one typeless node `json` stores.
    fn further_members(self, json: &StoredJson) -> Result<FurtherMembers, AssemblyError> {
        let misplaced = || self.mismatch(ValueKind::DataIntegrityProof);
        let StructuredValue::Values(values) = self.read(json)? else {
            return Err(misplaced());
        };
        let mut values = self.stored_values(values)?.into_iter();
        let (Some(ExpandedValue::Node(node)), None) = (values.next(), values.next()) else {
            return Err(misplaced());
        };
        match node.into_parts() {
            (id, types, members) if types.is_empty() => Ok((id, members)),
            (_, _, _) => Err(misplaced()),
        }
    }
}
