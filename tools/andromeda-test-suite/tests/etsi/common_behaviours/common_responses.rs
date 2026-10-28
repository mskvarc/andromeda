//! The ETSI common responses: media types, missing `Accept`, unavailable `@context`s and invalid parameters.

mod verify_get_without_accept;
mod verify_invalid_parameters;
mod verify_ld_context_not_available;
mod verify_merge_patch_json;
mod verify_not_acceptable_media_type;
mod verify_unsupported_media_type;
