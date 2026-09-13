/* Issue #48 (G1) fixture: the production-consumed export declaration
 * for the incompatible `cadd` variant. This header genuinely declares
 * `c_add_v2`, not `c_add` -- `crates/laminaria-ir/src/c_header_discover.rs`
 * reading this file is what makes the negative case a real declared
 * mismatch, not a fabricated one. */
#ifndef CADD_V2_H
#define CADD_V2_H

int c_add_v2(int a, int b);

#endif
