//! Generate a standard Source Map v3 from the emitter's checkpoints, so a
//! debugger, a bundler chaining maps, or `node --enable-source-maps` can trace
//! a position in the emitted `.ts` back to the original `.glyph`.
//!
//! The emitter records the `.ts` byte range of every declaration, statement
//! and expression it rendered. The start of each range becomes a mapping
//! segment from the generated (line, col) to the start of its Glyph span, and
//! the end of a range nested in another becomes a segment back to the
//! enclosing span, so code after an inner expression is not attributed to it.
//! Granularity is the expression, not the token.

use glyph_emit::MappedRange;

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
    ranges: &[MappedRange],
) -> String {
    // (generated offset, source offset) points, in the order they take effect.
    // A later point at the same generated offset overrides an earlier one, so
    // an inner range starting where its parent starts wins.
    let mut points: Vec<(usize, usize)> = Vec::new();
    let mut sorted: Vec<&MappedRange> = ranges.iter().collect();
    sorted.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    // Ranges whose end has not been reached yet, outermost first.
    let mut open: Vec<&MappedRange> = Vec::new();
    let close_until = |open: &mut Vec<&MappedRange>, points: &mut Vec<(usize, usize)>, at: usize| {
        while let Some(top) = open.last() {
            if top.end > at {
                break;
            }
            let end = top.end;
            open.pop();
            if let Some(parent) = open.last() {
                points.push((end, parent.span.start as usize));
            }
        }
    };
    for r in sorted {
        close_until(&mut open, &mut points, r.start);
        points.push((r.start, r.span.start as usize));
        open.push(r);
    }
    close_until(&mut open, &mut points, usize::MAX);

    // (gen_line, gen_col, src_line, src_col), all 0-based. Points are already in
    // generated order; collapse each run at one generated position to its last.
    let mut segs: Vec<(i64, i64, i64, i64)> = Vec::new();
    for (gen_off, src_off) in points {
        let (gl, gc) = line_col0(ts, gen_off);
        let (sl, sc) = line_col0(glyph_source, src_off);
        match segs.last_mut() {
            Some(last) if (last.0, last.1) == (gl, gc) => *last = (gl, gc, sl, sc),
            _ => segs.push((gl, gc, sl, sc)),
        }
    }

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
    use glyph_ast::Span;

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

    /// An inner range gets its own segment at its start, and the code after it
    /// in the enclosing range maps back to the enclosing span, not to it.
    #[test]
    fn a_nested_range_maps_back_to_its_parent_after_it_ends() {
        let ts = "f(a, b);\n";
        let glyph = "f(\n  a,\n  b,\n)\n";
        let range = |start, end, span, kind| MappedRange {
            start,
            end,
            span,
            kind,
        };
        let ranges = vec![
            range(0, 8, Span::new(0, 12), glyph_emit::MappedKind::Stmt),
            range(2, 3, Span::new(5, 6), glyph_emit::MappedKind::Expr),
        ];
        let map = build_v3_map(ts, glyph, "x.glyph", "x.ts", &ranges);
        let v: serde_json::Value = serde_json::from_str(&map).expect("valid JSON");
        // col 0 -> (0,0); col 2 -> (1,2) i.e. delta +1 line +2 col; col 3 ->
        // back to (0,0) i.e. -1 line -2 col.
        assert_eq!(v["mappings"], "AAAA,EACE,CADF");
    }

    #[test]
    fn builds_a_valid_v3_map() {
        // Two ranges: ts 0..4 -> glyph 0; ts line 3 (offset 8) -> glyph line 2.
        let ts = "aaa\nbbb\nccc\n";
        let glyph = "module x\nfn f() {}\n";
        let range = |start, end, span| MappedRange {
            start,
            end,
            span,
            kind: glyph_emit::MappedKind::Stmt,
        };
        let ranges = vec![range(0, 4, Span::new(0, 3)), range(8, 12, Span::new(9, 12))];
        let map = build_v3_map(ts, glyph, "x.glyph", "x.ts", &ranges);
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
