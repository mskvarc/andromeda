//! The v1.0 key tree: every key loads at its path, no earlier spelling of it
//! is read, and every `mode` refuses a value it does not define.

use crate::{
    load::tests::{ignored_in, in_file, load_file, violations_of},
    validate::violation::ConfigurationViolation,
};
use pretty_assertions::assert_eq;
use serial_test::serial;
use url_macro::url;

/// Every key that moved, as the file spelling it once had, the path of the
/// setting the load ignores in it, and the file spelling it has now.
const MOVED: &[(&str, &str, &str)] = &[
    (
        "[database]\nmax_connections = 8\n",
        "database.max_connections",
        "[database]\nplatform_max_connections = 8\n\n[tenancy.default_tenant]\nmax_connections = 8\n",
    ),
    (
        "[server]\ngraceful_drain = \"30s\"\n",
        "server.graceful_drain",
        "[server]\ngraceful_drain_timeout = \"30s\"\n",
    ),
    (
        "[server]\nannounced_close_delay = \"1s\"\n",
        "server.announced_close_delay",
        "[server]\nannounced_close_timeout = \"1s\"\n",
    ),
    (
        "[server]\nmax_request_header_section = \"16 KiB\"\n",
        "server.max_request_header_section",
        "[request_limits]\nmax_header_section = \"16 KiB\"\n",
    ),
    ("[http3]\nmode = \"disabled\"\n", "http3", "[server.http3]\nmode = \"disabled\"\n"),
    (
        "[server.http3]\nadvertisement_max_age = \"1h\"\n",
        "server.http3.advertisement_max_age",
        "[server.http3]\nadvertisement_lifetime = \"1h\"\n",
    ),
    (
        "[tls]\ncertificate_path = \"/c.crt\"\n",
        "tls.certificate_path",
        "[tls]\ncertificate = \"/c.crt\"\n",
    ),
    (
        "[tls]\nprivate_key_path = \"/c.key\"\n",
        "tls.private_key_path",
        "[tls]\nprivate_key = \"/c.key\"\n",
    ),
    (
        "[tls]\nclient_ca_path = \"/ca.crt\"\n",
        "tls.client_ca_path",
        "[tls.client_authentication]\nmode = \"required\"\nca = \"/ca.crt\"\n",
    ),
    (
        "[tls]\nreload_debounce = \"2s\"\n",
        "tls.reload_debounce",
        "[tls]\nreload_settle_after = \"2s\"\n",
    ),
    (
        "[management]\nmetrics = \"prometheus\"\n",
        "management.metrics",
        "[observability.metrics.prometheus]\nmode = \"enabled\"\n",
    ),
    (
        "[http]\nrequest_timeout = \"30s\"\n",
        "http.request_timeout",
        "[request_limits]\ntimeout = \"30s\"\n",
    ),
    (
        "[http]\nmax_request_body = \"1 MiB\"\n",
        "http.max_request_body",
        "[request_limits]\nmax_body = \"1 MiB\"\n",
    ),
    (
        "[http]\nmax_request_json_values = 10\n",
        "http.max_request_json_values",
        "[request_limits]\nmax_json_values = 10\n",
    ),
    (
        "[entity]\nmax_entity_json_values = 10\n",
        "entity.max_entity_json_values",
        "[request_limits]\nmax_entity_json_values = 10\n",
    ),
    (
        "[entity]\nmax_geometry_positions = 10\n",
        "entity.max_geometry_positions",
        "[request_limits]\nmax_geometry_positions = 10\n",
    ),
    (
        "[entity]\nmax_attribute_nesting = 10\n",
        "entity.max_attribute_nesting",
        "[request_limits]\nmax_attribute_nesting = 10\n",
    ),
    (
        "[http]\nblocking_decode_threshold = \"1 MiB\"\n",
        "http.blocking_decode_threshold",
        "[http.body_decoding]\nblocking_threshold = \"1 MiB\"\n",
    ),
    (
        "[http]\nmax_concurrent_blocking_decodes = 2\n",
        "http.max_concurrent_blocking_decodes",
        "[http.body_decoding]\nmax_concurrent_blocking = 2\n",
    ),
    (
        "[http.cors]\nallow_credentials = false\n",
        "http.cors.allow_credentials",
        "[http.cors]\ncredentials = \"refused\"\n",
    ),
    (
        "[http.cors]\nallow_private_network = false\n",
        "http.cors.allow_private_network",
        "[http.cors]\nprivate_network = \"refused\"\n",
    ),
    (
        "[http.cors]\nmax_age = \"10m\"\n",
        "http.cors.max_age",
        "[http.cors]\npreflight_lifetime = \"10m\"\n",
    ),
    (
        "[http.decompression]\npass_through_unaccepted = true\n",
        "http.decompression.pass_through_unaccepted",
        "[http.decompression]\nunaccepted_encoding = \"allowed\"\n",
    ),
    (
        "[http.helmet]\nmode = \"enabled\"\n",
        "http.helmet",
        "[http.security_headers]\nmode = \"enabled\"\n",
    ),
    (
        "[database]\npreload_name_dictionaries = false\n",
        "database.preload_name_dictionaries",
        "[database.name_cache]\nloading = \"lazy\"\n",
    ),
    (
        "[database]\ncontext_hit_flush = \"5s\"\n",
        "database.context_hit_flush",
        "[jsonld.cache]\nhit_flush_interval = \"5s\"\n",
    ),
    (
        "[database.name_cache]\ncapacity_per_dictionary = \"8 MiB\"\n",
        "database.name_cache.capacity_per_dictionary",
        "[database.name_cache]\nmax_size_per_dictionary = \"8 MiB\"\n",
    ),
    (
        "[database.name_cache]\nidle_eviction = \"1h\"\n",
        "database.name_cache.idle_eviction",
        "[database.name_cache]\nidle_eviction_after = \"1h\"\n",
    ),
    ("[tenancy]\nenabled = true\n", "tenancy.enabled", "[tenancy]\nmode = \"enabled\"\n"),
    (
        "[tenancy]\ntenant_db_prefix = \"t\"\n",
        "tenancy.tenant_db_prefix",
        "[tenancy.databases]\nname_prefix = \"t\"\n",
    ),
    (
        "[tenancy]\ntenant_max_connections = 4\n",
        "tenancy.tenant_max_connections",
        "[tenancy.databases]\nmax_connections = 4\n",
    ),
    (
        "[tenancy]\nruntime_idle_ttl = \"15m\"\n",
        "tenancy.runtime_idle_ttl",
        "[tenancy.residency]\nidle_eviction_after = \"15m\"\n",
    ),
    (
        "[tenancy]\nruntime_max_cached = 4\n",
        "tenancy.runtime_max_cached",
        "[tenancy.residency]\nmax_resident = 4\n",
    ),
    (
        "[tenancy]\njanitor_interval = \"30s\"\n",
        "tenancy.janitor_interval",
        "[tenancy.residency]\nsweep_interval = \"30s\"\n",
    ),
    (
        "[tenancy]\nmin_dormancy = \"30s\"\n",
        "tenancy.min_dormancy",
        "[tenancy.residency]\nmin_wake_interval = \"30s\"\n",
    ),
    (
        "[tenancy]\nwake_residency = \"1m\"\n",
        "tenancy.wake_residency",
        "[tenancy.residency]\nwoken_lifetime = \"1m\"\n",
    ),
    (
        "[tenancy]\nwaker_max_wakes_per_pass = 2\n",
        "tenancy.waker_max_wakes_per_pass",
        "[tenancy.residency]\nmax_wakes_per_pass = 2\n",
    ),
    (
        "[tenancy]\neviction_concurrency = 2\n",
        "tenancy.eviction_concurrency",
        "[tenancy.residency]\nmax_concurrent_evictions = 2\n",
    ),
    (
        "[tenancy]\nmax_concurrent_builds = 2\n",
        "tenancy.max_concurrent_builds",
        "[tenancy.build]\nmax_concurrent = 2\n",
    ),
    (
        "[tenancy]\nbuild_admission_timeout = \"20s\"\n",
        "tenancy.build_admission_timeout",
        "[tenancy.build]\nadmission_timeout = \"20s\"\n",
    ),
    (
        "[tenancy]\nprovisional_tenant_ttl = \"10m\"\n",
        "tenancy.provisional_tenant_ttl",
        "[tenancy]\nprovisional_tenant_lifetime = \"10m\"\n",
    ),
    (
        "[temporal]\nexpiration_check_interval = \"30s\"\n",
        "temporal.expiration_check_interval",
        "[expiry]\nsweep_interval = \"30s\"\n",
    ),
    ("[entity]\nexpiry_batch_size = 10\n", "entity.expiry_batch_size", "[expiry]\nbatch_size = 10\n"),
    (
        "[jsonld]\nallow_remote_contexts = false\n",
        "jsonld.allow_remote_contexts",
        "[jsonld.fetch]\nmode = \"disabled\"\n",
    ),
    ("[jsonld]\ntimeout = \"10s\"\n", "jsonld.timeout", "[jsonld.fetch]\ntimeout = \"10s\"\n"),
    (
        "[federation.forwarding]\nprivate_ip_blocking = \"refused\"\n",
        "federation.forwarding.private_ip_blocking",
        "[federation.forwarding]\nprivate_addresses = \"refused\"\n",
    ),
    ("[jsonld]\nmax_redirects = 5\n", "jsonld.max_redirects", "[jsonld.fetch]\nmax_redirects = 5\n"),
    (
        "[jsonld]\nmax_context = \"2 MiB\"\n",
        "jsonld.max_context",
        "[jsonld.fetch]\nmax_size = \"2 MiB\"\n",
    ),
    ("[jsonld]\ncache_capacity = 10\n", "jsonld.cache_capacity", "[jsonld.cache]\nmax_entries = 10\n"),
    (
        "[jsonld]\ncache_ttl = \"1h\"\n",
        "jsonld.cache_ttl",
        "[jsonld.cache]\ndefault_lifetime = \"1h\"\n",
    ),
    (
        "[jsonld]\nhosted_context_revalidation = \"1m\"\n",
        "jsonld.hosted_context_revalidation",
        "[jsonld.cache]\nhosted_revalidation_interval = \"1m\"\n",
    ),
    (
        "[jsonld.preload_contexts]\n\"https://example.com/c.jsonld\" = \"/c.jsonld\"\n",
        "jsonld.preload_contexts",
        "[[jsonld.preloaded_contexts]]\nurl = \"https://example.com/c.jsonld\"\npath = \"/c.jsonld\"\n",
    ),
    (
        "[entity]\nbatch_chunk_size = 10\n",
        "entity.batch_chunk_size",
        "[entity.batch]\nchunk_size = 10\n",
    ),
    (
        "[entity]\nbatch_large_threshold = 10\n",
        "entity.batch_large_threshold",
        "[entity.batch]\nlarge_threshold = 10\n",
    ),
    (
        "[entity]\nbatch_prepare_blocking_threshold = 10\n",
        "entity.batch_prepare_blocking_threshold",
        "[entity.batch]\nprepare_blocking_threshold = 10\n",
    ),
    (
        "[entity]\nbatch_chunk_concurrency = 2\n",
        "entity.batch_chunk_concurrency",
        "[entity.batch]\nmax_concurrent_chunks = 2\n",
    ),
    (
        "[entity]\nbatch_chunk_write_concurrency = 2\n",
        "entity.batch_chunk_write_concurrency",
        "[entity.batch]\nmax_concurrent_chunk_writes = 2\n",
    ),
    (
        "[entity]\nbatch_async_side_effect_threshold = 10\n",
        "entity.batch_async_side_effect_threshold",
        "[entity.batch]\nasync_side_effect_threshold = 10\n",
    ),
    (
        "[entity]\npending_side_effects_poll = \"1s\"\n",
        "entity.pending_side_effects_poll",
        "[entity.side_effects.queue]\npoll_interval = \"1s\"\n",
    ),
    (
        "[entity]\npending_side_effects_poll_max = \"30s\"\n",
        "entity.pending_side_effects_poll_max",
        "[entity.side_effects.queue]\nmax_poll_interval = \"30s\"\n",
    ),
    (
        "[entity]\npending_side_effects_lease = \"30s\"\n",
        "entity.pending_side_effects_lease",
        "[entity.side_effects.queue]\nlease_timeout = \"30s\"\n",
    ),
    (
        "[entity]\npending_side_effects_batch_size = 8\n",
        "entity.pending_side_effects_batch_size",
        "[entity.side_effects]\nbatch_size = 8\n",
    ),
    (
        "[entity]\npending_side_effects_max_attempts = 3\n",
        "entity.pending_side_effects_max_attempts",
        "[entity.side_effects]\nstuck_after_attempts = 3\n",
    ),
    (
        "[entity.side_effects.queue]\nmax_attempts = 3\n",
        "entity.side_effects.queue.max_attempts",
        "[entity.side_effects]\nstuck_after_attempts = 3\n",
    ),
    (
        "[entity]\npending_side_effects_base_backoff = \"2s\"\n",
        "entity.pending_side_effects_base_backoff",
        "[entity.side_effects.queue]\nretry_interval = \"2s\"\n",
    ),
    (
        "[entity]\npending_side_effects_dead_letter_after = \"7days\"\n",
        "entity.pending_side_effects_dead_letter_after",
        "[entity.side_effects.retention]\ndead_letter_after = \"7days\"\n",
    ),
    (
        "[entity]\npending_side_effects_cleanup_interval = \"5m\"\n",
        "entity.pending_side_effects_cleanup_interval",
        "[entity.side_effects.retention]\nsweep_interval = \"5m\"\n",
    ),
    (
        "[entity.side_effects.retention]\ncleanup_interval = \"5m\"\n",
        "entity.side_effects.retention.cleanup_interval",
        "[entity.side_effects.retention]\nsweep_interval = \"5m\"\n",
    ),
    (
        "[spatial]\nconsistency = \"watermark\"\n",
        "spatial",
        "[entity.spatial_index]\nconsistency = \"watermark\"\n",
    ),
    (
        "[temporal]\ncore_write_history = \"recorded\"\n",
        "temporal.core_write_history",
        "[temporal.history]\ncore_writes = \"recorded\"\n",
    ),
    (
        "[temporal]\ndefault_last_n = 10\n",
        "temporal.default_last_n",
        "[temporal.query]\ndefault_last_n = 10\n",
    ),
    (
        "[temporal]\nmax_last_n = 1000\n",
        "temporal.max_last_n",
        "[temporal.query]\nmax_last_n = 1000\n",
    ),
    (
        "[temporal]\nmax_edge_group = 100000\n",
        "temporal.max_edge_group",
        "[temporal.query]\nmax_edge_group = 100000\n",
    ),
    (
        "[temporal]\naggregation_instance_budget = 10\n",
        "temporal.aggregation_instance_budget",
        "[temporal.query]\nmax_aggregation_instances = 10\n",
    ),
    (
        "[temporal]\nintent_fallback = \"1s\"\n",
        "temporal.intent_fallback",
        "[temporal.history.queue]\npoll_interval = \"1s\"\n",
    ),
    (
        "[temporal]\nintent_fallback_max = \"30s\"\n",
        "temporal.intent_fallback_max",
        "[temporal.history.queue]\nmax_poll_interval = \"30s\"\n",
    ),
    (
        "[temporal]\nintent_batch_size = 10\n",
        "temporal.intent_batch_size",
        "[temporal.history]\nbatch_size = 10\n",
    ),
    (
        "[temporal]\nintent_lease = \"30s\"\n",
        "temporal.intent_lease",
        "[temporal.history.queue]\nlease_timeout = \"30s\"\n",
    ),
    (
        "[temporal]\nintent_retry = \"2s\"\n",
        "temporal.intent_retry",
        "[temporal.history.queue]\nretry_interval = \"2s\"\n",
    ),
    (
        "[temporal]\nintent_retry_max = \"5m\"\n",
        "temporal.intent_retry_max",
        "[temporal.history.queue]\nmax_retry_interval = \"5m\"\n",
    ),
    (
        "[temporal]\nintent_stuck_after_attempts = 3\n",
        "temporal.intent_stuck_after_attempts",
        "[temporal.history]\nstuck_after_attempts = 3\n",
    ),
    (
        "[temporal.history.queue]\nmax_attempts = 3\n",
        "temporal.history.queue.max_attempts",
        "[temporal.history]\nstuck_after_attempts = 3\n",
    ),
    (
        "[temporal]\nchunk_interval = \"7days\"\n",
        "temporal.chunk_interval",
        "[temporal.storage]\nchunk_interval = \"7days\"\n",
    ),
    (
        "[temporal]\ncolumnstore_after = \"30days\"\n",
        "temporal.columnstore_after",
        "[temporal.storage.columnstore]\nconvert_after = \"30days\"\n",
    ),
    (
        "[temporal]\ncolumnstore_sweep_interval = \"1h\"\n",
        "temporal.columnstore_sweep_interval",
        "[temporal.storage.columnstore]\nsweep_interval = \"1h\"\n",
    ),
    (
        "[temporal]\nretention_horizon = \"365days\"\n",
        "temporal.retention_horizon",
        "[temporal.storage.retention]\nmode = \"enabled\"\ndelete_after = \"365days\"\n",
    ),
    (
        "[temporal]\nretention_sweep_interval = \"1h\"\n",
        "temporal.retention_sweep_interval",
        "[temporal.storage.retention]\nsweep_interval = \"1h\"\n",
    ),
    (
        "[notifications]\ninterval_max_sleep = \"30s\"\n",
        "notifications.interval_max_sleep",
        "[notifications.periodic]\nmax_poll_interval = \"30s\"\n",
    ),
    (
        "[notifications]\nlive_change_capacity = 10\n",
        "notifications.live_change_capacity",
        "[notifications.evaluation]\nmax_queued_changes = 10\n",
    ),
    (
        "[notifications]\nlive_evaluation_lanes = 4\n",
        "notifications.live_evaluation_lanes",
        "[notifications.evaluation]\nmax_concurrent_changes = 4\n",
    ),
    (
        "[notifications.delivery.queue]\nmax_attempts = 3\n",
        "notifications.delivery.queue.max_attempts",
        "[notifications.delivery]\nmax_attempts = 3\n",
    ),
    (
        "[notifications.delivery]\nbase_backoff = \"5s\"\n",
        "notifications.delivery.base_backoff",
        "[notifications.delivery.queue]\nretry_interval = \"5s\"\n",
    ),
    (
        "[notifications.delivery]\npoll_interval = \"1s\"\n",
        "notifications.delivery.poll_interval",
        "[notifications.delivery.queue]\npoll_interval = \"1s\"\n",
    ),
    (
        "[notifications.delivery]\npoll_interval_max = \"30s\"\n",
        "notifications.delivery.poll_interval_max",
        "[notifications.delivery.queue]\nmax_poll_interval = \"30s\"\n",
    ),
    (
        "[entity.side_effects.queue]\nbatch_size = 8\n",
        "entity.side_effects.queue.batch_size",
        "[entity.side_effects]\nbatch_size = 8\n",
    ),
    (
        "[temporal.history.queue]\nbatch_size = 10\n",
        "temporal.history.queue.batch_size",
        "[temporal.history]\nbatch_size = 10\n",
    ),
    (
        "[notifications.delivery]\nlease = \"30s\"\n",
        "notifications.delivery.lease",
        "[notifications.delivery.queue]\nlease_timeout = \"30s\"\n",
    ),
    (
        "[notifications.delivery]\nconcurrency = 4\n",
        "notifications.delivery.concurrency",
        "[notifications.delivery]\nmax_concurrent = 4\n",
    ),
    (
        "[notifications.delivery]\nerror_max_chars = 64\n",
        "notifications.delivery.error_max_chars",
        "[notifications.delivery]\nmax_error_length = 64\n",
    ),
    (
        "[notifications.delivery]\ncleanup_after = \"1day\"\n",
        "notifications.delivery.cleanup_after",
        "[notifications.delivery.retention]\nacknowledged_after = \"1day\"\n",
    ),
    (
        "[notifications.delivery]\ncleanup_interval = \"5m\"\n",
        "notifications.delivery.cleanup_interval",
        "[notifications.delivery.retention]\nsweep_interval = \"5m\"\n",
    ),
    (
        "[notifications.delivery.retention]\ncleanup_interval = \"5m\"\n",
        "notifications.delivery.retention.cleanup_interval",
        "[notifications.delivery.retention]\nsweep_interval = \"5m\"\n",
    ),
    (
        "[notifications.delivery]\ndead_letter_after = \"7days\"\n",
        "notifications.delivery.dead_letter_after",
        "[notifications.delivery.retention]\ndead_letter_after = \"7days\"\n",
    ),
    (
        "[notifications.delivery]\norphan_event_after = \"1day\"\n",
        "notifications.delivery.orphan_event_after",
        "[notifications.delivery.retention]\norphan_event_after = \"1day\"\n",
    ),
    (
        "[federation]\nforward_concurrency = 4\n",
        "federation.forward_concurrency",
        "[federation.forwarding]\nmax_concurrent = 4\n",
    ),
    (
        "[federation]\nconnect_timeout = \"3s\"\n",
        "federation.connect_timeout",
        "[federation.forwarding]\nconnect_timeout = \"3s\"\n",
    ),
    (
        "[federation]\nrequest_timeout = \"5s\"\n",
        "federation.request_timeout",
        "[federation.forwarding]\nrequest_timeout = \"5s\"\n",
    ),
    (
        "[federation]\nsplit_entities_default = \"split\"\n",
        "federation.split_entities_default",
        "[federation.query]\nsplit_entities = \"split\"\n",
    ),
    (
        "[federation]\nmax_query_entities = 10\n",
        "federation.max_query_entities",
        "[federation.query]\nmax_entities = 10\n",
    ),
    (
        "[federation]\nmax_query_instances = 10\n",
        "federation.max_query_instances",
        "[federation.query]\nmax_instances = 10\n",
    ),
    (
        "[federation.forwarding]\nsplit_entities = \"split\"\n",
        "federation.forwarding.split_entities",
        "[federation.query]\nsplit_entities = \"split\"\n",
    ),
    (
        "[federation.forwarding]\nmax_query_entities = 10\n",
        "federation.forwarding.max_query_entities",
        "[federation.query]\nmax_entities = 10\n",
    ),
    (
        "[federation.forwarding]\nmax_query_source_rows = 30000\n",
        "federation.forwarding.max_query_source_rows",
        "[federation.query]\nmax_source_rows = 30000\n",
    ),
    (
        "[federation.forwarding]\nmax_query_instances = 10\n",
        "federation.forwarding.max_query_instances",
        "[federation.query]\nmax_instances = 10\n",
    ),
    (
        "[federation.forwarding]\nfederated_aggregation = \"instances\"\n",
        "federation.forwarding.federated_aggregation",
        "[federation.query]\naggregation = \"instances\"\n",
    ),
    (
        "[federation]\ncache_max_entries = 10\n",
        "federation.cache_max_entries",
        "[federation.response_cache]\nmax_entries = 10\n",
    ),
    (
        "[federation]\ncooldown_mode = \"constant\"\n",
        "federation.cooldown_mode",
        "[federation.cooldown]\nmode = \"constant\"\n",
    ),
    (
        "[federation]\ncooldown_window = \"1s\"\n",
        "federation.cooldown_window",
        "[federation.cooldown]\nretry_interval = \"1s\"\n",
    ),
    (
        "[federation]\ncooldown_max = \"1h\"\n",
        "federation.cooldown_max",
        "[federation.cooldown]\nmax_retry_interval = \"1h\"\n",
    ),
    (
        "[identity]\nsource_alias = \"node\"\n",
        "identity",
        "[federation.identity]\nsource_alias = \"node\"\n",
    ),
    (
        "[federation.identity.source_extras]\nregion = \"eu\"\n",
        "federation.identity.source_extras",
        "[federation.identity.extras]\nregion = \"eu\"\n",
    ),
    (
        "[snapshot]\nmaximum_lifetime = \"30days\"\n",
        "snapshot.maximum_lifetime",
        "[snapshot]\nmax_lifetime = \"30days\"\n",
    ),
    (
        "[snapshot]\ncapture_page_size = 10\n",
        "snapshot.capture_page_size",
        "[snapshot.preparation]\npage_size = 10\n",
    ),
    (
        "[snapshot]\npreparation_concurrency = 2\n",
        "snapshot.preparation_concurrency",
        "[snapshot.preparation]\nmax_concurrent = 2\n",
    ),
    (
        "[snapshot]\nscoped_operation_wait = \"20s\"\n",
        "snapshot.scoped_operation_wait",
        "[snapshot.preparation]\nscoped_operation_timeout = \"20s\"\n",
    ),
    (
        "[snapshot]\npreparation_lease = \"30s\"\n",
        "snapshot.preparation_lease",
        "[snapshot.preparation]\nlease_timeout = \"30s\"\n",
    ),
    (
        "[snapshot]\npreparation_recovery = \"30s\"\n",
        "snapshot.preparation_recovery",
        "[snapshot.preparation]\nrecovery_interval = \"30s\"\n",
    ),
    (
        "[snapshot]\ntenant_count_quota = 10\n",
        "snapshot.tenant_count_quota",
        "[snapshot.quota]\nmax_per_tenant = 10\n",
    ),
    (
        "[snapshot]\ntenant_storage_quota = \"1 GiB\"\n",
        "snapshot.tenant_storage_quota",
        "[snapshot.quota]\nmax_storage_per_tenant = \"1 GiB\"\n",
    ),
    (
        "[observability]\nlog_level = \"info\"\n",
        "observability.log_level",
        "[observability.log]\nlevel = \"info\"\n",
    ),
    (
        "[observability.file_logging]\nmode = \"disabled\"\n",
        "observability.file_logging",
        "[observability.log.file]\nmode = \"disabled\"\n",
    ),
    (
        "[observability.log.file]\nfile_name = \"a.log\"\n",
        "observability.log.file.file_name",
        "[observability.log.file]\nname = \"a.log\"\n",
    ),
    (
        "[observability.log.file]\nretained_files = 3\n",
        "observability.log.file.retained_files",
        "[observability.log.file]\nmax_retained_files = 3\n",
    ),
    (
        "[extensions]\nconformance_mode = \"enabled\"\n",
        "extensions.conformance_mode",
        "[extensions]\nmode = \"conformance\"\n",
    ),
    (
        "[pagination]\ndefault_mode = \"offset\"\n",
        "pagination",
        "[extensions.cursor_pagination]\nmode = \"enabled\"\nfresh_requests = \"offset\"\n",
    ),
    (
        "[extensions]\ntemporal_delete_by_query_max_matches = 10\n",
        "extensions.temporal_delete_by_query_max_matches",
        "[extensions.temporal_delete_by_query]\nmax_matches = 10\n",
    ),
    (
        "[extensions]\ntemporal_aggregation_grouping_max_groups = 10\n",
        "extensions.temporal_aggregation_grouping_max_groups",
        "[extensions.temporal_aggregation_grouping]\nmax_groups = 10\n",
    ),
    (
        "[observability.metrics]\nmode = \"prometheus\"\n",
        "observability.metrics.mode",
        "[observability.metrics.prometheus]\nmode = \"enabled\"\n",
    ),
];

/// Every extension that is one table with a `mode` that turns it on or off.
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
fn every_moved_key_loads_at_its_new_path_and_its_old_path_is_ignored_with_a_warning() {
    for (old, old_path, new) in MOVED {
        assert_eq!(ignored_in(old), in_file(old_path), "the old spelling {old:?}");
        assert_eq!(ignored_in(new), Vec::new(), "the new spelling {new:?}");
    }
}

/// Every earlier spelling no file loads with, as the file spelling it once
/// had, the name its refusal must give, and the file spelling it has now: a
/// value a defined key no longer defines, or a key renamed in a table that
/// requires it under its new name, which the refusal names as missing.
const REFUSED_OLD_SPELLINGS: &[(&str, &str, &str)] = &[
    ("[management]\nmode = \"merged\"\n", "merged", "[management]\nmode = \"shared-port\"\n"),
    (
        "[database.pooler]\nurl = \"postgres://p/db\"\ntenant_max_connections = 2\n",
        "max_connections_per_tenant",
        "[database.pooler]\nurl = \"postgres://p/db\"\nmax_connections_per_tenant = 2\n",
    ),
];

#[test]
#[serial]
fn every_refused_old_spelling_is_refused_by_name_and_its_new_spelling_loads() {
    for (old, named, new) in REFUSED_OLD_SPELLINGS {
        let refused = load_file(old).map(|_| ()).map_err(|error| error.to_string());
        assert!(
            refused.as_ref().is_err_and(|message| message.contains(named)),
            "the old spelling {old:?} was not refused by `{named}`: {refused:?}"
        );
        assert_eq!(ignored_in(new), Vec::new(), "the new spelling {new:?}");
    }
}

/// A delivery pass keeps `max_concurrent` deliveries in flight until the queue
/// holds no more due ones, so no batch bounds it and the delivery queue reads
/// no `batch_size`, in either table it was ever written in.
#[test]
#[serial]
fn a_notification_delivery_batch_size_is_ignored_with_a_warning() {
    for (removed, path) in [
        ("[notifications.delivery]\nbatch_size = 10\n", "notifications.delivery.batch_size"),
        ("[notifications.delivery.queue]\nbatch_size = 10\n", "notifications.delivery.queue.batch_size"),
    ] {
        assert_eq!(ignored_in(removed), in_file(path), "{removed:?}");
    }
}

/// An extension switch is a table of its own, so a scalar written at its key
/// is a value of the wrong type for a key the configuration defines, and is
/// refused by its type.
#[test]
#[serial]
fn an_extension_switch_written_as_a_scalar_is_refused() {
    for extension in EXTENSION_TABLES {
        let refused = load_file(&format!("[extensions]\n{extension} = \"enabled\"\n"))
            .map(|_| ())
            .map_err(|error| error.to_string());
        assert!(refused.as_ref().is_err_and(|message| message.contains(extension)), "{extension}: {refused:?}");
        assert!(load_file(&format!("[extensions.{extension}]\nmode = \"enabled\"\n")).is_ok(), "{extension}");
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
        "http.compression",
        "http.decompression",
        "http.server_timing",
        "http.security_headers",
        "http.csrf",
        "database.maintenance",
        "database.maintenance.repack",
        "tenancy",
        "jsonld.fetch",
        "temporal.storage.retention",
        "notifications.delivery",
        "federation.cooldown",
        "extensions",
        "observability.log.file",
        "observability.metrics.prometheus",
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
