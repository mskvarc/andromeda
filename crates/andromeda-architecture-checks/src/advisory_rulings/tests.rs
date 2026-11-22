use crate::advisory_rulings::{
    assert_vex_statements_repeat_advisory_rulings,
    broker_image::BrokerImage,
    disagreement::Disagreement,
    disagreements,
    locked_crates::LockedCrates,
    vex_document::{Status, VexDocument},
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, fmt::Write, fs, process};
use toml_edit::DocumentMut;

const ADVISORY: &str = "RUSTSEC-2026-0104";

const ALIAS: &str = "GHSA-82j2-j2ch-gfr8";

const REASON: &str = "rustls-webpki 0.102 panics parsing a revocation list, which rumqttc 0.25.1, the only crate linking it, never calls";

const OCI: &str = "pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda";

const DOCKER: &str = "pkg:docker/example/andromeda?repository_url=ghcr.io";

const CRATE: &str = "pkg:cargo/rustls-webpki@0.102.8";

const MANIFEST: &str = "[workspace.package]\nrepository = \"https://github.com/example/andromeda\"\n";

fn ruling() -> Value {
    json!({
        "vulnerability": { "name": ADVISORY },
        "timestamp": "2026-11-21T19:47:08Z",
        "products": [
            { "@id": OCI, "subcomponents": [{ "@id": CRATE }] },
            { "@id": DOCKER, "subcomponents": [{ "@id": CRATE }] },
        ],
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

/// The ruling named `name`, listing `aliases`.
fn ruling_named(name: &str, aliases: &[&str]) -> Value {
    ruling_with("vulnerability", json!({ "name": name, "aliases": aliases }))
}

/// The ruling with `products`, each a package URL and the crates inside it.
fn ruling_naming(products: &[(&str, &[&str])]) -> Value {
    let products: Vec<Value> = products
        .iter()
        .map(|(product, subcomponents)| {
            let subcomponents: Vec<Value> = subcomponents.iter().map(|id| json!({ "@id": id })).collect();
            json!({ "@id": product, "subcomponents": subcomponents })
        })
        .collect();
    ruling_with("products", json!(products))
}

/// The ruling with `subcomponents` inside both forms of the image.
fn ruling_naming_crates(subcomponents: &[&str]) -> Value {
    ruling_naming(&[(OCI, subcomponents), (DOCKER, subcomponents)])
}

#[test]
fn a_statement_repeating_an_ignore_for_a_locked_crate_agrees() {
    assert_eq!(disagreements(&parsed(&[ruling()]), &ignores(), &locked(), &image()), Vec::<Disagreement>::new());
}

#[test]
fn statements_under_every_identifier_of_an_ignored_advisory_agree() {
    let statements = [ruling_named(ADVISORY, &[ALIAS]), ruling_named(ALIAS, &[ADVISORY])];
    assert_eq!(disagreements(&parsed(&statements), &ignores(), &locked(), &image()), Vec::<Disagreement>::new());
}

#[test]
fn an_ignore_without_a_statement_disagrees() {
    let mut ignores = ignores();
    ignores.insert("RUSTSEC-2024-0370".to_owned(), Some("build-time only".to_owned()));
    assert_eq!(
        disagreements(&parsed(&[ruling()]), &ignores, &locked(), &image()),
        [Disagreement::IgnoreWithoutStatement { advisory: "RUSTSEC-2024-0370" }]
    );
}

#[test]
fn an_ignore_only_an_alias_names_is_reported_as_the_alias_without_a_statement() {
    assert_eq!(
        disagreements(&parsed(&[ruling_named(ALIAS, &[ADVISORY])]), &ignores(), &locked(), &image()),
        [Disagreement::AliasWithoutStatement {
            advisory: ALIAS,
            alias: ADVISORY
        }]
    );
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
        [Disagreement::IgnoredWithoutReason {
            advisory: ADVISORY,
            ignored: ADVISORY
        }]
    );
}

#[test]
fn an_impact_statement_other_than_the_ignore_reason_disagrees() {
    let statement = ruling_with("impact_statement", json!("rustls-webpki 0.102 is linked by rumqttc 0.25.1 alone"));
    assert_eq!(
        disagreements(&parsed(&[statement]), &ignores(), &locked(), &image()),
        [Disagreement::ImpactDiffers {
            advisory: ADVISORY,
            ignored: ADVISORY
        }]
    );
}

#[test]
fn an_impact_statement_other_than_the_reason_the_alias_is_ignored_for_disagrees() {
    let mut aliased = ruling_named(ALIAS, &[ADVISORY]);
    aliased["impact_statement"] = json!("rustls-webpki 0.102 is linked by rumqttc 0.25.1 alone");
    assert_eq!(
        disagreements(&parsed(&[ruling_named(ADVISORY, &[ALIAS]), aliased]), &ignores(), &locked(), &image()),
        [Disagreement::ImpactDiffers {
            advisory: ALIAS,
            ignored: ADVISORY
        }]
    );
}

#[test]
fn a_statement_without_an_impact_statement_disagrees() {
    assert_eq!(
        disagreements(&parsed(&[ruling_without("impact_statement")]), &ignores(), &locked(), &image()),
        [
            Disagreement::ImpactDiffers {
                advisory: ADVISORY,
                ignored: ADVISORY
            },
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
fn an_alias_without_a_statement_of_its_own_disagrees() {
    assert_eq!(
        disagreements(&parsed(&[ruling_named(ADVISORY, &[ALIAS])]), &ignores(), &locked(), &image()),
        [Disagreement::AliasWithoutStatement {
            advisory: ADVISORY,
            alias: ALIAS
        }]
    );
}

#[test]
fn statements_of_one_advisory_listing_other_identifiers_disagree() {
    let other = "CVE-2026-93599";
    let statements = [ruling_named(ADVISORY, &[ALIAS, other]), ruling_named(ALIAS, &[ADVISORY])];
    assert_eq!(
        disagreements(&parsed(&statements), &ignores(), &locked(), &image()),
        [
            Disagreement::AliasesDiffer {
                advisory: ADVISORY,
                alias: ALIAS
            },
            Disagreement::AliasWithoutStatement {
                advisory: ADVISORY,
                alias: other
            },
            Disagreement::AliasesDiffer {
                advisory: ALIAS,
                alias: ADVISORY
            },
        ]
    );
}

#[test]
fn statements_of_one_advisory_ruling_otherwise_disagree() {
    let mut justified_otherwise = ruling_named(ALIAS, &[ADVISORY]);
    justified_otherwise["justification"] = json!("vulnerable_code_cannot_be_controlled_by_adversary");
    let mut crated_otherwise = ruling_named(ALIAS, &[ADVISORY]);
    crated_otherwise["products"] = ruling_naming_crates(&[CRATE, "pkg:cargo/rustls-webpki@0.103.17"])["products"].clone();
    for aliased in [justified_otherwise, crated_otherwise] {
        assert_eq!(
            disagreements(&parsed(&[ruling_named(ADVISORY, &[ALIAS]), aliased]), &ignores(), &locked(), &image()),
            [
                Disagreement::AliasRulesOtherwise {
                    advisory: ADVISORY,
                    alias: ALIAS
                },
                Disagreement::AliasRulesOtherwise {
                    advisory: ALIAS,
                    alias: ADVISORY
                },
            ]
        );
    }
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
fn a_statement_without_a_timestamp_of_its_own_disagrees() {
    assert_eq!(
        disagreements(&parsed(&[ruling_without("timestamp")]), &ignores(), &locked(), &image()),
        [Disagreement::Undated { advisory: ADVISORY }],
    );
}

#[test]
fn a_status_justification_or_timestamp_the_specification_does_not_define_does_not_parse() {
    for statement in [
        ruling_with("status", json!("not-affected")),
        ruling_with("justification", json!("unreachable")),
        ruling_with("timestamp", json!("21 November 2026")),
    ] {
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
        "pkg:oci/andromeda?arch=amd64&repository_url=ghcr.io/example/andromeda",
        "pkg:docker/example/andromeda?arch=arm64&repository_url=ghcr.io",
        "pkg:oci/andromeda",
        "pkg:oci/andromedaa?repository_url=ghcr.io/example/andromeda",
        "pkg:oci/andromeda?repository_url=ghcr.io/other/andromeda",
        "pkg:oci/andromeda?repository_url=docker.io/example/andromeda",
        "pkg:docker/andromeda?repository_url=ghcr.io/example/andromeda",
        "pkg:docker/other/andromeda?repository_url=ghcr.io",
        "pkg:docker/example/andromeda",
        "pkg:docker/example/andromeda@1.0.0?repository_url=ghcr.io",
        "pkg:docker/ghcr.io/example/andromeda",
        "ghcr.io/example/andromeda",
    ] {
        let statement = ruling_naming(&[(OCI, &[CRATE]), (DOCKER, &[CRATE]), (product, &[CRATE])]);
        assert_eq!(
            disagreements(&parsed(&[statement]), &ignores(), &locked(), &image()),
            [Disagreement::ProductNotImage {
                advisory: ADVISORY,
                product,
                oci: OCI,
                docker: DOCKER,
            }],
        );
    }
}

#[test]
fn a_statement_naming_the_broker_image_in_one_form_alone_disagrees() {
    for (named, missing) in [(OCI, DOCKER), (DOCKER, OCI)] {
        assert_eq!(
            disagreements(&parsed(&[ruling_naming(&[(named, &[CRATE])])]), &ignores(), &locked(), &image()),
            [Disagreement::ImageFormMissing {
                advisory: ADVISORY,
                form: missing
            }],
        );
    }
}

#[test]
fn a_product_without_a_crate_inside_it_disagrees() {
    for statement in [
        ruling_naming(&[(OCI, &[]), (DOCKER, &[CRATE])]),
        ruling_with("products", json!([{ "@id": OCI }, { "@id": DOCKER, "subcomponents": [{ "@id": CRATE }] }])),
    ] {
        assert_eq!(
            disagreements(&parsed(&[statement]), &ignores(), &locked(), &image()),
            [Disagreement::NoSubcomponent {
                advisory: ADVISORY,
                product: OCI
            }],
        );
    }
}

#[test]
fn a_subcomponent_that_is_no_crate_at_one_version_disagrees_once() {
    let subcomponents = ["pkg:cargo/rustls-webpki", "pkg:npm/rustls-webpki@0.102.8", "rustls-webpki 0.102.8"];
    assert_eq!(
        disagreements(&parsed(&[ruling_naming_crates(&subcomponents)]), &ignores(), &locked(), &image()),
        subcomponents.map(|subcomponent| Disagreement::SubcomponentNotCrate {
            advisory: ADVISORY,
            subcomponent
        }),
    );
}

#[test]
fn a_crate_version_the_lockfile_does_not_hold_disagrees_once() {
    let subcomponents = ["pkg:cargo/rustls-webpki@0.102.9", "pkg:cargo/webpki@0.102.8"];
    assert_eq!(
        disagreements(&parsed(&[ruling_naming_crates(&subcomponents)]), &ignores(), &locked(), &image()),
        subcomponents.map(|subcomponent| Disagreement::NotLocked {
            advisory: ADVISORY,
            subcomponent
        }),
    );
}

#[test]
fn a_crate_linking_the_crate_version_that_a_ruling_on_its_callers_does_not_name_disagrees() {
    for justification in ["vulnerable_code_not_in_execute_path", "vulnerable_code_cannot_be_controlled_by_adversary"] {
        let statement = ruling_with("justification", json!(justification));
        assert_eq!(
            disagreements(&parsed(&[statement]), &ignores(), &locked_with(&[("reqwest", "0.13.5")]), &image()),
            [Disagreement::DependentUnnamed {
                advisory: ADVISORY,
                subcomponent: CRATE,
                dependent: "reqwest",
                version: "0.13.5",
            }],
            "{justification}"
        );
    }
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
fn a_ruling_resting_on_the_crate_itself_needs_no_linking_crate_named() {
    for justification in ["component_not_present", "vulnerable_code_not_present", "inline_mitigations_already_exist"] {
        let statement = ruling_with("justification", json!(justification));
        assert_eq!(
            disagreements(&parsed(&[statement]), &ignores(), &locked_with(&[("reqwest", "0.13.5")]), &image()),
            Vec::<Disagreement>::new(),
            "{justification}"
        );
    }
}

/// The ignores and the ruling with an impact statement that relies on
/// rustls-webpki 0.103.16 or later.
fn floored() -> (BTreeMap<String, Option<String>>, Value) {
    let reason = format!("{REASON}, and the broker verifies with rustls-webpki 0.103.16 or later");
    let ignores = BTreeMap::from([(ADVISORY.to_owned(), Some(reason.clone()))]);
    (ignores, ruling_with("impact_statement", json!(reason)))
}

/// The lockfile of [`lockfile`] with its rustls-webpki 0.103 entry at
/// `version`.
fn locked_with_patched(version: &str) -> LockedCrates {
    LockedCrates::from_document(&lockfile(&[]).replace("0.103.17", version).parse::<DocumentMut>().unwrap())
}

#[test]
fn a_floor_every_locked_version_reaches_or_the_statement_rules_on_agrees() {
    let (ignores, statement) = floored();
    let document = parsed(&[statement]);
    for version in ["0.103.16", "0.103.17", "0.104.0"] {
        assert_eq!(
            disagreements(&document, &ignores, &locked_with_patched(version), &image()),
            Vec::<Disagreement>::new(),
            "{version}"
        );
    }
}

#[test]
fn a_locked_version_below_the_floor_the_statement_does_not_rule_on_disagrees() {
    let (ignores, statement) = floored();
    assert_eq!(
        disagreements(&parsed(&[statement]), &ignores, &locked_with_patched("0.103.15"), &image()),
        [
            Disagreement::FloorNotLocked {
                advisory: ADVISORY,
                name: "rustls-webpki",
                floor: "0.103.16",
            },
            Disagreement::BelowFloor {
                advisory: ADVISORY,
                name: "rustls-webpki",
                version: "0.103.15",
                floor: "0.103.16",
            },
        ]
    );
}

#[test]
fn a_floor_no_locked_version_reaches_disagrees() {
    let (ignores, statement) = floored();
    let unpatched = lockfile(&[]).replace("[[package]]\nname = \"rustls-webpki\"\nversion = \"0.103.17\"\n", "");
    let locked = LockedCrates::from_document(&unpatched.parse::<DocumentMut>().unwrap());
    assert_eq!(
        disagreements(&parsed(&[statement]), &ignores, &locked, &image()),
        [Disagreement::FloorNotLocked {
            advisory: ADVISORY,
            name: "rustls-webpki",
            floor: "0.103.16",
        }]
    );
}

#[test]
#[should_panic(
    expected = "Cargo.lock holds rustls-webpki 0.103.15, which the VEX statement of RUSTSEC-2026-0104 does not rule on, though its impact statement relies on rustls-webpki 0.103.16 or later"
)]
fn a_workspace_whose_lockfile_moved_a_crate_below_its_floor_fails() {
    let root = scratch_workspace("floor", "0.102.8", Some(&[floored().1]));
    let configuration = fs::read_to_string(format!("{root}/deny.toml")).unwrap();
    fs::write(
        format!("{root}/deny.toml"),
        configuration.replace(REASON, &format!("{REASON}, and the broker verifies with rustls-webpki 0.103.16 or later")),
    )
    .unwrap();
    let lockfile = fs::read_to_string(format!("{root}/Cargo.lock")).unwrap();
    fs::write(format!("{root}/Cargo.lock"), lockfile.replace("0.103.17", "0.103.15")).unwrap();
    assert_vex_statements_repeat_advisory_rulings(&root);
}

#[test]
fn disagreements_are_listed_in_statement_order_before_the_ignores_no_statement_names() {
    let other = "RUSTSEC-2026-0049";
    let mut statement = ruling_with("status", json!("affected"));
    statement["vulnerability"]["name"] = json!(other);
    let mut ignores = ignores();
    ignores.insert("RUSTSEC-2024-0370".to_owned(), Some("build-time only".to_owned()));
    assert_eq!(
        disagreements(&parsed(&[statement, ruling_without("justification")]), &ignores, &locked(), &image()),
        [
            Disagreement::NotIgnored { advisory: other },
            Disagreement::NotNotAffected {
                advisory: other,
                status: Status::Affected,
            },
            Disagreement::Unjustified { advisory: ADVISORY },
            Disagreement::IgnoreWithoutStatement { advisory: "RUSTSEC-2024-0370" },
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
    expected = "the VEX statement of RUSTSEC-2026-0104 names pkg:oci/andromeda?repository_url=ghcr.io/other/andromeda, but scanners match every digest of the broker image only as pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda or pkg:docker/example/andromeda?repository_url=ghcr.io"
)]
fn a_workspace_whose_statement_names_another_owner_fails() {
    let statement = ruling_naming(&[("pkg:oci/andromeda?repository_url=ghcr.io/other/andromeda", &[CRATE]), (DOCKER, &[CRATE])]);
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

#[test]
#[should_panic(expected = "deny.toml ignores RUSTSEC-2026-0104, but no VEX statement names it")]
fn a_workspace_whose_ignore_has_no_statement_fails() {
    assert_vex_statements_repeat_advisory_rulings(&scratch_workspace("unstated", "0.102.8", Some(&[])));
}
