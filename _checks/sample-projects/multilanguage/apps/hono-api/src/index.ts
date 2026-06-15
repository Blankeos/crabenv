import { Hono } from "hono";
import { privateEnv } from "./env.private";
import { publicEnv } from "./env.public";

const app = new Hono();

app.get("/health", (c) =>
  c.json({
    ok: true,
    origin: publicEnv.PUBLIC_HONO_ORIGIN,
    database: privateEnv.DATABASE_URL,
  }),
);

export default app;

console.log(`Multilanguage Hono fixture configured for port ${privateEnv.HONO_PORT}`);

