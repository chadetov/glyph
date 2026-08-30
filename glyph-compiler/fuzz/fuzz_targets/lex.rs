//! The lexer must not panic, and must terminate.
//!
//! Lexer defects surface as parser crashes, which is a confusing place to find
//! them, so it gets its own target. Byte-offset arithmetic over multi-byte
//! characters is the historical source of trouble here, which is why the input
//! is not restricted to ASCII.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(src) = std::str::from_utf8(data) {
        let _ = glyph_lexer::tokenize(src);
        // Comment recovery walks the source separately from tokenizing, so it
        // has its own offset arithmetic and its own way to be wrong.
        let _ = glyph_lexer::comments(src);
    }
});
