//! The two steps every pipeline takes a written `q` filter through (clause
//! 4.9).
//!
//! [`admit_q_filter`] admits the filter once per request, where the request
//! is validated: its complexity (clause 5.5.6), its Linked Entity hops for the
//! operation (clauses 5.7.2.4, 5.7.4.4 and 5.14.4.4) through a port the Linked
//! Entity engine implements, and its target languages. None of them changes
//! under term expansion. [`compile_q_filter`] takes only the
//! [`AdmittedFilter`] that admission makes, so no filter is compiled
//! unadmitted, and expands it under the request's `jsonKeys` and
//! `expandValues` (clauses 4.9 and 6.4.3.2) wherever the request is read.
//! Query Entities, the temporal operations, Subscriptions and registration
//! discovery all take these two steps.

use crate::{
    filter::{
        Filter,
        complexity::FilterComplexityLimits,
        expansion::{apply_expand_values, expand_attribute_names, expand_json_keys, expand_q_expr},
        hop_admission::FilterHopAdmission,
        json_key_attributes::JsonKeyAttributes,
        language_targeting::LanguageTargeting,
    },
    query_error::QueryError,
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::collections::HashSet;

/// What a written `q` filter is admitted under: how much it may ask of the
/// broker, and which target languages it may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterAdmission {
    /// The most comparisons and nesting the filter may carry (clause 5.5.6).
    pub complexity: FilterComplexityLimits,
    /// The `q_target_languages` level the filter's target languages are
    /// admitted under.
    pub language_targeting: LanguageTargeting,
}

/// The clause 6.4.3.2 opt-in lists a `q` filter is expanded under, each
/// already expanded through the request `@context`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterOptIns {
    /// The attributes whose values are raw JSON, so the object keys a path
    /// addresses under them stay literal (`jsonKeys`).
    pub json_keys: JsonKeyAttributes,
    /// The attributes whose string operands expand through the `@context`
    /// (`expandValues`).
    pub expand_values: HashSet<AttributeName>,
}

impl FilterOptIns {
    /// Expand the `jsonKeys` and then the `expandValues` list a request
    /// declared. A list it does not declare opts nothing in.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`] or
    /// [`FilterError::InvalidAttributeName`] when a name does not expand to a
    /// usable attribute name.
    pub async fn expanded(
        json_keys: Option<&[AttributeName]>,
        expand_values: Option<&[AttributeName]>,
        expander: &impl TermExpander,
    ) -> Result<FilterOptIns, QueryError> {
        let json_keys = expand_json_keys(json_keys, expander).await?;
        let expand_values = match expand_values {
            Some(names) => expand_attribute_names(names, expander).await?.into_iter().collect(),
            None => HashSet::new(),
        };
        Ok(FilterOptIns { json_keys, expand_values })
    }
}

/// A written `q` filter one request has admitted: within the complexity the
/// broker resolves (clause 5.5.6), its Linked Entity hops admitted for the
/// operation (clauses 5.7.2.4, 5.7.4.4 and 5.14.4.4), and its target languages
/// admitted under the `q_target_languages` level. Only [`admit_q_filter`]
/// makes one, so every filter a pipeline compiles was admitted once, by the
/// request that wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedFilter(Filter);

impl AdmittedFilter {
    /// The filter as the request wrote it.
    #[must_use]
    pub const fn written(&self) -> &Filter {
        &self.0
    }
}

/// Admit the written `q` filter of one request: its complexity under
/// `admission.complexity`, then its hops through `hops`, then its target
/// languages under `admission.language_targeting`.
///
/// None of the three changes under term expansion, so a request is admitted
/// once, where it is validated, and its filter is compiled wherever it is
/// read.
///
/// # Errors
///
/// Returns the [`QueryError`] of the complexity admission or of a target
/// language the level does not admit, and the refusal of `hops` for a hop the
/// operation does not admit, each converted into `E`.
pub fn admit_q_filter<E, H>(filter: &Filter, admission: FilterAdmission, hops: &H) -> Result<AdmittedFilter, E>
where
    E: From<QueryError> + From<H::Refusal>,
    H: FilterHopAdmission,
{
    admission.complexity.admit(filter).map_err(QueryError::from)?;
    hops.admit_hops(filter)?;
    admission.language_targeting.admit(filter).map_err(QueryError::from)?;
    // clone: the request keeps the filter it wrote, which forwarding reads
    Ok(AdmittedFilter(filter.clone()))
}

/// Compile an admitted `q` filter into the filter a pipeline evaluates: its
/// terms expanded under `opt_ins.json_keys`, then the operands of the
/// attributes `opt_ins.expand_values` names.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`], [`QueryError::TermNotAnIri`] or
/// [`FilterError::InvalidAttributeName`] when a term or an opted-in operand
/// does not expand.
pub async fn compile_q_filter<X: TermExpander>(filter: &AdmittedFilter, opt_ins: &FilterOptIns, expander: &X) -> Result<Filter, QueryError> {
    // clone: expansion consumes its tree, and every compilation of the request reads the admitted filter again
    let filter = expand_q_expr(filter.0.clone(), &opt_ins.json_keys, expander).await?;
    apply_expand_values(filter, &opt_ins.expand_values, expander).await
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            Filter,
            compilation::{AdmittedFilter, FilterAdmission, FilterOptIns, admit_q_filter, compile_q_filter},
            complexity::FilterComplexityLimits,
            display::serialize,
            filter_error::FilterError,
            hop_admission::FilterHopAdmission,
            language_targeting::LanguageTargeting,
            parser::parse,
        },
        namespace_expander::NamespaceExpander,
        query_error::QueryError,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use iri_rs::{IriBuf, iri};
    use std::num::NonZeroUsize;

    /// Expands every term and type under `https://example.org/`.
    const EXPANDER: NamespaceExpander = NamespaceExpander::uniform(iri!("https://example.org/"));

    /// The refusal of a hop, standing for the operation's Linked Entity rule.
    #[derive(Debug, PartialEq, Eq)]
    struct HopRefused;

    /// Why admission refused a filter in these tests.
    #[derive(Debug, derive_more::From)]
    enum Refused {
        /// A refusal of the `q` language.
        Query(QueryError),
        /// A refusal of a hop.
        Hop(HopRefused),
    }

    /// Admits a filter's hops, or refuses any.
    enum Hops {
        /// Every hop is admitted, as for a Subscription.
        Admitted,
        /// Every hop is refused, as for Purge Entities.
        Refused,
    }

    impl FilterHopAdmission for Hops {
        type Refusal = HopRefused;

        fn admit_hops(&self, filter: &Filter) -> Result<(), HopRefused> {
            match self {
                Hops::Refused if filter.linked_depth() > 0 => Err(HopRefused),
                Hops::Refused | Hops::Admitted => Ok(()),
            }
        }
    }

    fn namespaced(term: &str) -> IriBuf {
        EXPANDER.term_iri(term).expect("a term resolves against the namespace")
    }

    /// The broker's default limits: 1000 comparisons, 16 levels of nesting.
    fn default_limits() -> FilterComplexityLimits {
        FilterComplexityLimits::new(
            NonZeroUsize::new(1000).expect("a non-zero limit"),
            NonZeroUsize::new(16).expect("a non-zero limit"),
        )
    }

    fn admission(language_targeting: LanguageTargeting) -> FilterAdmission {
        FilterAdmission {
            complexity: default_limits(),
            language_targeting,
        }
    }

    fn names(listed: &[&str]) -> Vec<AttributeName> {
        listed.iter().map(|name| name.parse().expect("a valid attribute name")).collect()
    }

    async fn opt_ins(json_keys: &[&str], expand_values: &[&str]) -> FilterOptIns {
        FilterOptIns::expanded(Some(&names(json_keys)), Some(&names(expand_values)), &EXPANDER)
            .await
            .expect("the opt-ins expand")
    }

    fn admitted(q: &str, admission: FilterAdmission, hops: &Hops) -> Result<AdmittedFilter, Refused> {
        admit_q_filter::<Refused, _>(&parse(q).expect("the filter parses"), admission, hops)
    }

    async fn compiled_text(q: &str, opt_ins: &FilterOptIns) -> String {
        let filter = admitted(q, admission(LanguageTargeting::Specification), &Hops::Admitted).expect("the filter is admitted");
        serialize(&compile_q_filter(&filter, opt_ins, &EXPANDER).await.expect("the filter compiles"))
    }

    /// A term expands to the IRI the context maps it to.
    #[tokio::test]
    async fn a_filter_term_expands_to_its_iri() {
        pretty_assertions::assert_eq!(
            compiled_text("speed>5", &FilterOptIns::default()).await,
            format!("\"{}\">5", namespaced("speed"))
        );
    }

    /// Clause 6.4.3.2: an operand of an attribute `expandValues` names
    /// expands, and one of any other attribute stays as written.
    #[tokio::test]
    async fn an_operand_of_an_expand_values_attribute_expands() {
        let opted = opt_ins(&[], &["gender"]).await;
        pretty_assertions::assert_eq!(
            compiled_text("gender==\"Male\"", &opted).await,
            format!("\"{}\"==\"{}\"", namespaced("gender"), namespaced("Male"))
        );
        pretty_assertions::assert_eq!(compiled_text("colour==\"Red\"", &opted).await, format!("\"{}\"==\"Red\"", namespaced("colour")));
    }

    /// Clause 5.5.6: a filter of 1 001 comparisons is too complex under
    /// the default limits, and one admitted under no limit past the parser's,
    /// as a stored filter is read, is not.
    #[test]
    fn a_filter_past_the_comparison_limit_is_refused_and_one_read_back_is_not() {
        let values = (0..1001).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
        let q = format!("speed=={values}");
        let refused = admitted(&q, admission(LanguageTargeting::Specification), &Hops::Admitted);
        assert!(
            matches!(refused, Err(Refused::Query(QueryError::Filter(FilterError::TooManyComparisons { .. })))),
            "{refused:?}"
        );
        let stored = FilterAdmission {
            complexity: FilterComplexityLimits::PARSED,
            language_targeting: LanguageTargeting::Specification,
        };
        admitted(&q, stored, &Hops::Admitted).expect("a stored filter is read under no limit");
    }

    /// Clause 4.9: a full language tag as a target language is admitted
    /// at the language-tags level and refused at the specification level.
    #[test]
    fn a_language_tag_target_is_admitted_only_at_the_language_tags_level() {
        admitted("name[en-GB]==\"x\"", admission(LanguageTargeting::LanguageTags), &Hops::Admitted).expect("the language-tags level admits a tag");
        let refused = admitted("name[en-GB]==\"x\"", admission(LanguageTargeting::Specification), &Hops::Admitted);
        assert!(
            matches!(refused, Err(Refused::Query(QueryError::Filter(FilterError::LanguageTagNotAdmitted(_))))),
            "{refused:?}"
        );
    }

    /// Clause 4.9 EXAMPLE 11: the member under a `jsonKeys` attribute
    /// stays the literal key, and under any other attribute it expands.
    #[tokio::test]
    async fn a_member_under_a_json_keys_attribute_stays_literal() {
        let declared = opt_ins(&["jp"], &[]).await;
        pretty_assertions::assert_eq!(
            compiled_text("jp[value]==\"x\"", &declared).await,
            format!("\"{}\"[value]==\"x\"", namespaced("jp"))
        );
        pretty_assertions::assert_eq!(
            compiled_text("jp[value]==\"x\"", &FilterOptIns::default()).await,
            format!("\"{}\"[\"{}\"]==\"x\"", namespaced("jp"), namespaced("value"))
        );
    }

    /// Admission refuses a filter's hops before its target languages, and its
    /// complexity before either.
    #[test]
    fn admission_runs_complexity_then_hops_then_target_languages() {
        let refused = admitted("r{name[en-GB]}==\"x\"", admission(LanguageTargeting::Specification), &Hops::Refused);
        assert!(matches!(refused, Err(Refused::Hop(HopRefused))), "{refused:?}");
        let values = (0..1001).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
        let refused = admitted(&format!("r{{a}}=={values}"), admission(LanguageTargeting::Specification), &Hops::Refused);
        assert!(
            matches!(refused, Err(Refused::Query(QueryError::Filter(FilterError::TooManyComparisons { .. })))),
            "{refused:?}"
        );
    }

    /// The admitted filter is the filter the request wrote, and compiling it
    /// leaves it as written for the next compilation.
    #[tokio::test]
    async fn an_admitted_filter_keeps_the_filter_as_written() {
        let written = parse("speed>5").expect("the filter parses");
        let filter = admit_q_filter::<Refused, _>(&written, admission(LanguageTargeting::Specification), &Hops::Admitted).expect("the filter is admitted");
        compile_q_filter(&filter, &FilterOptIns::default(), &EXPANDER)
            .await
            .expect("the filter compiles");
        pretty_assertions::assert_eq!(filter.written(), &written);
    }
}
