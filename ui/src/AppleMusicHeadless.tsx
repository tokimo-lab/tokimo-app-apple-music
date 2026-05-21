/**
 * AppleMusicHeadless — background mount that keeps the MusicKit instance
 * configured (so login + catalog API work the moment any window opens) and
 * registers the Apple Music provider with the host MediaCenter so playback
 * continues after the window is closed.
 *
 * Rendered by the host BackgroundAppHost into a hidden container.
 */

import { useQuery } from "@tanstack/react-query";
import { AppleMusicProvider } from "./components/AppleMusicProvider";

export default function AppleMusicHeadless() {
  const { data } = useQuery({
    queryKey: ["apple-music-token-headless"],
    queryFn: async () => {
      const r = await fetch("/api/apps/apple-music/token", {
        credentials: "include",
      });
      if (!r.ok) throw new Error(`${r.status}`);
      const json = (await r.json()) as {
        success: boolean;
        data?: { developerToken: string };
      };
      if (!json.success || !json.data?.developerToken)
        throw new Error("no token");
      return json.data as { developerToken: string };
    },
    staleTime: 1000 * 60 * 60,
    retry: 3,
  });

  if (!data?.developerToken) return null;

  return (
    <AppleMusicProvider developerToken={data.developerToken}>
      {/* headless: no UI */}
    </AppleMusicProvider>
  );
}
