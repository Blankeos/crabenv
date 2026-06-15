import { privateEnv } from "../env.private";
import { publicEnv } from "../env.public";

export default function Page() {
  return (
    <main>
      <h1>crabenv multilanguage fixture</h1>
      <p>{publicEnv.NEXT_PUBLIC_SITE_URL}</p>
      <p>{publicEnv.NEXT_PUBLIC_API_URL}</p>
      <p>{privateEnv.STRIPE_SECRET_KEY ? "stripe configured" : "stripe optional"}</p>
    </main>
  );
}

