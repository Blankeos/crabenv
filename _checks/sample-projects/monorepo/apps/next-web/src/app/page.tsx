import { privateEnv } from "../env.private";
import { publicEnv } from "../env.public";

export default function Page() {
  return (
    <main>
      <h1>crabenv Next fixture</h1>
      <p>{publicEnv.NEXT_PUBLIC_APP_URL}</p>
      <p>{publicEnv.NEXT_PUBLIC_API_URL}</p>
      <p>{privateEnv.GITHUB_CLIENT_ID ?? "no github client"}</p>
    </main>
  );
}

