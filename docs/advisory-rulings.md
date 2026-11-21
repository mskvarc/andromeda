# Advisory rulings

`deny.toml` ignores some advisories because they can't affect the broker, and `.vex/andromeda.openvex.json` tells scanners the same thing. This page holds the proof behind each such ruling. The proof reads source code, so it holds for the lockfile it was made against, which locks rumqttc 0.25.1, rustls-webpki 0.102.8 and rustls-webpki 0.103.17. The gate `advisory_rulings` fails once the lockfile adds a crate that links an affected version, or moves one to another version, and that is when the proof has to be redone, as [Redoing a proof](#redoing-a-proof) describes.

## rustls-webpki 0.102

Five advisories concern rustls-webpki 0.102.8, and none of them affects the broker, because no code in the broker ever runs rustls-webpki 0.102. Every certificate the broker checks goes through rustls 0.23 and rustls-webpki 0.103.17, which carries the fix for all five.

### What links it

`cargo tree -p andromeda -i rustls-webpki@0.102.8 -e normal,build --locked --target x86_64-unknown-linux-gnu` lists rumqttc 0.25.1 as the only package depending on it, and `--target aarch64-unknown-linux-gnu` gives the same tree. The dependency comes in through rumqttc's default `use-rustls` feature, which enables `use-rustls-no-provider` and with it `dep:rustls-webpki`. `andromeda-delivery` enables `use-rustls-no-provider` for MQTT notifications.

### What rumqttc does with it

The crate's library name is `webpki`. Searching rumqttc 0.25.1 for `webpki` finds the feature lines of its manifest and one line of code, `WebPki(#[from] webpki::Error)` at `src/tls.rs:42`, a variant of its `TlsError`.

- rumqttc has no `use` of `webpki`, calls none of its functions and re-exports nothing from it.
- Every `?` in `src/tls.rs` converts another error type. rustls-pemfile returns `io::Error`, `with_client_auth_cert` returns `rustls::Error`, `ServerName::try_from` returns `InvalidDnsNameError` and the connection returns `io::Error`.

So no value of a rustls-webpki 0.102 type is ever made, and the variant only gives the compiler derived `Display`, `Debug` and drop code that never runs. No other route exists either. rustls-pki-types, the one crate rustls 0.23 shares with 0.102, doesn't depend on rustls-webpki, and the signature algorithm values 0.102 defines are exported by neither rumqttc nor the broker.

### What checks certificates instead

- **MQTT.** `andromeda-delivery` builds a rustls `ClientConfig` with the aws-lc-rs provider and the platform verifier, and hands it to rumqttc as `TlsConfiguration::Rustls`. It never builds `TlsConfiguration::Simple`, the one rumqttc fills from certificate files.
- **Outbound HTTP.** reqwest verifies with rustls and the platform verifier. No custom verifier or root store is set.
- **Server TLS and client certificates.** `andromeda-server-tls` verifies client certificates with rustls's `WebPkiClientVerifier` and gives it no revocation lists.
- **HTTP/3.** quinn runs on rustls 0.23.
- **PostgreSQL.** sqlx is built without a TLS feature, so database connections verify no certificates.

rustls 0.23.46 and rustls-platform-verifier 0.7.1 both resolve to rustls-webpki 0.103.17. On Linux the platform verifier loads the system certificate store and hands each check to rustls's `WebPkiServerVerifier`. No code in the broker configures a revocation list.

### Each advisory

Each line reference below is to the crate's source as published on crates.io at the version named.

- **RUSTSEC-2026-0049**, revocation lists not consulted for a later distribution point.
  - The flaw is in `IssuingDistributionPoint::authoritative_for` and `uri_name_in_common` (rustls-webpki 0.102.8, `src/crl/types.rs:590` and `:640`). Only a certificate's first distribution point is compared, so a revoked certificate can pass when unknown revocation status is allowed.
  - It runs only inside `EndEntityCert::verify_for_usage` (`src/end_entity.rs:102`) called with revocation options, which first need a list from `OwnedCertRevocationList::from_der` or `BorrowedCertRevocationList::from_der` (`src/crl/types.rs:193`, `:240`).
  - Fixed in 0.103.10. The broker configures no revocation lists, so the code doesn't run in 0.103 either.
- **RUSTSEC-2026-0098**, URI name constraints ignored.
  - The flaw is in `check_presented_id_conforms_to_constraints` (rustls-webpki 0.102.8, `src/subject_name/verify.rs:69`), which skips a URI constraint as a mismatch of name types.
  - It runs only while `verify_for_usage` checks a chain's name constraints, through `check_name_constraints` (`src/subject_name/verify.rs:22`), and only after the signatures verify, so exploiting it needs a misissued certificate.
  - Fixed in 0.103.12, which rejects every URI name constraint.
- **RUSTSEC-2026-0099**, wildcard names accepted under a permitted constraint.
  - The flaw is in `presented_id_matches_reference_id` (rustls-webpki 0.102.8, `src/subject_name/dns_name.rs:217`). Its wildcard branch lets `*.example.com` satisfy a permitted constraint of `accept.example.com`.
  - It runs only through the same name constraint check inside `verify_for_usage`.
  - Fixed in 0.103.12.
- **RUSTSEC-2026-0104**, panic parsing a revocation list.
  - The flaw is in `bit_string_flags` (rustls-webpki 0.102.8, `src/der.rs:367`), which indexes the last byte of an empty bit string at `src/der.rs:384`. An empty `onlySomeReasons` in an issuing distribution point reaches it before the list's signature is checked.
  - It runs only when a revocation list is parsed with `from_der`, or re-read during a revocation check.
  - Fixed in 0.103.13. The broker parses no revocation lists. rustls-pemfile can read them, but rumqttc never asks it to.
- **RUSTSEC-2026-0382**, distrusted emSign intermediates accepted.
  - rustls-webpki 0.102.8 has no distrust check at all, only a `TODO` at `src/verify_cert.rs:71`, so `verify_for_usage` accepts a path through the emSign intermediates whose private keys are public.
  - 0.103.17 rejects their keys from its `DISTRUSTED_SPKIS` list (`src/verify_cert.rs:930`), which `verify_for_usage` applies (`src/end_entity.rs:119`). The check landed in 0.103.16.
  - The broker ships no root set of its own. `webpki-roots` isn't in the lockfile, and `webpki-root-certs` is a dependency of rustls-platform-verifier only on wasm32 and in its own tests. Trust anchors come from the system store of the image.

Each statement says `vulnerable_code_not_in_execute_path`, since the crate is linked and the code above never runs. Whether the linker drops that code from the binary, which `vulnerable_code_not_present` would claim, isn't checked, because a scanner treats both justifications the same.

## rustls-pemfile 2.2.0

RUSTSEC-2025-0134 reports that the crate is unmaintained, not a vulnerability. rumqttc is the only crate that links it, and it calls it only in its `TlsConfiguration::Simple` branch (`src/tls.rs:71` of rumqttc 0.25.1) to read certificate files. The broker always passes `TlsConfiguration::Rustls`, so that branch never runs. Even there the certificates would be checked by rustls 0.23. Its statement says `vulnerable_code_not_in_execute_path`, so it names rumqttc 0.25.1 like the rustls-webpki ones. GitHub's advisory database has no entry for it, but scanners that read the Rust advisory database report it.

## Unmaintained crates

RUSTSEC-2024-0370, RUSTSEC-2026-0542 and RUSTSEC-2026-0543 report that proc-macro-error 1.0.4, humantime 2.4.0 and chrono 0.4.45 are unmaintained. None of them reports a vulnerability, so their statements say `vulnerable_code_not_present`. That ruling doesn't rest on what the crates linking them call, so the statements name none of those crates, and a dependency update calls for no new proof.

- **proc-macro-error.** Only the procedural macros of static-automata-macros, static-regular-grammar and str-newtype-derive use it, so it runs inside the compiler and none of its code is in the binary. The newest releases of static-automata-macros (1.1.0) and static-regular-grammar (2.0.2) still depend on it.
- **humantime.** The advisory counts it as unmaintained because it wraps chrono. No maintained release exists.
- **chrono.** No maintained release exists.

## Redoing a proof

When the gate reports that a crate linking an affected version isn't named in a statement, the ruling hasn't been proven for that crate yet.

1. Run the `cargo tree -i` command above for both targets with the affected crate and version, and add `-e features` to see which feature pulls it in.
2. Search the new dependent's source, under the cargo registry, for the affected crate's library name. Note every function it calls and every value of the crate's types it makes. Calls into the code an advisory names mean the ruling doesn't hold, and the advisory has to be fixed rather than ignored.
3. Check that the broker's own TLS paths above still run on a fixed version.
4. Name the crate and its version in the ignore's reason in `deny.toml`, and copy the reason word for word into the impact statement of each of the advisory's statements. Set the `timestamp` of each of those statements and the document's `last_updated`, and raise the document's `version`. Update this page.
