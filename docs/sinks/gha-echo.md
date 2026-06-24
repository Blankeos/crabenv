---
title: gha-echo sink
description: Generate GitHub Actions shell echo lines that append crabenv variables to a dotenv file
---

Use inside a GitHub Actions `run: |` block when the build expects a dotenv file such as `.env.local`.

```yaml
steps:
  - name: Create env file
    run: |
      # crabenv:start format=gha-echo scope=all owner=apps/web dest=.env.local
      echo "MYSQL_DB_URL=${{ secrets.MYSQL_DB_URL }}" >> .env.local
      echo "NEXT_PUBLIC_BASE_ORIGIN=${{ vars.NEXT_PUBLIC_BASE_ORIGIN }}" >> .env.local
      # crabenv:end
```

## Marker

```yaml
# crabenv:start format=gha-echo scope=all owner=apps/web dest=.env.local
# crabenv:end
```

## Extra options

| Option | Required | Meaning                                             |
| ------ | -------- | --------------------------------------------------- |
| `dest` | yes      | Dotenv file to append to, for example `.env.local`. |

## Output

```sh
echo "NEXT_PUBLIC_API_URL=${{ vars.NEXT_PUBLIC_API_URL }}" >> .env.local
echo "DATABASE_URL=${{ secrets.DATABASE_URL }}" >> .env.local
```

Public variables default to GitHub `vars`. Private variables default to GitHub `secrets`.
