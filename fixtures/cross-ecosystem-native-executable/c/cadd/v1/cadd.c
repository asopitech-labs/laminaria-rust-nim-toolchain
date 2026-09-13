/* Issue #48 (G1) fixture: the correct `cadd` provider. Exports exactly
 * the symbol `app/src/main.rs`'s `extern "C" { fn c_add(...) }`
 * declaration requires. Includes its own header so the header's
 * declaration and this implementation cannot silently drift apart. */
#include "cadd.h"

int c_add(int a, int b) {
    return a + b;
}
