# Python

> Pydantic settings conventions for env.py, Field aliases, and monorepo run scripts.

Dependencies: `pydantic`, `pydantic-settings`; monorepo justfile examples use `dotenv-cli`

```sh
# single app
- .env
- .env.example
- src/
    - <package>/
        - env.py
```

```sh
# monorepo
- .env
- .env.example
- apps/
    - api/
        - .env.example
        - src/api/env.py
```

Crabenv discovers the first `env.py` under `src/` (depth ≤ 4). Env var names are the `Field(alias="...")` values (typically `SCREAMING_SNAKE`). Server apps only — no client/public split.

## Example

`src/api/env.py`

```python
from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    database_url: str = Field(alias="DATABASE_URL")
    secret_key: str = Field(min_length=32, alias="SECRET_KEY")
    log_level: str = Field(default="info", alias="LOG_LEVEL")

    model_config = SettingsConfigDict(env_file=".env", extra="ignore")


settings = Settings()
```

`main.py`

```python
from api.env import settings


def main() -> None:
    print(settings.database_url)
```

Monorepo local run (root `.env` → process env):

```just
_env *cmd:
    dotenv -e ../../.env -- {{ cmd }}

dev *args:
    just _env "uv run python -m api.main {{ args }}"
```

Use `settings` in code — not `os.environ`. Docs and defaults live on `Field(...)`; keep `.env.example` keys and aliases aligned.
