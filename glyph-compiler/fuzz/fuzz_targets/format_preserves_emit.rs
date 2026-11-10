//! Formatting must not change the emitted TypeScript, byte for byte.
//!
//! `format_idempotent` checks what the formatted source says: its literals,
//! its comments, and that a second pass changes nothing. It cannot see a
//! formatter that writes the same text as a different program. The nightly
//! found one in JSX: `"(    "` came back as `"(", " "`, three spaces of
//! rendered text gone and a child added, from a formatter whose output was a
//! fixed point (record 84). The emit oracle is the property that names it.
//!
//! The corpus test in `glyph-formatter/tests/format.rs` asserts the same thing
//! over every example; this target asks it of input nobody wrote by hand.
//!
//! It is a target of its own because it runs the resolver and the checker on
//! that input. A panic in either is a real bug and should fail under this
//! target's name, but inside `format_idempotent` it would stop every
//! formatter check behind it.
#![no_main]
use libfuzzer_sys::fuzz_target;

/// Parse, resolve, assign types, emit; `None` where the pipeline declines.
/// The same steps as `emit_ts` in `glyph-formatter/tests/format.rs`.
fn emit_ts(src: &str) -> Option<String> {
    let m = glyph_parser::parse(src).ok()?;
    let syms = glyph_resolver::collect_module_symbols(&m).ok()?;
    let prelude = glyph_resolver::build_prelude();
    let (resolved, _) = glyph_resolver::resolve_module(&m, syms, &prelude);
    let (types, _) = glyph_typechecker::assign_types(&m, &resolved, &prelude);
    glyph_emit::emit_module(&m, &resolved, &types, &prelude, glyph_emit::EmitContext::single())
        .ok()
}

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(module) = glyph_parser::parse(src) else {
        return;
    };
    // A source the emitter declines has no TypeScript to preserve.
    let Some(before) = emit_ts(src) else {
        return;
    };
    let comments = glyph_lexer::comments(src);
    let once = glyph_formatter::format_module(&module, &comments, src);
    let Some(after) = emit_ts(&once) else {
        panic!("the source emits and its formatted form does not:\n{once}");
    };
    assert_eq!(before, after, "glyph fmt changed the emitted TypeScript:\n{once}");
});
