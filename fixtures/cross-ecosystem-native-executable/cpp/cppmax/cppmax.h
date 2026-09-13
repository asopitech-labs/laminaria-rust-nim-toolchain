// Issue #48 (G1) fixture: the production-consumed export declaration
// for the C++ `cppmax` package's `extern "C"` adapter. The template
// `max_value<T>` itself has no linkable symbol and is deliberately not
// declared here -- only the real adapter/instantiation unit's own
// exported entry point is. `crates/laminaria-ir/src/c_header_discover.rs`
// reads this header's own prototype, never a hand-maintained expected
// symbol list.
#ifndef CPPMAX_H
#define CPPMAX_H

extern "C" {
int cpp_max_i32(int a, int b);
}

#endif
