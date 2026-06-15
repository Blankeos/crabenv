import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {
    HONO_PORT: z.coerce.number().int().default(8890),
    DATABASE_URL: z.string(),
    HONO_SIGNING_SECRET: z.string().min(32),
  },
});

