/* Issue #48 (G1) fixture: the correct `cadd` provider. Exports exactly
 * the symbol `app/src/main.rs`'s `extern "C" { fn c_add(...) }`
 * declaration requires. */
int c_add(int a, int b) {
    return a + b;
}
