# Isolated correctness-reproduction environment for issue #43 R0.
#
# Go is fixed because the pinned Hike revision declares go 1.22. Clang, LLD,
# Node.js, WABT, Binaryen, and zstd come from the recorded Debian bookworm
# package snapshot selected when this image is built; the reproduction report
# records their exact installed versions.
FROM golang:1.22.12-bookworm@sha256:3d699e4d15d0f8f13c9195c0632a16702b8cbdece2955af1c23b37ae5d55a253

RUN apt-get update && apt-get install -y --no-install-recommends \
      binaryen=108-1 \
      clang=1:14.0-55.7~deb12u1 \
      lld=1:14.0-55.7~deb12u1 \
      nodejs=18.20.4+dfsg-1~deb12u2 \
      wabt=1.0.32-1 \
      zstd=1.5.4+dfsg2-5 \
    && rm -rf /var/lib/apt/lists/*

ARG LAMINARIA_UID=1000
ARG LAMINARIA_GID=1000
RUN groupadd --gid "${LAMINARIA_GID}" laminaria \
    && useradd --create-home --uid "${LAMINARIA_UID}" --gid "${LAMINARIA_GID}" --shell /bin/bash laminaria

USER laminaria
ENV HOME=/home/laminaria
WORKDIR /work

ENTRYPOINT ["python3", "scripts/hike_r0_reproduce.py"]
