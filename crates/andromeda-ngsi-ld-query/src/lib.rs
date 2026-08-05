//! The NGSI-LD query languages: their grammars, their ASTs and their compiled forms.
//!
//! ETSI GS CIM 009 v1.9.1 defines a family of query facilities — `q` (clause
//! 4.9), `georel` (4.10), `scopeQ` (4.18), type selection (4.17), `orderBy`
//! (4.23), projection (6.3.15) and the temporal window (4.5.19) — that a broker
//! must read identically wherever they appear. This crate owns all of them: each
//! concept's grammar, its AST and its compiled model live in one directory, so
//! a change to one concept touches one directory rather than several pipeline
//! stages.
//!
//! Modules are therefore named after NGSI-LD concepts, never after stages. How
//! a compiled query is rendered as SQL is deliberately absent: that is a
//! property of a backend, and it lives in `andromeda-postgresql-query` and the
//! two statement crates above it. Nothing here names a database.
//!
//! Term expansion is likewise inverted rather than depended upon: compilation
//! that needs a term expanded against an `@context` asks a
//! [`term_expander::TermExpander`], which the JSON-LD engine implements.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod csr;
pub mod dataset;
pub mod entity;
pub mod entity_selection;
pub mod error;
pub mod filter;
pub mod geo;
pub mod identifier;
pub mod ordering;
pub mod pagination;
pub mod pattern;
pub mod projection;
pub mod scope;
pub mod temporal;
pub mod term_expander;
pub mod type_selection;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// A crate asserts its own manifest rather than a central test asserting
    /// everyone's: the failure then names the crate that broke the rule, and a
    /// crate added to the workspace cannot be forgotten by a table it was never
    /// added to.
    ///
    /// The data model is deliberately absent: ETSI GS CIM 009 puts query
    /// expressions *inside* resources (clauses 5.2.9, 5.2.12, 5.2.23), so the
    /// resources sit above the languages. An edge the other way is what forced
    /// `Subscription.q` to be a `String`.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never declare.
        const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "geozero", "bytes", "axum", "reqwest", "confique", "clap", "tokio"];
        /// In-workspace crates this one may name. Anything else is an upward or
        /// sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
        ];

        let manifest = include_str!("../Cargo.toml");
        let dependencies: Vec<&str> = manifest
            .split("[dependencies]")
            .nth(1)
            .unwrap_or_else(|| unreachable!("manifest has a [dependencies] table"))
            .split("\n[")
            .next()
            .unwrap_or_else(|| unreachable!("dependency table is terminated"))
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();

        for line in &dependencies {
            let name = line.split(['=', '.', ' ']).next().unwrap_or_default();
            assert!(!FORBIDDEN.contains(&name), "this crate must not depend on {name}");
            if name.starts_with("andromeda-") {
                assert!(ALLOWED_IN_WORKSPACE.contains(&name), "{name} is not an edge this crate's layer permits");
            }
        }
    }
}

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a specification crate
    /// can spell an entire SQL predicate without declaring `sea-query`. Six
    /// items did — a comparison operator, a POSIX regex operator, an `AND`
    /// composer, a JSONB cast, and two column mappings on a resource — and
    /// nothing in the workspace could see them.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        /// Fragments that only ever appear in a rendering, never in a grammar.
        const DIALECT: &[&str] = &[
            "::numeric",
            "::boolean",
            "::timestamptz",
            "::lquery",
            "::geography",
            "ST_",
            "jsonb_",
            "= ANY(",
            "SELECT ",
        ];

        for (path, source) in SOURCES {
            // Comments may name the counterpart a backend renders — that is how
            // two readings of one clause stay comparable. Only code is scanned.
            let code: String = source.lines().filter(|line| !line.trim_start().starts_with("//")).collect();
            for fragment in DIALECT {
                assert!(
                    !code.contains(fragment),
                    "{path} emits '{fragment}'; how a value is spelled in a store belongs to that backend"
                );
            }
        }
    }

    /// Every source file in this crate, so a new one cannot be added outside
    /// the guard without the list saying so.
    const SOURCES: &[(&str, &str)] = &[
        ("csr/compiled.rs", include_str!("csr/compiled.rs")),
        ("dataset.rs", include_str!("dataset.rs")),
        ("entity/compiled.rs", include_str!("entity/compiled.rs")),
        ("entity_selection.rs", include_str!("entity_selection.rs")),
        ("error.rs", include_str!("error.rs")),
        ("filter/eval.rs", include_str!("filter/eval.rs")),
        ("filter/expansion.rs", include_str!("filter/expansion.rs")),
        ("filter/expression.rs", include_str!("filter/expression.rs")),
        ("filter/lexer.rs", include_str!("filter/lexer.rs")),
        ("filter/parser.rs", include_str!("filter/parser.rs")),
        ("filter/path.rs", include_str!("filter/path.rs")),
        ("filter/predicate.rs", include_str!("filter/predicate.rs")),
        ("filter/scalar.rs", include_str!("filter/scalar.rs")),
        ("filter/serialize.rs", include_str!("filter/serialize.rs")),
        ("geo/compile.rs", include_str!("geo/compile.rs")),
        ("geo/query.rs", include_str!("geo/query.rs")),
        ("identifier.rs", include_str!("identifier.rs")),
        ("ordering/collation.rs", include_str!("ordering/collation.rs")),
        ("ordering/compiled.rs", include_str!("ordering/compiled.rs")),
        ("ordering/geometry.rs", include_str!("ordering/geometry.rs")),
        ("ordering/term.rs", include_str!("ordering/term.rs")),
        ("pagination.rs", include_str!("pagination.rs")),
        ("pattern.rs", include_str!("pattern.rs")),
        ("projection/compiled.rs", include_str!("projection/compiled.rs")),
        ("projection/expansion.rs", include_str!("projection/expansion.rs")),
        ("projection/purge.rs", include_str!("projection/purge.rs")),
        ("projection/term.rs", include_str!("projection/term.rs")),
        ("scope/pattern.rs", include_str!("scope/pattern.rs")),
        ("scope/query.rs", include_str!("scope/query.rs")),
        ("temporal/aggregation_period.rs", include_str!("temporal/aggregation_period.rs")),
        ("temporal/compiled.rs", include_str!("temporal/compiled.rs")),
        ("temporal/instance_selection.rs", include_str!("temporal/instance_selection.rs")),
        ("temporal/last_n.rs", include_str!("temporal/last_n.rs")),
        ("temporal/representation.rs", include_str!("temporal/representation.rs")),
        ("temporal/window.rs", include_str!("temporal/window.rs")),
        ("term_expander.rs", include_str!("term_expander.rs")),
        ("type_selection/expansion.rs", include_str!("type_selection/expansion.rs")),
        ("type_selection/grammar.rs", include_str!("type_selection/grammar.rs")),
        ("type_selection/parser.rs", include_str!("type_selection/parser.rs")),
    ];
}
