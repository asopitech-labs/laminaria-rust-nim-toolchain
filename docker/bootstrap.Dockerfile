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
# Build:  docker build -f docker/bootstrap.Dockerfile -t laminaria-bootstrap .
# Run:    docker run --rm -it laminaria-bootstrap doctor
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

# Left at rustup/choosenim's defaults under $HOME (/root here, running as
# root) so the ~/.cargo, ~/.rustup, ~/.choosenim paths this repo's own lock
# file and doctor code already expect (see exec::expand_tilde) just work,
# with no container-specific path overrides to keep in sync by hand.
ENV PATH=/root/.cargo/bin:/root/.nimble/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustup component add llvm-tools

RUN curl https://nim-lang.org/choosenim/init.sh -sSf | sh -s -- -y \
    && choosenim --yes 2.2.10

RUN cargo install wasm-tools --version 1.255.0

WORKDIR /workspace
COPY . .

RUN cargo build --workspace

# Actually build #11's first two committed "Core workloads" fixtures here,
# not just run doctor — this is the real evidence for #18's acceptance
# criterion that a fresh environment can reproduce the tool subsets a
# fixture needs, rather than an assertion about it.
RUN cd fixtures/rust-heavy-workspace && cargo build --workspace && cargo run -q -p fixture-bin
RUN cd fixtures/nim-heavy-workspace && nim c -o:fixture_out src/fixture.nim && ./fixture_out

ENTRYPOINT ["cargo", "run", "-q", "-p", "laminaria-cli", "--"]
CMD ["doctor"]
