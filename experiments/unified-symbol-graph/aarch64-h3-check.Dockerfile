# Scratch, non-committed image to support issue #68's H3 experiment
# (can Cranelift's InstructionData/MachInst two-layer split be avoided
# with one target-generic IR lowered to two different targets?) without
# touching the host's own rustup toolchain -- the user's own explicit
# instruction, after an earlier mistake in this session installed the
# aarch64-unknown-linux-gnu rust-std component directly on the host
# without asking first (since reverted).
#
# Provides: a real AArch64 cross-assembler/objdump (via gcc-aarch64-linux-gnu)
# to independently verify any hand-assembled AArch64 bytes
# lower_cfg_body_to_aarch64_code_body produces (the same "objdump, not
# self-mirrored arithmetic" discipline this crate's own x86_64 lowering
# functions already follow), plus a nightly Rust toolchain with the
# rustc-dev/llvm-tools components this repo's own experiments/rustc-driver-poc
# crate requires and the aarch64-unknown-linux-gnu target, so
# `rustc --target aarch64-unknown-linux-gnu --emit=asm` can be run inside
# this container to compare real LLVM-generated AArch64 output against
# this crate's own direct lowering, exactly as the x86_64 side of this
# investigation already did against real `rustc -O --emit=asm` output.
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
      gcc-aarch64-linux-gnu \
      binutils-aarch64-linux-gnu \
      libc6-dev-arm64-cross \
      build-essential \
      qemu-user \
    && rm -rf /var/lib/apt/lists/*

# Minimal, direct rustup install (no shell profile sourcing needed inside
# a container) -- pins to whatever nightly is current at build time,
# matching this repo's own worktree environment (no toolchains.lock.toml
# entry exists yet for the rustc-driver-poc nightly requirement).
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
      sh -s -- -y --default-toolchain nightly --profile minimal && \
    rustup component add rustc-dev llvm-tools --toolchain nightly && \
    rustup target add aarch64-unknown-linux-gnu --toolchain nightly

WORKDIR /work
