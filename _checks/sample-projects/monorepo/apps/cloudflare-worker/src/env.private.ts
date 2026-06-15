import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {
    WORKER_WEBHOOK_SECRET: z.string().min(32),
    WORKER_KV_NAMESPACE: z.string(),
    WORKER_LOG_LEVEL: z.enum(["debug", "info", "warn", "error"]).default("info"),
  },
});

