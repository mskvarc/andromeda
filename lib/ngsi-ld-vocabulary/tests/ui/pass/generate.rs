use iref::Iri;

mod vocab {
    ngsi_ld_vocabulary::generate!(
        "../contexts/core.jsonld",
        "../contexts/data-integrity.jsonld",
        "../contexts/andromeda.jsonld",
    );
}

fn main() {
    let _: &'static Iri = vocab::PROPERTY;
    let _: &'static Iri = vocab::PROOF_VALUE;
    let _: &'static Iri = vocab::ENTITY_ID_SEALED;
    let _: &'static Iri = vocab::NULL_URI;
    assert_eq!(vocab::PROPERTY.as_str(), "https://uri.etsi.org/ngsi-ld/Property");
}
