/**
 * AppleMusicContent — Window content for the native Apple Music player.
 *
 * Auto-fetches the MusicKit developer token from our backend (which scrapes
 * it from Apple Music's website). No manual setup required.
 *
 * Rendered as a page component via PageWindowContent (uses useWindowNav for
 * window state access).
 */

import { Alert, Spin } from "@tokimo/ui";
import { useQuery } from "@tanstack/react-query";
import { useWindowNavHook as useWindowNav } from "./shell/hooks";
import { AppleMusicLayout } from "./components/AppleMusicLayout";
import { AppleMusicProvider } from "./components/AppleMusicProvider";
import type { AppleMusicPage } from "./components/types";

export default function AppleMusicContent() {
  const { route, replace } = useWindowNav();
  const { data, isLoading, error } = useQuery<{ developerToken: string }, Error>({
    queryKey: ["apple-music-token"],
    queryFn: async () => {
      const r = await fetch("/api/apps/apple-music/token", { credentials: "include" });
      if (!r.ok) throw new Error(`${r.status} ${await r.text()}`);
      return r.json() as Promise<{ developerToken: string }>;
    },
    staleTime: 1000 * 60 * 60,
  });

  const initialPage = parseRouteToPage(route);

  function handlePageChange(page: AppleMusicPage): void {
    replace(pageToRoute(page));
  }

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spin />
      </div>
    );
  }

  if (error || !data?.developerToken) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <Alert
          type="error"
          message={
            error instanceof Error
              ? error.message
              : "Failed to initialize Apple Music. The server could not obtain a developer token."
          }
        />
      </div>
    );
  }

  return (
    <AppleMusicProvider
      developerToken={data.developerToken}
      initialPage={initialPage}
      onPageChange={handlePageChange}
    >
      <AppleMusicLayout />
    </AppleMusicProvider>
  );
}

/** Convert an AppleMusicPage to a route path. */
function pageToRoute(page: AppleMusicPage): string {
  if (page.type === "library" && page.tab) {
    return `/library/${page.tab}`;
  }
  if (page.type === "artist") {
    return `/artist/${page.id}`;
  }
  if (page.type === "album" || page.type === "playlist") {
    const source = page.isLibrary ? "library" : "catalog";
    return `/${page.type}/${source}/${page.id}`;
  }
  if (page.type === "search" && page.query) {
    return `/search/${encodeURIComponent(page.query)}`;
  }
  return `/${page.type}`;
}

/** Parse a route path back into an AppleMusicPage. */
function parseRouteToPage(route: string): AppleMusicPage | undefined {
  if (!route || route === "/") return undefined;
  const segments = route.replace(/^\//, "").split("/");
  const [type, source, id] = segments;
  if (type === "library") {
    const validTabs = ["songs", "albums", "artists", "playlists"];
    if (source && validTabs.includes(source)) {
      return {
        type: "library",
        tab: source as "songs" | "albums" | "artists" | "playlists",
      };
    }
    return { type: "library" };
  }
  if (type === "artist" && source) {
    return { type: "artist", id: source };
  }
  if (
    (type === "album" || type === "playlist") &&
    (source === "library" || source === "catalog") &&
    id
  ) {
    return {
      type,
      id,
      isLibrary: source === "library",
    } as AppleMusicPage;
  }
  const validTypes = ["browse", "for-you", "search", "now-playing", "setup"];
  if (validTypes.includes(type)) {
    if (type === "search" && source) {
      return { type: "search", query: decodeURIComponent(source) };
    }
    return { type } as AppleMusicPage;
  }
  return undefined;
}
