import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const publicEnv = createEnv({
  emptyStringAsUndefined: true,
  clientPrefix: "PUBLIC_",
  client: {
    PUBLIC_API_ORIGIN: z.string().url(),
  },
  runtimeEnvStrict: {
    PUBLIC_API_ORIGIN: process.env.PUBLIC_API_ORIGIN,
  },
});

