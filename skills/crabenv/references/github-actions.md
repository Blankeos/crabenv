# GitHub Actions Sinks

> Deployment sink notes for GitHub Actions.

Sinks are optional deployment/build files that need crabenv variables.

crabenv only manages explicit marker blocks:

```txt
# crabenv:start format=<format> scope=<scope> owner=<owner>
# crabenv:end
```

Everything outside the markers is yours. Inside the markers, crabenv controls the variable list, ordering, and output shape for the selected format.

## Supported formats

| Format                      | File type                          | Output                                          |
| --------------------------- | ---------------------------------- | ----------------------------------------------- |
| [`gha-env`](./gha-env.md)   | `.github/workflows/*.yml`, `.yaml` | GitHub Actions `env:` map entries               |
| [`gha-echo`](./gha-echo.md) | `.github/workflows/*.yml`, `.yaml` | shell `echo` lines that append to a dotenv file |

> [Request here](https://github.com/Blankeos/crabenv/issues/new) if you think a new format is necessary.

## Marker fields

| Field    | Required | Meaning                                                |
| -------- | -------- | ------------------------------------------------------ |
| `format` | yes      | Sink renderer, for example `gha-env` or `gha-echo`.    |
| `scope`  | yes      | `public`, `private`, or `all`.                         |
| `owner`  | yes      | One app/workspace path, for example `apps/web` or `.`. |

## Sync

```sh
crabenv doctor              # report sink drift
crabenv doctor --fix        # preview fix plan
crabenv doctor --fix --yes  # rewrite managed sink blocks
```

`crabenv format` / `crabenv fmt` does not rewrite sinks.

## Rules

- Sinks are opt-in. crabenv never infers variables from arbitrary deployment files.
- `format`, `scope`, and `owner` are required.
- Unknown options fail closed.
- Supported GitHub Actions sinks scan `.github/workflows/*.yml` and `.github/workflows/*.yaml`.
- Generated names come from schema variables for the selected owner/scope.
- Entries are sorted with crabenv's deterministic env ordering.
- If an existing generated line uses `${{ vars.NAME }}` or `${{ secrets.NAME }}`, crabenv preserves that GitHub namespace choice for that variable.
