import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {
    DATABASE_URL: z.string(),

    HONO_API_PORT: z.coerce.number().int().default(8787),
    HONO_JWT_SECRET: z.string().min(32),
    HONO_LOG_LEVEL: z.enum(["debug", "info", "warn", "error"]).default("info"),
  },
});

