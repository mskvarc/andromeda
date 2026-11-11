//! A W3C Data Integrity proof, the value of an `ngsildproof` (ETSI GS CIM 009
//! v1.9.1 clause 4.5.2.2).
//!
//! ETSI TS 104 179 V1.1.1 clause 5.1.2 has a proof contain at least its
//! `type`, the fixed `DataIntegrityProof`, and the five members
//! [`ProofMember`] names. Each is held as the value it is: the suite and the
//! encoded signature as text, the purpose and the verification method as the
//! IRIs the W3C Data Integrity v2 context coerces them to, and `created` as
//! the NGSI-LD `DateTime` clause 5.1.2 makes it, with the width it was written
//! at. Every other member a proof carries (`expires`, `domain`, `challenge`,
//! `nonce`, `previousProof`, or one a cryptosuite defines) is held as the
//! expanded values it was written as, as is the proof's `@id`: a verifier
//! reads every member but the signature as signed data, so none is dropped or
//! respelled.

use crate::{
    expanded_value::{ExpandedValue, ValueNode},
    expanded_value_name::ExpandedValueName,
    literal::{Literal, LiteralValue},
    node_id::NodeId,
    proof::{cryptosuite::Cryptosuite, proof_fault::ProofFault, proof_member::ProofMember, proof_value::ProofValue},
};
use andromeda_ngsi_ld_lexicon::{
    datatype::proof_datatypes::PROOF_CREATED_DATATYPE,
    reference::NgsiLdReference,
    time::spelled_date_time::SpelledDateTime,
    uri::Uri,
};
use andromeda_ngsi_ld_vocabulary::data_integrity_vocabulary::{CRYPTOSUITE_STRING, MULTIBASE, expanded::classes};
use compact_str::CompactString;
use indexmap::IndexMap;
use iri_rs::Iri;
use std::str::FromStr;
use typed_builder::TypedBuilder;

/// The members of a Data Integrity proof beyond its `type`.
#[derive(Clone, Debug, PartialEq, TypedBuilder)]
pub struct DataIntegrityProof {
    /// The proof's `@id`, when it has one.
    #[builder(default = None)]
    id: Option<NodeId>,
    /// `created`.
    created: SpelledDateTime,
    /// `verificationMethod`.
    verification_method: Uri,
    /// `cryptosuite`.
    cryptosuite: Cryptosuite,
    /// `proofPurpose`.
    proof_purpose: Uri,
    /// `proofValue`.
    proof_value: ProofValue,
    /// Every other member, by expanded name, in written order.
    #[builder(default = IndexMap::new())]
    further: IndexMap<ExpandedValueName, Vec<ExpandedValue>>,
}

impl DataIntegrityProof {
    /// The proof's `@id`, when it has one.
    #[must_use]
    pub const fn id(&self) -> Option<&NodeId> {
        self.id.as_ref()
    }

    /// `created`.
    #[must_use]
    pub const fn created(&self) -> SpelledDateTime {
        self.created
    }

    /// `verificationMethod`.
    #[must_use]
    pub const fn verification_method(&self) -> &Uri {
        &self.verification_method
    }

    /// `cryptosuite`.
    #[must_use]
    pub const fn cryptosuite(&self) -> &Cryptosuite {
        &self.cryptosuite
    }

    /// `proofPurpose`.
    #[must_use]
    pub const fn proof_purpose(&self) -> &Uri {
        &self.proof_purpose
    }

    /// `proofValue`.
    #[must_use]
    pub const fn proof_value(&self) -> &ProofValue {
        &self.proof_value
    }

    /// Every member beyond the required ones, by expanded name, in written
    /// order.
    #[must_use]
    pub const fn further(&self) -> &IndexMap<ExpandedValueName, Vec<ExpandedValue>> {
        &self.further
    }

    /// The proof as the expanded node it is written as: the required members
    /// in the order annex C.11 prints them, then the others in written order.
    #[must_use]
    pub fn to_expanded(&self) -> ExpandedValue {
        let mut members = IndexMap::with_capacity(5 + self.further.len());
        members.insert(member_name(ProofMember::Created), vec![typed(self.created.to_string(), PROOF_CREATED_DATATYPE)]);
        members.insert(member_name(ProofMember::VerificationMethod), vec![reference(&self.verification_method)]);
        members.insert(
            member_name(ProofMember::Cryptosuite),
            vec![typed(self.cryptosuite.as_str(), CRYPTOSUITE_STRING)],
        );
        members.insert(member_name(ProofMember::ProofPurpose), vec![reference(&self.proof_purpose)]);
        members.insert(member_name(ProofMember::ProofValue), vec![typed(self.proof_value.as_str(), MULTIBASE)]);
        members.extend(self.further.iter().map(|(name, values)| (name.clone(), values.clone()))); // clone: the node is built beside the proof it shows.
        let types = vec![ExpandedValueName::from_static(classes::DATA_INTEGRITY_PROOF)];
        ExpandedValue::Node(Box::new(ValueNode::new(self.id.clone(), types, members))) // clone: the node is built beside the proof it shows.
    }
}

impl TryFrom<ValueNode> for DataIntegrityProof {
    type Error = ProofFault;

    /// Read a proof from the expanded node of an `ngsildproof` value.
    fn try_from(node: ValueNode) -> Result<DataIntegrityProof, ProofFault> {
        let (id, types, mut members) = node.into_parts();
        match types.as_slice() {
            [proof_type] if proof_type.as_str() == classes::DATA_INTEGRITY_PROOF.as_str() => {}
            [] | [_] | [_, _, ..] => return Err(ProofFault::NotDataIntegrityProof),
        }
        let created = typed_text_member(&mut members, ProofMember::Created, PROOF_CREATED_DATATYPE)?;
        let verification_method = reference_member(&mut members, ProofMember::VerificationMethod)?;
        let cryptosuite = typed_text_member(&mut members, ProofMember::Cryptosuite, CRYPTOSUITE_STRING)?;
        let proof_purpose = reference_member(&mut members, ProofMember::ProofPurpose)?;
        let proof_value = typed_text_member(&mut members, ProofMember::ProofValue, MULTIBASE)?;
        Ok(DataIntegrityProof {
            id,
            created,
            verification_method,
            cryptosuite,
            proof_purpose,
            proof_value,
            further: members,
        })
    }
}

/// The expanded name of a required member.
fn member_name(member: ProofMember) -> ExpandedValueName {
    ExpandedValueName::from_static(member.iri())
}

/// A string literal of the datatype the W3C Data Integrity v2 context coerces
/// a member to.
fn typed(text: impl Into<CompactString>, datatype: Iri<&'static str>) -> ExpandedValue {
    ExpandedValue::Literal(Literal::typed(LiteralValue::String(text.into()), NgsiLdReference::from_static(datatype)))
}

/// A node reference to `uri`.
fn reference(uri: &Uri) -> ExpandedValue {
    ExpandedValue::Node(Box::new(ValueNode::new(Some(NodeId::Uri(uri.clone())), Vec::new(), IndexMap::new()))) // clone: the node is built beside the proof it shows.
}

/// The one value of `member`, taken out of the node's members.
fn single(members: &mut IndexMap<ExpandedValueName, Vec<ExpandedValue>>, member: ProofMember) -> Result<ExpandedValue, ProofFault> {
    let mut values = members.shift_remove(member.iri().as_str()).ok_or(ProofFault::MissingMember(member))?;
    match (values.pop(), values.is_empty()) {
        (Some(value), true) => Ok(value),
        (Some(_) | None, _) => Err(ProofFault::MalformedMember(member)),
    }
}

/// The value of `member`, a string of the datatype `datatype` the parser
/// `T` reads.
fn typed_text_member<T: FromStr>(
    members: &mut IndexMap<ExpandedValueName, Vec<ExpandedValue>>,
    member: ProofMember,
    datatype: Iri<&'static str>,
) -> Result<T, ProofFault> {
    let malformed = ProofFault::MalformedMember(member);
    match single(members, member)? {
        ExpandedValue::Literal(literal) if literal.datatype().map(NgsiLdReference::as_str) == Some(datatype.as_str()) => match literal.value() {
            LiteralValue::String(text) => text.parse().map_err(|_| malformed),
            LiteralValue::Number(_) | LiteralValue::Boolean(_) => Err(malformed),
        },
        ExpandedValue::Literal(_) | ExpandedValue::Json(_) | ExpandedValue::Node(_) | ExpandedValue::List(_) => Err(malformed),
    }
}

/// The value of `member`, a node reference to an absolute IRI.
fn reference_member(members: &mut IndexMap<ExpandedValueName, Vec<ExpandedValue>>, member: ProofMember) -> Result<Uri, ProofFault> {
    let malformed = ProofFault::MalformedMember(member);
    match single(members, member)? {
        ExpandedValue::Node(node) => match node.into_parts() {
            (Some(NodeId::Uri(uri)), types, node_members) if types.is_empty() && node_members.is_empty() => Ok(uri),
            (Some(NodeId::Uri(_) | NodeId::Blank(_) | NodeId::Verbatim(_)) | None, _, _) => Err(malformed),
        },
        ExpandedValue::Literal(_) | ExpandedValue::Json(_) | ExpandedValue::List(_) => Err(malformed),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        expanded_value::{ExpandedValue, ValueNode},
        expanded_value_name::ExpandedValueName,
        literal::{Literal, LiteralValue},
        node_id::NodeId,
        proof::{data_integrity_proof::DataIntegrityProof, proof_fault::ProofFault, proof_member::ProofMember},
    };
    use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
    use indexmap::IndexMap;

    const SECURITY: &str = "https://w3id.org/security#";

    fn name(raw: &str) -> ExpandedValueName {
        raw.parse().expect("an expanded name")
    }

    fn typed(text: &str, datatype: &str) -> Vec<ExpandedValue> {
        vec![ExpandedValue::Literal(Literal::typed(
            LiteralValue::String(text.into()),
            datatype.parse::<NgsiLdReference>().expect("a datatype"),
        ))]
    }

    fn reference(iri: &str) -> Vec<ExpandedValue> {
        vec![ExpandedValue::Node(Box::new(ValueNode::new(
            Some(NodeId::from(iri)),
            Vec::new(),
            IndexMap::new(),
        )))]
    }

    /// The members of the annex C.11 proof, expanded by the W3C Data Integrity
    /// v2 context.
    fn c_11_members() -> IndexMap<ExpandedValueName, Vec<ExpandedValue>> {
        IndexMap::from([
            (
                name("http://purl.org/dc/terms/created"),
                typed("2025-01-27T21:02:24Z", "http://www.w3.org/2001/XMLSchema#dateTime"),
            ),
            (
                name(&format!("{SECURITY}verificationMethod")),
                reference("https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT"),
            ),
            (
                name(&format!("{SECURITY}cryptosuite")),
                typed("eddsa-rdfc-2022", &format!("{SECURITY}cryptosuiteString")),
            ),
            (name(&format!("{SECURITY}proofPurpose")), reference(&format!("{SECURITY}assertionMethod"))),
            (
                name(&format!("{SECURITY}proofValue")),
                typed("z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS", &format!("{SECURITY}multibase")),
            ),
        ])
    }

    fn node(members: IndexMap<ExpandedValueName, Vec<ExpandedValue>>) -> ValueNode {
        ValueNode::new(None, vec![name(&format!("{SECURITY}DataIntegrityProof"))], members)
    }

    #[test]
    fn the_annex_c_11_proof_is_read_member_by_member() {
        let proof = DataIntegrityProof::try_from(node(c_11_members())).expect("the annex C.11 proof");
        pretty_assertions::assert_eq!(proof.created().to_string(), "2025-01-27T21:02:24Z");
        pretty_assertions::assert_eq!(proof.cryptosuite().as_str(), "eddsa-rdfc-2022");
        pretty_assertions::assert_eq!(proof.proof_purpose().as_str(), "https://w3id.org/security#assertionMethod");
        pretty_assertions::assert_eq!(
            proof.verification_method().as_str(),
            "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT"
        );
        pretty_assertions::assert_eq!(proof.proof_value().as_str(), "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS");
        assert!(proof.further().is_empty());
    }

    #[test]
    fn the_proof_is_written_back_as_the_node_it_was_read_from() {
        let read = node(c_11_members());
        let proof = DataIntegrityProof::try_from(read.clone()).expect("the annex C.11 proof");
        pretty_assertions::assert_eq!(proof.to_expanded(), ExpandedValue::Node(Box::new(read)));
    }

    /// W3C VC Data Integrity 1.0 section 2.1 defines further members, and a
    /// cryptosuite may add its own; each is kept as written, as is the `@id`.
    #[test]
    fn further_members_and_the_id_are_kept_as_written() {
        let mut members = c_11_members();
        members.insert(
            name(&format!("{SECURITY}nonce")),
            vec![ExpandedValue::Literal(Literal::plain(LiteralValue::String("abc".into())))],
        );
        members.insert(name(&format!("{SECURITY}previousProof")), reference("urn:uuid:1"));
        let read = ValueNode::new(Some(NodeId::from("urn:uuid:2")), vec![name(&format!("{SECURITY}DataIntegrityProof"))], members);
        let proof = DataIntegrityProof::try_from(read.clone()).expect("a proof with further members");
        pretty_assertions::assert_eq!(proof.further().len(), 2);
        pretty_assertions::assert_eq!(proof.to_expanded(), ExpandedValue::Node(Box::new(read)));
    }

    /// The `created` width is part of the signed data, so a width the broker
    /// spells otherwise elsewhere reads back unchanged.
    #[test]
    fn created_keeps_the_width_it_was_written_at() {
        let mut members = c_11_members();
        members.insert(
            name("http://purl.org/dc/terms/created"),
            typed("2025-01-27T21:02:24.50Z", "http://www.w3.org/2001/XMLSchema#dateTime"),
        );
        let proof = DataIntegrityProof::try_from(node(members)).expect("a proof");
        pretty_assertions::assert_eq!(proof.created().to_string(), "2025-01-27T21:02:24.50Z");
    }

    #[test]
    fn each_missing_member_is_named() {
        for member in [
            ProofMember::Created,
            ProofMember::VerificationMethod,
            ProofMember::Cryptosuite,
            ProofMember::ProofPurpose,
            ProofMember::ProofValue,
        ] {
            let mut members = c_11_members();
            members.shift_remove(member.iri().as_str());
            pretty_assertions::assert_eq!(DataIntegrityProof::try_from(node(members)), Err(ProofFault::MissingMember(member)), "{member}");
        }
    }

    #[test]
    fn a_member_of_the_wrong_kind_is_named() {
        for (member, values) in [
            (
                ProofMember::Created,
                typed("2025-01-27T23:02:24+02:00", "http://www.w3.org/2001/XMLSchema#dateTime"),
            ),
            (ProofMember::Created, typed("2025-01-27T21:02:24Z", "http://www.w3.org/2001/XMLSchema#string")),
            (ProofMember::Cryptosuite, typed("", &format!("{SECURITY}cryptosuiteString"))),
            (ProofMember::ProofValue, reference("urn:uuid:3")),
            (ProofMember::ProofPurpose, typed("assertionMethod", &format!("{SECURITY}multibase"))),
            (ProofMember::VerificationMethod, reference("_:key")),
            (
                ProofMember::Cryptosuite,
                [
                    typed("a", &format!("{SECURITY}cryptosuiteString")),
                    typed("b", &format!("{SECURITY}cryptosuiteString")),
                ]
                .concat(),
            ),
        ] {
            let mut members = c_11_members();
            members.insert(name(member.iri().as_str()), values);
            pretty_assertions::assert_eq!(
                DataIntegrityProof::try_from(node(members)),
                Err(ProofFault::MalformedMember(member)),
                "{member}"
            );
        }
    }

    /// ETSI TS 104 179 clause 5.1.2 fixes the type as `DataIntegrityProof`.
    #[test]
    fn another_or_a_further_proof_type_is_refused() {
        for types in [
            Vec::new(),
            vec![name(&format!("{SECURITY}Ed25519Signature2020"))],
            vec![name(&format!("{SECURITY}DataIntegrityProof")), name("https://example.org/Extra")],
        ] {
            pretty_assertions::assert_eq!(
                DataIntegrityProof::try_from(ValueNode::new(None, types, c_11_members())),
                Err(ProofFault::NotDataIntegrityProof)
            );
        }
    }
}
