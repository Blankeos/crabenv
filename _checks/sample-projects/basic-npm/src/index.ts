import { privateEnv } from "./env.private";
import { publicEnv } from "./env.public";

console.log({
  origin: publicEnv.PUBLIC_APP_ORIGIN,
  database: privateEnv.DATABASE_URL,
  mailFrom: privateEnv.SMTP_FROM,
});
