//! The one compile entry of the `q` language (clause 4.9).
//!
//! Every pipeline that reads a written `q` filter (Query Entities, the
//! temporal operations, Subscriptions and registration discovery) turns it
//! into the filter it evaluates through [`compile_q_filter`], so the order of
//! the steps is stated once: the complexity admission of clause 5.5.6, term
//! expansion under the request's `jsonKeys` and `expandValues` (clauses 4.9
//! and 6.4.3.2), the Linked Entity admission of its hops (clauses 5.7.2.4,
//! 5.7.4.4 and 5.14.4.4), and the admission of its target languages. The
//! pipelines differ only in where the filter and its opt-ins come from and
//! which terms they admit it under.

use crate::{
    error::QueryError,
    filter::{
        complexity::FilterComplexityLimits,
        expansion::{apply_expand_values, expand_attribute_names, expand_json_keys, expand_q_expr},
        expression::Expr,
        hop_admission::FilterHopAdmission,
        json_key_attributes::JsonKeyAttributes,
        language_targeting::LanguageTargeting,
    },
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
    /// [`QueryError::AttributeNameInvalid`] when a name does not expand to a
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

/// Compile a written `q` filter into the filter a pipeline evaluates.
///
/// The filter is admitted under `admission.complexity`, expanded under
/// `opt_ins`, its hops admitted by `hops`, and its target languages admitted
/// under `admission.language_targeting`, in that order. Neither the complexity
/// nor the target languages of a filter change under expansion, so the order
/// decides only which refusal a filter breaking two rules reports.
///
/// # Errors
///
/// Returns the [`QueryError`] of the complexity admission, of a term or an
/// opted-in operand that does not expand, or of a target language the level
/// does not admit, and the refusal of `hops` for a hop the operation does not
/// admit, each converted into `E`.
pub async fn compile_q_filter<E, H, X>(filter: Expr, opt_ins: &FilterOptIns, admission: FilterAdmission, hops: &H, expander: &X) -> Result<Expr, E>
where
    E: From<QueryError> + From<H::Refusal>,
    H: FilterHopAdmission,
    X: TermExpander,
{
    admission.complexity.admit(&filter)?;
    let filter = expand_q_expr(filter, &opt_ins.json_keys, expander).await?;
    let filter = apply_expand_values(filter, &opt_ins.expand_values, expander).await?;
    hops.admit_hops(&filter)?;
    admission.language_targeting.admit(&filter)?;
    Ok(filter)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        filter::{
            compilation::{FilterAdmission, FilterOptIns, compile_q_filter},
            complexity::FilterComplexityLimits,
            expression::Expr,
            hop_admission::FilterHopAdmission,
            language_targeting::LanguageTargeting,
            parser::parse,
            serialize::serialize,
        },
        prefix_expander::{PrefixExpander, namespaced},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use std::num::NonZeroUsize;

    /// The refusal of a hop, standing for the operation's Linked Entity rule.
    #[derive(Debug, PartialEq, Eq)]
    struct HopRefused;

    /// Why the entry refused a filter in these tests.
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

        fn admit_hops(&self, filter: &Expr) -> Result<(), HopRefused> {
            match self {
                Hops::Refused if filter.linked_depth() > 0 => Err(HopRefused),
                Hops::Refused | Hops::Admitted => Ok(()),
            }
        }
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
        FilterOptIns::expanded(Some(&names(json_keys)), Some(&names(expand_values)), &PrefixExpander)
            .await
            .expect("the opt-ins expand")
    }

    async fn compiled(q: &str, opt_ins: &FilterOptIns, admission: FilterAdmission, hops: &Hops) -> Result<Expr, Refused> {
        compile_q_filter::<Refused, _, _>(parse(q).expect("the filter parses"), opt_ins, admission, hops, &PrefixExpander).await
    }

    async fn compiled_text(q: &str, opt_ins: &FilterOptIns) -> String {
        let filter = compiled(q, opt_ins, admission(LanguageTargeting::Specification), &Hops::Admitted)
            .await
            .expect("the filter compiles");
        serialize(&filter)
    }

    /// CP-1: a term expands to the IRI the context maps it to.
    #[tokio::test]
    async fn a_filter_term_expands_to_its_iri() {
        pretty_assertions::assert_eq!(
            compiled_text("speed>5", &FilterOptIns::default()).await,
            format!("\"{}\">5", namespaced("speed"))
        );
    }

    /// CP-2, clause 6.4.3.2: an operand of an attribute `expandValues` names
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

    /// CP-3, clause 5.5.6: a filter of 1 001 comparisons is too complex under
    /// the default limits, and a stored filter read under no limit is not.
    #[tokio::test]
    async fn a_filter_past_the_comparison_limit_is_refused_and_one_read_back_is_not() {
        let values = (0..1001).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
        let q = format!("speed=={values}");
        let refused = compiled(&q, &FilterOptIns::default(), admission(LanguageTargeting::Specification), &Hops::Admitted).await;
        assert!(matches!(refused, Err(Refused::Query(QueryError::QTooManyComparisons { .. }))), "{refused:?}");
        let stored = FilterAdmission {
            complexity: FilterComplexityLimits::PARSED,
            language_targeting: LanguageTargeting::Specification,
        };
        compiled(&q, &FilterOptIns::default(), stored, &Hops::Admitted)
            .await
            .expect("a stored filter is read under no limit");
    }

    /// CP-4, clause 4.9: a full language tag as a target language is admitted
    /// at the language-tags level and refused at the specification level.
    #[tokio::test]
    async fn a_language_tag_target_is_admitted_only_at_the_language_tags_level() {
        compiled(
            "name[en-GB]==\"x\"",
            &FilterOptIns::default(),
            admission(LanguageTargeting::LanguageTags),
            &Hops::Admitted,
        )
        .await
        .expect("the language-tags level admits a tag");
        let refused = compiled(
            "name[en-GB]==\"x\"",
            &FilterOptIns::default(),
            admission(LanguageTargeting::Specification),
            &Hops::Admitted,
        )
        .await;
        assert!(matches!(refused, Err(Refused::Query(QueryError::LanguageTagNotAdmitted(_)))), "{refused:?}");
    }

    /// CP-9, clause 4.9 EXAMPLE 11: the member under a `jsonKeys` attribute
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

    /// The entry refuses a filter's hops before its target languages, and
    /// admits its complexity before either.
    #[tokio::test]
    async fn the_admissions_run_complexity_then_hops_then_target_languages() {
        let hop_and_tag = "r{name[en-GB]}==\"x\"";
        let refused = compiled(
            hop_and_tag,
            &FilterOptIns::default(),
            admission(LanguageTargeting::Specification),
            &Hops::Refused,
        )
        .await;
        assert!(matches!(refused, Err(Refused::Hop(HopRefused))), "{refused:?}");
        let values = (0..1001).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
        let refused = compiled(
            &format!("r{{a}}=={values}"),
            &FilterOptIns::default(),
            admission(LanguageTargeting::Specification),
            &Hops::Refused,
        )
        .await;
        assert!(matches!(refused, Err(Refused::Query(QueryError::QTooManyComparisons { .. }))), "{refused:?}");
    }
}
