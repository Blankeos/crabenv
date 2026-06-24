---
title: gha-env sink
description: Generate GitHub Actions env map entries from crabenv schema variables
---

Use inside a GitHub Actions `env:` map.

```yaml
jobs:
  build-web:
    runs-on: ubuntu-latest
    env:
      # crabenv:start format=gha-env scope=public owner=apps/web
      NEXT_PUBLIC_BASE_ORIGIN: ${{ vars.NEXT_PUBLIC_BASE_ORIGIN }}
      NEXT_PUBLIC_GOOGLE_MAPS_API_KEY: ${{ vars.NEXT_PUBLIC_GOOGLE_MAPS_API_KEY }}
      # crabenv:end
    steps:
      - run: pnpm --filter web build
```

## Marker

```yaml
# crabenv:start format=gha-env scope=public owner=apps/web
# crabenv:end
```

## Output

```yaml
NEXT_PUBLIC_API_URL: ${{ vars.NEXT_PUBLIC_API_URL }}
```

Public variables default to GitHub `vars`. Private variables default to GitHub `secrets`.
