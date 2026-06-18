# TypeScript / JavaScript

> Schema conventions for @t3-oss/env-core, zod, public/private env files, and monorepo with-env scripts.

Dependencies: `@t3-oss/env-core`, `zod`, `dotenv-cli`

```sh
# single app
- .env
- .env.example
- src/
    - env.private.ts
    - env.public.ts
```

```sh
# monorepo
- .env
- .env.example
- apps/
    - web/
        - .env.example
        - src/env.private.ts
        - src/env.public.ts
```

## Example

`src/env.private.ts`

```ts
import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {
    DATABASE_URL: z.string(),
    JWT_SECRET: z.string().min(32),
  },
});
```

`src/env.public.ts` (Next: `NEXT_PUBLIC_`, Vite: `VITE_`, etc.)

```ts
export const publicEnv = createEnv({
  emptyStringAsUndefined: true,
  clientPrefix: "NEXT_PUBLIC_",
  client: {
    NEXT_PUBLIC_APP_URL: z.string().url(),
  },
  runtimeEnvStrict: {
    NEXT_PUBLIC_APP_URL: process.env.NEXT_PUBLIC_APP_URL,
  },
});
```

`package.json` (monorepo app)

```json
"with-env": "dotenv -e ../../.env --",
"dev": "pnpm with-env next dev"
```

Use `privateEnv` / `publicEnv` in code — not `process.env`.
