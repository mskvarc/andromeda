# Notice

Andromeda is licensed under the [European Union Public Licence v. 1.2](LICENSE.md) (EUPL-1.2). The licence in every official EU language is at [eupl.eu](https://eupl.eu/).

Article 5 of the EUPL-1.2 requires this notice to be kept intact and distributed with every copy of the work, and with every work derived from it.

Andromeda includes the third-party material below. Each keeps its own licence, and a copy of that licence sits beside the material it covers.

### NGSI-LD Core Contexts

The NGSI-LD core `@context` releases v1.0 to v1.9 are built into the broker, unmodified, from the [vocabulary crate](crates/andromeda-ngsi-ld-vocabulary/context). ETSI publishes them at `https://uri.etsi.org/ngsi-ld/v1/`, from the ETSI ISG CIM [`NGSI-LD`](https://forge.etsi.org/rep/cim/NGSI-LD) repository. Copyright 2019 ETSI, distributed under the [BSD 3-Clause licence](crates/andromeda-ngsi-ld-vocabulary/context/LICENSE-ETSI-NGSI-LD.md).

### Data Integrity Context

The W3C Verifiable Credential Data Integrity v2 `@context` is built into the broker, unmodified, from the [vocabulary crate](crates/andromeda-ngsi-ld-vocabulary/context). This software includes material copied from it, `https://www.w3.org/2025/credentials/vcdi/context/v2.jsonld`. Copyright © 2025 [World Wide Web Consortium](https://www.w3.org/). <https://www.w3.org/copyright/software-license-2023/> Distributed under the [W3C Software and Document License](crates/andromeda-ngsi-ld-vocabulary/context/LICENSE-W3C.md), 2023 version.

### Test Suite Context

The compound `@context` of the ETSI NGSI-LD test suite is kept, unmodified, in the [vocabulary crate](crates/andromeda-ngsi-ld-vocabulary/context) for the tests that compare against it. It comes from the ETSI ISG CIM [`ngsi-ld-test-suite`](https://forge.etsi.org/rep/cim/ngsi-ld-test-suite) repository. Copyright 2021 ETSI, distributed under the [BSD 3-Clause licence](crates/andromeda-ngsi-ld-vocabulary/context/LICENSE-ETSI-TEST-SUITE.md).

### Annex C Examples

The examples of ETSI GS CIM 009 v1.9.1 annex C are test fixtures of the [expansion](crates/andromeda-ngsi-ld-expansion/tests/fixtures) and [model](crates/andromeda-ngsi-ld-model/tests/fixtures) crates, and are replayed by the [annex C tests](tools/andromeda-test-suite/tests/annex_c). They come from the ETSI ISG CIM [`NGSI-LD`](https://forge.etsi.org/rep/cim/NGSI-LD) repository. Copyright 2019 ETSI, distributed under the [BSD 3-Clause licence](tools/andromeda-test-suite/tests/annex_c/LICENSE-ETSI-NGSI-LD.md).

### ETSI Test Cases

The cases of the ETSI NGSI-LD test suite are ported to the [ETSI tests](tools/andromeda-test-suite/tests/etsi) and the [interoperability tests](tools/andromeda-test-suite/tests/interoperability), and some of their payloads are test fixtures of the [expansion](crates/andromeda-ngsi-ld-expansion/tests/fixtures) and [model](crates/andromeda-ngsi-ld-model/tests/fixtures) crates. They come from the ETSI ISG CIM [`ngsi-ld-test-suite`](https://forge.etsi.org/rep/cim/ngsi-ld-test-suite) repository. Copyright 2021 ETSI, distributed under the [BSD 3-Clause licence](tools/andromeda-test-suite/tests/etsi/LICENSE-ETSI-TEST-SUITE.md).

### HTTP Headers

Several typed HTTP headers in the [headers crate](crates/andromeda-http-headers/src) are adapted from the [headers](https://github.com/hyperium/headers) crate, or kept locally after that crate disabled them. Each of those files says so at the top, and each, with its local modifications, is distributed under the [MIT License](crates/andromeda-http-headers/LICENSE-MIT.md). Copyright 2014-2025 Sean McArthur.
