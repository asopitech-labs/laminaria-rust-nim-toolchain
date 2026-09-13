/* Issue #48 (G1) fixture: a real, incompatible `cadd` variant -- a
 * genuine "library variant that cannot provide the required symbol"
 * negative case (one of the work instruction's own named examples),
 * not a fabricated missing-symbol claim: this variant really renamed
 * its exported entry point to `c_add_v2`, so it genuinely does not
 * export `c_add` at all. */
int c_add_v2(int a, int b) {
    return a + b;
}
