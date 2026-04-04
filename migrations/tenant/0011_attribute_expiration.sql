-- Remove expired attribute instances from an expanded JSON-LD entity payload.
--
-- The expanded payload is a JSON array with one entity object. Attribute keys
-- are full IRIs, values are arrays of instances. Each instance may have
-- "https://uri.etsi.org/ngsi-ld/expiresAt" with [{"@type": "...", "@value": "..."}].
--
-- The function iterates each key in the entity object, filters out instances
-- whose expiresAt <= cutoff, removes attribute keys with empty arrays, and
-- returns the cleaned payload.
CREATE OR REPLACE FUNCTION remove_expired_attribute_instances(
    payload JSONB,
    cutoff TIMESTAMPTZ
) RETURNS JSONB AS $$
DECLARE
    entity_obj JSONB;
    result_obj JSONB := '{}';
    attr_key TEXT;
    attr_instances JSONB;
    filtered_instances JSONB;
    instance JSONB;
    expires_value TEXT;
BEGIN
    -- Extract the single entity object from the array wrapper.
    entity_obj := payload->0;

    IF entity_obj IS NULL OR jsonb_typeof(entity_obj) != 'object' THEN
        RETURN payload;
    END IF;

    FOR attr_key, attr_instances IN SELECT key, value FROM jsonb_each(entity_obj)
    LOOP
        -- Preserve JSON-LD structural keys (@id, @type, @context, etc.) as-is.
        IF attr_key LIKE '@%' THEN
            result_obj := result_obj || jsonb_build_object(attr_key, attr_instances);
            CONTINUE;
        END IF;

        -- For attribute arrays, filter out expired instances.
        IF jsonb_typeof(attr_instances) = 'array' THEN
            filtered_instances := '[]'::jsonb;
            FOR instance IN SELECT value FROM jsonb_array_elements(attr_instances)
            LOOP
                -- Check if this instance has an expiresAt that is <= cutoff.
                expires_value := instance
                    #>>'{https://uri.etsi.org/ngsi-ld/expiresAt,0,@value}';

                IF expires_value IS NOT NULL
                   AND expires_value::timestamptz <= cutoff THEN
                    -- Skip this expired instance.
                    CONTINUE;
                END IF;

                filtered_instances := filtered_instances || jsonb_build_array(instance);
            END LOOP;

            -- Only keep the attribute key if at least one instance remains.
            IF jsonb_array_length(filtered_instances) > 0 THEN
                result_obj := result_obj || jsonb_build_object(attr_key, filtered_instances);
            END IF;
        ELSE
            -- Non-array values (shouldn't happen for well-formed expanded JSON-LD,
            -- but preserve them for safety).
            result_obj := result_obj || jsonb_build_object(attr_key, attr_instances);
        END IF;
    END LOOP;

    RETURN jsonb_build_array(result_obj);
END;
$$ LANGUAGE plpgsql IMMUTABLE;
