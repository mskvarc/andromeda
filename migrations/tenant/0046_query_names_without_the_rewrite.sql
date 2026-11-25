-- ============================================================================
-- 0046: Query names without the rewrite
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.9
--
-- Every branch, the live one and each Snapshot's, has quoted the colon names
-- of its Subscriptions' queries, so nothing calls the function of the
-- `query_name_rewrite` schema again. It is dropped by name, and the schema
-- after it without `cascade`, so a function left out of the list fails the
-- migration rather than outlive the rewrite.
-- ============================================================================
drop function query_name_rewrite.path_text_with_colon_names_quoted;

drop schema query_name_rewrite;
