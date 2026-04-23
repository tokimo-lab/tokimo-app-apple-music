import { useCallback } from "react";
import { useAppleMusic } from "../AppleMusicProvider";

/**
 * Navigates to an artist page by catalog ID or by searching the artist name.
 * Reusable across TrackList, MediaItemCard, and page components.
 */
export function useArtistNavigation() {
  const { navigateTo, api } = useAppleMusic();

  const navigateToArtist = useCallback(
    async (artistName: string, item?: MusicKit.Resource) => {
      // Try relationships first if item is provided
      if (item) {
        const rels = (item as unknown as Record<string, unknown>)
          .relationships as
          | { artists?: { data?: { id: string }[] } }
          | undefined;
        const artistId = rels?.artists?.data?.[0]?.id;
        if (artistId) {
          navigateTo({ type: "artist", id: artistId });
          return;
        }
      }
      // Fall back to catalog search
      try {
        const resp = await api(
          `/v1/catalog/us/search?types=artists&term=${encodeURIComponent(artistName)}&limit=1`,
        );
        const results = resp.data?.results?.artists?.data;
        if (results?.[0]?.id) {
          navigateTo({ type: "artist", id: results[0].id });
        }
      } catch {
        // Ignore search failures
      }
    },
    [navigateTo, api],
  );

  return navigateToArtist;
}
