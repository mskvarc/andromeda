# Porting Tests from the NGSI-LD Test Suite

This guide explains how to port tests from the ETSI NGSI-LD conformance test suite
(`ngsi-ld-test-suite/TP/NGSI-LD/`) into this Rust integration test framework.

## Core Principles

1. **1:1 mapping with the Robot test suite.** Every test must correspond to a specific
   Robot test case by ID (e.g., `001_01_02`). The test name must include the Robot test
   ID. Do not invent tests that don't exist in the Robot suite.

2. **Use fixture files, not inline JSON.** Test data comes from `tests/fixtures/`,
   which mirrors `ngsi-ld-test-suite/data/`. Inline `serde_json::json!()` is only
   acceptable for error-case payloads (malformed JSON, empty bodies, wrong types).

3. **Automatic cleanup.** Tests use `TestCtx` which automatically tracks and cleans up
   created resources. No manual `delete_entity` calls needed.

4. **Serial execution.** All tests share a database and run serially via
   `--test-threads=1` (set in `main.rs`).

## Test Harness

The harness uses `libtest-mimic` with a custom `fn main()` entry point (`tests/main.rs`).
Tests are registered via `pub fn tests(trials: &mut Vec<Trial>)` in each module.

### Writing a test

```rust
use crate::harness::{client::{QueryParams, RequestHeaders}, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "provision::entities::create";

pub fn tests(trials: &mut Vec<Trial>) {
    trials.push(trial(&format!("{PREFIX}::001_01::with_properties"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:001_01_02");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx.client.get_entity(
            &id, &QueryParams::new(),
            RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
        ).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["value"], "Eiffel Tower");
        // No manual cleanup needed — ctx.cleanup() runs automatically
    }));
}
```

### Key types

- **`TestCtx`** — test context with `client` and automatic resource tracking
  - `ctx.entity_id("urn:ngsi-ld:Building:001")` — generates UUID, tracks for cleanup
  - `ctx.track_temporal_entity(&id)` — track temporal entities separately
  - `ctx.track_context(&id)` / `ctx.track_subscription(&id)` / `ctx.track_registration(&id)`
- **`RequestHeaders`** — composable headers builder
  - `.accept_ld_json()` / `.accept_geo_json()` / `.content_type("...")`
  - `.link_context(url)` — adds NGSI-LD Link header
- **`QueryParams`** — builder pattern for query parameters
  - `QueryParams::new().type_("Building").limit(10).options([NgsiLdOption::KeyValues])`
- **`trial(name, closure)`** — wraps async test with cleanup
- **`trial_clean_db(name, closure)`** — truncates DB before running
- **`trial_ignored(name, reason, closure)`** — marks test as ignored

### Content negotiation

`.jsonld` files contain `@context` — the client auto-detects and sends as
`application/ld+json`. For `.json` files without `@context`:

```rust
let resp = ctx.client.request(
    Method::POST, "/entities", Some(&entity),
    RequestHeaders::new()
        .content_type("application/json")
        .link_context(fixtures::test_suite_context_url()),
).await;
```

## Fixture Pitfalls

The fixture loading functions in `harness/fixtures.rs` silently transform data.
Getting this wrong causes subtle, hard-to-debug failures (e.g., a CSR registration
that looks correct but never matches any entity because its property names resolved
to the wrong IRIs). Know the differences:

| Function | ID replacement | `@context` rewriting |
|---|---|---|
| `load_fixture(path, id)` | Yes | **Replaces** ETSI test-suite context URLs with core context |
| `load_fixture_original_context(path, id)` | Yes | No — preserves original `@context` |
| `load_fragment(path)` | No | Replaces context URLs |
| `load_expectation(path)` | No | Replaces context URLs |
| `load_fixture_raw(path)` | No | No — returns raw string |

**When to use which:**

- **Entity fixtures** (create/merge/replace payloads): Use `load_fixture`. The broker
  stores entities with the core context, so ETSI URLs must be replaced.
- **CSR registration fixtures**: Also use `load_fixture`. The broker expands property
  names (e.g., `"speed"`) to full IRIs at registration time using the `@context`.
  If the context is wrong, `propertyNames` expand to wrong IRIs and the registration
  will never match incoming entities. The core context maps short names to
  `https://uri.etsi.org/ngsi-ld/default-context/{name}`, which is what the broker
  uses internally.
- **Response comparison**: Use `load_expectation` for expected response bodies.
- **Payloads that must keep ETSI context** (e.g., testing context negotiation
  behaviour): Use `load_fixture_original_context`.

**Common mistakes:**

1. **Using `load_fixture_original_context` for CSR fixtures.** The ETSI compound
   context URL points to an external server. If the broker can't resolve it (or
   resolves it to different term mappings than the core context), the CSR's expanded
   property names won't match entity attributes. This causes forwarding to silently
   not happen — the CSR exists but the broker considers it non-matching.

2. **Forgetting that `load_fixture` replaces the `id` field.** If the fixture has
   `"id": "urn:ngsi-ld:Vehicle:randomUUID"`, the second argument becomes the new ID.
   But if the fixture has *no* `id` field, the argument is ignored. Check your
   fixture file.

3. **CSR `endpoint` replacement.** `load_fixture` does NOT touch the `endpoint`
   field. For distributed operation tests using a `MockServer`, you must manually
   replace the endpoint after loading (the `prepare_registration` helper does this).

4. **CSR `information[].entities[].id` replacement.** `load_fixture` only replaces
   the top-level `id` field (the CSR's own ID). It does NOT replace entity IDs
   nested inside the `information` array. You must do this manually if the CSR
   fixture references a specific entity ID.

## Directory Mapping

| NGSI-LD Test Suite Path | Rust Module Path |
|---|---|
| `CommonBehaviours/` | `tests/common_behaviours/` |
| `ContextInformation/Provision/Entities/` | `tests/context_information/provision/entities/` |
| `ContextInformation/Provision/EntityAttributes/` | `tests/context_information/provision/entity_attributes/` |
| `ContextInformation/Provision/BatchEntities/` | `tests/context_information/provision/batch_entities/` |
| `ContextInformation/Provision/TemporalEntity/` | `tests/context_information/provision/temporal_entity/` |
| `ContextInformation/Provision/TemporalEntityAttributes/` | `tests/context_information/provision/temporal_entity_attributes/` |
| `ContextInformation/Consumption/Entity/` | `tests/context_information/consumption/entity/` |
| `ContextInformation/Consumption/TemporalEntity/` | `tests/context_information/consumption/temporal_entity/` |
| `ContextInformation/Consumption/Discovery/` | `tests/context_information/consumption/discovery/` |
| `ContextInformation/Subscription/` | `tests/context_information/subscription/` |
| `ContextSource/Registration/` | `tests/context_source/registration/` |
| `ContextSource/Discovery/` | `tests/context_source/discovery/` |
| `jsonldContext/Provision/` | `tests/jsonld_context/provision/` |
| `jsonldContext/Consumption/` | `tests/jsonld_context/consumption/` |

## Debugging Test Failures

When a ported test fails, follow this procedure strictly:

### Step 1: Verify test-to-suite alignment

Before investigating the failure, confirm the Rust test exactly matches the
original Robot test case:

- Read the `.robot` file for the failing test ID (e.g., `047_09.robot`).
- Compare **every detail**: fixture files used, HTTP method, endpoint, setup
  order, expected status code, expected response body/headers, and
  setup/teardown steps.
- Pay special attention to which fixtures the Robot test loads — the fixture
  file names may be the same but the entity types, filters, or field values
  inside them may not match what the Rust test assumes.
- If the Rust test diverges from the Robot test in any way, fix the Rust test
  first and re-run before investigating further.

### Step 2: Determine fault — broker or test

Once the Rust test faithfully reproduces the Robot test, decide whether the
failure is caused by the context broker or by the test itself.

**Always consult the NGSI-LD spec before making this decision.** The spec PDF
is at `docs/gs_cim009v010901p.pdf`. Find the relevant clause (the Robot file's
tags usually reference it, e.g., `5_11_2` → clause 5.11.2) and quote the exact
normative text that dictates the expected behaviour.

**Do not implement any fix before presenting the spec quote to the user.** The
workflow is:

1. Identify the relevant spec clause.
2. Quote the exact sentence(s) that define the correct behaviour.
3. State whether the broker or the test violates that text.
4. Wait for user confirmation before changing any code.

### Step 3: Implement the fix

- If the **test** is wrong: fix the Rust test to match the spec. No changes to
  `src/`.
- If the **broker** is wrong: fix the broker in `src/`, keeping the fix
  minimal and spec-aligned. Add a comment citing the spec clause at the fix
  site (e.g., `// Per spec 5.11.3: send notification after subscription update`).
- Never work around a broker bug in the test (e.g., adding arbitrary sleeps,
  relaxing assertions, skipping checks). If the broker is wrong, fix the
  broker.
- Never add unnecessary abstractions, feature flags, or compatibility shims.
  The right fix is the smallest change that makes the behaviour match the spec.

## Running Tests

```bash
# Run all integration tests
cargo test --test integration

# Filter by hierarchical name
cargo test --test integration -- provision::entities::create
cargo test --test integration -- 001_01

# Check compilation without running
cargo check --test integration

# Format code (always use nightly)
cargo +nightly fmt
```
