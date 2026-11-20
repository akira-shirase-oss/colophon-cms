//! A content-type definition from any bytes, with its components expanded first as the API does: refused
//! or parsed, never a panic or a quadratic stall (libFuzzer's `-timeout` catches one); a parsed type then
//! checks an entry and fills its defaults.
#![no_main]

use colophon_domain::schema::{ContentType, Unlisted, components_used, expand_components};
use libfuzzer_sys::fuzz_target;
use serde_json::json;

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    let _ = components_used(&value);
    let expanded = expand_components(&value, &Default::default(), Unlisted::Keep).unwrap_or(value);
    if let Ok(parsed) = ContentType::parse(&expanded) {
        let _ = parsed.validate(&json!({}));
        let _ = parsed.with_defaults(&json!({}));
    }
});
