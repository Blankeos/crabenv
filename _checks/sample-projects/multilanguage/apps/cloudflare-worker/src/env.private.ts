import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {
    EDGE_WEBHOOK_SECRET: z.string().min(32),
    EDGE_QUEUE_NAME: z.string(),
  },
});

