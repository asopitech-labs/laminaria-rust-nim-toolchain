// Issue #48 (G1) fixture: a small C++ library requiring an explicit
// adapter/instantiation unit. `max_value<T>` is a template -- no
// directly linkable `cpp_max_i32` symbol exists until an explicit
// instantiation is compiled. The `extern "C" cpp_max_i32` function
// below is exactly that adapter (per
// `docs/02-research-areas/compiler/nim-c-cpp-library-integration_ja.md`'s
// own "C++にはoverload resolution...templateが加わる...明示的なC++
// adapter/instantiation unitを生成" requirement). Includes its own
// header so the header's declaration and this implementation cannot
// silently drift apart.
#include "cppmax.h"

template <typename T>
static T max_value(T a, T b) {
    return a > b ? a : b;
}

extern "C" int cpp_max_i32(int a, int b) {
    return max_value<int>(a, b);
}
