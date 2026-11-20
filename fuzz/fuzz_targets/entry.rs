//! An entry's content from any bytes, against a type with every field kind: refused or accepted, never a
//! panic; what is accepted is accepted again from its stored form.
#![no_main]

mod common;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    let posts = common::posts();
    if posts.validate(&value).is_ok() {
        assert!(posts.accepts(&value.to_string()).is_ok(), "accepted, then refused stored");
    }
});
