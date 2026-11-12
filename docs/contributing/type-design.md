# Type design

## Domain values have types

`String` and `&str` are only for text a person reads, such as an error message, a free-form description or a log line. Every value that means something to the broker has a type of its own, and that holds for parameters, return types, fields, enum payloads and map keys alike. A `HashMap<String, _>` needs a typed key.

- A URL is a `url::Url`.
- An IRI is an `IriBuf` or a borrowed `Iri` from `iri-rs`, and an IRI reference is an `IriRefBuf`.
- A URN is an `urn_rs::Urn`. A `urn:ngsi-ld:<Kind>:<id>` URN is built with the lexicon's `ngsi_ld_urn` and read through its namespace and namespace-specific string, never by a string prefix.
- A language tag is a `langtag::LangTagBuf`.
- A media type is a `mediatype::MediaType`.
- Everything the specification gives a grammar has a validated type in `andromeda-ngsi-ld-lexicon`, such as `Identifier`, `Uri`, `Name`, `Scope`, `ScopePath`, `TenantName`, `NgsiLdDateTime` and `NonEmptyText`. The resources built from them live in `andromeda-ngsi-ld-model`. Follow the patterns of both crates when you add a type.
- Anything else with structure gets a newtype.

Short owned text inside a newtype is usually a `compact_str::CompactString`.

A value of such a type can only be made from valid input, so the check happens once, when the value is made, and no later step can forget it. Date-times, dates and times are read and written only by the lexicon's `time` module, which accepts exactly the forms of ETSI GS CIM 009 v1.9.1 clause 4.6.3 and spells every instant the broker sends. Parsing a time with `chrono` directly, a strftime template or a regular expression fails the gate `time_handling`, in tests as well.

## Decoding

A request body is decoded straight into the typed model of its operation, such as an entity, a subscription, a registration or a query. It's never parsed into a general JSON tree and then searched. Anything the model can't hold stops the decode at that member, and the error names the member. So a JSON `null`, a member written twice or a value of the wrong kind is refused, a refused request changes nothing, and a batch judges each entity on its own. A query parameter the specification doesn't define is refused with `InvalidRequest`, so a misspelt parameter can't quietly do nothing.

## Constants

A constant is validated at compile time, never parsed when the broker starts.

- An IRI constant uses `iri_rs::iri!`.
- A URN constant uses `urn_rs::urn!`. Declare it `static` when a caller keeps the text it lends, because every use of a `const` is a temporary.
- A URL constant uses `url_macro::url!`.
- A string newtype gets an infallible `from_static(&'static str)` constructor in the crate that owns it.
- A fixed token such as an entity's `type` discriminator is a zero-sized type declared with the lexicon's `constant_string!` macro.

## JSON

Production code holds JSON in exactly three forms.

1. A typed model, such as an entity, a temporal representation, a subscription, a registration, a snapshot, a notification, a discovery result, problem details or a context.
2. Bytes, which serde visitors decode straight into the typed model and write straight from it.
3. A `JsonLiteral` from `andromeda-json-literal`, for JSON the specification treats as opaque data, such as a JsonProperty value, an `@json` literal or a GeoJSON foreign member. It is held as validated text.

`serde_json::Value`, `serde_json::Map`, `json!`, `serde_json::to_value` and `serde_json::from_value` don't appear in production code. The JSON-LD processor's own tree (`jstrict`, `jsonld_syntax::Value`) is used only inside the engine in `andromeda-jsonld` and inside `andromeda-json-literal`. Tests may use `json!` to build fixtures and to compare parsed bodies. The gate `untyped_json` enforces this with no baseline. A general JSON tree allocates per node, loses member order and can be made arbitrarily deep, so a deep document would become a risk to the process. Keeping JSON typed is also why a JsonProperty comes back exactly as it was written.

## Derives

A trait that can be derived is derived. Look for the derive before you write an `impl`.

| Need | Use |
|---|---|
| `Display`, `FromStr`, `&'static str`, variant iteration and counts on an enum | `strum` (`Display`, `EnumString`, `IntoStaticStr`, `EnumIter`, `EnumCount`, `EnumDiscriminants`) |
| `Display`, `From`, `Into`, `FromStr`, `TryFrom`, `AsRef`, `Deref`, `Constructor` and `IsVariant` | `derive_more` |
| A `Debug` that leaves out a field | `derive_more::Debug` with `#[debug(skip)]` |
| `Error` with its source chained | `thiserror` |
| `Default` on an enum | `#[default]` on the variant |
| `Clone`, `Copy`, `Default`, `PartialEq`, `Eq` and `Hash` | the standard derives |
| A secret that must never be printed | `secrecy::SecretString` |
| Skipping `None` fields when serializing | `serde_with::skip_serializing_none` |
| A builder | `typed-builder` |
| A set of flags | `enumset` |

Derive only what is true of the type. A struct with more than three or four fields, or with optional ones, gets a builder. When building it can fail validation, the type has a constructor that returns a `Result` instead. A hand-written `impl` of a derivable trait carries a comment saying why no derive fits.

## Serialization casing

Who owns the wire format decides the casing.

- JSON that crosses the NGSI-LD API boundary is camelCase throughout, including every member and value Andromeda adds itself, such as those of an extension. A token the specification defines keeps exactly the spelling ETSI GS CIM 009 v1.9.1 gives it, such as `Property`, `GeoProperty`, `observedAt`, `datasetId` and `keyValues`. Where that spelling looks inconsistent, it still wins, and the code cites the clause. The same holds for query parameter values and every enum that maps to a token of the specification.
- Andromeda's own side, meaning configuration, CLI values, internal enums and formats Andromeda defines outside the API, uses kebab-case values with `#[serde(rename_all = "kebab-case")]` and `#[strum(serialize_all = "kebab-case")]`. Keys keep their default casing.

When it isn't clear which side a type is on, treat it as crossing the API until shown otherwise.

## HTTP headers

Every header the broker reads or writes has a typed form in `andromeda-http-headers`. Most of them, including `Link`, `Accept`, `Warning` and the `NGSILD-*` headers, implement `headers::Header`. A language preference, whether it comes from `Accept-Language` or from the `lang` query parameter, is a `LanguageFilter` ranked by quality. Never read or build a header from a raw string.

Andromeda writes header names in lowercase, and field names in lowercase where a header lists them, as `Vary` does. This is a house convention. Field names are case-insensitive (RFC 9110 section 5.1), so no test asserts their casing and every test looks headers up case-insensitively.
