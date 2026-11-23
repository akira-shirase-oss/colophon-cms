//! Migration and bulk operations from any bytes, applied to a document: refused or applied, never a
//! panic. The input is `{"ops": …, "item": …, "doc": …}`.
#![no_main]

use colophon_domain::{bulk, migrate};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    let document = value.get("doc").cloned().unwrap_or(Value::Null);
    if let Some(ops) = value.get("ops")
        && let Ok(ops) = migrate::parse(ops)
    {
        let _ = migrate::apply(&document, &ops);
    }
    if let Some(item) = value.get("item")
        && let Ok(op) = bulk::parse(item)
    {
        let _ = bulk::apply(&op, &document);
    }
});
