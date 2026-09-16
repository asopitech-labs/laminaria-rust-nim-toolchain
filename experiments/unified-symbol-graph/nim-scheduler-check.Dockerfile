# Scratch, non-committed image for the Nim port of unified-symbol-graph's
# SharedSymbolGraph (issue #68 follow-up: verifying the scheduler/planner
# implementation, not the parser layer, can be done in Nim while keeping
# Nim's own algorithmic surface minimal per compiler-ownership-contract_ja.md
# section "C/C++ library再利用をfirst-class requirement" -- Nim wraps
# existing C/C++ data structures rather than reimplementing them).
#
# Never touches the host's own toolchain -- the user's own standing
# instruction after an earlier session installed a rustup target directly
# on the host without asking first (since reverted, see this session's
# memory file feedback_container_env.md).
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
      xz-utils \
      gcc \
      g++ \
      make \
    && rm -rf /var/lib/apt/lists/*

ENV CHOOSENIM_NO_ANALYTICS=1
ENV PATH=/root/.nimble/bin:$PATH

RUN curl https://nim-lang.org/choosenim/init.sh -sSf -o /tmp/init.sh && \
    sh /tmp/init.sh -y && \
    rm /tmp/init.sh

WORKDIR /work
