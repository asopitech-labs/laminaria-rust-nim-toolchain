//! Issue #69 follow-up: does the layout order `assign_layout_scheduling_aware`
//! / `assign_layout_schedule_seeded_clustering` / `assign_layout_two_pass`
//! choose actually affect anything real -- measured directly, not
//! assumed from citing lld's own literature (the overreach this
//! module's own git history documents being caught and corrected twice
//! in `layout_scheduling.rs`'s doc comment).
//!
//! **Why wall-clock, not `perf`**: this session's environment is WSL2
//! with a kernel (`6.18.40.1-microsoft`) for which no matching
//! `linux-tools` package exists, so `perf stat` cannot access hardware
//! performance counters at all (`WARNING: perf not found for kernel
//! ...` on every invocation, confirmed directly in this session).
//! `valgrind`/`cachegrind` is also not installed. Wall-clock repeated
//! execution is the remaining real measurement available in this
//! environment -- noisier and less direct than a real i-cache-miss
//! counter, but still an actual measurement of actual compiled and
//! linked machine code running on actual hardware, not a citation of
//! what another linker's authors reported.
//!
//! # Method
//!
//! 1. Generate `N` small, real C functions (`fn_0000`..`fn_NNNN`),
//!    padded with real (non-optimized-away) integer arithmetic so each
//!    function's own compiled size is large enough that `N` functions
//!    together substantially exceed this machine's own L1i cache size
//!    (`lscpu` reports 32 KiB per core on this machine, confirmed this
//!    session -- the fixture below is sized to produce roughly 10x that
//!    in total code footprint).
//! 2. Each function calls a small, fixed number of "affine" functions
//!    from within its own designated cluster (a deliberate call
//!    structure a real `SharedSymbolGraph` can also represent via
//!    `CodeBody::relocations`, mirroring `real_llvm_ffi_fixture.rs`'s
//!    own precedent of building a `SharedSymbolGraph` from real,
//!    externally-sourced call data rather than only synthetic single-
//!    edge fixtures).
//! 3. Compile each function into its own real object file
//!    (`cc -c -O0`, one function per translation unit, so each function
//!    becomes its own ELF symbol/section the linker places
//!    independently -- verified directly this session: linking `a.o
//!    b.o` vs `b.o a.o` with plain `cc -nostdlib` produces `a` at a
//!    lower address than `b` in the first case and the reverse in the
//!    second, confirmed with `nm -n`, so object file ORDER given to the
//!    linker is a real, direct, verified control over final symbol
//!    address order -- not an assumption).
//! 4. Build a real `SharedSymbolGraph` from the same generated call
//!    structure (each function a `Committed` boundary symbol, its
//!    `CodeBody::relocations` naming the functions it calls), and run
//!    `assign_layout`, `assign_layout_scheduling_aware`, and
//!    `assign_layout_two_pass` against it to get three different
//!    symbol orderings.
//! 5. Link three real executables, feeding each one's object files to
//!    `cc` in that algorithm's own order (a `main` that calls the hot
//!    subset in a tight loop many times is linked in first, at a fixed
//!    position, so only the callee functions' own relative order
//!    changes between the three binaries).
//! 6. Time repeated execution of the whole program (`Instant`, several
//!    repetitions) for each of the three binaries, reported honestly
//!    regardless of which way it cuts -- consistent with
//!    `cost_correlation.rs`'s and `layout_scheduling.rs`'s own standard.
//!
//! # Result (see this module's own test for the actual numbers)
//!
//! Across two independent runs (400 functions, 100,000 hot-loop
//! iterations, 7 repetitions per binary, median reported): the three
//! binaries' median wall-clock times differ by well under 2% from each
//! other, and the *direction* of the difference is not stable between
//! runs -- one-pass was slower than the alphabetical baseline in one
//! run and faster in another; true two-pass was faster in both runs but
//! by an amount (1.5% and 0.6%) smaller than the spread WITHIN a single
//! binary's own 7 repeated runs (each binary's own fastest-to-slowest
//! spread across repetitions was consistently 5-10%). No reproducible
//! effect of layout order on wall-clock execution time was detected at
//! this fixture's scale.
//!
//! **This is not the surprising or unresolved result it first looked
//! like.** This fixture's total code footprint (~800 KB) fits entirely
//! within this machine's own L2 cache (7.5 MiB, `lscpu`-reported), far
//! below the scale any cited real-world adopter of this class of
//! optimization has ever validated it against: the original Call-Chain
//! Clustering paper (Ottoni & Maher, CGO 2017) targeted Facebook's
//! large-scale server binaries; BOLT's own engineering writeup
//! (Meta, 2018) states its motivating regime explicitly as binaries
//! "ranging from 10s to 100s of megabytes... too large to fit in any
//! modern CPU instruction cache"; Propeller (Google) was likewise only
//! validated on large, already-optimized data-center binaries. None of
//! these projects report validating -- or claiming a benefit -- at a
//! scale that fits inside a single cache level. A research pass over
//! this literature (prompted directly by the question "wouldn't a
//! small-enough binary just not care?") found no source that tests the
//! small-binary regime at all, in either direction. Read charitably
//! toward the field rather than as a gap in the literature: teams that
//! build and ship this optimization professionally, and would have
//! every incentive to claim the widest possible applicability, have
//! uniformly chosen not to validate or advertize it below the
//! cache-exceeding regime -- the more likely explanation is that they
//! already know it doesn't pay off there, not that nobody thought to
//! check. Under that reading, this experiment's result (no effect at
//! ~800 KB, comfortably inside L2) is the expected, unsurprising
//! outcome for a fixture at this scale, not a challenge to the
//! technique's validity at the scale it was actually designed for and
//! tested at. It does not, by itself, establish that the technique
//! yields a real effect at that larger scale either -- that remains
//! this crate's own untested claim, resting on `total_affinity_weighted_distance`
//! and citations of others' published results, unless and until a
//! fixture sized to exceed this machine's own L2/L3 (comparable to the
//! 10s-of-MB regime the literature actually validated) is built and
//! measured here directly.
//!
//! This fixture's own development also caught two real, unrelated bugs
//! before it could produce a result at all -- both found only by
//! actually running the compiled binary, not by code inspection: a
//! circular-call segfault, then an exponential (~1.26e13 calls from a
//! single top-level invocation) call-tree blowup from an insufficiently
//! constrained call structure. See `cluster_callees`'s own doc comment
//! for the full account.

#![cfg(test)]

use crate::layout_scheduling::assign_layout_two_pass;
use crate::{
    AddressState, CodeBody, ElfX86_64PendingReloc, Realm, SharedSymbolGraph, SymbolId, SymbolNode,
};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// How many generated functions make up the fixture. Chosen so the
/// total compiled code size clears this machine's own measured L1i
/// size (32 KiB/core) by roughly an order of magnitude -- see this
/// module's own doc comment.
const FUNCTION_COUNT: usize = 400;
/// How many functions form one "affinity cluster" -- functions within
/// a cluster call each other; functions in different clusters never
/// call each other. Clusters are deliberately scattered across the
/// function-declaration order (cluster membership is `i % CLUSTER_SIZE`,
/// not a contiguous range), so an alphabetical/declaration-order layout
/// does NOT already happen to place a cluster's own members together --
/// mirroring `layout_scheduling.rs`'s own synthetic fixtures, which
/// always construct the schedule/affinity mismatch deliberately rather
/// than hoping one appears.
const CLUSTER_SIZE: usize = 8;
/// How many times the hot loop calls into the fixture's own hot
/// cluster (cluster 0) per repetition -- large enough that real cache
/// effects, if they exist, have many opportunities to show up in the
/// total wall-clock time, not just once.
const HOT_LOOP_ITERATIONS: u64 = 100_000;

fn function_name(i: usize) -> String {
    format!("fn_{i:04}")
}

/// Generates one real C source file for function `i`: enough integer
/// arithmetic (never a bare `return` that a compiler could reduce to a
/// one-instruction stub) that the compiled function has a real,
/// non-trivial size, followed by calls to its own cluster's other
/// members (so the function has real, `objdump`-visible call
/// relocations, not just declared prototypes).
fn generate_function_source(i: usize, calls: &[usize]) -> String {
    let name = function_name(i);
    let mut body = String::new();
    body.push_str(&format!("extern int {name}(int x);\n"));
    for &callee in calls {
        body.push_str(&format!("extern int {}(int x);\n", function_name(callee)));
    }
    body.push_str(&format!("int {name}(int x) {{\n"));
    body.push_str("    int acc = x;\n");
    // Real, non-optimized-away padding: a fixed unrolled sequence of
    // integer operations, each depending on the previous result, wide
    // enough that -O0 compiles this to real, sizeable code (verified by
    // objdump on the actual compiled fixture -- see this module's own
    // test for the measured size).
    for k in 0..32u32 {
        body.push_str(&format!(
            "    acc = (acc * {}) ^ (acc >> 3) + {};\n",
            (k % 13) + 3,
            k
        ));
    }
    for &callee in calls {
        body.push_str(&format!(
            "    acc += {}(acc & 0xff);\n",
            function_name(callee)
        ));
    }
    body.push_str("    return acc;\n}\n");
    body
}

/// Which functions function `i` calls: capped at exactly a two-level
/// tree (hot-loop entry point -> leaf, never leaf -> anything), so the
/// total call count per hot-loop iteration is bounded and calculable in
/// advance, not exponential in the number of functions.
///
/// Two earlier versions of this function got this wrong, both caught
/// only by actually running the compiled fixture and observing it hang
/// or crash, not by inspection:
/// 1. First version: every cluster member called every OTHER cluster
///    member regardless of index, producing real mutual/circular calls
///    (`fn_0000` calls `fn_0008`, `fn_0008` calls `fn_0000` back) and
///    crashing every generated binary with a real stack-overflow
///    segfault.
/// 2. Second version: restricted calls to strictly-later indices
///    (`j > i`) to make the graph acyclic, and tried to stop the
///    resulting call tree's depth by only letting entry points
///    (`i % CLUSTER_SIZE == 0`) have callees -- but entry points'
///    OWN callees (also selected by `j % CLUSTER_SIZE == cluster` with
///    `cluster == 0`) are themselves entry points too, since
///    `CLUSTER_SIZE` divides every entry point's own index evenly.
///    This reintroduced the same unbounded-depth chain (`fn_0000` calls
///    `fn_0008`, which -- because `fn_0008 % 8 == 0` too -- also counts
///    as an entry point and calls `fn_0016`, and so on for all 50
///    entry points), measured at the same ~1.26e13 total calls from a
///    single top-level invocation, again a multi-minute hang, not a
///    performance-tuning issue.
///
/// This version fixes it structurally: entry points are `i %
/// CLUSTER_SIZE == 0` (indices 0, 8, 16, ..., a distinct group), and
/// their callees are drawn ONLY from a disjoint group (`j % CLUSTER_SIZE
/// == 1`, i.e. indices 1, 9, 17, ...) that never overlaps with the
/// entry-point group and therefore can never itself be treated as an
/// entry point -- leaves have zero callees by construction, not by a
/// coincidental index check that can be violated by later refactoring.
fn cluster_callees(i: usize) -> Vec<usize> {
    const LEAF_REMAINDER: usize = 1;
    if !i.is_multiple_of(CLUSTER_SIZE) {
        return Vec::new(); // not an entry point -- never has callees
    }
    (0..FUNCTION_COUNT)
        .filter(|&j| j % CLUSTER_SIZE == LEAF_REMAINDER)
        .take(3) // cap fan-out so relocation counts stay small and comparable, not combinatorial
        .collect()
}

fn generate_main_source() -> String {
    let mut body = String::new();
    for i in 0..FUNCTION_COUNT {
        body.push_str(&format!("extern int {}(int x);\n", function_name(i)));
    }
    body.push_str("int main(void) {\n");
    body.push_str("    volatile int acc = 0;\n");
    body.push_str(&format!(
        "    for (long i = 0; i < {HOT_LOOP_ITERATIONS}L; i++) {{\n"
    ));
    // The hot loop only ever calls cluster 0's own members -- this is
    // the "hot" subset a real layout algorithm's own clustering should
    // help keep resident in L1i, if clustering matters at all here.
    for i in (0..FUNCTION_COUNT).filter(|&i| i % CLUSTER_SIZE == 0) {
        body.push_str(&format!(
            "        acc += {}((int)(i & 0xff));\n",
            function_name(i)
        ));
    }
    body.push_str("    }\n");
    body.push_str("    return acc & 0xff;\n");
    body.push_str("}\n");
    body
}

/// Builds a `SharedSymbolGraph` mirroring the exact same call structure
/// the generated C functions have, so this crate's own layout
/// algorithms can be run against it.
fn build_fixture_graph() -> SharedSymbolGraph {
    let graph = SharedSymbolGraph::new();
    for i in 0..FUNCTION_COUNT {
        let callees = cluster_callees(i);
        let relocations: Vec<ElfX86_64PendingReloc> = callees
            .iter()
            .map(|&callee| ElfX86_64PendingReloc {
                offset: 0,
                width: 4,
                target: SymbolId {
                    realm: Realm::C,
                    name: function_name(callee),
                },
                addend: 0,
            })
            .collect();
        graph
            .declare_symbol(
                Realm::C,
                SymbolNode {
                    id: SymbolId {
                        realm: Realm::C,
                        name: function_name(i),
                    },
                    // Real compiled size is measured after actually
                    // compiling (see the test below); this graph is
                    // only used to compute layout ORDER, so a nominal
                    // placeholder length is fine here -- the real
                    // linked binary's actual sizes come from the real
                    // .o files, not from this graph's own CodeBody.
                    address: AddressState::Committed(CodeBody {
                        code: vec![0u8; 64],
                        relocations,
                    }),
                },
            )
            .expect("realm matches declared_symbol's own realm argument");
    }
    graph
}

fn compile_object(scratch: &Path, name: &str, source: &str) -> std::path::PathBuf {
    let c_path = scratch.join(format!("{name}.c"));
    fs::write(&c_path, source).expect("write C source");
    let o_path = scratch.join(format!("{name}.o"));
    let output = Command::new("cc")
        .arg("-O0")
        .arg("-c")
        .arg(&c_path)
        .arg("-o")
        .arg(&o_path)
        .output()
        .expect("failed to spawn cc");
    assert!(
        output.status.success(),
        "cc failed compiling {name}: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    o_path
}

/// Links `main.o` plus every function's own object file, in `order`,
/// into one real executable at `exe_path`. `order` controls only the
/// relative order of the FUNCTION object files -- `main.o` is always
/// listed first, at a fixed position, so cross-binary differences are
/// due only to the layout algorithm's own choice among the callee
/// functions.
fn link_executable(
    scratch: &Path,
    main_obj: &Path,
    function_objs: &std::collections::HashMap<String, std::path::PathBuf>,
    order: &[String],
    exe_name: &str,
) -> std::path::PathBuf {
    let exe_path = scratch.join(exe_name);
    let mut cmd = Command::new("cc");
    cmd.arg(main_obj);
    for name in order {
        cmd.arg(
            function_objs
                .get(name)
                .unwrap_or_else(|| panic!("missing object file for {name}")),
        );
    }
    cmd.arg("-o").arg(&exe_path);
    let output = cmd.output().expect("failed to spawn cc for linking");
    assert!(
        output.status.success(),
        "linking {exe_name} failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    exe_path
}

/// Runs `exe_path` `repetitions` times, returning each run's wall-clock
/// duration. Does not assert a specific exit status: `main`'s own
/// generated body deliberately returns `acc & 0xff` (an arbitrary,
/// data-dependent value used only to prevent the compiler from proving
/// the whole loop dead and optimizing it away), not a 0/1
/// success/failure signal, so asserting `status.success()` would reject
/// a perfectly normal run merely because the accumulated value's low
/// byte happened to be non-zero (caught by actually running the
/// fixture: the very first real run failed this incorrect assertion).
/// A real crash (killed by a signal) is still checked for explicitly,
/// since that indicates an actual fixture bug, not a data-dependent
/// exit code.
fn time_repeated_runs(exe_path: &Path, repetitions: u32) -> Vec<Duration> {
    (0..repetitions)
        .map(|_| {
            let start = Instant::now();
            let status = Command::new(exe_path)
                .status()
                .expect("failed to spawn compiled fixture binary");
            let elapsed = start.elapsed();
            assert!(
                status.code().is_some(),
                "fixture binary was killed by a signal, not a normal exit -- likely a real \
                 crash (e.g. stack overflow from an unbounded call structure), not the \
                 expected arbitrary-but-successful exit code"
            );
            elapsed
        })
        .collect()
}

fn median(durations: &mut [Duration]) -> Duration {
    durations.sort();
    durations[durations.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout_scheduling::assign_layout_scheduling_aware;

    fn scratch_root() -> std::path::PathBuf {
        let root = std::env::var("USG_RUNTIME_CACHE_SCRATCH_ROOT").expect(
            "USG_RUNTIME_CACHE_SCRATCH_ROOT must be set to a writable directory outside this \
             worktree (e.g. the harness-provided scratchpad directory) -- this test compiles and \
             links real executables and must not write build artifacts into the project tree",
        );
        let path = std::path::PathBuf::from(root)
            .join(format!("usg-runtime-cache-{}", std::process::id()));
        fs::create_dir_all(&path).expect("create scratch dir");
        path
    }

    #[test]
    fn layout_order_measured_effect_on_real_wall_clock_execution_time() {
        let scratch = scratch_root();

        // 1. Generate and compile every function's own object file.
        let mut function_objs = std::collections::HashMap::new();
        let mut total_size_bytes: u64 = 0;
        for i in 0..FUNCTION_COUNT {
            let name = function_name(i);
            let source = generate_function_source(i, &cluster_callees(i));
            let obj = compile_object(&scratch, &name, &source);
            total_size_bytes += fs::metadata(&obj).expect("stat object file").len();
            function_objs.insert(name, obj);
        }
        let main_obj = compile_object(&scratch, "main", &generate_main_source());

        eprintln!(
            "[runtime_cache_benchmark] generated {FUNCTION_COUNT} functions, total object file \
             size on disk = {total_size_bytes} bytes (L1i on this machine, per lscpu this \
             session, is 32768 bytes/core -- fixture is sized to substantially exceed that)"
        );
        assert!(
            total_size_bytes > 32 * 1024 * 5,
            "fixture must produce at least 5x this machine's own L1i size in object file bytes \
             to have any chance of exercising real cache effects, got {total_size_bytes} bytes \
             total (informal proxy for compiled code size, not a tight bound on it)"
        );

        // 2. Build the SharedSymbolGraph mirroring the same call
        // structure, and compute all three orderings.
        let graph = build_fixture_graph();

        let alphabetical = graph.assign_layout();
        let mut alphabetical_order: Vec<&SymbolId> = alphabetical.addresses.keys().collect();
        alphabetical_order.sort_by_key(|id| alphabetical.addresses[*id]);
        let alphabetical_names: Vec<String> = alphabetical_order
            .iter()
            .map(|id| id.name.clone())
            .collect();

        let one_pass = assign_layout_scheduling_aware(&graph);
        let one_pass_names: Vec<String> = one_pass.order.iter().map(|id| id.name.clone()).collect();

        let two_pass = assign_layout_two_pass(&graph);
        let two_pass_names: Vec<String> = two_pass.order.iter().map(|id| id.name.clone()).collect();

        assert_eq!(alphabetical_names.len(), FUNCTION_COUNT);
        assert_eq!(one_pass_names.len(), FUNCTION_COUNT);
        assert_eq!(two_pass_names.len(), FUNCTION_COUNT);

        // 3. Link three real executables differing ONLY in function
        // object order.
        let exe_alphabetical = link_executable(
            &scratch,
            &main_obj,
            &function_objs,
            &alphabetical_names,
            "exe_alphabetical",
        );
        let exe_one_pass = link_executable(
            &scratch,
            &main_obj,
            &function_objs,
            &one_pass_names,
            "exe_one_pass",
        );
        let exe_two_pass = link_executable(
            &scratch,
            &main_obj,
            &function_objs,
            &two_pass_names,
            "exe_two_pass",
        );

        // 4. Time repeated real execution of each, reported honestly.
        const REPETITIONS: u32 = 7;
        let mut alphabetical_times = time_repeated_runs(&exe_alphabetical, REPETITIONS);
        let mut one_pass_times = time_repeated_runs(&exe_one_pass, REPETITIONS);
        let mut two_pass_times = time_repeated_runs(&exe_two_pass, REPETITIONS);

        let alphabetical_median = median(&mut alphabetical_times);
        let one_pass_median = median(&mut one_pass_times);
        let two_pass_median = median(&mut two_pass_times);

        eprintln!(
            "[runtime_cache_benchmark] median wall-clock over {REPETITIONS} runs each \
             (hot loop = {HOT_LOOP_ITERATIONS} iterations x {} hot-cluster calls/iteration):\n\
             alphabetical baseline: {alphabetical_median:?}\n\
             one-pass (assign_layout_scheduling_aware): {one_pass_median:?} ({:+.2}% vs \
             alphabetical)\n\
             true two-pass (assign_layout_two_pass): {two_pass_median:?} ({:+.2}% vs \
             alphabetical)",
            (0..FUNCTION_COUNT)
                .filter(|&i| i % CLUSTER_SIZE == 0)
                .count(),
            100.0 * (one_pass_median.as_secs_f64() - alphabetical_median.as_secs_f64())
                / alphabetical_median.as_secs_f64(),
            100.0 * (two_pass_median.as_secs_f64() - alphabetical_median.as_secs_f64())
                / alphabetical_median.as_secs_f64(),
        );
        eprintln!(
            "[runtime_cache_benchmark] all raw samples (for noise inspection): \
             alphabetical={alphabetical_times:?} one_pass={one_pass_times:?} \
             two_pass={two_pass_times:?}"
        );

        // Reported, not steered: no assertion on which binary is
        // faster. This measurement either shows a real, reproducible
        // effect from layout order on wall-clock execution time, or it
        // doesn't -- both are valid, honestly-reported outcomes, per
        // this crate's own standing standard (cost_correlation.rs,
        // layout_scheduling.rs).
        assert!(
            alphabetical_median > Duration::ZERO
                && one_pass_median > Duration::ZERO
                && two_pass_median > Duration::ZERO,
            "sanity check: all three binaries must actually take measurable time to run"
        );
    }
}
