use crate::advisory_rulings::{
    assert_vex_document_is_generated_from_advisory_rulings,
    broker_image::BrokerImage,
    disagreement::Disagreement,
    disagreements,
    locked_crates::LockedCrates,
    rendering::{document_text, vex_document},
    rulings_file::{Ruling, read_rulings},
    source_repository::SourceRepository,
    vex_document::VexDocument,
};
use jiff::Timestamp;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, fmt::Write, fs, process};
use toml_edit::DocumentMut;
use url::Url;

const ADVISORY: &str = "RUSTSEC-2026-0104";

const CVE: &str = "CVE-2026-93599";

const GHSA: &str = "GHSA-82j2-j2ch-gfr8";

const REASON: &str = "rustls-webpki 0.102 panics parsing a revocation list, which rumqttc 0.25.1, the only crate linking it, never calls";

const OCI: &str = "pkg:oci/andromeda?repository_url=ghcr.io/example/andromeda";

const DOCKER: &str = "pkg:docker/example/andromeda?repository_url=ghcr.io";

const CRATE: &str = "pkg:cargo/rustls-webpki@0.102.8";

const MANIFEST: &str = "[workspace.package]\nrepository = \"https://github.com/example/andromeda\"\n";

const DOCUMENT_ID: &str = "https://github.com/example/andromeda/blob/main/.vex/andromeda.openvex.json";

/// The rulings file ruling on the advisory with `justification`.
fn rulings_file(justification: &str) -> String {
    format!("[{ADVISORY}]\naliases = [\"{CVE}\", \"{GHSA}\"]\njustification = \"{justification}\"\ncrates = [\"rustls-webpki@0.102.8\"]\n")
}

fn rulings_with(file: &str) -> BTreeMap<String, Ruling> {
    let mut violations = Vec::new();
    let rulings = read_rulings(&file.parse::<DocumentMut>().unwrap(), "rulings.toml", &mut violations);
    assert_eq!(violations, Vec::<String>::new());
    rulings
}

fn rulings() -> BTreeMap<String, Ruling> {
    rulings_with(&rulings_file("vulnerable_code_not_in_execute_path"))
}

fn ignores_with(reason: &str) -> BTreeMap<String, Option<String>> {
    BTreeMap::from([(ADVISORY.to_owned(), Some(reason.to_owned()))])
}

fn ignores() -> BTreeMap<String, Option<String>> {
    ignores_with(REASON)
}

/// A lockfile holding rustls-webpki 0.102.8 and 0.103.17, where rumqttc 0.25.1
/// and every package `dependents` names, each at its version, link 0.102.8.
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

fn locked_from(lockfile: &str) -> LockedCrates {
    LockedCrates::from_document(&lockfile.parse::<DocumentMut>().unwrap())
}

fn locked() -> LockedCrates {
    locked_from(&lockfile(&[]))
}

fn source() -> SourceRepository {
    SourceRepository::from_manifest(&MANIFEST.parse::<DocumentMut>().unwrap()).unwrap()
}

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).unwrap()
}

fn document_id() -> Url {
    Url::parse(DOCUMENT_ID).unwrap()
}

/// The document generated at `now` from `rulings` and `ignores`, carrying over
/// from `existing`.
fn generated(rulings: &BTreeMap<String, Ruling>, ignores: &BTreeMap<String, Option<String>>, existing: Option<&VexDocument>, now: Timestamp) -> VexDocument {
    vex_document(rulings, ignores, &BrokerImage::published_by(&source()), document_id(), existing, now).unwrap()
}

/// The statement of `name` as the generated document spells it, dated
/// `timestamp`.
fn statement(name: &str, aliases: &[&str], timestamp: &str) -> Value {
    json!({
        "vulnerability": { "name": name, "aliases": aliases },
        "timestamp": timestamp,
        "products": [
            { "@id": OCI, "subcomponents": [{ "@id": CRATE }] },
            { "@id": DOCKER, "subcomponents": [{ "@id": CRATE }] },
        ],
        "status": "not_affected",
        "justification": "vulnerable_code_not_in_execute_path",
        "impact_statement": REASON,
    })
}

#[test]
fn rulings_that_agree_with_deny_toml_and_the_lockfile_raise_nothing() {
    assert_eq!(disagreements(&ignores(), &rulings(), &locked()), Vec::<Disagreement>::new());
}

#[test]
fn an_ignore_without_a_ruling_and_a_ruling_without_an_ignore_disagree() {
    let mut ignores = ignores();
    ignores.insert("RUSTSEC-2024-0370".to_owned(), Some("build-time only".to_owned()));
    let mut rulings = rulings();
    rulings.extend(rulings_with(
        "[RUSTSEC-2026-0049]\njustification = \"vulnerable_code_not_present\"\ncrates = [\"rustls-webpki@0.102.8\"]\n",
    ));
    assert_eq!(
        disagreements(&ignores, &rulings, &locked()),
        [
            Disagreement::IgnoreWithoutRuling { advisory: "RUSTSEC-2024-0370" },
            Disagreement::RulingNotIgnored { advisory: "RUSTSEC-2026-0049" },
        ]
    );
}

#[test]
fn an_ignore_without_a_reason_disagrees() {
    let ignores = BTreeMap::from([(ADVISORY.to_owned(), None)]);
    assert_eq!(
        disagreements(&ignores, &rulings(), &locked()),
        [Disagreement::IgnoredWithoutReason { advisory: ADVISORY }]
    );
}

#[test]
fn an_identifier_two_rulings_name_disagrees() {
    let mut ignores = ignores();
    ignores.insert("RUSTSEC-2026-0049".to_owned(), Some("unreachable".to_owned()));
    let mut rulings = rulings();
    rulings.extend(rulings_with(&format!(
        "[RUSTSEC-2026-0049]\naliases = [\"{CVE}\"]\njustification = \"vulnerable_code_not_present\"\ncrates = [\"rustls-webpki@0.102.8\"]\n"
    )));
    assert_eq!(
        disagreements(&ignores, &rulings, &locked()),
        [Disagreement::IdentifierRepeated { identifier: CVE }]
    );
}

#[test]
fn a_crate_version_the_lockfile_does_not_hold_disagrees() {
    let rulings = rulings_with(&rulings_file("vulnerable_code_not_present").replace("0.102.8", "0.102.9"));
    assert_eq!(
        disagreements(&ignores(), &rulings, &locked()),
        [Disagreement::NotLocked {
            advisory: ADVISORY,
            name: "rustls-webpki",
            version: "0.102.9",
        }]
    );
}

#[test]
fn a_crate_linking_the_version_that_a_ruling_on_its_callers_does_not_name_disagrees() {
    for justification in ["vulnerable_code_not_in_execute_path", "vulnerable_code_cannot_be_controlled_by_adversary"] {
        assert_eq!(
            disagreements(
                &ignores(),
                &rulings_with(&rulings_file(justification)),
                &locked_from(&lockfile(&[("reqwest", "0.13.5")]))
            ),
            [Disagreement::DependentUnnamed {
                advisory: ADVISORY,
                name: "rustls-webpki",
                version: "0.102.8",
                dependent: "reqwest",
                dependent_version: "0.13.5",
            }],
            "{justification}"
        );
    }
}

#[test]
fn a_crate_linking_the_version_that_the_reason_names_agrees() {
    let ignores = ignores_with(&format!("{REASON}, nor does reqwest 0.13.5"));
    assert_eq!(
        disagreements(&ignores, &rulings(), &locked_from(&lockfile(&[("reqwest", "0.13.5")]))),
        Vec::<Disagreement>::new()
    );
}

#[test]
fn a_ruling_resting_on_the_crate_itself_needs_no_linking_crate_named() {
    for justification in ["component_not_present", "vulnerable_code_not_present", "inline_mitigations_already_exist"] {
        assert_eq!(
            disagreements(
                &ignores(),
                &rulings_with(&rulings_file(justification)),
                &locked_from(&lockfile(&[("reqwest", "0.13.5")]))
            ),
            Vec::<Disagreement>::new(),
            "{justification}"
        );
    }
}

/// The ignores with a reason that relies on rustls-webpki 0.103.16 or later.
fn floored() -> BTreeMap<String, Option<String>> {
    ignores_with(&format!("{REASON}, and the broker verifies with rustls-webpki 0.103.16 or later"))
}

#[test]
fn a_floor_every_locked_version_reaches_or_the_ruling_is_about_agrees() {
    for version in ["0.103.16", "0.103.17", "0.104.0"] {
        assert_eq!(
            disagreements(&floored(), &rulings(), &locked_from(&lockfile(&[]).replace("0.103.17", version))),
            Vec::<Disagreement>::new(),
            "{version}"
        );
    }
}

#[test]
fn a_locked_version_below_the_floor_the_ruling_is_not_about_disagrees() {
    assert_eq!(
        disagreements(&floored(), &rulings(), &locked_from(&lockfile(&[]).replace("0.103.17", "0.103.15"))),
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
    let unpatched = lockfile(&[]).replace("[[package]]\nname = \"rustls-webpki\"\nversion = \"0.103.17\"\n", "");
    assert_eq!(
        disagreements(&floored(), &rulings(), &locked_from(&unpatched)),
        [Disagreement::FloorNotLocked {
            advisory: ADVISORY,
            name: "rustls-webpki",
            floor: "0.103.16",
        }]
    );
}

#[test]
fn an_advisory_is_stated_once_under_each_identifier_with_the_others_as_aliases() {
    let document = serde_json::to_value(generated(&rulings(), &ignores(), None, at(1_790_000_000))).unwrap();
    let now = "2026-09-21T14:13:20Z";
    assert_eq!(
        document,
        json!({
            "@context": "https://openvex.dev/ns/v0.2.0",
            "@id": DOCUMENT_ID,
            "author": "Andromeda",
            "timestamp": now,
            "last_updated": now,
            "version": 1,
            "statements": [
                statement(ADVISORY, &[CVE, GHSA], now),
                statement(CVE, &[ADVISORY, GHSA], now),
                statement(GHSA, &[ADVISORY, CVE], now),
            ],
        })
    );
}

#[test]
fn a_ruling_without_aliases_lists_none() {
    let rulings = rulings_with("[RUSTSEC-2026-0104]\njustification = \"vulnerable_code_not_present\"\ncrates = [\"rustls-webpki@0.102.8\"]\n");
    let document = serde_json::to_value(generated(&rulings, &ignores(), None, at(1_790_000_000))).unwrap();
    assert_eq!(document["statements"][0]["vulnerability"], json!({ "name": ADVISORY }));
}

#[test]
fn a_document_regenerated_without_a_change_keeps_its_timestamps_and_version() {
    let existing = generated(&rulings(), &ignores(), None, at(1_790_000_000));
    assert_eq!(generated(&rulings(), &ignores(), Some(&existing), at(1_800_000_000)), existing);
}

#[test]
fn a_changed_reason_dates_its_statements_anew_and_raises_the_version() {
    let mut ignores = ignores();
    ignores.insert("RUSTSEC-2024-0370".to_owned(), Some("build-time only".to_owned()));
    let mut rulings = rulings();
    rulings.extend(rulings_with(
        "[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error@1.0.4\"]\n",
    ));
    let existing = generated(&rulings, &ignores, None, at(1_790_000_000));
    ignores.insert(ADVISORY.to_owned(), Some(format!("{REASON}, and never will")));
    let changed = generated(&rulings, &ignores, Some(&existing), at(1_800_000_000));
    let dates = changed
        .statements
        .iter()
        .map(|statement| (statement.vulnerability.name.as_str(), statement.timestamp))
        .collect::<Vec<_>>();
    assert_eq!(
        dates,
        [
            ("RUSTSEC-2024-0370", at(1_790_000_000)),
            (ADVISORY, at(1_800_000_000)),
            (CVE, at(1_800_000_000)),
            (GHSA, at(1_800_000_000)),
        ]
    );
    assert_eq!(
        (changed.timestamp, changed.last_updated, changed.version),
        (at(1_790_000_000), at(1_800_000_000), 2)
    );
}

#[test]
fn a_removed_ruling_raises_the_version() {
    let mut ignores = ignores();
    ignores.insert("RUSTSEC-2024-0370".to_owned(), Some("build-time only".to_owned()));
    let mut rulings = rulings();
    rulings.extend(rulings_with(
        "[RUSTSEC-2024-0370]\njustification = \"component_not_present\"\ncrates = [\"proc-macro-error@1.0.4\"]\n",
    ));
    let existing = generated(&rulings, &ignores, None, at(1_790_000_000));
    let removed = generated(&self::rulings(), &self::ignores(), Some(&existing), at(1_800_000_000));
    assert_eq!((removed.last_updated, removed.version), (at(1_800_000_000), 2));
    assert_eq!(removed.statements[0].timestamp, at(1_790_000_000));
}

#[test]
fn a_checked_in_text_that_is_no_document_is_generated_afresh() {
    let text = document_text(&rulings(), &ignores(), &source(), ".vex/andromeda.openvex.json", Some("{}")).unwrap();
    let document = serde_json::from_str::<VexDocument>(&text).unwrap();
    assert_eq!((document.version, document.statements.len()), (1, 3));
}

/// A workspace published under the owner `example`, whose `deny.toml` ignores
/// the advisory with `reason` and whose rulings file rules on it, holding the
/// generated VEX document.
fn scratch_workspace(name: &str, reason: &str) -> String {
    let root = env::temp_dir().join(format!("andromeda-advisory-rulings-{}-{name}", process::id()));
    fs::create_dir_all(root.join(".vex")).unwrap();
    fs::write(root.join("Cargo.toml"), MANIFEST).unwrap();
    fs::write(
        root.join("deny.toml"),
        format!("[advisories]\nignore = [\n    {{ id = \"{ADVISORY}\", reason = \"{reason}\" }},\n]\n"),
    )
    .unwrap();
    fs::write(root.join(".vex/rulings.toml"), rulings_file("vulnerable_code_not_in_execute_path")).unwrap();
    fs::write(root.join("Cargo.lock"), lockfile(&[])).unwrap();
    let text = document_text(&rulings(), &ignores_with(reason), &source(), ".vex/andromeda.openvex.json", None).unwrap();
    fs::write(root.join(".vex/andromeda.openvex.json"), text).unwrap();
    root.to_string_lossy().into_owned()
}

#[test]
fn a_workspace_holding_the_generated_document_passes() {
    assert_vex_document_is_generated_from_advisory_rulings(&scratch_workspace("agree", REASON));
}

#[test]
#[should_panic(expected = ".vex/andromeda.openvex.json differs from what is generated for it")]
fn a_workspace_whose_reason_changed_without_the_document_fails() {
    let root = scratch_workspace("stale", REASON);
    let configuration = fs::read_to_string(format!("{root}/deny.toml")).unwrap();
    fs::write(format!("{root}/deny.toml"), configuration.replace(REASON, &format!("{REASON}, and never will"))).unwrap();
    assert_vex_document_is_generated_from_advisory_rulings(&root);
}

#[test]
#[should_panic(expected = "the ruling on RUSTSEC-2026-0104 names rustls-webpki@0.102.8, but Cargo.lock holds no such crate version")]
fn a_workspace_whose_lockfile_moved_the_crate_fails() {
    let root = scratch_workspace("moved", REASON);
    fs::write(format!("{root}/Cargo.lock"), lockfile(&[]).replace("0.102.8", "0.102.9")).unwrap();
    assert_vex_document_is_generated_from_advisory_rulings(&root);
}

#[test]
#[should_panic(
    expected = "the ruling on RUSTSEC-2026-0104 is about rustls-webpki 0.102.8, which rumqttc 0.25.2 links, but the reason deny.toml gives does not name rumqttc 0.25.2"
)]
fn a_workspace_whose_lockfile_moved_the_linking_crate_fails() {
    let root = scratch_workspace("dependent", REASON);
    fs::write(format!("{root}/Cargo.lock"), lockfile(&[]).replace("0.25.1", "0.25.2")).unwrap();
    assert_vex_document_is_generated_from_advisory_rulings(&root);
}

#[test]
#[should_panic(expected = "Cargo.lock holds rustls-webpki 0.103.15, which the ruling on RUSTSEC-2026-0104 is not about")]
fn a_workspace_whose_lockfile_moved_a_crate_below_its_floor_fails() {
    let root = scratch_workspace("floor", &format!("{REASON}, and the broker verifies with rustls-webpki 0.103.16 or later"));
    fs::write(format!("{root}/Cargo.lock"), lockfile(&[]).replace("0.103.17", "0.103.15")).unwrap();
    assert_vex_document_is_generated_from_advisory_rulings(&root);
}

#[test]
#[should_panic(expected = "deny.toml ignores RUSTSEC-2026-0104, but the rulings file has no ruling on it")]
fn a_workspace_whose_ignore_has_no_ruling_fails() {
    let root = scratch_workspace("unruled", REASON);
    fs::write(format!("{root}/.vex/rulings.toml"), "").unwrap();
    assert_vex_document_is_generated_from_advisory_rulings(&root);
}
