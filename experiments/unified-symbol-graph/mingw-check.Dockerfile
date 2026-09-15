# Scratch, non-committed image to verify Windows/COFF relocation shape
# using a MinGW-w64 cross toolchain inside Linux -- avoids depending on
# the WSL host's own Windows filesystem/MSVC installation (which this
# repo's own toolchains.lock.toml does not pin, and which was found to
# lack cl.exe entirely when checked directly).
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
      gcc-mingw-w64-x86-64 \
      binutils-mingw-w64-x86-64 \
    && rm -rf /var/lib/apt/lists/*
