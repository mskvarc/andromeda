-- ============================================================================
-- 0037: Declared registration time intervals
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 4.9, Table 5.2.11-1
--
-- The Context Source Property document a `csf` filter reads declares the
-- `startAt` and `endAt` of `managementInterval` and `observationInterval` as
-- the `DateTime` value objects Table 5.2.11-1 types them as, the way it
-- declares `expiresAt`, so a time operand compares with each. A document
-- v1.0.0 stored holds each of those instants as a plain string, which no time
-- operand compares with. The rewrite declares each one in place, its text
-- kept as the `@value`, and leaves every other member, and every document
-- holding no such string, as it is.
-- ============================================================================
-- `document` with the instant at path `instant` declared as a `DateTime` when
-- it is a string, and unchanged otherwise. Temporary, as only the rewrite
-- here calls it.
create function pg_temp.declared_date_time (document jsonb, instant text[])
    returns jsonb
    language sql
    immutable
    parallel safe
    return case when jsonb_typeof(document #> instant) = 'string' then
        jsonb_set(document, instant, jsonb_build_object('@type', 'https://uri.etsi.org/ngsi-ld/DateTime', '@value', document #> instant))
    else
        document
    end;

update
    context_source_registrations
set
    context_source_properties = pg_temp.declared_date_time(pg_temp.declared_date_time(pg_temp.declared_date_time(pg_temp.declared_date_time(context_source_properties, '{https://uri.etsi.org/ngsi-ld/managementInterval,startAt}'), '{https://uri.etsi.org/ngsi-ld/managementInterval,endAt}'), '{https://uri.etsi.org/ngsi-ld/observationInterval,startAt}'), '{https://uri.etsi.org/ngsi-ld/observationInterval,endAt}')
where
    jsonb_typeof(context_source_properties #> '{https://uri.etsi.org/ngsi-ld/managementInterval,startAt}') = 'string'
    or jsonb_typeof(context_source_properties #> '{https://uri.etsi.org/ngsi-ld/managementInterval,endAt}') = 'string'
    or jsonb_typeof(context_source_properties #> '{https://uri.etsi.org/ngsi-ld/observationInterval,startAt}') = 'string'
    or jsonb_typeof(context_source_properties #> '{https://uri.etsi.org/ngsi-ld/observationInterval,endAt}') = 'string';

drop function pg_temp.declared_date_time;
