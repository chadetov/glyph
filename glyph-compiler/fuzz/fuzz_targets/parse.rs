//! The parser must not panic on any input.
//!
//! It is the one component reachable from untrusted bytes: an editor, an LSP
//! client, and an agent all hand it text nobody wrote by hand. A rejection is a
//! `ParseError` and a value; a panic is a crashed language server and an
//! unusable editor, so this asserts only that the function returns.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(src) = std::str::from_utf8(data) {
        let _ = glyph_parser::parse(src);
    }
});
