# Disposable, isolated measurement environment for issue #14 (white-box LLVM
# pass timing) and the direct follow-up question raised in this session:
# "which LLVM process is dominant in alopex-cli's own build time?"
#
# Rationale for a container here, matching the precedent set by
# .reference/alopex/Dockerfile.issue62-monoitems (issue #62) and
# docker/bootstrap.Dockerfile (issue #18): reproducible toolchain bootstrap,
# not a canonical performance baseline. -Z time-passes / -Z time-llvm-passes
# require a nightly rustc; alopex-cli's own build otherwise only needs the
# vendored, prebuilt x86_64-unknown-linux-gnu Nim parser shared library
# already checked into crates/alopex-sql/nim-sql-parser/vendor/ -- no Nim
# toolchain install needed here, unlike docker/bootstrap.Dockerfile.
FROM rust:1.96-bookworm

RUN rustup toolchain install nightly && rustup default nightly

WORKDIR /work
COPY . /work

CMD ["bash"]
