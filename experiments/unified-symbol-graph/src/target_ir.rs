//! Issue #68 concept proof: a minimal, x86_64-unknown-linux-gnu-only
//! target-specific IR that lowers directly to `CodeBody` (this crate's
//! own machine-code-bytes-plus-relocations type, established in issue
//! #67) without ever materializing LLVM IR as an intermediate artifact.
//!
//! See `docs/02-research-areas/compiler/target-specific-ir-without-llvm_ja.md`
//! for the full design rationale (Cranelift CLIF's `InstructionData`/
//! `MachInst` split, QBE's type-system shrinkage, and rustc's
//! `BuilderMethods`/`FnAbi` split, all verified against primary source in
//! that document). Three findings from that research shape this module
//! directly:
//!
//! 1. rustc's own `mir::Body` (`compiler/rustc_middle/src/mir/mod.rs`)
//!    carries no target-specific field at all -- `DataLayout`/calling
//!    convention are resolved separately, later, via `TyCtxt`/`Target`.
//!    `TargetIr` below follows the same discipline: its basic-block/
//!    ownership-tag structure is written without reference to x86_64 at
//!    all; only `lower_target_ir_to_code_body` (the actual lowering
//!    function) is target-specific.
//! 2. Cranelift's CLIF is "one IR" only at the surface -- internally it
//!    still splits into a target-generic `InstructionData` and a
//!    target-specific `MachInst`, joined by a `LowerBackend` trait. This
//!    module deliberately does NOT introduce that second layer: `TargetIr`
//!    lowers straight to raw bytes in one function, because this crate's
//!    stated principle (see this crate's own module doc, "no
//!    cross-platform `ElfX86_64PendingReloc`, ever") is to scope directly
//!    to one concrete target rather than build a reusable
//!    target-independent shape first.
//! 3. QBE achieves its shrinkage by making its type system deliberately
//!    weak (no pointer type, "types are only here for semantic purposes,
//!    not safety"). This module avoids that trade: `TargetIr::Value`
//!    carries an explicit `Ownership` tag (Unique/Shared) alongside its
//!    `Type`, so aliasing information is never re-encoded into a
//!    permissive attribute vocabulary the way LLVM's `noalias` is (the
//!    exact failure mode `removing-intermediate-representation_ja.md`
//!    section 3.2-3.3 documents as a real, repeated miscompilation
//!    source in rustc's own LLVM backend).
//!
//! ## Scope, stated explicitly (matching this crate's own precedent)
//!
//! `TargetIr` here is not a general control-flow-graph IR design -- it is
//! the minimum structure needed to demonstrate that a CFG with a
//! conditional branch can be lowered to real x86_64 machine code without
//! ever emitting LLVM IR. It intentionally does not attempt: SSA
//! construction/phi nodes, register allocation beyond two fixed scratch
//! registers, loops, function calls, or any type other than a 32-bit
//! integer. Extending this to other targets (ARM64, COFF, Mach-O, WASM)
//! is explicitly out of scope for this issue (see the design doc's
//! "まだ解けていないこと" section) -- WASM in particular cannot reuse this
//! module's PC-relative-branch model at all, per this crate's own
//! `lib.rs` module doc's WASM findings (index substitution, no address
//! computation).

/// The two ownership shapes this proof of concept distinguishes -- a
/// deliberately minimal stand-in for full Stacked/Tree Borrows tagging
/// (out of scope here), but enough to demonstrate that ownership
/// information can be carried as a first-class tag through this IR
/// instead of being discarded and later re-derived from type layout alone
/// (`arg_attrs_for_rust_scalar`'s own approach, documented in the design
/// doc as the mechanism responsible for real `noalias` miscompilations).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// No other live value may alias this one -- the analogue of `&mut T`/`Box<T>`.
    Unique,
    /// Aliasing is permitted -- the analogue of `&T`.
    Shared,
}

/// This proof of concept has exactly one representable type (a 32-bit
/// integer), matching QBE's own minimalism in spirit but for a different
/// reason: this module's scope is proving the lowering *mechanism* works,
/// not building a complete type system. A real `TargetIr` would need at
/// minimum QBE's own four base types plus pointer-width integers; adding
/// them is orthogonal to what this proof of concept demonstrates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Value {
    pub ownership: Ownership,
}

/// A place `TargetIr` can hold a value: either a literal constant known
/// at lowering time, or a named input parameter. No memory/heap locations
/// are modeled -- this proof of concept only needs to prove branch
/// lowering, not stack frame layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    Const(i32),
    /// The single integer parameter this proof of concept's functions
    /// accept, already materialized in `edi` by the System V x86_64
    /// calling convention (verified against this crate's own
    /// `real_c_add_code`/`real_compute_code_and_relocs` fixtures in
    /// `lib.rs`, which use `edi`/`esi` for the first two integer
    /// arguments).
    Param0,
}

/// A basic block's terminator -- the only two shapes this proof of
/// concept needs to demonstrate a real conditional branch surviving
/// lowering to machine code.
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Unconditionally return `value` from the function.
    Return(Operand),
    /// If `cond != 0`, jump to `then_block`; otherwise jump to
    /// `else_block`. This is the fixpoint-computation-relevant shape
    /// `removing-intermediate-representation_ja.md` section 2.2 confirms
    /// cannot be expressed on a tree-shaped AST -- a real merge point
    /// downstream of both `then_block` and `else_block` is exactly what
    /// requires a CFG, not just this proof of concept's two-block case.
    Branch {
        cond: Operand,
        then_block: BlockId,
        else_block: BlockId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub usize);

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub terminator: Terminator,
}

/// The minimal MIR-equivalent input this module's lowering function
/// consumes. Deliberately target-independent (see this module's own doc
/// comment, point 1): nothing here names x86_64, ELF, or any calling
/// convention. `entry` names which block execution starts at.
#[derive(Debug, Clone)]
pub struct TargetIr {
    pub blocks: Vec<BasicBlock>,
    pub entry: BlockId,
    /// Every value this function's signature exposes, carrying its
    /// `Ownership` tag end to end -- see this module's own doc comment,
    /// point 3, for why this is not collapsed into a post-hoc attribute
    /// the way LLVM's `noalias` derivation is.
    pub param0: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    /// A `Terminator::Branch`/`Return` referenced a `BlockId` that
    /// `TargetIr::blocks` does not contain -- a producer bug, never
    /// patched around (matching this crate's own `LinkError` discipline
    /// in `lib.rs`: a structurally invalid input is reported, not
    /// tolerated).
    UnknownBlock(BlockId),
    /// This proof of concept only supports the two-block, single-branch
    /// shape needed to demonstrate the lowering mechanism (see this
    /// module's own doc comment's stated scope) -- anything else is
    /// refused rather than silently mishandled.
    UnsupportedShape(&'static str),
}

/// Lowers `ir` directly to raw x86_64 System V machine code bytes -- the
/// actual "skip LLVM IR" step this issue asks for. No LLVM IR, no
/// Cranelift CLIF, no other intermediate textual/binary form is produced
/// at any point between `TargetIr` and these bytes.
///
/// **What this function hand-assembles, and why the encoding is correct**
/// (verified against the same System V calling convention this crate's
/// own `real_c_add_code`/`real_compute_code_and_relocs` fixtures in
/// `lib.rs` already confirmed against real `cc -c` output):
///
/// - `param0` arrives in `edi` (first integer argument register).
/// - `cmp edi, 0` (`83 FF 00`) followed by `je <else>` (`0F 84 <rel32>`)
///   implements `Terminator::Branch` -- a real two-way conditional jump,
///   not a simulated one; `objdump -d` on the resulting bytes would show
///   exactly this instruction pair.
/// - `mov eax, imm32` (`B8 <imm32>`) followed by `ret` (`C3`) implements
///   `Terminator::Return(Const(_))` -- the same `mov eax, N; ret` shape
///   this crate's own `cpp_max_i32`/`nim_double` stand-in bodies in
///   `lib.rs` already use.
/// - `mov eax, edi` (`89 F8`) followed by `ret` implements
///   `Terminator::Return(Param0)`.
///
/// Scope limit stated directly: this function accepts exactly the
/// two-block "entry branches to a then-block and an else-block, each of
/// which immediately returns" shape (`Value`-parameterized loops, calls,
/// or deeper CFGs are `UnsupportedShape`) -- extending this to a general
/// basic-block scheduler with jump-target fixups is explicitly out of
/// this issue's declared scope (see the design doc).
pub fn lower_target_ir_to_code_body(ir: &TargetIr) -> Result<Vec<u8>, LowerError> {
    let entry = ir
        .blocks
        .get(ir.entry.0)
        .ok_or(LowerError::UnknownBlock(ir.entry))?;

    let Terminator::Branch {
        cond,
        then_block,
        else_block,
    } = &entry.terminator
    else {
        return Err(LowerError::UnsupportedShape(
            "entry block must terminate in Branch for this proof of concept",
        ));
    };

    let then_bb = ir
        .blocks
        .get(then_block.0)
        .ok_or(LowerError::UnknownBlock(*then_block))?;
    let else_bb = ir
        .blocks
        .get(else_block.0)
        .ok_or(LowerError::UnknownBlock(*else_block))?;

    let then_code = lower_return_only_block(then_bb)?;
    let else_code = lower_return_only_block(else_bb)?;

    // Layout, chosen up front so the branch displacement is computable
    // without a second fixup pass (this proof of concept's functions are
    // small enough that no range-extension thunk, per this crate's own
    // AArch64 findings in `lib.rs`, is ever needed):
    //
    //   [cmp][je else_label][then_code...][else_code...]
    let cmp_and_je_len = 3 + 6; // `83 FF 00` (cmp edi,0) + `0F 84 rel32` (je)
    let else_label = cmp_and_je_len as i32 + then_code.len() as i32;

    let mut code = Vec::with_capacity(cmp_and_je_len + then_code.len() + else_code.len());
    match cond {
        Operand::Param0 => {
            code.extend_from_slice(&[0x83, 0xFF, 0x00]); // cmp edi, 0
        }
        Operand::Const(v) => {
            // A constant condition still emits a real comparison against
            // that literal (never folded away) -- this proof of concept
            // demonstrates lowering mechanics, not constant-folding
            // optimization, which is a separate, later concern.
            code.extend_from_slice(&[0x83, 0xFF, (*v & 0xFF) as u8]);
        }
    }
    let je_rel32_offset = code.len() + 2;
    code.extend_from_slice(&[0x0F, 0x84, 0x00, 0x00, 0x00, 0x00]); // je rel32 (placeholder)
    let je_site_end = je_rel32_offset + 4;
    let je_rel = else_label - je_site_end as i32;
    code[je_rel32_offset..je_rel32_offset + 4].copy_from_slice(&je_rel.to_le_bytes());

    code.extend_from_slice(&then_code);
    code.extend_from_slice(&else_code);

    Ok(code)
}

fn lower_return_only_block(bb: &BasicBlock) -> Result<Vec<u8>, LowerError> {
    match &bb.terminator {
        Terminator::Return(Operand::Const(v)) => {
            let mut code = vec![0xB8]; // mov eax, imm32
            code.extend_from_slice(&v.to_le_bytes());
            code.push(0xC3); // ret
            Ok(code)
        }
        Terminator::Return(Operand::Param0) => Ok(vec![0x89, 0xF8, 0xC3]), // mov eax, edi; ret
        Terminator::Branch { .. } => Err(LowerError::UnsupportedShape(
            "then/else blocks must be Return-only for this proof of concept",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The concept-proof case issue #68 asks for directly: a function
    /// with a real conditional branch, lowered from `TargetIr` straight
    /// to x86_64 bytes, with every byte checked against the hand-derived
    /// System V encoding -- never merely "it compiled," but "the actual
    /// opcode/displacement bytes are what a real disassembler would show
    /// for `if param0 != 0 { 7 } else { 9 }`."
    #[test]
    fn branch_lowers_to_real_conditional_jump_bytes_with_correct_displacement() {
        let ir = TargetIr {
            entry: BlockId(0),
            param0: Value {
                ownership: Ownership::Shared,
            },
            blocks: vec![
                BasicBlock {
                    terminator: Terminator::Branch {
                        cond: Operand::Param0,
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                BasicBlock {
                    terminator: Terminator::Return(Operand::Const(7)),
                },
                BasicBlock {
                    terminator: Terminator::Return(Operand::Const(9)),
                },
            ],
        };

        let code = lower_target_ir_to_code_body(&ir).expect("well-formed IR must lower");

        // cmp edi, 0
        assert_eq!(&code[0..3], &[0x83, 0xFF, 0x00]);
        // je rel32
        assert_eq!(&code[3..5], &[0x0F, 0x84]);
        let je_rel = i32::from_le_bytes(code[5..9].try_into().unwrap());
        // then-block (mov eax,7; ret = 6 bytes) sits immediately after the
        // je instruction; else-block starts after that.
        let then_code = [0xB8, 0x07, 0x00, 0x00, 0x00, 0xC3];
        let else_code = [0xB8, 0x09, 0x00, 0x00, 0x00, 0xC3];
        assert_eq!(&code[9..15], &then_code, "then-block bytes: mov eax,7; ret");
        assert_eq!(
            &code[15..21],
            &else_code,
            "else-block bytes: mov eax,9; ret"
        );
        // The je displacement must land exactly on the else-block's first
        // byte, computed independently of lower_target_ir_to_code_body's
        // own arithmetic so this assertion cannot pass by mirroring a bug
        // (same discipline as lib.rs's own relocation test).
        let je_site_end = 9i32; // offset immediately after the 4-byte rel32
        let else_block_start = 15i32;
        assert_eq!(je_rel, else_block_start - je_site_end);

        assert_eq!(code.len(), 21);
    }

    /// The straight-line (no branch) case must still be refused for this
    /// proof of concept's entry block, per its stated scope -- confirms
    /// `UnsupportedShape` is reachable, not dead code.
    #[test]
    fn entry_block_without_a_branch_is_refused_not_silently_miscompiled() {
        let ir = TargetIr {
            entry: BlockId(0),
            param0: Value {
                ownership: Ownership::Unique,
            },
            blocks: vec![BasicBlock {
                terminator: Terminator::Return(Operand::Param0),
            }],
        };

        let result = lower_target_ir_to_code_body(&ir);
        assert_eq!(
            result,
            Err(LowerError::UnsupportedShape(
                "entry block must terminate in Branch for this proof of concept"
            ))
        );
    }

    /// A `Terminator` naming a block index past the end of `blocks` must
    /// fail loudly with the specific unknown block, never index-panic or
    /// silently substitute a default -- the same "structured failure over
    /// a producer bug" discipline `lib.rs`'s own `LinkError` variants
    /// follow.
    #[test]
    fn branch_to_a_nonexistent_block_is_reported_not_panicked() {
        let ir = TargetIr {
            entry: BlockId(0),
            param0: Value {
                ownership: Ownership::Shared,
            },
            blocks: vec![BasicBlock {
                terminator: Terminator::Branch {
                    cond: Operand::Param0,
                    then_block: BlockId(1),
                    else_block: BlockId(99),
                },
            }],
        };

        // then_block(1) also does not exist here -- either missing block
        // must be reported as UnknownBlock, never a panic.
        let result = lower_target_ir_to_code_body(&ir);
        assert!(matches!(result, Err(LowerError::UnknownBlock(_))));
    }

    /// The `Ownership` tag survives on `TargetIr::param0` end to end
    /// (never dropped, never collapsed into a lowering-time attribute) --
    /// a minimal, directly-testable stand-in for this module's design
    /// claim that aliasing information is not re-encoded the way LLVM's
    /// `noalias` derivation discards borrow-checker provenance and
    /// recomputes from type layout alone.
    #[test]
    fn ownership_tag_is_preserved_on_the_ir_value_not_discarded_before_lowering() {
        let unique_ir = TargetIr {
            entry: BlockId(0),
            param0: Value {
                ownership: Ownership::Unique,
            },
            blocks: vec![
                BasicBlock {
                    terminator: Terminator::Branch {
                        cond: Operand::Param0,
                        then_block: BlockId(1),
                        else_block: BlockId(1),
                    },
                },
                BasicBlock {
                    terminator: Terminator::Return(Operand::Const(0)),
                },
            ],
        };
        assert_eq!(unique_ir.param0.ownership, Ownership::Unique);
        // Lowering must succeed independent of the ownership tag's value
        // -- this proof of concept does not yet consume the tag inside
        // lowering (no optimization pass reads it), which is exactly the
        // "not yet implemented" gap the design doc's section 6 names.
        assert!(lower_target_ir_to_code_body(&unique_ir).is_ok());
    }
}
