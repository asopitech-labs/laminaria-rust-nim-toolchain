/* Issue #48 (G1) fixture: the production-consumed export declaration
 * for the correct `cadd` provider. `crates/laminaria-ir/src/c_header_discover.rs`
 * reads this header's own prototype -- never a hand-maintained expected
 * symbol list -- to learn that this candidate declares `c_add`. */
#ifndef CADD_V1_H
#define CADD_V1_H

int c_add(int a, int b);

#endif
