//! Issue #68, task #5: the controlled re-run of the LLVM "energy"
//! (runtime-execution efficiency) comparison the design doc's own
//! section 5.11.2 pre-registered. Two conditions this crate's earlier
//! comparison (`examples/target_ir_energy_bench.rs`) did NOT control
//! for are fixed here:
//!
//! 1. **opt-level=0 on both sides.** The earlier comparison built the
//!    LLVM reference function under this crate's default `--release`
//!    profile (opt-level=3), under which LLVM eliminates the branch
//!    entirely (`sete`+`lea`, no `cmp`/`je` -- confirmed via objdump in
//!    that file's own comments). `lower_target_ir_to_code_body` itself
//!    performs no optimization passes at all, so that comparison was
//!    "optimized LLVM vs. unoptimized direct lowering," not "LLVM's
//!    codegen vs. this crate's own codegen for the identical
//!    unoptimized instruction selection problem." Run this file via
//!    `cargo run --profile opt0 --example energy_comparison_controlled`
//!    (see this crate's own `Cargo.toml` `[profile.opt0]`) so the LLVM
//!    side is also opt-level=0 -- independently confirmed via objdump
//!    below to still contain a real `cmp`/`jne` pair, not a
//!    branch-eliminated form.
//! 2. **Identical call mechanism on both sides.** The earlier
//!    comparison called `target_ir_fn` through an `extern "C" fn`
//!    pointer (mmap'd code) but called `llvm_branch` as a direct,
//!    statically-linked call -- a direct call is link-time-resolvable
//!    and can be inlined or have its call overhead optimized away by
//!    LLVM even with `#[inline(never)]` limiting only actual inlining,
//!    not necessarily call-site overhead in every case. Both sides here
//!    go through the exact same `extern "C" fn(i32) -> i32` pointer
//!    type and the exact same measurement loop shape.
//! 3. **N=30 independent trials**, each its own process-level timing
//!    loop, reporting min/median/mean/max, not a single run -- the
//!    design doc's own pre-registered "支持条件" ("N≥30試行").
//!
//! Run: `cargo run --profile opt0 --example energy_comparison_controlled -- <iters-per-trial> <trials>`

use std::hint::black_box;
use unified_symbol_graph::target_ir::{lower_target_ir_to_code_body, mir_text};

const PROT_READ: i32 = 0x1;
const PROT_WRITE: i32 = 0x2;
const PROT_EXEC: i32 = 0x4;
const MAP_PRIVATE: i32 = 0x02;
const MAP_ANONYMOUS: i32 = 0x20;
const MAP_FAILED: *mut std::ffi::c_void = usize::MAX as *mut std::ffi::c_void;

extern "C" {
    fn mmap(
        addr: *mut std::ffi::c_void,
        len: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut std::ffi::c_void;
}

const REAL_NE_BRANCH_MIR: &str = "\
fn branch(_1: i32) -> i32 {
    debug param0 => _1;
    let mut _0: i32;
    let mut _2: bool;

    bb0: {
        _2 = Ne(copy _1, const 0_i32);
        switchInt(move _2) -> [0: bb2, otherwise: bb1];
    }

    bb1: {
        _0 = const 7_i32;
        goto -> bb3;
    }

    bb2: {
        _0 = const 9_i32;
        goto -> bb3;
    }

    bb3: {
        return;
    }
}
";

/// The identical function LLVM compiles for comparison. Under
/// `--profile opt0` (opt-level=0), this retains a real `cmp`/`jne`
/// pair -- independently confirmed via objdump against a standalone
/// `rustc -C opt-level=0` compile of this exact function body during
/// this experiment, unlike the opt-level=3 branch-eliminated form the
/// earlier (uncontrolled) comparison used.
#[inline(never)]
extern "C" fn llvm_branch(param0: i32) -> i32 {
    if param0 != 0 {
        7
    } else {
        9
    }
}

unsafe fn make_callable(code: &[u8]) -> extern "C" fn(i32) -> i32 {
    let page_size = 4096;
    let len = code.len().div_ceil(page_size) * page_size;
    let ptr = mmap(
        std::ptr::null_mut(),
        len,
        PROT_READ | PROT_WRITE | PROT_EXEC,
        MAP_PRIVATE | MAP_ANONYMOUS,
        -1,
        0,
    );
    assert_ne!(ptr, MAP_FAILED, "mmap failed");
    std::ptr::copy_nonoverlapping(code.as_ptr(), ptr as *mut u8, code.len());
    std::mem::transmute::<*mut std::ffi::c_void, extern "C" fn(i32) -> i32>(ptr)
}

/// One trial: runs `iters` calls through `f` and returns elapsed time.
/// `f` is always called through the same `extern "C" fn(i32) -> i32`
/// pointer type for both the target_ir side and the LLVM side, closing
/// the "identical call mechanism" gap the earlier comparison had.
fn time_trial(f: extern "C" fn(i32) -> i32, iters: i64) -> std::time::Duration {
    let start = std::time::Instant::now();
    let mut acc: i64 = 0;
    for i in 0..iters {
        acc = acc.wrapping_add(f(black_box((i % 2) as i32)) as i64);
    }
    black_box(acc);
    start.elapsed()
}

fn summarize(label: &str, mut samples: Vec<f64>) {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = samples.len();
    let min = samples[0];
    let max = samples[n - 1];
    let median = if n.is_multiple_of(2) {
        (samples[n / 2 - 1] + samples[n / 2]) / 2.0
    } else {
        samples[n / 2]
    };
    let mean = samples.iter().sum::<f64>() / n as f64;
    let variance = samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    let stddev = variance.sqrt();
    println!(
        "{label}: n={n} min={min:.6}s median={median:.6}s mean={mean:.6}s max={max:.6}s stddev={stddev:.6}s"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let iters_per_trial: i64 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000_000);
    let trials: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);
    assert!(
        trials >= 30,
        "design doc's own pre-registered success condition requires N>=30 trials, got {trials}"
    );

    if cfg!(debug_assertions) {
        eprintln!(
            "[energy_comparison_controlled] WARNING: this looks like a debug build, not \
             `--profile opt0`. Run with `cargo run --profile opt0 --example \
             energy_comparison_controlled` for a meaningful opt-level=0-vs-opt-level=0 result."
        );
    }

    let ir = mir_text::parse_mir_text(REAL_NE_BRANCH_MIR).expect("real rustc MIR must parse");
    let code = lower_target_ir_to_code_body(&ir).expect("parsed IR must lower");
    eprintln!(
        "[energy_comparison_controlled] target_ir-generated machine code: {} bytes: {:02x?}",
        code.len(),
        code
    );
    let target_ir_fn = unsafe { make_callable(&code) };

    for v in [0, 1, -1, 42] {
        assert_eq!(
            target_ir_fn(v),
            llvm_branch(v),
            "target_ir-generated code and LLVM-compiled code must agree on branch({v})"
        );
    }
    eprintln!(
        "[energy_comparison_controlled] correctness check passed: both implementations agree on \
         all sampled inputs"
    );

    // Warm-up trial for each side (not recorded), to let any first-call
    // page-fault/branch-predictor-cold-state cost land outside the
    // measured samples for both sides equally.
    time_trial(target_ir_fn, iters_per_trial.min(1_000_000));
    time_trial(llvm_branch, iters_per_trial.min(1_000_000));

    let mut target_ir_samples = Vec::with_capacity(trials);
    let mut llvm_samples = Vec::with_capacity(trials);
    // Interleaved trial order (target_ir, llvm, target_ir, llvm, ...)
    // rather than all-target_ir-then-all-llvm, so a monotonic drift in
    // system conditions (thermal throttling, other host load) does not
    // asymmetrically bias one side.
    for _ in 0..trials {
        target_ir_samples.push(time_trial(target_ir_fn, iters_per_trial).as_secs_f64());
        llvm_samples.push(time_trial(llvm_branch, iters_per_trial).as_secs_f64());
    }

    summarize(
        "target_ir (direct lowering, no optimizer)",
        target_ir_samples.clone(),
    );
    summarize(
        "llvm_branch (rustc, opt-level per build profile)",
        llvm_samples.clone(),
    );

    let target_ir_samples_sorted = {
        let mut s = target_ir_samples;
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s
    };
    let llvm_samples_sorted = {
        let mut s = llvm_samples;
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s
    };
    let median = |s: &[f64]| {
        let n = s.len();
        if n.is_multiple_of(2) {
            (s[n / 2 - 1] + s[n / 2]) / 2.0
        } else {
            s[n / 2]
        }
    };
    let target_ir_median = median(&target_ir_samples_sorted);
    let llvm_median = median(&llvm_samples_sorted);
    println!(
        "ratio of medians (target_ir / llvm): {:.4}x",
        target_ir_median / llvm_median
    );

    // A/A control: an independent reviewer of this experiment pointed
    // out that a ratio "near 1.0x" is meaningless without first
    // measuring this harness's own noise floor -- two byte-identical
    // functions, timed through the exact same measurement loop, should
    // report a ratio of 1.0x if the harness can resolve anything
    // smaller than its own measurement noise. Re-uses `target_ir_fn`
    // as both "sides" (same underlying mmap'd bytes, called through
    // the same function-pointer mechanism) rather than compiling a
    // second identical function, since the goal is to measure this
    // specific harness's noise, not a second codegen path's.
    eprintln!(
        "[energy_comparison_controlled] running A/A control (same code on both sides) to \
         measure this harness's own noise floor..."
    );
    let mut aa_samples_a = Vec::with_capacity(trials);
    let mut aa_samples_b = Vec::with_capacity(trials);
    for _ in 0..trials {
        aa_samples_a.push(time_trial(target_ir_fn, iters_per_trial).as_secs_f64());
        aa_samples_b.push(time_trial(target_ir_fn, iters_per_trial).as_secs_f64());
    }
    summarize("A/A control, side A (target_ir_fn)", aa_samples_a.clone());
    summarize("A/A control, side B (target_ir_fn)", aa_samples_b.clone());
    let aa_a_sorted = {
        let mut s = aa_samples_a;
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s
    };
    let aa_b_sorted = {
        let mut s = aa_samples_b;
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s
    };
    let aa_ratio = median(&aa_a_sorted) / median(&aa_b_sorted);
    println!("A/A control ratio of medians (should be ~1.0x if harness resolves real signal): {aa_ratio:.4}x");
    let real_ratio = target_ir_median / llvm_median;
    let deviation_from_one = (real_ratio - 1.0).abs();
    let aa_deviation_from_one = (aa_ratio - 1.0).abs();
    if aa_deviation_from_one >= deviation_from_one {
        eprintln!(
            "[energy_comparison_controlled] WARNING: the A/A control's own deviation from 1.0x \
             ({aa_deviation_from_one:.4}) is >= the target_ir-vs-llvm ratio's deviation from 1.0x \
             ({deviation_from_one:.4}). This harness cannot distinguish the measured ratio from \
             pure noise -- do NOT report the target_ir/llvm ratio above as evidence of equivalent \
             performance without also reporting this A/A result alongside it."
        );
    }
}
