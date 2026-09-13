# Repository development rules

## Single-source executable verification

The canonical fixture rules are [docs/01-foundations/fixture-policy.md](docs/01-foundations/fixture-policy.md).
Treat fixtures as production-consumed inputs, states, counterexamples, or workloads—not
as a second implementation of a configuration, document, or production algorithm.

- Do not encode one behavior contract redundantly as a hand-maintained YAML
  fixture, a fixture-only validator, and unit tests for that validator when
  the same agent changes all three. This merely expands the change surface and
  does not provide an independent correctness guarantee.
- Prefer direct, executable behavioral tests against the production
  implementation. Keep a separate machine-readable fixture only when it has
  an independent runtime consumer or another concrete purpose beyond checking
  its own internal consistency.
- Do not treat a fixture validator passing as evidence that the production
  implementation is correct. If a fixture is retained, test the production
  implementation by consuming it directly, rather than duplicating its rules
  in a fixture-only validator.
- Do not repeat a checked-in configuration or lock's complete names, counts,
  revisions, features, or attributes in a test. Test the production consumer's
  generic behavior with minimal constructed inputs; the declaration remains the
  sole authority for its values.
- Freeze an expected value only when it has an independent oracle: an external
  standard, a documented manual derivation, an independent reference, a semantic
  relation, or a reduced real failure. Output captured from the implementation
  under test is not an independent expected result.

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
  lifecycle are documented in `docs/04-guides/windows-wslc-development.md`.
- Keep the image's default unprivileged `laminaria` user. Do not add
  `--user root` to normal build or test commands.
- For the Nim planning-kernel test on Windows, use the documented direct
  `nim c -r` command. Do not use `nimble test`, because dependency resolution
  can download and select a compiler other than the repository-pinned 2.2.10.
- Do not treat container measurements as native Windows performance evidence.
  The measurement policy in `docs/02-research-areas/measurement/measurement-foundation.md` still applies.
