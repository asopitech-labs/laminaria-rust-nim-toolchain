//! `unused-pkg-28` -- M8-many-unrequested-cargo (large scale, issue
//! #28 D1-b2). Never depended on by `fixture-bin`; part of a real
//! internal dependency chain among the unused packages (depends on
//! `unused-pkg-29`), to prove Cargo excludes a whole unreachable
//! connected subgraph, not just isolated leaves.
pub fn value() -> u32 {
    unused_pkg_29::value() + 1
}
