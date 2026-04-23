import { useCallback } from "react";
import { useAppleMusic } from "../AppleMusicProvider";

/**
 * Navigates to an album page by relationship ID or by searching the album name.
 * Mirrors useArtistNavigation for consistent patterns.
 */
export function useAlbumNavigation() {
  const { navigateTo, api } = useAppleMusic();

  const navigateToAlbum = useCallback(
    async (albumName: string, item?: MusicKit.Resource) => {
      // Try relationships first if item is provided
      if (item) {
        const rels = (item as unknown as Record<string, unknown>)
          .relationships as
          | { albums?: { data?: { id: string }[] } }
          | undefined;
        const albumId = rels?.albums?.data?.[0]?.id;
        if (albumId) {
          navigateTo({ type: "album", id: albumId });
          return;
        }
      }
      // Fall back to catalog search
      try {
        const resp = await api(
          `/v1/catalog/us/search?types=albums&term=${encodeURIComponent(albumName)}&limit=1`,
        );
        const results = resp.data?.results?.albums?.data;
        if (results?.[0]?.id) {
          navigateTo({ type: "album", id: results[0].id });
        }
      } catch {
        // Ignore search failures
      }
    },
    [navigateTo, api],
  );

  return navigateToAlbum;
}
