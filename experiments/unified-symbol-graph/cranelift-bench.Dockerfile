# Cranelift backend build-time measurement for alopex-cli (issue #48 follow-up).
#
# This is a throwaway measurement image kept as a standalone Dockerfile (not a
# temporary layer that gets deleted after use) so that the exact steps that
# produced the number are reproducible and auditable later — see the user's
# explicit instruction to keep separate Dockerfiles per backend rather than
# delete a scratch layer after reading a number off it once.
#
# Baseline for comparison (previously measured, same alopex-cli clean build,
# default LLVM backend): 122.78s
# mold-linked baseline (same clean build, default LLVM backend + mold linker): 124.71s
#
# This image measures: rustc_codegen_cranelift (nightly), Cargo default
# (non-mold) linker.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      curl \
      git \
      python3 \
      xz-utils \
      clang \
      llvm \
      lld \
      cmake \
      pkg-config \
    && rm -rf /var/lib/apt/lists/*

ARG LAMINARIA_UID=1000
ARG LAMINARIA_GID=1000
RUN groupadd --gid "${LAMINARIA_GID}" laminaria \
    && useradd --create-home --uid "${LAMINARIA_UID}" --gid "${LAMINARIA_GID}" --shell /bin/bash laminaria

USER laminaria
ENV HOME=/home/laminaria
ENV PATH=/home/laminaria/.cargo/bin:$PATH

# nightly is required: rustc_codegen_cranelift is only distributed as a
# rustup component on nightly toolchains, verified from
# github.com/rust-lang/rustc_codegen_cranelift's README during this
# session's web research.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain nightly \
    && rustup component add rustc-codegen-cranelift-preview --toolchain nightly

WORKDIR /workspace
# Nim SQL parser is a prebuilt FFI dependency of alopex-sql — alopex-cli's
# own build does not invoke Nim, so the shared libalopex_sql_parser.so/.a
# built once on the host (scripts/build-nim-parser.sh) is copied in rather
# than rebuilt inside this image. This keeps the measurement below scoped to
# the Rust/Cranelift compile-and-link step only, matching how the existing
# 122.78s (default LLVM) and 124.71s (mold) baselines were measured.
COPY --chown=laminaria:laminaria --from=alopexsrc . /workspace/alopex
USER root
RUN rm -rf /workspace/alopex/target
USER laminaria

ENV NIM_SQL_PARSER_LIB_DIR=/workspace/alopex/crates/alopex-sql/nim-sql-parser
# The checked-in vendor manifest is stale relative to this build_support.rs
# (missing the static_library field it now requires), which is a pre-existing
# mismatch in the .reference/alopex checkout unrelated to this Cranelift
# measurement. ALOPEX_NIM_PARSER_ALLOW_LOCAL_BUILD=1 skips that strict
# manifest/hash match and accepts the locally built parser library instead.
ENV ALOPEX_NIM_PARSER_ALLOW_LOCAL_BUILD=1
ENV CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift

ENTRYPOINT ["/bin/bash", "-c"]
CMD ["cd /workspace/alopex && time (rustup run nightly cargo build -p alopex-cli -Zcodegen-backend) 2>&1 | grep -iE 'aws-lc-sys|cmake|Running.*build-script|error|warning: unused' | head -60"]
