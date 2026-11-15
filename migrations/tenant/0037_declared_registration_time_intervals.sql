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
-- `instant` declared as a `DateTime` when it is a string, as it is otherwise.
-- Temporary, as only the rewrite here calls it.
create function pg_temp.declared_date_time (instant jsonb)
    returns jsonb
    language sql
    immutable
    strict
    parallel safe
    return case when jsonb_typeof(instant) = 'string' then
        jsonb_build_object('@type', 'https://uri.etsi.org/ngsi-ld/DateTime', '@value', instant)
    else
        instant
    end;

-- A time interval with its `startAt` and its `endAt`, when it has one,
-- declared as a `DateTime`. Temporary, as only the rewrite here calls it.
create function pg_temp.declared_time_interval (time_interval jsonb)
    returns jsonb
    language sql
    immutable
    strict
    parallel safe
    return time_interval || jsonb_strip_nulls(jsonb_build_object('startAt', pg_temp.declared_date_time (time_interval -> 'startAt'), 'endAt', pg_temp.declared_date_time (time_interval -> 'endAt')));

update
    context_source_registrations
set
    context_source_properties = context_source_properties || jsonb_strip_nulls(jsonb_build_object('https://uri.etsi.org/ngsi-ld/managementInterval', pg_temp.declared_time_interval(context_source_properties -> 'https://uri.etsi.org/ngsi-ld/managementInterval'), 'https://uri.etsi.org/ngsi-ld/observationInterval', pg_temp.declared_time_interval(context_source_properties -> 'https://uri.etsi.org/ngsi-ld/observationInterval')))
where
    jsonb_typeof(context_source_properties #> '{https://uri.etsi.org/ngsi-ld/managementInterval,startAt}') = 'string'
    or jsonb_typeof(context_source_properties #> '{https://uri.etsi.org/ngsi-ld/managementInterval,endAt}') = 'string'
    or jsonb_typeof(context_source_properties #> '{https://uri.etsi.org/ngsi-ld/observationInterval,startAt}') = 'string'
    or jsonb_typeof(context_source_properties #> '{https://uri.etsi.org/ngsi-ld/observationInterval,endAt}') = 'string';
