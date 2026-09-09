## First smoke test for issue #4's genuinely-open question: can Nim
## reach a native artifact through a route that never generates C at
## all? `nim c` (every other experiment in this directory) always goes
## through a C-generation step before a C compiler produces the native
## object -- even without a header file, that route's ABI/calling
## convention/symbol shape is fundamentally C's. `nlvm`
## (https://github.com/arnetheduck/nlvm) is Nim's existing LLVM-direct
## backend: Nim source -> nlvm's own LLVM IR emission -> native object,
## with no C source or C compiler anywhere in the path. This file only
## checks that nlvm can compile and run *something* on this CI runner
## before attempting the harder direct-link-with-Rust experiment.

echo "hello from nlvm"
