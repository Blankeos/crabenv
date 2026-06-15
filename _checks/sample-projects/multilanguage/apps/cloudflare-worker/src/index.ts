import { privateEnv } from "./env.private";
import { publicEnv } from "./env.public";

export default {
  fetch() {
    return Response.json({
      ok: true,
      origin: publicEnv.PUBLIC_EDGE_ORIGIN,
      queue: privateEnv.EDGE_QUEUE_NAME,
    });
  },
};
