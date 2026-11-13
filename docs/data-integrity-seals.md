# Data-integrity seals

An `ngsildproof` (ETSI GS CIM 009 v1.9.1 clause 4.5.2.2) seals the attribute it is a sub-attribute of. It's a Property whose value is a W3C Data Integrity proof and whose two non-reified members, `entityIdSealed` and `entityTypeSealed`, name the entity the proof was made for. ETSI TS 104 179 V1.1.1 describes how such a proof is made and checked. The broker doesn't sign or verify seals. It stores each one typed and serves it back as written, so a client can verify it, and so verification can be added later without another change to storage.

## What a write may carry

- An `ngsildproof` is admitted only as a sub-attribute, at any depth. The attribute data types (Table 5.2.5-1 and its siblings) have an `ngsildproof` member and the Entity data type (Table 5.2.4-1) doesn't, and a seal on an entity would sign nothing. A write that puts one on an entity is answered with 400 `BadRequestData`.
- It's a Property. A seal of any other attribute type is refused.
- Its value is one JSON object holding a proof. TS 104 179 clause 5.1.2 requires `type` (exactly `DataIntegrityProof`), `cryptosuite`, `proofPurpose`, `verificationMethod`, `created` and `proofValue`. A value that is a scalar, an array, `null`, an empty object or a proof missing any of the six is refused.
- `created` is an NGSI-LD `DateTime` (clause 4.6.3). `verificationMethod` and `proofPurpose` are URIs, the latter usually a term the W3C Data Integrity v2 context expands. `cryptosuite` and `proofValue` are non-empty strings. Further members are kept as written.
- Both seals are required when a seal is written whole, by a create, an append or a replace, or onto an attribute that holds none. A merge or a partial update of a held seal may write either seal alone and keeps the other one, so no write leaves a seal with only one. A partial update replaces the proof whole, so it writes a complete one.
- `entityIdSealed` and `entityTypeSealed` appear on no other attribute.

## How it is held

In the model a seal is `FamilyValue::Proof`, a Property value holding the two seals and the typed proof. In storage it's a `sub_attribute` element of value kind 12, as [Schema](database/schema.md#entities-as-typed-rows) describes. The proof's `created` is held as an instant together with the number of fractional-second digits it was written with, so `2025-01-27T21:02:24.500Z` reads back with all three digits. Every other instant the broker serves has one canonical spelling.

## Upgrading a database that holds seals

A database that holds an `ngsildproof` as an object value with a `proof_seals` member is converted when it upgrades. A seal whose value is a complete proof and whose seals name an entity becomes typed. Any other one is dropped with its own sub-attributes, and so is every top-level `ngsildproof`, which leaves the current state, its history and the discovery counts as a delete of it would. The conversion admits a slightly narrower set of URIs than the broker does, so a seal naming an unusual URI is dropped rather than stored in a form the broker can't read. Every Snapshot's seals are converted in the same way, before the tenant's types change, as [Schema](database/schema.md#migrations) describes.

## Known gaps for verification

- The broker spells a `DateTime` and a number inside the value of a sealed attribute canonically, not as written. A proof made with an RDF Dataset Canonicalization cryptosuite such as `eddsa-rdfc-2022` signs the lexical form, so such a proof won't verify against the value the broker serves if the client wrote another spelling.
- A JSON Canonicalization cryptosuite such as `eddsa-jcs-2022` signs the JSON document as written, which the broker doesn't keep, so such a proof can't be verified from stored data.
