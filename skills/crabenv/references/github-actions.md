# GitHub Actions Sinks

> Implemented deployment sink notes for GitHub Actions.

Supported formats: `gha-env`, `gha-echo`.

## `gha-env`

Use inside a workflow/job/step `env:` map.

```yaml
jobs:
  build-web:
    env:
      # crabenv:start format=gha-env scope=public owner=apps/web
      NEXT_PUBLIC_BASE_ORIGIN: ${{ vars.NEXT_PUBLIC_BASE_ORIGIN }}
      NEXT_PUBLIC_GOOGLE_MAPS_API_KEY: ${{ vars.NEXT_PUBLIC_GOOGLE_MAPS_API_KEY }}
      # crabenv:end
```

Rules:

- No `source` or `dest` option.
- Public vars default to `${{ vars.NAME }}`.
- Private vars default to `${{ secrets.NAME }}`.

## `gha-echo`

Use inside a `run: |` block to append variables to a dotenv file.

```yaml
steps:
  - name: Create env file
    run: |
      # crabenv:start format=gha-echo scope=all owner=apps/web dest=.env.local
      echo "MYSQL_DB_URL=${{ secrets.MYSQL_DB_URL }}" >> .env.local
      echo "NEXT_PUBLIC_BASE_ORIGIN=${{ vars.NEXT_PUBLIC_BASE_ORIGIN }}" >> .env.local
      # crabenv:end
```

Rules:

- `scope=public`, `scope=private`, or `scope=all`.
- `dest=<dotenv path>` is required.
- Public vars default to `${{ vars.NAME }}`.
- Private vars default to `${{ secrets.NAME }}`.

## Sync

```sh
crabenv doctor
crabenv doctor --fix
crabenv doctor --fix --yes
```

crabenv only rewrites text between the managed markers. Existing generated lines that use `${{ vars.NAME }}` or `${{ secrets.NAME }}` preserve that namespace choice for the same variable.
