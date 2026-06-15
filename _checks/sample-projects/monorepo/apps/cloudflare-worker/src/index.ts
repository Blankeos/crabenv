import { privateEnv } from "./env.private";
import { publicEnv } from "./env.public";

export default {
  fetch() {
    return Response.json({
      ok: true,
      origin: publicEnv.PUBLIC_WORKER_ORIGIN,
      namespace: privateEnv.WORKER_KV_NAMESPACE,
    });
  },
};

