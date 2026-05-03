use iref::Iri;

mod vocab {
    ngsi_ld_vocabulary::generate!(
        "lib/ngsi-ld-vocabulary/tests/ui/contexts/core.jsonld",
        "lib/ngsi-ld-vocabulary/tests/ui/contexts/andromeda.jsonld",
    );
}

fn main() {
    let _: &'static Iri = vocab::expanded::classes::PROPERTY;
    let _: &'static Iri = vocab::expanded::properties::NULL_URI;
    assert_eq!(vocab::expanded::classes::PROPERTY.as_str(), "https://uri.etsi.org/ngsi-ld/Property");
}
