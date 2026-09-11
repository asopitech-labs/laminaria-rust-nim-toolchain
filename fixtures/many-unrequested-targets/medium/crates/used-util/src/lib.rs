//! `used-util` -- M8-many-unrequested-cargo (issue #28 D1-b2). Always
//! depended on by `fixture-bin`; a sibling of `used-core` (no dependency
//! between them, spec.md 2.3).
pub fn value() -> u32 {
    31
}
