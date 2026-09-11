# Repository development rules

## Windows build execution

- When development is initiated from a Windows host, run every build, check,
  test, lint, format-check, and toolchain diagnostic inside the `wslc`
  container. Do not invoke host Windows `cargo`, `rustc`, `nim`, `nimble`, or
  the bootstrap script for project development.
- Build or refresh the development image from the repository root with:

  ```powershell
  wslc build --progress plain -f docker/bootstrap.Dockerfile -t laminaria-bootstrap .
  ```

- Run commands with `wslc run --rm --pull never`. Override the image entrypoint
  when invoking tools other than the LAMINARIA CLI. The canonical commands and
  lifecycle are documented in `docs/windows-wslc-development.md`.
- Keep the image's default unprivileged `laminaria` user. Do not add
  `--user root` to normal build or test commands.
- For the Nim planning-kernel test on Windows, use the documented direct
  `nim c -r` command. Do not use `nimble test`, because dependency resolution
  can download and select a compiler other than the repository-pinned 2.2.10.
- Do not treat container measurements as native Windows performance evidence.
  The measurement policy in `docs/measurement-foundation.md` still applies.
