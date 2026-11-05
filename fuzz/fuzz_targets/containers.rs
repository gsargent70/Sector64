#![no_main]
//! Fuzz the container parsers: no input may panic.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = sector64::parse_t64(data);
    let _ = sector64::parse_lnx(data);
    let _ = sector64::parse_zipcode([data, data, data, data]);
});
