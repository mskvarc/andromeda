//! The v1.0 key tree: every key loads at its path, no earlier spelling of it
//! is read, and every `mode` refuses a value it does not define.

use crate::{
    load::tests::{load_file, violations_of},
    validate::violation::ConfigurationViolation,
};
use pretty_assertions::assert_eq;
use serial_test::serial;
use url_macro::url;

/// Every key that moved, as the file spelling it once had, the key name a
/// refusal must name, and the file spelling it has now.
const MOVED: &[(&str, &str, &str)] = &[
    (
        "[server]\ngraceful_drain = \"30s\"\n",
        "graceful_drain",
        "[server]\ngraceful_drain_timeout = \"30s\"\n",
    ),
    (
        "[server]\nannounced_close_delay = \"1s\"\n",
        "announced_close_delay",
        "[server]\nannounced_close_timeout = \"1s\"\n",
    ),
    (
        "[server]\nmax_request_header_section = \"16 KiB\"\n",
        "max_request_header_section",
        "[request_limits]\nmax_header_section = \"16 KiB\"\n",
    ),
    ("[http3]\nmode = \"disabled\"\n", "http3", "[server.http3]\nmode = \"disabled\"\n"),
    (
        "[server.http3]\nadvertisement_max_age = \"1h\"\n",
        "advertisement_max_age",
        "[server.http3]\nadvertisement_lifetime = \"1h\"\n",
    ),
    (
        "[tls]\ncertificate_path = \"/c.crt\"\n",
        "certificate_path",
        "[tls]\ncertificate = \"/c.crt\"\n",
    ),
    (
        "[tls]\nprivate_key_path = \"/c.key\"\n",
        "private_key_path",
        "[tls]\nprivate_key = \"/c.key\"\n",
    ),
    (
        "[tls]\nclient_ca_path = \"/ca.crt\"\n",
        "client_ca_path",
        "[tls.client_authentication]\nmode = \"required\"\nca = \"/ca.crt\"\n",
    ),
    ("[tls]\nreload_debounce = \"2s\"\n", "reload_debounce", "[tls]\nreload_settle_after = \"2s\"\n"),
    (
        "[management]\nmetrics = \"prometheus\"\n",
        "metrics",
        "[observability.metrics]\nmode = \"prometheus\"\n",
    ),
    ("[management]\nmode = \"merged\"\n", "merged", "[management]\nmode = \"shared-port\"\n"),
    (
        "[http]\nrequest_timeout = \"30s\"\n",
        "request_timeout",
        "[request_limits]\ntimeout = \"30s\"\n",
    ),
    (
        "[http]\nmax_request_body = \"1 MiB\"\n",
        "max_request_body",
        "[request_limits]\nmax_body = \"1 MiB\"\n",
    ),
    (
        "[http]\nmax_request_json_values = 10\n",
        "max_request_json_values",
        "[request_limits]\nmax_json_values = 10\n",
    ),
    (
        "[entity]\nmax_entity_json_values = 10\n",
        "max_entity_json_values",
        "[request_limits]\nmax_entity_json_values = 10\n",
    ),
    (
        "[entity]\nmax_geometry_positions = 10\n",
        "max_geometry_positions",
        "[request_limits]\nmax_geometry_positions = 10\n",
    ),
    (
        "[entity]\nmax_attribute_nesting = 10\n",
        "max_attribute_nesting",
        "[request_limits]\nmax_attribute_nesting = 10\n",
    ),
    (
        "[http]\nblocking_decode_threshold = \"1 MiB\"\n",
        "blocking_decode_threshold",
        "[http.body_decoding]\nblocking_threshold = \"1 MiB\"\n",
    ),
    (
        "[http]\nmax_concurrent_blocking_decodes = 2\n",
        "max_concurrent_blocking_decodes",
        "[http.body_decoding]\nmax_concurrent_blocking = 2\n",
    ),
    (
        "[http.cors]\nallow_credentials = false\n",
        "allow_credentials",
        "[http.cors]\ncredentials = \"disallowed\"\n",
    ),
    (
        "[http.cors]\nallow_private_network = false\n",
        "allow_private_network",
        "[http.cors]\nprivate_network = \"disallowed\"\n",
    ),
    ("[http.cors]\nmax_age = \"10m\"\n", "max_age", "[http.cors]\npreflight_lifetime = \"10m\"\n"),
    (
        "[http.decompression]\npass_through_unaccepted = true\n",
        "pass_through_unaccepted",
        "[http.decompression]\nunaccepted_encoding = \"pass-through\"\n",
    ),
    ("[http.helmet]\nmode = \"enabled\"\n", "helmet", "[http.security_headers]\nmode = \"enabled\"\n"),
    (
        "[database]\npreload_name_dictionaries = false\n",
        "preload_name_dictionaries",
        "[database.name_cache]\nloading = \"lazy\"\n",
    ),
    (
        "[database]\ncontext_hit_flush = \"5s\"\n",
        "context_hit_flush",
        "[jsonld.cache]\nhit_flush_interval = \"5s\"\n",
    ),
    (
        "[database.pooler]\nurl = \"postgres://p/db\"\ntenant_max_connections = 2\n",
        "tenant_max_connections",
        "[database.pooler]\nurl = \"postgres://p/db\"\nmax_connections_per_tenant = 2\n",
    ),
    (
        "[database.name_cache]\ncapacity_per_dictionary = \"8 MiB\"\n",
        "capacity_per_dictionary",
        "[database.name_cache]\nmax_size_per_dictionary = \"8 MiB\"\n",
    ),
    (
        "[database.name_cache]\nidle_eviction = \"1h\"\n",
        "idle_eviction",
        "[database.name_cache]\nidle_eviction_after = \"1h\"\n",
    ),
    ("[tenancy]\nenabled = true\n", "enabled", "[tenancy]\nmode = \"enabled\"\n"),
    (
        "[tenancy]\ntenant_db_prefix = \"t\"\n",
        "tenant_db_prefix",
        "[tenancy.databases]\nname_prefix = \"t\"\n",
    ),
    (
        "[tenancy]\ntenant_max_connections = 4\n",
        "tenant_max_connections",
        "[tenancy.databases]\nmax_connections = 4\n",
    ),
    (
        "[tenancy]\nruntime_idle_ttl = \"15m\"\n",
        "runtime_idle_ttl",
        "[tenancy.residency]\nidle_eviction_after = \"15m\"\n",
    ),
    (
        "[tenancy]\nruntime_max_cached = 4\n",
        "runtime_max_cached",
        "[tenancy.residency]\nmax_resident = 4\n",
    ),
    (
        "[tenancy]\njanitor_interval = \"30s\"\n",
        "janitor_interval",
        "[tenancy.residency]\nsweep_interval = \"30s\"\n",
    ),
    (
        "[tenancy]\nmin_dormancy = \"30s\"\n",
        "min_dormancy",
        "[tenancy.residency]\nmin_wake_interval = \"30s\"\n",
    ),
    (
        "[tenancy]\nwake_residency = \"1m\"\n",
        "wake_residency",
        "[tenancy.residency]\nwoken_lifetime = \"1m\"\n",
    ),
    (
        "[tenancy]\nwaker_max_wakes_per_pass = 2\n",
        "waker_max_wakes_per_pass",
        "[tenancy.residency]\nmax_wakes_per_pass = 2\n",
    ),
    (
        "[tenancy]\neviction_concurrency = 2\n",
        "eviction_concurrency",
        "[tenancy.residency]\nmax_concurrent_evictions = 2\n",
    ),
    (
        "[tenancy]\nmax_concurrent_builds = 2\n",
        "max_concurrent_builds",
        "[tenancy.build]\nmax_concurrent = 2\n",
    ),
    (
        "[tenancy]\nbuild_admission_timeout = \"20s\"\n",
        "build_admission_timeout",
        "[tenancy.build]\nadmission_timeout = \"20s\"\n",
    ),
    (
        "[tenancy]\nprovisional_tenant_ttl = \"10m\"\n",
        "provisional_tenant_ttl",
        "[tenancy]\nprovisional_tenant_lifetime = \"10m\"\n",
    ),
    (
        "[temporal]\nexpiration_check_interval = \"30s\"\n",
        "expiration_check_interval",
        "[expiry]\nsweep_interval = \"30s\"\n",
    ),
    ("[entity]\nexpiry_batch_size = 10\n", "expiry_batch_size", "[expiry]\nbatch_size = 10\n"),
    (
        "[jsonld]\nallow_remote_contexts = false\n",
        "allow_remote_contexts",
        "[jsonld.fetch]\nmode = \"disabled\"\n",
    ),
    ("[jsonld]\ntimeout = \"30s\"\n", "timeout", "[jsonld.fetch]\ntimeout = \"30s\"\n"),
    (
        "[federation.forwarding]\nprivate_ip_blocking = \"refused\"\n",
        "private_ip_blocking",
        "[federation.forwarding]\nprivate_addresses = \"refused\"\n",
    ),
    ("[jsonld]\nmax_redirects = 5\n", "max_redirects", "[jsonld.fetch]\nmax_redirects = 5\n"),
    ("[jsonld]\nmax_context = \"2 MiB\"\n", "max_context", "[jsonld.fetch]\nmax_size = \"2 MiB\"\n"),
    ("[jsonld]\ncache_capacity = 10\n", "cache_capacity", "[jsonld.cache]\nmax_entries = 10\n"),
    ("[jsonld]\ncache_ttl = \"1h\"\n", "cache_ttl", "[jsonld.cache]\ndefault_lifetime = \"1h\"\n"),
    (
        "[jsonld]\nhosted_context_revalidation = \"1m\"\n",
        "hosted_context_revalidation",
        "[jsonld.cache]\nhosted_revalidation_interval = \"1m\"\n",
    ),
    (
        "[jsonld.preload_contexts]\n\"https://example.com/c.jsonld\" = \"/c.jsonld\"\n",
        "preload_contexts",
        "[[jsonld.preloaded_contexts]]\nurl = \"https://example.com/c.jsonld\"\npath = \"/c.jsonld\"\n",
    ),
    ("[entity]\nbatch_chunk_size = 10\n", "batch_chunk_size", "[entity.batch]\nchunk_size = 10\n"),
    (
        "[entity]\nbatch_large_threshold = 10\n",
        "batch_large_threshold",
        "[entity.batch]\nlarge_threshold = 10\n",
    ),
    (
        "[entity]\nbatch_prepare_blocking_threshold = 10\n",
        "batch_prepare_blocking_threshold",
        "[entity.batch]\nprepare_blocking_threshold = 10\n",
    ),
    (
        "[entity]\nbatch_chunk_concurrency = 2\n",
        "batch_chunk_concurrency",
        "[entity.batch]\nmax_concurrent_chunks = 2\n",
    ),
    (
        "[entity]\nbatch_chunk_write_concurrency = 2\n",
        "batch_chunk_write_concurrency",
        "[entity.batch]\nmax_concurrent_chunk_writes = 2\n",
    ),
    (
        "[entity]\nbatch_async_side_effect_threshold = 10\n",
        "batch_async_side_effect_threshold",
        "[entity.batch]\nasync_side_effect_threshold = 10\n",
    ),
    (
        "[entity]\npending_side_effects_poll = \"1s\"\n",
        "pending_side_effects_poll",
        "[entity.side_effects.queue]\npoll_interval = \"1s\"\n",
    ),
    (
        "[entity]\npending_side_effects_poll_max = \"30s\"\n",
        "pending_side_effects_poll_max",
        "[entity.side_effects.queue]\nmax_poll_interval = \"30s\"\n",
    ),
    (
        "[entity]\npending_side_effects_lease = \"30s\"\n",
        "pending_side_effects_lease",
        "[entity.side_effects.queue]\nlease_timeout = \"30s\"\n",
    ),
    (
        "[entity]\npending_side_effects_batch_size = 8\n",
        "pending_side_effects_batch_size",
        "[entity.side_effects]\nbatch_size = 8\n",
    ),
    (
        "[entity]\npending_side_effects_max_attempts = 3\n",
        "pending_side_effects_max_attempts",
        "[entity.side_effects.queue]\nmax_attempts = 3\n",
    ),
    (
        "[entity]\npending_side_effects_base_backoff = \"2s\"\n",
        "pending_side_effects_base_backoff",
        "[entity.side_effects.queue]\nretry_interval = \"2s\"\n",
    ),
    (
        "[entity]\npending_side_effects_dead_letter_after = \"7days\"\n",
        "pending_side_effects_dead_letter_after",
        "[entity.side_effects.retention]\ndead_letter_after = \"7days\"\n",
    ),
    (
        "[entity]\npending_side_effects_cleanup_interval = \"5m\"\n",
        "pending_side_effects_cleanup_interval",
        "[entity.side_effects.retention]\ncleanup_interval = \"5m\"\n",
    ),
    (
        "[spatial]\nconsistency = \"watermark\"\n",
        "spatial",
        "[entity.spatial_index]\nconsistency = \"watermark\"\n",
    ),
    (
        "[temporal]\ncore_write_history = \"recorded\"\n",
        "core_write_history",
        "[temporal.history]\ncore_writes = \"recorded\"\n",
    ),
    ("[temporal]\ndefault_last_n = 10\n", "default_last_n", "[temporal.query]\ndefault_last_n = 10\n"),
    ("[temporal]\nmax_last_n = 1000\n", "max_last_n", "[temporal.query]\nmax_last_n = 1000\n"),
    (
        "[temporal]\nmax_edge_group = 100000\n",
        "max_edge_group",
        "[temporal.query]\nmax_edge_group = 100000\n",
    ),
    (
        "[temporal]\naggregation_instance_budget = 10\n",
        "aggregation_instance_budget",
        "[temporal.query]\nmax_aggregation_instances = 10\n",
    ),
    (
        "[temporal]\nintent_fallback = \"1s\"\n",
        "intent_fallback",
        "[temporal.history.queue]\npoll_interval = \"1s\"\n",
    ),
    (
        "[temporal]\nintent_fallback_max = \"30s\"\n",
        "intent_fallback_max",
        "[temporal.history.queue]\nmax_poll_interval = \"30s\"\n",
    ),
    (
        "[temporal]\nintent_batch_size = 10\n",
        "intent_batch_size",
        "[temporal.history]\nbatch_size = 10\n",
    ),
    (
        "[temporal]\nintent_lease = \"30s\"\n",
        "intent_lease",
        "[temporal.history.queue]\nlease_timeout = \"30s\"\n",
    ),
    (
        "[temporal]\nintent_retry = \"2s\"\n",
        "intent_retry",
        "[temporal.history.queue]\nretry_interval = \"2s\"\n",
    ),
    (
        "[temporal]\nintent_retry_max = \"5m\"\n",
        "intent_retry_max",
        "[temporal.history.queue]\nmax_retry_interval = \"5m\"\n",
    ),
    (
        "[temporal]\nintent_stuck_after_attempts = 3\n",
        "intent_stuck_after_attempts",
        "[temporal.history.queue]\nmax_attempts = 3\n",
    ),
    (
        "[temporal]\nchunk_interval = \"7days\"\n",
        "chunk_interval",
        "[temporal.storage]\nchunk_interval = \"7days\"\n",
    ),
    (
        "[temporal]\ncolumnstore_after = \"30days\"\n",
        "columnstore_after",
        "[temporal.storage.columnstore]\nconvert_after = \"30days\"\n",
    ),
    (
        "[temporal]\ncolumnstore_sweep_interval = \"1h\"\n",
        "columnstore_sweep_interval",
        "[temporal.storage.columnstore]\nsweep_interval = \"1h\"\n",
    ),
    (
        "[temporal]\nretention_horizon = \"365days\"\n",
        "retention_horizon",
        "[temporal.storage.retention]\nmode = \"enabled\"\ndelete_after = \"365days\"\n",
    ),
    (
        "[temporal]\nretention_sweep_interval = \"1h\"\n",
        "retention_sweep_interval",
        "[temporal.storage.retention]\nsweep_interval = \"1h\"\n",
    ),
    (
        "[notifications]\ninterval_max_sleep = \"30s\"\n",
        "interval_max_sleep",
        "[notifications.periodic]\nmax_poll_interval = \"30s\"\n",
    ),
    (
        "[notifications]\nlive_change_capacity = 10\n",
        "live_change_capacity",
        "[notifications.evaluation]\nmax_queued_changes = 10\n",
    ),
    (
        "[notifications]\nlive_evaluation_lanes = 4\n",
        "live_evaluation_lanes",
        "[notifications.evaluation]\nmax_concurrent_changes = 4\n",
    ),
    (
        "[notifications.delivery]\nmax_attempts = 3\n",
        "max_attempts",
        "[notifications.delivery.queue]\nmax_attempts = 3\n",
    ),
    (
        "[notifications.delivery]\nbase_backoff = \"5s\"\n",
        "base_backoff",
        "[notifications.delivery.queue]\nretry_interval = \"5s\"\n",
    ),
    (
        "[notifications.delivery]\npoll_interval = \"1s\"\n",
        "poll_interval",
        "[notifications.delivery.queue]\npoll_interval = \"1s\"\n",
    ),
    (
        "[notifications.delivery]\npoll_interval_max = \"30s\"\n",
        "poll_interval_max",
        "[notifications.delivery.queue]\nmax_poll_interval = \"30s\"\n",
    ),
    (
        "[entity.side_effects.queue]\nbatch_size = 8\n",
        "batch_size",
        "[entity.side_effects]\nbatch_size = 8\n",
    ),
    (
        "[temporal.history.queue]\nbatch_size = 10\n",
        "batch_size",
        "[temporal.history]\nbatch_size = 10\n",
    ),
    (
        "[notifications.delivery]\nlease = \"30s\"\n",
        "lease",
        "[notifications.delivery.queue]\nlease_timeout = \"30s\"\n",
    ),
    (
        "[notifications.delivery]\nconcurrency = 4\n",
        "concurrency",
        "[notifications.delivery]\nmax_concurrent = 4\n",
    ),
    (
        "[notifications.delivery]\nerror_max_chars = 64\n",
        "error_max_chars",
        "[notifications.delivery]\nmax_error_length = 64\n",
    ),
    (
        "[notifications.delivery]\ncleanup_after = \"1day\"\n",
        "cleanup_after",
        "[notifications.delivery.retention]\nacknowledged_after = \"1day\"\n",
    ),
    (
        "[notifications.delivery]\ncleanup_interval = \"5m\"\n",
        "cleanup_interval",
        "[notifications.delivery.retention]\ncleanup_interval = \"5m\"\n",
    ),
    (
        "[notifications.delivery]\ndead_letter_after = \"7days\"\n",
        "dead_letter_after",
        "[notifications.delivery.retention]\ndead_letter_after = \"7days\"\n",
    ),
    (
        "[notifications.delivery]\norphan_event_after = \"1day\"\n",
        "orphan_event_after",
        "[notifications.delivery.retention]\norphan_event_after = \"1day\"\n",
    ),
    (
        "[federation]\nforward_concurrency = 4\n",
        "forward_concurrency",
        "[federation.forwarding]\nmax_concurrent = 4\n",
    ),
    (
        "[federation]\nconnect_timeout = \"3s\"\n",
        "connect_timeout",
        "[federation.forwarding]\nconnect_timeout = \"3s\"\n",
    ),
    (
        "[federation]\nrequest_timeout = \"5s\"\n",
        "request_timeout",
        "[federation.forwarding]\nrequest_timeout = \"5s\"\n",
    ),
    (
        "[federation]\nsplit_entities_default = \"split\"\n",
        "split_entities_default",
        "[federation.forwarding]\nsplit_entities = \"split\"\n",
    ),
    (
        "[federation]\nmax_query_entities = 10\n",
        "max_query_entities",
        "[federation.forwarding]\nmax_query_entities = 10\n",
    ),
    (
        "[federation]\nmax_query_instances = 10\n",
        "max_query_instances",
        "[federation.forwarding]\nmax_query_instances = 10\n",
    ),
    (
        "[federation]\ncache_max_entries = 10\n",
        "cache_max_entries",
        "[federation.response_cache]\nmax_entries = 10\n",
    ),
    (
        "[federation]\ncooldown_mode = \"constant\"\n",
        "cooldown_mode",
        "[federation.cooldown]\nmode = \"constant\"\n",
    ),
    (
        "[federation]\ncooldown_window = \"1s\"\n",
        "cooldown_window",
        "[federation.cooldown]\nretry_interval = \"1s\"\n",
    ),
    (
        "[federation]\ncooldown_max = \"1h\"\n",
        "cooldown_max",
        "[federation.cooldown]\nmax_retry_interval = \"1h\"\n",
    ),
    (
        "[identity]\nsource_alias = \"node\"\n",
        "identity",
        "[federation.identity]\nsource_alias = \"node\"\n",
    ),
    (
        "[federation.identity.source_extras]\nregion = \"eu\"\n",
        "source_extras",
        "[federation.identity.extras]\nregion = \"eu\"\n",
    ),
    (
        "[snapshot]\nmaximum_lifetime = \"30days\"\n",
        "maximum_lifetime",
        "[snapshot]\nmax_lifetime = \"30days\"\n",
    ),
    (
        "[snapshot]\ncapture_page_size = 10\n",
        "capture_page_size",
        "[snapshot.preparation]\npage_size = 10\n",
    ),
    (
        "[snapshot]\npreparation_concurrency = 2\n",
        "preparation_concurrency",
        "[snapshot.preparation]\nmax_concurrent = 2\n",
    ),
    (
        "[snapshot]\nscoped_operation_wait = \"30s\"\n",
        "scoped_operation_wait",
        "[snapshot.preparation]\nscoped_operation_timeout = \"30s\"\n",
    ),
    (
        "[snapshot]\npreparation_lease = \"30s\"\n",
        "preparation_lease",
        "[snapshot.preparation]\nlease_timeout = \"30s\"\n",
    ),
    (
        "[snapshot]\npreparation_recovery = \"30s\"\n",
        "preparation_recovery",
        "[snapshot.preparation]\nrecovery_interval = \"30s\"\n",
    ),
    (
        "[snapshot]\ntenant_count_quota = 10\n",
        "tenant_count_quota",
        "[snapshot.quota]\nmax_per_tenant = 10\n",
    ),
    (
        "[snapshot]\ntenant_storage_quota = \"1 GiB\"\n",
        "tenant_storage_quota",
        "[snapshot.quota]\nmax_storage_per_tenant = \"1 GiB\"\n",
    ),
    (
        "[observability]\nlog_level = \"info\"\n",
        "log_level",
        "[observability.log]\nlevel = \"info\"\n",
    ),
    (
        "[observability.file_logging]\nmode = \"disabled\"\n",
        "file_logging",
        "[observability.log.file]\nmode = \"disabled\"\n",
    ),
    (
        "[observability.log.file]\nfile_name = \"a.log\"\n",
        "file_name",
        "[observability.log.file]\nname = \"a.log\"\n",
    ),
    (
        "[observability.log.file]\nretained_files = 3\n",
        "retained_files",
        "[observability.log.file]\nmax_retained_files = 3\n",
    ),
    (
        "[extensions]\nconformance_mode = \"enabled\"\n",
        "conformance_mode",
        "[extensions]\nmode = \"conformance\"\n",
    ),
    (
        "[pagination]\ndefault_mode = \"offset\"\n",
        "pagination",
        "[extensions.cursor_pagination]\nmode = \"available\"\n",
    ),
    (
        "[extensions]\ntemporal_delete_by_query_max_matches = 10\n",
        "temporal_delete_by_query_max_matches",
        "[extensions.temporal_delete_by_query]\nmax_matches = 10\n",
    ),
    (
        "[extensions]\ntemporal_aggregation_grouping_max_groups = 10\n",
        "temporal_aggregation_grouping_max_groups",
        "[extensions.temporal_aggregation_grouping]\nmax_groups = 10\n",
    ),
];

/// Every extension that is one table with a `mode`.
const EXTENSION_TABLES: [&str; 9] = [
    "cursor_pagination",
    "temporal_delete_by_query",
    "temporal_aggregation_grouping",
    "temporal_linked_retrieval",
    "purge_dataset_scope",
    "tenant_management",
    "near_distance_band",
    "accept_language",
    "q_target_languages",
];

#[test]
#[serial]
fn every_moved_key_loads_at_its_new_path_and_its_old_path_is_refused_by_name() {
    for (old, old_key, new) in MOVED {
        let refused = load_file(old).map(|_| ()).map_err(|error| error.to_string());
        assert!(
            refused.as_ref().is_err_and(|message| message.contains(old_key)),
            "the old spelling {old:?} was not refused by `{old_key}`: {refused:?}"
        );
        assert!(
            load_file(new).is_ok(),
            "the new spelling {new:?} did not load: {:?}",
            load_file(new).map(|_| ())
        );
    }
}

/// A delivery pass keeps `max_concurrent` deliveries in flight until the queue
/// holds no more due ones, so no batch bounds it and the delivery queue takes
/// no `batch_size`, in either table it was ever written in.
#[test]
#[serial]
fn a_notification_delivery_batch_size_is_refused_by_name() {
    for removed in [
        "[notifications.delivery]\nbatch_size = 10\n",
        "[notifications.delivery.queue]\nbatch_size = 10\n",
    ] {
        let refused = load_file(removed).map(|_| ()).map_err(|error| error.to_string());
        assert!(
            refused.as_ref().is_err_and(|message| message.contains("batch_size")),
            "{removed:?} was not refused by `batch_size`: {refused:?}"
        );
    }
}

/// An extension switch was a key of `[extensions]`; it is a table of its own
/// now, so the old scalar is refused by its type.
#[test]
#[serial]
fn an_extension_switch_written_as_a_scalar_is_refused() {
    for extension in EXTENSION_TABLES {
        let refused = load_file(&format!("[extensions]\n{extension} = \"enabled\"\n"))
            .map(|_| ())
            .map_err(|error| error.to_string());
        assert!(refused.as_ref().is_err_and(|message| message.contains(extension)), "{extension}: {refused:?}");
        assert!(
            load_file(&format!("[extensions.{extension}]\nmode = \"{}\"\n", enabled_value(extension))).is_ok(),
            "{extension}"
        );
    }
}

/// The value that switches `extension` on.
fn enabled_value(extension: &str) -> &'static str {
    match extension {
        "cursor_pagination" => "available",
        "q_target_languages" => "language-tags",
        _ => "enabled",
    }
}

#[test]
#[serial]
fn every_mode_refuses_a_value_it_does_not_define() {
    let mut tables = vec![
        "server.http3",
        "tls.client_authentication",
        "management",
        "http.cors",
        "http.server_timing",
        "http.security_headers",
        "http.csrf",
        "database.maintenance",
        "tenancy",
        "jsonld.fetch",
        "temporal.storage.retention",
        "notifications.delivery",
        "federation.cooldown",
        "extensions",
        "observability.log.file",
        "observability.metrics",
    ];
    let extensions = EXTENSION_TABLES.map(|extension| format!("extensions.{extension}"));
    tables.extend(extensions.iter().map(String::as_str));
    for table in tables {
        let refused = load_file(&format!("[{table}]\nmode = \"bogus\"\n"))
            .map(|_| ())
            .map_err(|error| error.to_string());
        assert!(
            refused
                .as_ref()
                .is_err_and(|message| message.contains("bogus") && message.contains(&format!("{table}.mode"))),
            "{table}: {refused:?}"
        );
    }
}

#[test]
#[serial]
fn a_url_preloaded_twice_is_refused() {
    let refused = violations_of(
        "[[jsonld.preloaded_contexts]]\nurl = \"https://example.com/c.jsonld\"\npath = \"/a.jsonld\"\n\
         [[jsonld.preloaded_contexts]]\nurl = \"https://example.com/c.jsonld\"\npath = \"/b.jsonld\"\n",
    );
    assert_eq!(
        refused,
        Some(vec![ConfigurationViolation::DuplicatePreloadedContext {
            url: url!("https://example.com/c.jsonld")
        }])
    );
}

/// `contextSourceExtras` is raw `@json` (Table 5.2.40-1 and the core
/// context), never interpreted, so no key of it is reserved: `id` loads like
/// any other.
#[test]
#[serial]
fn the_extras_table_reserves_no_key() {
    let config = load_file("[federation.identity.extras]\nid = \"node-7\"\n").map(|config| config.federation.identity.extras.is_some());
    assert!(matches!(config, Ok(true)), "{:?}", config.map_err(|error| error.to_string()));
}
