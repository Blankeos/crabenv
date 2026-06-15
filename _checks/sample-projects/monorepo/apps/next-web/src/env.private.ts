import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  emptyStringAsUndefined: true,
  runtimeEnv: process.env,
  server: {
    // Required outside local fixture bootstrapping.
    AUTH_SECRET: z.string().min(32),
    GITHUB_CLIENT_ID: z.string().optional(),
    GITHUB_CLIENT_SECRET: z.string().optional(),
    DATABASE_URL: z.string(),
  
  },
});
