# Check Fixtures

This folder contains small projects for manually checking `crabenv` behavior while the CLI and TUI are being built.

Use these as disposable fixtures. They intentionally do not include lockfiles or installed dependencies.

Suggested checks:

- `crabenv list`
- `crabenv doctor`
- `crabenv doctor --fix`
- `crabenv doctor --fix --yes`
- `crabenv copy`
- `crabenv add VARIABLE_NAME`

Sink check:

```sh
cargo run -- --root _checks/sample-projects/monorepo doctor
```

The monorepo fixture includes synced managed GitHub Actions sink blocks for `gha-env` and `gha-echo` in `.github/workflows/deploy.yml`. To test the fixer, edit a line inside one of those blocks and run `doctor --fix --yes`.

