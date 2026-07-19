//! Generate a standard Source Map v3 from the emitter's checkpoints, so a
//! debugger, a bundler chaining maps, or `node --enable-source-maps` can trace
//! a position in the emitted `.ts` back to the original `.glyph`.
//!
//! The emitter records `(byte offset in the .ts, Glyph span)` checkpoints at
//! each declaration and top-level statement. Each becomes one mapping segment
//! from a generated (line, col) to a source (line, col). Granularity is
//! statement-level — coarser than per-token, but it points the debugger at the
//! right Glyph line, which is the whole point.

use glyph_ast::Span;

const BASE64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Append the base64 VLQ encoding of a signed value.
fn vlq_encode(value: i64, out: &mut String) {
    // Shift the sign into the least-significant bit.
    let mut vlq: i64 = if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    };
    loop {
        let mut digit = (vlq & 0b1_1111) as usize;
        vlq >>= 5;
        if vlq > 0 {
            digit |= 0b10_0000; // continuation bit
        }
        out.push(BASE64[digit] as char);
        if vlq == 0 {
            break;
        }
    }
}

/// 0-based (line, column) of `offset` bytes into `src`. Columns count UTF-16
/// code units, which is what the Source Map spec uses.
fn line_col0(src: &str, offset: usize) -> (i64, i64) {
    let clamped = offset.min(src.len());
    let mut line = 0i64;
    let mut line_start = 0usize;
    for (i, b) in src.as_bytes().iter().enumerate() {
        if i >= clamped {
            break;
        }
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    let col = src[line_start..clamped].encode_utf16().count() as i64;
    (line, col)
}

/// Build the v3 source map JSON mapping `ts` back to `glyph_source`.
/// `ts_basename` is the generated file name (`main.ts`); `glyph_rel` is the
/// source path recorded in the map (`main.glyph`).
pub fn build_v3_map(
    ts: &str,
    glyph_source: &str,
    glyph_rel: &str,
    ts_basename: &str,
    checkpoints: &[(usize, Span)],
) -> String {
    // (gen_line, gen_col, src_line, src_col), all 0-based, sorted by generated
    // position and deduped.
    let mut segs: Vec<(i64, i64, i64, i64)> = checkpoints
        .iter()
        .map(|(gen_off, span)| {
            let (gl, gc) = line_col0(ts, *gen_off);
            let (sl, sc) = line_col0(glyph_source, span.start as usize);
            (gl, gc, sl, sc)
        })
        .collect();
    segs.sort_unstable();
    segs.dedup();

    let mut mappings = String::new();
    let mut cur_line = 0i64;
    let mut prev_gen_col = 0i64;
    let mut prev_src_line = 0i64;
    let mut prev_src_col = 0i64;
    let mut first_on_line = true;
    for &(gl, gc, sl, sc) in &segs {
        // Advance to the segment's generated line, emitting `;` per line. The
        // generated-column delta resets at the start of each line.
        while cur_line < gl {
            mappings.push(';');
            cur_line += 1;
            prev_gen_col = 0;
            first_on_line = true;
        }
        if !first_on_line {
            mappings.push(',');
        }
        vlq_encode(gc - prev_gen_col, &mut mappings);
        vlq_encode(0, &mut mappings); // sourceIndex delta (always source 0)
        vlq_encode(sl - prev_src_line, &mut mappings);
        vlq_encode(sc - prev_src_col, &mut mappings);
        prev_gen_col = gc;
        prev_src_line = sl;
        prev_src_col = sc;
        first_on_line = false;
    }

    let value = serde_json::json!({
        "version": 3,
        "file": ts_basename,
        "sources": [glyph_rel],
        "sourcesContent": [glyph_source],
        "names": [],
        "mappings": mappings,
    });
    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_encodes_known_values() {
        let mut s = String::new();
        vlq_encode(0, &mut s);
        assert_eq!(s, "A");
        s.clear();
        vlq_encode(1, &mut s);
        assert_eq!(s, "C");
        s.clear();
        vlq_encode(-1, &mut s);
        assert_eq!(s, "D");
        s.clear();
        vlq_encode(16, &mut s);
        assert_eq!(s, "gB");
    }

    #[test]
    fn builds_a_valid_v3_map() {
        // Two checkpoints: ts offset 0 -> glyph 0; ts offset (line 2) -> glyph line 2.
        let ts = "aaa\nbbb\nccc\n";
        let glyph = "module x\nfn f() {}\n";
        // checkpoint at ts line 3 start (offset 8) -> glyph line 2 start (offset 9)
        let checkpoints = vec![(0usize, Span::new(0, 3)), (8usize, Span::new(9, 12))];
        let map = build_v3_map(ts, glyph, "x.glyph", "x.ts", &checkpoints);
        let v: serde_json::Value = serde_json::from_str(&map).expect("valid JSON");
        assert_eq!(v["version"], 3);
        assert_eq!(v["sources"][0], "x.glyph");
        assert_eq!(v["sourcesContent"][0], glyph);
        // Mappings are non-empty and contain a line separator for the 3rd line.
        let m = v["mappings"].as_str().unwrap();
        assert!(!m.is_empty(), "mappings: {m}");
        assert!(m.contains(';'), "expected a line separator: {m}");
    }
}
