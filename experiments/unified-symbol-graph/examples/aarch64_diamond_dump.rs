//! Issue #68, H3 experiment: dumps the AArch64 machine code
//! `diamond_and_loop_cfg::lower_cfg_body_to_aarch64_code_body` produces
//! for both real CFG shapes the x86_64 lowering already handles
//! correctly and verifies at runtime
//! (`diamond_and_loop_cfg::tests`/`examples/diamond_and_loop_check.rs`):
//! the diamond CFG (a merge point downstream of two branch arms) and the
//! countdown loop CFG (a real back-edge). Written as raw binary files for
//! independent QEMU user-mode execution inside the H3 container (this
//! crate's own AArch64-hosting mmap-execution approach, used for
//! x86_64, cannot run AArch64 code on this x86_64 host).
//!
//! Notably, `lower_cfg_body_to_aarch64_code_body` required **no code
//! change at all** to accept the countdown CFG below: its block-layout
//! and branch-fixup mechanism is already `CfgBody`-generic (a back-edge
//! is just a `Goto`/`Branch` fixup whose target's `block_starts` entry
//! happens to sit earlier than the branch site), exactly mirroring the
//! x86_64 lowering's own behavior. That shared genericity -- not
//! sharing a single line of *emission* code between architectures -- is
//! the actual candidate evidence for H3's "does a target-generic
//! `CfgBody` avoid Cranelift's two-layer split" question; see this
//! module's own AArch64 emission functions for the architecture-specific
//! arithmetic that does NOT unify (word-scaled vs byte-scaled branch
//! immediates, instruction-start vs instruction-end offset origins).
//!
//! Run: `cargo run --release --example aarch64_diamond_dump -- <diamond-out> <countdown-out>`

use unified_symbol_graph::target_ir::diamond_and_loop_cfg::{
    lower_cfg_body_to_aarch64_code_body, BasicBlock, BlockId, CfgBody, LocalId, Rvalue, Statement,
    Terminator,
};

fn build_diamond_cfg() -> CfgBody {
    let param0 = LocalId(0);
    let x = LocalId(1);
    let cond = LocalId(2);
    let merged_copy = LocalId(3);
    let one = LocalId(4);

    CfgBody {
        num_locals: 5,
        entry: BlockId(0),
        blocks: vec![
            BasicBlock {
                statements: vec![
                    Statement {
                        assign_to: one,
                        rvalue: Rvalue::Const(1),
                    },
                    Statement {
                        assign_to: cond,
                        rvalue: Rvalue::NotEqualZero(param0),
                    },
                ],
                terminator: Terminator::Branch {
                    cond,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            BasicBlock {
                statements: vec![Statement {
                    assign_to: x,
                    rvalue: Rvalue::Add(param0, one),
                }],
                terminator: Terminator::Goto(BlockId(3)),
            },
            BasicBlock {
                statements: vec![Statement {
                    assign_to: x,
                    rvalue: Rvalue::Sub(param0, one),
                }],
                terminator: Terminator::Goto(BlockId(3)),
            },
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

/// Same CFG shape as `examples/diamond_and_loop_check.rs`'s own
/// `build_countdown_cfg`, kept in sync with it: `pub fn countdown(param0:
/// i32) -> i32 { let mut n = param0; let mut acc = 0; while n > 0 { acc
/// += n; n -= 1; } acc }`, with the real back-edge `BlockId(3) ->
/// BlockId(1)`.
fn build_countdown_cfg() -> CfgBody {
    let param0 = LocalId(0);
    let n = LocalId(1);
    let acc = LocalId(2);
    let cond = LocalId(3);
    let const_one = LocalId(4);

    CfgBody {
        num_locals: 5,
        entry: BlockId(0),
        blocks: vec![
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
                    Statement {
                        assign_to: const_one,
                        rvalue: Rvalue::Const(1),
                    },
                ],
                terminator: Terminator::Goto(BlockId(1)),
            },
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
            BasicBlock {
                statements: vec![Statement {
                    assign_to: acc,
                    rvalue: Rvalue::Add(acc, n),
                }],
                terminator: Terminator::Goto(BlockId(3)),
            },
            BasicBlock {
                statements: vec![Statement {
                    assign_to: n,
                    rvalue: Rvalue::Sub(n, const_one),
                }],
                terminator: Terminator::Goto(BlockId(1)), // the real back-edge
            },
            BasicBlock {
                statements: vec![],
                terminator: Terminator::Return(acc),
            },
        ],
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (diamond_path, countdown_path) = match args.as_slice() {
        [_, d, c] => (d.clone(), c.clone()),
        _ => panic!("usage: aarch64_diamond_dump <diamond-out-path> <countdown-out-path>"),
    };

    let diamond_cfg = build_diamond_cfg();
    let diamond_code =
        lower_cfg_body_to_aarch64_code_body(&diamond_cfg).expect("diamond CfgBody must lower");
    eprintln!(
        "[aarch64_diamond_dump] diamond: {} bytes: {:02x?}",
        diamond_code.len(),
        diamond_code
    );
    std::fs::write(&diamond_path, &diamond_code).expect("write diamond output file");
    eprintln!("[aarch64_diamond_dump] wrote {diamond_path}");

    let countdown_cfg = build_countdown_cfg();
    let countdown_code =
        lower_cfg_body_to_aarch64_code_body(&countdown_cfg).expect("countdown CfgBody must lower");
    eprintln!(
        "[aarch64_diamond_dump] countdown: {} bytes: {:02x?}",
        countdown_code.len(),
        countdown_code
    );
    std::fs::write(&countdown_path, &countdown_code).expect("write countdown output file");
    eprintln!("[aarch64_diamond_dump] wrote {countdown_path}");
}
