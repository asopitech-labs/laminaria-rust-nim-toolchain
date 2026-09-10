//! Issue #27's own A4 acceptance test: a finite, *seeded and reproducible*
//! generative battery over both inlining candidates, covering the shapes
//! its own checklist names explicitly -- 0-3 compositions, nesting, the
//! same callee inlined at more than one call site, source-level shadowing,
//! and unused/duplicated/reordered arguments -- rather than only the small,
//! hand-picked regression cases in `transform`'s own `mod tests`.
//!
//! Every generated program is non-recursive (a strict DAG: `mark`/`helper`
//! are leaves, `g` may call `helper`, `f` may call `g`, `entry` may call
//! `f`) and uses only this crate's existing declared subset. For each
//! seed, this asserts:
//! - `anf_insert` always succeeds (given this fuzzer only ever builds
//!   single-`Return`-shaped callees, matching `TransformError::
//!   UnsupportedCalleeShape`'s own documented boundary) and always
//!   preserves both the return *value* and the observed effect sequence,
//!   at every composition depth attempted.
//! - `checked_inline` either preserves value and observed effects
//!   identically, or rejects the whole transform outright with one of its
//!   own declared `TransformError` variants -- silently producing a wrong
//!   value or a wrong effect order is the only outcome this battery treats
//!   as a failure, matching A3's explicit allowance to conservatively
//!   reject a combination it cannot yet prove safe
//!   ("最初は証明できない組合せを保守的に拒否してよい").
//!
//! "Observed effects" means calls to `mark` specifically -- a shared,
//! explicit instrumentation contract, not an ad-hoc per-test filter (A3's
//! own distinction against comparing the disappearing callee-call event
//! itself, e.g. the `g`/`helper`/`f` call node inlining necessarily
//! removes).

use super::anf_insert::anf_insert;
use super::checked_inline::checked_inline;
use crate::interpreter::eval_function;
use crate::types::{
    Expr, FnFact, FnId, IntWidth, Program, Provenance, SourceLanguage, SourcePosition, SourceSpan,
    Stmt,
};
use std::path::PathBuf;

/// A tiny deterministic PRNG (splitmix64) -- no external `rand` dependency
/// (this crate declares none beyond `syn`/`proc-macro2`, matching
/// `docs/compiler-ownership-contract.md`'s minimal-reuse stance), and fully
/// reproducible for a fixed seed, per issue #27's own A4 requirement
/// ("固定seedの生成テストで検証...再現可能に").
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Avoid the all-zero state, which would otherwise stay zero forever.
        Rng(seed ^ 0xD1B5_4A32_D192_ED03)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn range(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn bool_with_probability(&mut self, numerator: u64, denominator: u64) -> bool {
        self.next_u64() % denominator < numerator
    }

    /// A mix of the classic edge values this crate's other tests already
    /// use (0/±1/i32::MIN/MAX) and genuinely random `i32`s, matching A4's
    /// own "値には0/±1/i32 MIN/MAXを含める" requirement while still
    /// exercising the full range.
    fn i32_value(&mut self) -> i64 {
        match self.range(6) {
            0 => 0,
            1 => 1,
            2 => -1,
            3 => i32::MIN as i64,
            4 => i32::MAX as i64,
            _ => (self.next_u64() as i32) as i64,
        }
    }
}

fn prov() -> Provenance {
    Provenance {
        source_file: PathBuf::from("composition_fuzz"),
        span: SourceSpan {
            start: SourcePosition { line: 1, column: 1 },
            end: SourcePosition { line: 1, column: 1 },
        },
        language: SourceLanguage::Rust,
    }
}

fn fact(name: &str, params: usize, body: Stmt) -> FnFact {
    FnFact {
        name: name.to_string(),
        params: (0..params)
            .map(|i| (format!("p{i}"), IntWidth::I32))
            .collect(),
        return_width: IntWidth::I32,
        body,
        provenance: prov(),
    }
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call(FnId(name.to_string()), args, prov())
}

fn lit(v: i64) -> Expr {
    Expr::IntLit(v, IntWidth::I32, prov())
}

fn marked(inner: Expr) -> Expr {
    call("mark", vec![inner])
}

fn add(a: Expr, b: Expr) -> Expr {
    Expr::WrappingAdd(Box::new(a), Box::new(b), prov())
}

/// Generates a small expression tree over `param_count` parameters,
/// literals, `WrappingAdd`, and (with some probability) an embedded
/// `mark(..)` call directly in the body -- independent of any parameter,
/// exercising `checked_inline`'s callee-internal-call ordering check
/// (`transform::ObservedEvent`/`observed_event_order`). Parameter indices
/// are chosen uniformly at random, not in left-to-right order, so
/// unused/duplicated/reordered occurrences all arise naturally rather than
/// being hand-picked per case.
fn gen_expr(rng: &mut Rng, param_count: usize, depth: u32, next_mark_value: &mut i64) -> Expr {
    let leaf_or_recurse = depth == 0 || rng.bool_with_probability(1, 3);
    if leaf_or_recurse {
        match rng.range(3) {
            0 if param_count > 0 => Expr::Param(rng.range(param_count), prov()),
            1 => lit(rng.i32_value()),
            _ => {
                // An embedded, callee-internal effect -- not derived from
                // any parameter at all.
                let v = *next_mark_value;
                *next_mark_value += 1;
                marked(lit(v))
            }
        }
    } else {
        add(
            gen_expr(rng, param_count, depth - 1, next_mark_value),
            gen_expr(rng, param_count, depth - 1, next_mark_value),
        )
    }
}

/// One argument expression for a call site to `g`: either a plain literal
/// (no effect) or `mark(lit)` (an observable effect, distinguishable by
/// its own value from every other mark this generated program emits).
fn gen_call_arg(rng: &mut Rng, next_mark_value: &mut i64) -> Expr {
    if rng.bool_with_probability(1, 2) {
        lit(rng.i32_value())
    } else {
        let v = *next_mark_value;
        *next_mark_value += 1;
        marked(lit(v))
    }
}

fn observed_marks(effects: &[crate::interpreter::CallEvent]) -> Vec<i64> {
    effects
        .iter()
        .filter(|e| e.fn_name == "mark")
        .map(|e| e.args[0])
        .collect()
}

/// Builds one random program (`mark`, `helper`, `g`, `f`, `entry`) and
/// returns it alongside whether `g`'s body was pre-transformed (via
/// `anf_insert(helper, g)`) to already carry an embedded `Expr::Let` before
/// `f` ever grafts it -- the exact "callee body already transformed"
/// composition shape `add`/`g`/`f`'s own regression test names, generalized
/// and randomized instead of hand-written once.
fn build_random_program(rng: &mut Rng) -> (Program, bool) {
    let mut next_mark_value: i64 = 0;
    let mut program = Program::default();
    program.insert(fact(
        "mark",
        1,
        Stmt::Return(Expr::Param(0, prov()), prov()),
    ));
    program.insert(fact(
        "helper",
        2,
        Stmt::Return(add(Expr::Param(0, prov()), Expr::Param(1, prov())), prov()),
    ));

    let param_count_g = 1 + rng.range(3); // 1..=3
    let pre_transform_g = rng.bool_with_probability(1, 2);
    let g_raw_body = if pre_transform_g {
        // Wrap two independently generated sub-expressions through
        // `helper`, so `anf_insert(helper, g)` below leaves `g`'s body with
        // its own embedded `Let`s -- exactly the shape that requires the
        // alpha-rename this module's own `alpha_rename_for_one_graft`
        // performs before *every* graft, not merely once per operation.
        call(
            "helper",
            vec![
                gen_expr(rng, param_count_g, 1, &mut next_mark_value),
                gen_expr(rng, param_count_g, 1, &mut next_mark_value),
            ],
        )
    } else {
        gen_expr(rng, param_count_g, 2, &mut next_mark_value)
    };
    program.insert(fact("g", param_count_g, Stmt::Return(g_raw_body, prov())));
    if pre_transform_g {
        program = anf_insert(&program, "g", "helper")
            .expect("helper always has a single-Return, no-internal-call body");
    }

    let num_sites = 1 + rng.range(3); // 1..=3 call sites to g inside f
    let nest_sites = rng.bool_with_probability(1, 2);
    let mut site_exprs = Vec::with_capacity(num_sites);
    for _ in 0..num_sites {
        let args: Vec<Expr> = (0..param_count_g)
            .map(|_| gen_call_arg(rng, &mut next_mark_value))
            .collect();
        site_exprs.push(call("g", args));
    }
    let f_body_expr = if nest_sites && param_count_g > 0 {
        // Fold sites into each other via slot 0, so every site after the
        // first is nested *inside* the previous one's own call -- the
        // shape that actually stresses per-graft-site freshness (one
        // graft's own substituted body becomes part of what the next graft
        // embeds), as opposed to independent siblings.
        let mut acc = site_exprs.remove(0);
        for site in site_exprs {
            let Expr::Call(name, mut site_args, p) = site else {
                unreachable!()
            };
            site_args[0] = acc;
            acc = Expr::Call(name, site_args, p);
        }
        acc
    } else {
        // Independent sibling call sites, summed together.
        site_exprs.into_iter().reduce(add).unwrap_or_else(|| lit(0))
    };
    program.insert(fact("f", 0, Stmt::Return(f_body_expr, prov())));

    let entry_body = if rng.bool_with_probability(1, 2) {
        call("f", vec![])
    } else {
        add(call("f", vec![]), gen_call_arg(rng, &mut next_mark_value))
    };
    program.insert(fact("entry", 0, Stmt::Return(entry_body, prov())));

    (program, pre_transform_g)
}

#[test]
fn ir_level_composition_fuzz_preserves_value_and_observed_effects() {
    const SEEDS: u64 = 120;
    let mut checked_inline_accepted = 0usize;
    let mut checked_inline_rejected = 0usize;
    let mut nonempty_effect_traces = 0usize;

    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed);
        let (program, _pre_transformed) = build_random_program(&mut rng);

        let baseline = eval_function(&program, "entry", &[])
            .unwrap_or_else(|e| panic!("seed {seed}: baseline eval failed: {e:?}"));
        if !baseline.effects.is_empty() {
            nonempty_effect_traces += 1;
        }
        let baseline_marks = observed_marks(&baseline.effects);

        // anf_insert(g into f) must always succeed (g is always a single
        // Return-shaped body by construction) and must always preserve
        // value and observed effects exactly.
        let after_anf_f = anf_insert(&program, "f", "g")
            .unwrap_or_else(|e| panic!("seed {seed}: anf_insert(f, g) rejected: {e:?}"));
        let anf_outcome = eval_function(&after_anf_f, "entry", &[])
            .unwrap_or_else(|e| panic!("seed {seed}: eval after anf_insert(f, g) failed: {e:?}"));
        assert_eq!(
            anf_outcome.value, baseline.value,
            "seed {seed}: anf_insert(f, g) changed entry()'s value"
        );
        assert_eq!(
            observed_marks(&anf_outcome.effects),
            baseline_marks,
            "seed {seed}: anf_insert(f, g) changed the observed mark() sequence"
        );

        // A second composition level: inline the now-transformed `f` into
        // `entry` too (A4's own "entryへfをANFで" step) -- `f`'s body
        // after the first anf_insert is still a single Return (shape is
        // preserved through `apply_inlined_caller`), so this must succeed.
        let after_anf_entry = anf_insert(&after_anf_f, "entry", "f").unwrap_or_else(|e| {
            panic!("seed {seed}: anf_insert(entry, f) rejected after a prior anf_insert: {e:?}")
        });
        let anf_entry_outcome = eval_function(&after_anf_entry, "entry", &[]).unwrap_or_else(|e| {
            panic!("seed {seed}: eval after anf_insert(entry, f) failed: {e:?}")
        });
        assert_eq!(
            anf_entry_outcome.value, baseline.value,
            "seed {seed}: anf_insert(entry, f) on top of anf_insert(f, g) changed the value"
        );
        assert_eq!(
            observed_marks(&anf_entry_outcome.effects),
            baseline_marks,
            "seed {seed}: anf_insert(entry, f) on top of anf_insert(f, g) changed observed marks"
        );

        // checked_inline(g into f): either it matches the baseline exactly,
        // or it conservatively refuses -- never a silent wrong answer.
        match checked_inline(&program, "f", "g") {
            Ok(after_checked_f) => {
                checked_inline_accepted += 1;
                let checked_outcome =
                    eval_function(&after_checked_f, "entry", &[]).unwrap_or_else(|e| {
                        panic!("seed {seed}: eval after checked_inline(f, g) failed: {e:?}")
                    });
                assert_eq!(
                    checked_outcome.value, baseline.value,
                    "seed {seed}: checked_inline(f, g) accepted but changed the value"
                );
                assert_eq!(
                    observed_marks(&checked_outcome.effects),
                    baseline_marks,
                    "seed {seed}: checked_inline(f, g) accepted but changed observed marks"
                );

                // Compose a second level here too, mirroring A4's mixed
                // ANF-then-checked-then-ANF sequence. A rejection here is
                // an acceptable, conservative outcome -- only checked and
                // wrong is a failure.
                if let Ok(after_checked_entry) = checked_inline(&after_checked_f, "entry", "f") {
                    let outcome =
                        eval_function(&after_checked_entry, "entry", &[]).unwrap_or_else(|e| {
                            panic!("seed {seed}: eval after checked_inline(entry, f) failed: {e:?}")
                        });
                    assert_eq!(
                        outcome.value, baseline.value,
                        "seed {seed}: second-level checked_inline changed the value"
                    );
                    assert_eq!(
                        observed_marks(&outcome.effects),
                        baseline_marks,
                        "seed {seed}: second-level checked_inline changed observed marks"
                    );
                }
            }
            Err(_) => {
                checked_inline_rejected += 1;
            }
        }
    }

    // A coverage self-check: if every single seed took the same path, this
    // battery would not actually be exercising both the acceptance and the
    // rejection sides of `checked_inline`'s safety checks, nor generating
    // any effects to compare at all -- silently passing either way.
    assert!(
        checked_inline_accepted > 0,
        "no seed exercised a checked_inline acceptance path -- generator is not representative"
    );
    assert!(
        checked_inline_rejected > 0,
        "no seed exercised a checked_inline rejection path -- generator never produces an \
         unsafe substitution, so the safety checks this battery exists to stress are untested"
    );
    assert!(
        nonempty_effect_traces > SEEDS as usize / 4,
        "too few seeds produced any observed effect at all to meaningfully compare"
    );
}

/// The source-level shadowing dimension A4 names explicitly
/// ("source shadowing"): built through the real `nim_frontend` (not
/// hand-constructed IR, so `LocalId`s are exactly what a real caller
/// produces), with a random number of same-named-in-source `let` bindings
/// ahead of a call site, generalizing the existing hand-written
/// `anf_insert_does_not_capture_a_pre_existing_local_with_a_colliding_fresh_id`
/// regression test across many seeds/shapes instead of one.
#[test]
fn source_level_shadowing_fuzz_preserves_value() {
    const SEEDS: u64 = 40;
    for seed in 0..SEEDS {
        let mut rng = Rng::new(seed ^ 0xABCD_EF01);
        let num_lets = 1 + rng.range(5); // 1..=5 pre-existing caller locals
        let values: Vec<i64> = (0..num_lets).map(|_| 1 + rng.range(50) as i64).collect();

        let mut source = String::from("proc combine(x, y: int32): int32 =\n  x +% y\n\n");
        source.push_str("proc caller(): int32 =\n");
        for (i, v) in values.iter().enumerate() {
            source.push_str(&format!("  let v{i} = {v}'i32\n"));
        }
        // Reference two of the shadowing locals (in reversed order when
        // there are at least two) so the call's arguments read real
        // caller-side bindings, not just literals.
        let (arg_x, arg_y) = if num_lets >= 2 {
            (format!("v{}", num_lets - 1), "v0".to_string())
        } else {
            ("v0".to_string(), "v0".to_string())
        };
        source.push_str(&format!("  combine({arg_x}, {arg_y})\n"));

        let program = crate::nim_frontend::lower_nim_source(
            &PathBuf::from("shadow_fuzz.nim"),
            &source,
            &["combine", "caller"],
        )
        .unwrap_or_else(|e| panic!("seed {seed}: source failed to lower: {e:?}\n{source}"));

        let expected = if num_lets >= 2 {
            values[num_lets - 1] + values[0]
        } else {
            values[0] + values[0]
        };
        let expected = expected as i32 as i64; // match wrapping i32 semantics

        let baseline = eval_function(&program, "caller", &[]).unwrap();
        assert_eq!(
            baseline.value, expected,
            "seed {seed}: baseline itself is wrong\n{source}"
        );

        for (label, transformed) in [
            ("anf_insert", anf_insert(&program, "caller", "combine")),
            (
                "checked_inline",
                checked_inline(&program, "caller", "combine"),
            ),
        ] {
            let transformed = transformed
                .unwrap_or_else(|e| panic!("seed {seed}: {label} rejected combine: {e:?}"));
            let after = eval_function(&transformed, "caller", &[]).unwrap();
            assert_eq!(
                after.value, expected,
                "seed {seed}: {label} corrupted a shadowed local's value (num_lets={num_lets})\n{source}"
            );
        }
    }
}
