import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {
    NEXT_SESSION_SECRET: z.string().min(32),
    STRIPE_SECRET_KEY: z.string().optional(),
  },
});

