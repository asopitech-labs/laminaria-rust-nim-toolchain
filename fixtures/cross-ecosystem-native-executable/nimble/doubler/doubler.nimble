# Issue #48 (G1) fixture: a real Nimble package consumed by the `app`
# Cargo crate's `use_nim_double` feature, introspected via `nimble dump
# --json` (a read-only manifest command, not a build) by
# `crates/laminaria-run/src/cross_ecosystem_ingest.rs`.
version       = "0.1.0"
author        = "LAMINARIA"
description   = "Issue #48 fixture: doubles an i32 via a real Nimble package."
license       = "MIT"
srcDir        = "src"

requires "nim >= 2.0.0"
