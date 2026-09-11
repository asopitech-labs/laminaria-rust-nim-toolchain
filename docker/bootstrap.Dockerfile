# Bootstrap/correctness reproduction environment for issue #18.
#
# Per docs/measurement-foundation.md section 2 ("Separate reproducibility
# from performance isolation") and the Environment rule in
# docs/issue-plan.md: containers may be used to reproduce bootstrap and
# correctness, but canonical performance baselines should normally execute
# natively on the measured host, so VM filesystem bridges and container-host
# effects are not silently attributed to the compiler. This image exists to
# exercise the "container" EnvironmentFingerprint class explicitly and to
# prove the toolchain bootstraps reproducibly independent of any one
# developer machine's system package manager state — not to replace native
# runs. `doctor` records environment_class = "container" for any Run
# executed here, and docs/measurement-foundation.md's comparability policy
# (crates/laminaria-fingerprint/src/comparability.rs) refuses to silently
# treat a container Run as comparable to a native one.
#
# Windows development uses wslc exclusively:
# Build:  wslc build --progress plain -f docker/bootstrap.Dockerfile -t laminaria-bootstrap .
# Run:    wslc run --rm --pull never laminaria-bootstrap doctor
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
      binaryen \
    && rm -rf /var/lib/apt/lists/*

# Build and run as an unprivileged user. Apart from avoiding root-owned build
# artifacts, this preserves permission-sensitive tests that intentionally make
# a directory unreadable. The UID/GID can be overridden when a future bind-mount
# workflow needs to match a host account; the standard COPY-based workflow uses
# the deterministic defaults.
ARG LAMINARIA_UID=1000
ARG LAMINARIA_GID=1000
RUN groupadd --gid "${LAMINARIA_GID}" laminaria \
    && useradd --create-home --uid "${LAMINARIA_UID}" --gid "${LAMINARIA_GID}" --shell /bin/bash laminaria

USER laminaria
ENV HOME=/home/laminaria

# Leave rustup/choosenim at their defaults under the unprivileged user's home so
# the ~/.cargo, ~/.rustup, ~/.choosenim paths this repository's lock and doctor
# code expect (see exec::expand_tilde) work without container-only overrides.
ENV PATH=/home/laminaria/.cargo/bin:/home/laminaria/.nimble/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustup component add llvm-tools

RUN curl https://nim-lang.org/choosenim/init.sh -sSf | sh -s -- -y \
    && choosenim --yes 2.2.10

RUN cargo install wasm-tools --version 1.255.0

WORKDIR /workspace
COPY --chown=laminaria:laminaria . .

RUN cargo build --workspace

# Actually build #11's first two committed "Core workloads" fixtures here,
# not just run doctor — this is the real evidence for #18's acceptance
# criterion that a fresh environment can reproduce the tool subsets a
# fixture needs, rather than an assertion about it.
RUN cd fixtures/rust-heavy-workspace && cargo build --workspace && cargo run -q -p fixture-bin
RUN cd fixtures/nim-heavy-workspace && nim c -o:fixture_out src/fixture.nim && ./fixture_out
RUN cd fixtures/rust-nim-c-abi-baseline/rust-bin && cargo run
RUN cd fixtures/wide-parallel-graph && cargo build --workspace && cargo test --workspace && cargo run -q -p aggregator

ENTRYPOINT ["cargo", "run", "-q", "-p", "laminaria-cli", "--"]
CMD ["doctor"]
