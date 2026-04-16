use iref::Iri;

mod vocab {
    ngsi_ld_vocabulary::generate!(
        "lib/ngsi-ld-vocabulary/tests/ui/contexts/core.jsonld",
        "lib/ngsi-ld-vocabulary/tests/ui/contexts/data-integrity.jsonld",
        "lib/ngsi-ld-vocabulary/tests/ui/contexts/andromeda.jsonld",
    );
}

fn main() {
    let _: &'static Iri = vocab::expanded::PROPERTY;
    let _: &'static Iri = vocab::expanded::PROOF_VALUE;
    let _: &'static Iri = vocab::expanded::ENTITY_ID_SEALED;
    let _: &'static Iri = vocab::expanded::NULL_URI;
    assert_eq!(vocab::expanded::PROPERTY.as_str(), "https://uri.etsi.org/ngsi-ld/Property");
}
