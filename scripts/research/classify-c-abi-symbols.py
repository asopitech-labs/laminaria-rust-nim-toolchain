#!/usr/bin/env python3
"""Issue #63 P0 (pull-driven rewrite): classify alopex-cli's non-mangled (bare C ABI) linked
symbols by known prefix conventions of the C libraries alopexDB's Cargo.lock actually vendors
(aws-lc-sys, zstd-sys, libsqlite3-sys, lz4-sys, ring, blake3). This is the pull-side half of the
comparison: which crate each symbol *in the produced binary* actually came from, established
before any push-side signal (Cargo.toml `links` etc.) is consulted.
"""
import collections
import re
import sys

PATTERNS = [
    # AWS-LC (aws-lc-sys): classic algorithm namespaces plus the post-quantum additions
    # (ML-KEM/ML-DSA/SLH-DSA/Kyber, FIPS-indicator plumbing) and low-level bignum/elliptic-curve
    # internals (bn_/ec_/ecp_/sk/aead/kP/two/poly/rand/dh/dsa/md/sig/decode/init/get/parse/thread
    # etc.) that only appear as link-time symbols inside this one vendored C library.
    ("aws-lc-sys", re.compile(
        r"^(AWSLC_|AWS_LC_|EVP_|SHA1_|SHA224_|SHA256_|SHA384_|SHA512_|SHA3_|FIPS202_|FIPS_|"
        r"MD5_|AES_|RSA_|EC_|ECDSA_|ECDH_|BN_|X509|ASN1_|HMAC_|CRYPTO_|ERR_(?!zstd)|RAND_|"
        r"OPENSSL_|bn_|aws_lc_|CBS_|CBB_|GCM_|ChaCha20_|Poly1305_|HKDF_|HRSS_|ML_KEM|ML_DSA|"
        r"SLH_DSA|Kyber|kyber_|bcm_|mldsa|mld_|mlk_|mlkem_|pkey_|jent_|constant_time_|ec_|"
        r"ecp_nistz|ecp_|p256_|p384_|p521_|edwards|Ed25519|ed25519|x25519|X25519|curve25519|"
        r"aead_|kem_|dh_|dsa_|sig_|pqdsa_|boringssl_|ksecp|shake_?128|shake_?256|poly1305_|"
        r"secp[0-9]+k1_)",
        re.IGNORECASE,
    )),
    ("rustc-runtime (personality/unwind, not a crate)", re.compile(r"^(DW\.ref\.|__rust_|rust_eh_personality|_Unwind_)")),
    # Per-function/per-TU compiler- or linker-generated table symbols, unrelated to which
    # crate the enclosing function came from -- by far the largest bucket (82.6% of all
    # non-mangled symbols in this run). Counted separately rather than folded into "other" so
    # the residual "other" size is honest.
    #   GCC_except_table<N>  -- one per function, LLVM's exception-handling landing-pad table
    #   CSWTCH.<N>            -- switch-statement jump table constant
    #   .L<...>               -- local (non-exported) assembler labels
    #   TM__<hash>_<N>        -- LLVM CFI (control-flow-integrity) type-metadata symbols
    #   __PRETTY_FUNCTION__.0 -- C `assert`/diagnostics function-name string literal, emitted
    #                            once per translation unit across every C library linked in
    ("compiler-generated (per-function/per-TU table, not crate-attributable)", re.compile(
        r"^(GCC_except_table|CSWTCH\.|\.L|TM__|__PRETTY_FUNCTION__)"
    )),
    ("zstd-sys", re.compile(r"^(ZSTD|ZDICT_|HUF|FSE(v[0-9]+)?_|ZSTDMT_|COVER_|BIT(v[0-9]+)?_|POOL_|XXH|MEM_|NTIv|ZBUFFv)")),
    ("libsqlite3-sys", re.compile(r"^sqlite3")),
    ("lz4-sys", re.compile(r"^LZ4")),
    ("ring", re.compile(r"^(GFp_|ring_core_)")),
    ("blake3", re.compile(r"^(blake3_|_?blake3)")),
    # nim-sql-parser (issue #40/#63): Nim's C backend mangles module-qualified names as
    # `<Ident>__<module><hash>`; alopexDB vendors this as a prebuilt static/shared library
    # consumed via FFI, not through Cargo at all -- its symbols exist in the binary but have
    # no Cargo.lock package entry, so no push-side signal (this script's whole point) could
    # ever name it.
    ("nim-sql-parser (FFI, not a Cargo package)", re.compile(r".*__[A-Za-z][A-Za-z0-9]*_u[0-9]+$")),
]


def main() -> int:
    in_path = sys.argv[1]
    out_path = sys.argv[2]
    counts = collections.Counter()
    sample_unmatched = []
    with open(in_path) as f:
        for line in f:
            name = line.rstrip("\n")
            if not name:
                continue
            matched = False
            for label, pat in PATTERNS:
                if pat.match(name):
                    counts[label] += 1
                    matched = True
                    break
            if not matched:
                counts["<other-c-abi-or-rustc-runtime>"] += 1
                if len(sample_unmatched) < 30:
                    sample_unmatched.append(name)

    with open(out_path, "w") as f:
        for label, n in counts.most_common():
            f.write(f"{label}\t{n}\n")

    print(f"wrote {out_path}", file=sys.stderr)
    for label, n in counts.most_common():
        print(f"{label}\t{n}")
    print("--- sample unmatched (<other-c-abi-or-rustc-runtime>) ---")
    for s in sample_unmatched:
        print(s)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
