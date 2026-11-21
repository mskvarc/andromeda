use crate::advisory_rulings::{
    assert_vex_statements_repeat_advisory_rulings,
    broker_image::BrokerImage,
    disagreement::Disagreement,
    disagreements,
    locked_crates::LockedCrates,
    names_crate_version,
    vex_document::{Status, VexDocument},
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, fmt::Write, fs, process};
use toml_edit::DocumentMut;

const ADVISORY: &str = "RUSTSEC-2026-0104";

const REASON: &str = "rustls-webpki 0.102 panics parsing a revocation list, which rumqttc 0.25.1, the only crate linking it, never calls";

const IMAGE: &str = "pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda";

const CRATE: &str = "pkg:cargo/rustls-webpki@0.102.8";

const MANIFEST: &str = "[workspace.package]\nrepository = \"https://github.com/example/andromeda\"\n";

fn ruling() -> Value {
    json!({
        "vulnerability": { "name": ADVISORY, "aliases": ["GHSA-82j2-j2ch-gfr8"] },
        "products": [{ "@id": IMAGE, "subcomponents": [{ "@id": CRATE }] }],
        "status": "not_affected",
        "justification": "vulnerable_code_not_in_execute_path",
        "impact_statement": REASON,
    })
}

fn document(statements: &[Value]) -> Value {
    json!({
        "@context": "https://openvex.dev/ns/v0.2.0",
        "@id": "https://example.com/vex",
        "author": "Andromeda",
        "timestamp": "2026-11-21T00:00:00Z",
        "version": 1,
        "statements": statements,
    })
}

fn parsed(statements: &[Value]) -> VexDocument {
    serde_json::from_value(document(statements)).unwrap()
}

fn ignores() -> BTreeMap<String, Option<String>> {
    BTreeMap::from([(ADVISORY.to_owned(), Some(REASON.to_owned()))])
}

/// A lockfile where rumqttc 0.25.1 and every package `dependents` names, each
/// at its version, link rustls-webpki 0.102.8.
fn lockfile(dependents: &[(&str, &str)]) -> String {
    let linking = [("rumqttc", "0.25.1")]
        .iter()
        .chain(dependents)
        .fold(String::new(), |mut linking, (name, version)| {
            write!(
                linking,
                "\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\ndependencies = [\n \"rustls-webpki 0.102.8\",\n]\n"
            )
            .unwrap();
            linking
        });
    format!(
        "version = 4\n\n[[package]]\nname = \"rustls-webpki\"\nversion = \"0.102.8\"\n\n[[package]]\nname = \"rustls-webpki\"\nversion = \"0.103.17\"\n{linking}"
    )
}

fn locked_with(dependents: &[(&str, &str)]) -> LockedCrates {
    LockedCrates::from_document(&lockfile(dependents).parse::<DocumentMut>().unwrap())
}

fn locked() -> LockedCrates {
    locked_with(&[])
}

fn image() -> BrokerImage {
    BrokerImage::from_manifest(&MANIFEST.parse::<DocumentMut>().unwrap()).unwrap()
}

fn ruling_with(member: &str, value: Value) -> Value {
    let mut statement = ruling();
    statement[member] = value;
    statement
}

fn ruling_without(member: &str) -> Value {
    let mut statement = ruling();
    statement.as_object_mut().unwrap().remove(member);
    statement
}

fn ruling_naming(product: &str, subcomponents: &[&str]) -> Value {
    let subcomponents: Vec<Value> = subcomponents.iter().map(|id| json!({ "@id": id })).collect();
    ruling_with("products", json!([{ "@id": product, "subcomponents": subcomponents }]))
}

#[test]
fn a_statement_repeating_an_ignore_for_a_locked_crate_agrees() {
    assert_eq!(disagreements(&parsed(&[ruling()]), &ignores(), &locked(), &image()), Vec::<Disagreement>::new());
}

#[test]
fn an_ignore_without_a_statement_agrees() {
    let mut ignores = ignores();
    ignores.insert("RUSTSEC-2024-0370".to_owned(), Some("build-time only".to_owned()));
    assert_eq!(disagreements(&parsed(&[ruling()]), &ignores, &locked(), &image()), Vec::<Disagreement>::new());
}

#[test]
fn a_statement_for_an_advisory_deny_toml_does_not_ignore_disagrees() {
    assert_eq!(
        disagreements(&parsed(&[ruling()]), &BTreeMap::new(), &locked(), &image()),
        [Disagreement::NotIgnored { advisory: ADVISORY }]
    );
}

#[test]
fn a_statement_for_an_advisory_ignored_without_a_reason_disagrees() {
    let ignores = BTreeMap::from([(ADVISORY.to_owned(), None)]);
    assert_eq!(
        disagreements(&parsed(&[ruling()]), &ignores, &locked(), &image()),
        [Disagreement::IgnoredWithoutReason { advisory: ADVISORY }]
    );
}

#[test]
fn an_impact_statement_other_than_the_ignore_reason_disagrees() {
    let statement = ruling_with("impact_statement", json!("rustls-webpki 0.102 is linked by rumqttc 0.25.1 alone"));
    assert_eq!(
        disagreements(&parsed(&[statement]), &ignores(), &locked(), &image()),
        [Disagreement::ImpactDiffers { advisory: ADVISORY }]
    );
}

#[test]
fn a_statement_without_an_impact_statement_disagrees() {
    assert_eq!(
        disagreements(&parsed(&[ruling_without("impact_statement")]), &ignores(), &locked(), &image()),
        [
            Disagreement::ImpactDiffers { advisory: ADVISORY },
            Disagreement::DependentUnnamed {
                advisory: ADVISORY,
                subcomponent: CRATE,
                dependent: "rumqttc",
                version: "0.25.1",
            },
        ],
    );
}

#[test]
fn a_second_statement_for_one_advisory_disagrees() {
    assert_eq!(
        disagreements(&parsed(&[ruling(), ruling()]), &ignores(), &locked(), &image()),
        [Disagreement::Repeated { advisory: ADVISORY }]
    );
}

#[test]
fn a_status_other_than_not_affected_disagrees() {
    for (token, status) in [
        ("affected", Status::Affected),
        ("fixed", Status::Fixed),
        ("under_investigation", Status::UnderInvestigation),
    ] {
        assert_eq!(
            disagreements(&parsed(&[ruling_with("status", json!(token))]), &ignores(), &locked(), &image()),
            [Disagreement::NotNotAffected { advisory: ADVISORY, status }],
        );
    }
}

#[test]
fn a_statement_without_a_justification_disagrees() {
    assert_eq!(
        disagreements(&parsed(&[ruling_without("justification")]), &ignores(), &locked(), &image()),
        [Disagreement::Unjustified { advisory: ADVISORY }],
    );
}

#[test]
fn a_status_or_justification_the_specification_does_not_define_does_not_parse() {
    for statement in [ruling_with("status", json!("not-affected")), ruling_with("justification", json!("unreachable"))] {
        assert!(serde_json::from_value::<VexDocument>(document(&[statement])).is_err());
    }
}

#[test]
fn a_statement_without_a_product_disagrees() {
    assert_eq!(
        disagreements(&parsed(&[ruling_with("products", json!([]))]), &ignores(), &locked(), &image()),
        [Disagreement::NoProduct { advisory: ADVISORY }],
    );
}

#[test]
fn a_product_that_is_not_every_digest_of_the_broker_image_disagrees() {
    for product in [
        "pkg:oci/andromeda@sha256:5bd5ab35814f86783561603ebb35d5d5d99006dcdcd5c3f828ea1afb4c12d159?repository_url=ghcr.io/example/andromeda",
        "pkg:oci/andromeda",
        "pkg:oci/andromedaa?repository_url=ghcr.io/example/andromeda",
        "pkg:oci/andromeda?repository_url=ghcr.io/other/andromeda",
        "pkg:oci/andromeda?repository_url=docker.io/example/andromeda",
        "pkg:docker/andromeda?repository_url=ghcr.io/example/andromeda",
        "ghcr.io/example/andromeda",
    ] {
        assert_eq!(
            disagreements(&parsed(&[ruling_naming(product, &[CRATE])]), &ignores(), &locked(), &image()),
            [Disagreement::ProductNotImage {
                advisory: ADVISORY,
                product,
                image: IMAGE,
            }],
        );
    }
}

#[test]
fn a_product_without_a_crate_inside_it_disagrees() {
    for statement in [ruling_naming(IMAGE, &[]), ruling_with("products", json!([{ "@id": IMAGE }]))] {
        assert_eq!(
            disagreements(&parsed(&[statement]), &ignores(), &locked(), &image()),
            [Disagreement::NoSubcomponent {
                advisory: ADVISORY,
                product: IMAGE
            }],
        );
    }
}

#[test]
fn a_subcomponent_that_is_no_crate_at_one_version_disagrees() {
    let subcomponents = ["pkg:cargo/rustls-webpki", "pkg:npm/rustls-webpki@0.102.8", "rustls-webpki 0.102.8"];
    assert_eq!(
        disagreements(&parsed(&[ruling_naming(IMAGE, &subcomponents)]), &ignores(), &locked(), &image()),
        subcomponents.map(|subcomponent| Disagreement::SubcomponentNotCrate {
            advisory: ADVISORY,
            subcomponent
        }),
    );
}

#[test]
fn a_crate_version_the_lockfile_does_not_hold_disagrees() {
    let subcomponents = ["pkg:cargo/rustls-webpki@0.102.9", "pkg:cargo/webpki@0.102.8"];
    assert_eq!(
        disagreements(&parsed(&[ruling_naming(IMAGE, &subcomponents)]), &ignores(), &locked(), &image()),
        subcomponents.map(|subcomponent| Disagreement::NotLocked {
            advisory: ADVISORY,
            subcomponent
        }),
    );
}

#[test]
fn a_crate_linking_the_crate_version_that_the_impact_statement_does_not_name_disagrees() {
    assert_eq!(
        disagreements(&parsed(&[ruling()]), &ignores(), &locked_with(&[("reqwest", "0.13.5")]), &image()),
        [Disagreement::DependentUnnamed {
            advisory: ADVISORY,
            subcomponent: CRATE,
            dependent: "reqwest",
            version: "0.13.5",
        }],
    );
}

#[test]
fn a_crate_linking_the_crate_version_that_the_impact_statement_names_agrees() {
    let reason = format!("{REASON}, nor does reqwest 0.13.5");
    let ignores = BTreeMap::from([(ADVISORY.to_owned(), Some(reason.clone()))]);
    let statement = ruling_with("impact_statement", json!(reason));
    assert_eq!(
        disagreements(&parsed(&[statement]), &ignores, &locked_with(&[("reqwest", "0.13.5")]), &image()),
        Vec::<Disagreement>::new()
    );
}

#[test]
fn a_crate_version_is_named_only_as_whole_words() {
    for (text, named) in [
        ("rumqttc 0.25.1, the only crate linking it", true),
        ("the crate rumqttc 0.25.1.", true),
        ("rumqttc 0.25.1", true),
        ("rumqttc 0.25.10 links it", false),
        ("xrumqttc 0.25.1 links it", false),
        ("old-rumqttc 0.25.1 links it", false),
        ("rumqttc 0.25.2 links it", false),
        ("rumqttc links it", false),
    ] {
        assert_eq!(names_crate_version(text, "rumqttc", "0.25.1"), named, "{text}");
    }
}

#[test]
fn disagreements_are_listed_in_statement_order() {
    let other = "RUSTSEC-2026-0049";
    let mut statement = ruling_with("status", json!("affected"));
    statement["vulnerability"]["name"] = json!(other);
    assert_eq!(
        disagreements(&parsed(&[statement, ruling_without("justification")]), &ignores(), &locked(), &image()),
        [
            Disagreement::NotIgnored { advisory: other },
            Disagreement::NotNotAffected {
                advisory: other,
                status: Status::Affected,
            },
            Disagreement::Unjustified { advisory: ADVISORY },
        ],
    );
}

/// A workspace published under the owner `example`, whose `deny.toml` ignores
/// the advisory, whose lockfile holds `locked_version` of the crate, and whose
/// VEX document holds `statements`.
fn scratch_workspace(name: &str, locked_version: &str, statements: Option<&[Value]>) -> String {
    let root = env::temp_dir().join(format!("andromeda-advisory-rulings-{}-{name}", process::id()));
    fs::create_dir_all(root.join(".vex")).unwrap();
    fs::write(root.join("Cargo.toml"), MANIFEST).unwrap();
    fs::write(
        root.join("deny.toml"),
        format!("[advisories]\nignore = [\n    {{ id = \"{ADVISORY}\", reason = \"{REASON}\" }},\n]\n"),
    )
    .unwrap();
    fs::write(root.join("Cargo.lock"), lockfile(&[]).replace("0.102.8", locked_version)).unwrap();
    if let Some(statements) = statements {
        fs::write(
            root.join(".vex/andromeda.openvex.json"),
            serde_json::to_string_pretty(&document(statements)).unwrap(),
        )
        .unwrap();
    }
    root.to_string_lossy().into_owned()
}

#[test]
fn a_workspace_whose_statements_repeat_its_rulings_passes() {
    assert_vex_statements_repeat_advisory_rulings(&scratch_workspace("agree", "0.102.8", Some(&[ruling()])));
}

#[test]
#[should_panic(expected = "the VEX statement of RUSTSEC-2026-0104 names pkg:cargo/rustls-webpki@0.102.8, but Cargo.lock holds no such crate version")]
fn a_workspace_whose_lockfile_moved_the_crate_fails() {
    assert_vex_statements_repeat_advisory_rulings(&scratch_workspace("moved", "0.102.9", Some(&[ruling()])));
}

#[test]
#[should_panic(expected = ".vex/andromeda.openvex.json could not be read")]
fn a_workspace_without_a_vex_document_fails() {
    assert_vex_statements_repeat_advisory_rulings(&scratch_workspace("absent", "0.102.8", None));
}

#[test]
#[should_panic(
    expected = "the VEX statement of RUSTSEC-2026-0104 names pkg:oci/andromeda?repository_url=ghcr.io/other/andromeda, but a scanner matches every digest of the broker image only as pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda"
)]
fn a_workspace_whose_statement_names_another_owner_fails() {
    let statement = ruling_naming("pkg:oci/andromeda?repository_url=ghcr.io/other/andromeda", &[CRATE]);
    assert_vex_statements_repeat_advisory_rulings(&scratch_workspace("owner", "0.102.8", Some(&[statement])));
}

#[test]
#[should_panic(
    expected = "the VEX statement of RUSTSEC-2026-0104 names pkg:cargo/rustls-webpki@0.102.8, which rumqttc 0.25.2 links, but its impact statement does not name rumqttc 0.25.2"
)]
fn a_workspace_whose_lockfile_moved_the_linking_crate_fails() {
    let root = scratch_workspace("dependent", "0.102.8", Some(&[ruling()]));
    let lockfile = fs::read_to_string(format!("{root}/Cargo.lock")).unwrap();
    fs::write(format!("{root}/Cargo.lock"), lockfile.replace("0.25.1", "0.25.2")).unwrap();
    assert_vex_statements_repeat_advisory_rulings(&root);
}
