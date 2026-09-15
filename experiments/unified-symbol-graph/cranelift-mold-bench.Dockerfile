# Cranelift backend + mold linker combined build-time measurement for
# alopex-cli (issue #48 follow-up, "moldとの組み合わせも実測して").
#
# Kept as its own standalone Dockerfile (not a deleted scratch layer) per the
# user's explicit instruction: several purpose-built Dockerfiles, one per
# measured configuration, is the intended artifact — not a temporary layer
# thrown away after reading a single number off it.
#
# Prior measurements this compares against (same alopex-cli clean build):
#   default LLVM backend, default (bfd/gold via cc) linker: 122.78s
#   default LLVM backend, mold linker:                       124.71s
#   Cranelift backend,    default linker:                    see cranelift-bench.Dockerfile
#   Cranelift backend,    mold linker (this image):           measured below
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
      mold \
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

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain nightly \
    && rustup component add rustc-codegen-cranelift-preview --toolchain nightly

WORKDIR /workspace
# Same prebuilt Nim SQL parser FFI library approach as cranelift-bench.Dockerfile:
# alopex-cli's own build does not invoke Nim, so the host-built
# libalopex_sql_parser.so/.a is copied in rather than rebuilt here, keeping
# this measurement scoped to the Rust/Cranelift+mold compile-and-link step.
COPY --chown=laminaria:laminaria --from=alopexsrc . /workspace/alopex
USER root
RUN rm -rf /workspace/alopex/target
USER laminaria

ENV NIM_SQL_PARSER_LIB_DIR=/workspace/alopex/crates/alopex-sql/nim-sql-parser
# See cranelift-bench.Dockerfile: the checked-in vendor manifest is stale
# relative to build_support.rs, a pre-existing mismatch in the
# .reference/alopex checkout unrelated to this measurement.
ENV ALOPEX_NIM_PARSER_ALLOW_LOCAL_BUILD=1
ENV CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift
# -C link-arg=-fuse-ld=mold routes final linking through mold via the clang/cc
# driver, matching how the prior mold-only baseline (124.71s) was measured.
ENV RUSTFLAGS="-C link-arg=-fuse-ld=mold"

ENTRYPOINT ["/bin/bash", "-c"]
CMD ["cd /workspace/alopex && time (rustup run nightly cargo build -p alopex-cli -Zcodegen-backend) 2>&1 | tail -30"]
