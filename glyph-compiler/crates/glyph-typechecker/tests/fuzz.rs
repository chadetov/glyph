//! Property-based fuzzing of the exhaustiveness checker (Phase 1 week 8).
//!
//! For a tagged union and a `match` covering an arbitrary subset of its
//! variants (no catch-all), the checker must report a non-exhaustive match
//! exactly when some variant is left uncovered — never a false positive on a
//! complete match, never a miss on an incomplete one.

use proptest::prelude::*;

use glyph_resolver::{build_prelude, collect_module_symbols, resolve_module};
use glyph_typechecker::{assign_types, TypeError};

fn ty_errors(src: &str) -> Vec<TypeError> {
    let m = glyph_parser::parse(src).expect("generated source should parse");
    let syms = collect_module_symbols(&m).expect("generated source should collect");
    let prelude = build_prelude();
    let (resolved, _re) = resolve_module(&m, syms, &prelude);
    let (_tm, te) = assign_types(&m, &resolved, &prelude);
    te
}

/// `(n, mask)`: a union of `n` variants and which ones the match covers. Index 0
/// is always covered so the match has at least one arm (an empty match would be
/// a parse error, not an exhaustiveness one).
fn union_and_cover() -> impl Strategy<Value = (usize, Vec<bool>)> {
    (1usize..=5).prop_flat_map(|n| {
        prop::collection::vec(any::<bool>(), n).prop_map(move |mut mask| {
            mask[0] = true;
            (n, mask)
        })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn non_exhaustive_iff_a_variant_is_uncovered((n, mask) in union_and_cover()) {
        let variants: String = (0..n).map(|i| format!("  | V{i}\n")).collect();
        let arms: String = (0..n)
            .filter(|i| mask[*i])
            .map(|i| format!("    V{i} => {i},\n"))
            .collect();
        let src = format!(
            "module m\ntype U =\n{variants}fn f(u: U) -> number {{\n  return match u {{\n{arms}  }}\n}}\n"
        );

        let errs = ty_errors(&src);
        let has_nonexhaustive = errs
            .iter()
            .any(|e| matches!(e, TypeError::NonExhaustiveMatch { .. }));
        let covered = mask.iter().filter(|b| **b).count();
        let exhaustive = covered == n;

        prop_assert_eq!(
            has_nonexhaustive,
            !exhaustive,
            "covered {}/{}; errs: {:?}\nsource:\n{}",
            covered,
            n,
            errs,
            src
        );
    }
}
