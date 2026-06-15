import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const publicEnv = createEnv({
  emptyStringAsUndefined: true,
  clientPrefix: "PUBLIC_",
  client: {
    PUBLIC_APP_ORIGIN: z.string().url(),
  },
  runtimeEnvStrict: {
    PUBLIC_APP_ORIGIN: process.env.PUBLIC_APP_ORIGIN,
  },
});

