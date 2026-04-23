import { Button, Spin, Tabs } from "@tokimo/ui";
import { Library, LogIn } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useAppleMusic } from "../AppleMusicProvider";
import { MediaItemCard } from "../components/MediaItemCard";
import { TrackList } from "../components/TrackList";
import { useArtistNavigation } from "../hooks/useArtistNavigation";
import { formatArtworkUrl } from "../types";

type LibraryTab = "songs" | "albums" | "artists" | "playlists";

const TABS: { key: LibraryTab; label: string }[] = [
  { key: "songs", label: "Songs" },
  { key: "albums", label: "Albums" },
  { key: "artists", label: "Artists" },
  { key: "playlists", label: "Playlists" },
];

export default function LibraryPage() {
  const {
    api,
    isAuthorized,
    tokenExpired,
    authorize,
    navigateTo,
    setQueue,
    setQueueFromTracks,
    currentPage,
  } = useAppleMusic();
  const navigateToArtist = useArtistNavigation();

  const activeTab: LibraryTab =
    currentPage.type === "library" && currentPage.tab
      ? currentPage.tab
      : "songs";

  const [items, setItems] = useState<MusicKit.Resource[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [offset, setOffset] = useState(0);
  // Composite artwork URLs for playlists without their own artwork
  const [playlistComposites, setPlaylistComposites] = useState<
    Record<string, string[]>
  >({});
  const compositesRef = useRef(playlistComposites);
  compositesRef.current = playlistComposites;

  const PAGE_SIZE = 100;

  useEffect(() => {
    if (!isAuthorized || tokenExpired) return;
    let cancelled = false;

    async function fetchLibrary() {
      setLoading(true);
      setError(null);
      setItems([]);
      setOffset(0);
      setHasMore(false);
      setPlaylistComposites({});
      try {
        const params: Record<string, unknown> = { limit: PAGE_SIZE };
        if (activeTab === "artists") params.include = "catalog";

        const res = await api(`/v1/me/library/${activeTab}`, params);
        if (cancelled) return;
        const data = res?.data?.data ?? [];
        setItems(data);
        setOffset(data.length);
        setHasMore(data.length >= PAGE_SIZE);
      } catch (err) {
        if (!cancelled) {
          setError(
            err instanceof Error ? err.message : "Failed to load library",
          );
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    fetchLibrary();
    return () => {
      cancelled = true;
    };
  }, [isAuthorized, tokenExpired, activeTab, api]);

  // Fetch tracks for playlists without artwork to build composite covers
  useEffect(() => {
    if (activeTab !== "playlists" || items.length === 0) return;
    let cancelled = false;

    const needArtwork = items.filter(
      (p) => !p.attributes?.artwork && !compositesRef.current[p.id],
    );
    if (needArtwork.length === 0) return;

    async function fetchComposites() {
      const results: Record<string, string[]> = {};
      // Fetch in parallel, max 6 at a time to avoid overload
      const batch = needArtwork.slice(0, 6);
      await Promise.allSettled(
        batch.map(async (playlist) => {
          try {
            const res = await api(`/v1/me/library/playlists/${playlist.id}`, {
              include: "tracks",
            });
            const tracks = res?.data?.data?.[0]?.relationships?.tracks?.data;
            if (tracks?.length) {
              const urls = tracks
                .slice(0, 4)
                .map((t: MusicKit.Resource) =>
                  formatArtworkUrl(t.attributes?.artwork, 160),
                )
                .filter(Boolean);
              if (urls.length > 0) results[playlist.id] = urls;
            }
          } catch {
            // ignore per-playlist fetch errors
          }
        }),
      );
      if (!cancelled && Object.keys(results).length > 0) {
        setPlaylistComposites((prev) => ({ ...prev, ...results }));
      }
    }

    fetchComposites();
    return () => {
      cancelled = true;
    };
  }, [activeTab, items, api]);

  async function loadMore(): Promise<void> {
    setLoadingMore(true);
    try {
      const params: Record<string, unknown> = { limit: PAGE_SIZE, offset };
      if (activeTab === "artists") params.include = "catalog";

      const res = await api(`/v1/me/library/${activeTab}`, params);
      const data = res?.data?.data ?? [];
      setItems((prev) => [...prev, ...data]);
      setOffset((prev) => prev + data.length);
      setHasMore(data.length >= PAGE_SIZE);
    } catch {
      // silently fail on load more
    } finally {
      setLoadingMore(false);
    }
  }

  async function handlePlaySong(index: number): Promise<void> {
    if (items.length === 0) return;
    await setQueueFromTracks(items, index);
  }

  function switchTab(tab: LibraryTab): void {
    navigateTo({ type: "library", tab });
  }

  if (!isAuthorized || tokenExpired) {
    const isReconnect = isAuthorized && tokenExpired;
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 text-[var(--text-secondary)]">
        <Library size={48} strokeWidth={1} />
        <p className="text-base font-medium">
          {isReconnect ? "Session expired" : "Sign in to access your library"}
        </p>
        <p className="text-sm text-[var(--text-tertiary)]">
          {isReconnect
            ? "Your Apple Music session has expired. Reconnect to access your library."
            : "Connect your Apple Music account to see your songs, albums, and playlists"}
        </p>
        <Button
          variant="primary"
          shape="round"
          icon={<LogIn size={16} />}
          onClick={authorize}
          style={{ backgroundColor: "#FA2D48", borderColor: "#FA2D48" }}
        >
          {isReconnect ? "Reconnect" : "Sign In"}
        </Button>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <Tabs
        type="pill"
        activeKey={activeTab}
        onChange={(key) => switchTab(key as LibraryTab)}
        items={TABS.map((tab) => ({ key: tab.key, label: tab.label }))}
        className="[--accent:#FA2D48] border-b border-border-base px-6 py-3"
      />

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-6">
        {loading && (
          <div className="flex h-full items-center justify-center">
            <Spin spinning tip="Loading library…" />
          </div>
        )}

        {error && (
          <div className="flex h-full items-center justify-center text-sm text-[var(--text-secondary)]">
            {error}
          </div>
        )}

        {!loading && !error && items.length === 0 && (
          <div className="flex h-full items-center justify-center text-sm text-[var(--text-tertiary)]">
            No {activeTab} in your library
          </div>
        )}

        {!loading && !error && items.length > 0 && (
          <>
            {activeTab === "songs" && (
              <TrackList
                tracks={items}
                showArtwork
                showAlbum
                onPlayTrack={handlePlaySong}
              />
            )}

            {activeTab === "albums" && (
              <div className="flex flex-wrap gap-4">
                {items.map((album) => (
                  <MediaItemCard
                    key={album.id}
                    item={album}
                    type="album"
                    onClick={() =>
                      navigateTo({
                        type: "album",
                        id: album.id,
                        isLibrary: true,
                      })
                    }
                    onPlay={() =>
                      setQueue({ album: album.id, startPlaying: true })
                    }
                    onSubtitleClick={
                      album.attributes?.artistName
                        ? () =>
                            navigateToArtist(
                              album.attributes!.artistName!,
                              album,
                            )
                        : undefined
                    }
                  />
                ))}
              </div>
            )}

            {activeTab === "artists" && (
              <div className="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] justify-items-center gap-2">
                {items.map((artist) => {
                  const catalogData = artist.relationships?.catalog?.data?.[0];
                  const catalogId = catalogData?.id;
                  const catalogArtwork = catalogData?.attributes?.artwork;
                  return (
                    <MediaItemCard
                      key={artist.id}
                      item={artist}
                      type="artist"
                      overrideArtwork={catalogArtwork}
                      onClick={() =>
                        navigateTo({
                          type: "artist",
                          id: catalogId ?? artist.id,
                        })
                      }
                    />
                  );
                })}
              </div>
            )}

            {activeTab === "playlists" && (
              <div className="flex flex-wrap gap-4">
                {items.map((playlist) => {
                  const compositeImages = !playlist.attributes?.artwork
                    ? playlistComposites[playlist.id]
                    : undefined;
                  return (
                    <MediaItemCard
                      key={playlist.id}
                      item={playlist}
                      type="playlist"
                      compositeImages={compositeImages}
                      onClick={() =>
                        navigateTo({
                          type: "playlist",
                          id: playlist.id,
                          isLibrary: true,
                        })
                      }
                      onPlay={() =>
                        setQueue({
                          playlist: playlist.id,
                          startPlaying: true,
                        })
                      }
                    />
                  );
                })}
              </div>
            )}

            {hasMore && (
              <div className="mt-6 flex justify-center">
                <Button
                  variant="default"
                  shape="round"
                  loading={loadingMore}
                  onClick={loadMore}
                  style={{ borderColor: "#FA2D48", color: "#FA2D48" }}
                >
                  Load More
                </Button>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
