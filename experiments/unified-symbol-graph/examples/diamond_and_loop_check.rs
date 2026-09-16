//! Issue #68, fourth-round critical review: proves
//! `target_ir::diamond_and_loop_cfg::lower_cfg_body_to_code_body`
//! correctly executes both a real diamond CFG (a merge point downstream
//! of two branch arms, where the merged value is read afterward) and a
//! real loop (a back-edge), mmap-executing the generated x86_64 bytes
//! and checking results against the equivalent real Rust functions for a
//! range of inputs.
//!
//! Run: `cargo run --release --example diamond_and_loop_check`

use unified_symbol_graph::target_ir::diamond_and_loop_cfg::{
    lower_cfg_body_to_code_body, BasicBlock, BlockId, CfgBody, LocalId, Rvalue, Statement,
    Terminator,
};

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

/// Real Rust equivalent of the diamond CfgBody below, for cross-checking
/// expected values (not part of the generated code path itself).
fn diamond_reference(param0: i32) -> i32 {
    let x = if param0 != 0 {
        param0.wrapping_add(1)
    } else {
        param0.wrapping_sub(1)
    };
    x.wrapping_mul(2)
}

/// `pub fn diamond(param0: i32) -> i32 { let x = if param0 != 0 { param0
/// + 1 } else { param0 - 1 }; x * 2 }` -- the real MIR shape captured
/// this session (`diamond.mir`), with overflow-check assertions omitted
/// (out of this submodule's stated scope) but the merge structure
/// (`_2` written on both arms, read at the merge block) preserved
/// exactly: `_1`=LocalId(0) (param0), `_2`=LocalId(1) (x), `_3`=LocalId(2)
/// (condition), `_6`=LocalId(3) (copy of x at the merge block, matching
/// the real MIR's own `_6 = copy _2` before the multiply).
fn build_diamond_cfg() -> CfgBody {
    let param0 = LocalId(0);
    let x = LocalId(1);
    let cond = LocalId(2);
    let merged_copy = LocalId(3);

    CfgBody {
        num_locals: 4,
        entry: BlockId(0),
        blocks: vec![
            // bb0: cond = (param0 != 0); branch
            BasicBlock {
                statements: vec![Statement {
                    assign_to: cond,
                    rvalue: Rvalue::NotEqualZero(param0),
                }],
                terminator: Terminator::Branch {
                    cond,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            // bb1 (then arm): x = param0 + 1; goto merge
            BasicBlock {
                statements: vec![Statement {
                    assign_to: x,
                    rvalue: Rvalue::Add(param0, param0), // placeholder overwritten below via constant local trick
                }],
                terminator: Terminator::Goto(BlockId(3)),
            },
            // bb2 (else arm): x = param0 - 1; goto merge
            BasicBlock {
                statements: vec![Statement {
                    assign_to: x,
                    rvalue: Rvalue::Sub(param0, param0), // placeholder, see below
                }],
                terminator: Terminator::Goto(BlockId(3)),
            },
            // bb3 (merge): merged_copy = copy x; return merged_copy * 2
            // (Rvalue has no Mul, so this block computes merged_copy = x + x
            // -- doubling via addition, matching `x * 2` exactly for i32
            // without needing a new Rvalue variant.)
            BasicBlock {
                statements: vec![
                    Statement {
                        assign_to: merged_copy,
                        rvalue: Rvalue::Copy(x),
                    },
                    Statement {
                        assign_to: merged_copy,
                        rvalue: Rvalue::Add(merged_copy, merged_copy),
                    },
                ],
                terminator: Terminator::Return(merged_copy),
            },
        ],
    }
}

/// Real Rust equivalent of the loop CfgBody below.
fn countdown_reference(param0: i32) -> i32 {
    let mut n = param0;
    let mut acc = 0i32;
    while n > 0 {
        acc = acc.wrapping_add(n);
        n = n.wrapping_sub(1);
    }
    acc
}

/// `pub fn countdown(param0: i32) -> i32 { let mut n = param0; let mut
/// acc = 0; while n > 0 { acc += n; n -= 1; } acc }` -- the real MIR
/// shape captured this session (`countdown.mir`), overflow checks
/// omitted, back-edge (`bb4 -> bb1`, this submodule's `BlockId(1) ->
/// BlockId(0)` since block 0 here is the loop header) preserved exactly.
/// `_1`=LocalId(0) (param0), `_2`=LocalId(1) (n), `_3`=LocalId(2) (acc),
/// `_4`=LocalId(3) (condition).
fn build_countdown_cfg() -> CfgBody {
    let param0 = LocalId(0);
    let n = LocalId(1);
    let acc = LocalId(2);
    let cond = LocalId(3);

    CfgBody {
        num_locals: 4,
        entry: BlockId(0),
        blocks: vec![
            // bb0 (loop header): cond = (n > 0); branch to body or exit.
            // n/acc are initialized by the prologue-equivalent first
            // pass through this same header (n = param0 on first entry,
            // via the Copy statement below), matching real MIR's own
            // bb0 (init) immediately falling into bb1 (header) via goto
            // -- collapsed here into one block since this submodule's
            // entry must be block 0 (see lower_cfg_body_to_code_body's
            // own doc comment) and the real MIR's bb0->bb1 is a single
            // straight-line goto with no branch of its own.
            BasicBlock {
                statements: vec![
                    Statement {
                        assign_to: n,
                        rvalue: Rvalue::Copy(param0),
                    },
                    Statement {
                        assign_to: acc,
                        rvalue: Rvalue::Const(0),
                    },
                ],
                terminator: Terminator::Goto(BlockId(1)),
            },
            // bb1 (real loop header): cond = n > 0; branch
            BasicBlock {
                statements: vec![Statement {
                    assign_to: cond,
                    rvalue: Rvalue::GreaterThanZero(n),
                }],
                terminator: Terminator::Branch {
                    cond,
                    then_block: BlockId(2),
                    else_block: BlockId(4),
                },
            },
            // bb2 (loop body, part 1): acc = acc + n
            BasicBlock {
                statements: vec![Statement {
                    assign_to: acc,
                    rvalue: Rvalue::Add(acc, n),
                }],
                terminator: Terminator::Goto(BlockId(3)),
            },
            // bb3 (loop body, part 2): n = n - 1 ; goto header (BACK-EDGE)
            BasicBlock {
                statements: vec![Statement {
                    assign_to: n,
                    rvalue: Rvalue::Sub(n, LocalId(4)), // n - 1, using a synthesized constant-1 local (see below)
                }],
                terminator: Terminator::Goto(BlockId(1)), // <-- the real back-edge
            },
            // bb4 (loop exit): return acc
            BasicBlock {
                statements: vec![],
                terminator: Terminator::Return(acc),
            },
        ],
    }
}

fn main() {
    // --- Diamond CFG ---
    // Rvalue has no immediate-subtract-by-1/add-by-1 form (only
    // local-vs-local Add/Sub), so build_diamond_cfg's own Add/Sub
    // placeholders above are corrected here to the real +1/-1 semantics
    // by using a dedicated constant-1 local, matching real MIR's own
    // `const 1_i32` operand exactly (rustc's MIR freely mixes Copy-local
    // and Const operands in one BinOp; this submodule's own Rvalue,
    // being local-only per its own stated minimalism, models the
    // constant as a separate assigned local instead).
    let mut diamond_cfg = build_diamond_cfg();
    let one = LocalId(4);
    diamond_cfg.num_locals = 5;
    diamond_cfg.blocks[0].statements.insert(
        0,
        Statement {
            assign_to: one,
            rvalue: Rvalue::Const(1),
        },
    );
    diamond_cfg.blocks[1].statements[0].rvalue = Rvalue::Add(LocalId(0), one);
    diamond_cfg.blocks[2].statements[0].rvalue = Rvalue::Sub(LocalId(0), one);

    let diamond_code =
        lower_cfg_body_to_code_body(&diamond_cfg).expect("diamond CfgBody must lower");
    eprintln!(
        "[diamond_and_loop_check] diamond code: {} bytes: {:02x?}",
        diamond_code.len(),
        diamond_code
    );
    let diamond_fn = unsafe { make_callable(&diamond_code) };

    let mut failures = 0;
    for input in [0, 1, -1, 5, -5, 100, -100] {
        let actual = diamond_fn(input);
        let expected = diamond_reference(input);
        let ok = actual == expected;
        println!(
            "diamond({input}) = {actual}, expected {expected} {}",
            if ok { "OK" } else { "MISMATCH" }
        );
        if !ok {
            failures += 1;
        }
    }

    // --- Loop CFG ---
    let mut countdown_cfg = build_countdown_cfg();
    let const_one = LocalId(4);
    countdown_cfg.num_locals = 5;
    countdown_cfg.blocks[0].statements.push(Statement {
        assign_to: const_one,
        rvalue: Rvalue::Const(1),
    });

    let countdown_code =
        lower_cfg_body_to_code_body(&countdown_cfg).expect("countdown CfgBody must lower");
    eprintln!(
        "[diamond_and_loop_check] countdown code: {} bytes: {:02x?}",
        countdown_code.len(),
        countdown_code
    );
    let countdown_fn = unsafe { make_callable(&countdown_code) };

    for input in [0, 1, 5, 10, 100] {
        let actual = countdown_fn(input);
        let expected = countdown_reference(input);
        let ok = actual == expected;
        println!(
            "countdown({input}) = {actual}, expected {expected} {}",
            if ok { "OK" } else { "MISMATCH" }
        );
        if !ok {
            failures += 1;
        }
    }

    if failures > 0 {
        eprintln!("[diamond_and_loop_check] {failures} MISMATCH(ES)");
        std::process::exit(1);
    }
    eprintln!(
        "[diamond_and_loop_check] both a real diamond CFG (merge point downstream of two branch \
         arms, reading the value either arm wrote) and a real loop (back-edge revisiting an \
         earlier block) executed correctly for all tested inputs -- the structural gap the \
         fourth-round critical review identified (H4: CFG fixpoint-computation model) is now \
         modeled and verified end to end, not merely asserted as a design principle"
    );
}
