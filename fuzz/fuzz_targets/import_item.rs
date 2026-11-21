//! One imported item (native, Strapi or Sanity) from any bytes, mapped onto a type with every field kind:
//! refused or mapped, never a panic.
#![no_main]

mod common;

use colophon_domain::transfer;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) {
        let _ = transfer::map(&value, &common::posts());
    }
});
