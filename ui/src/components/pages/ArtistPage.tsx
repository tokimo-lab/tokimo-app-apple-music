import { Button, Spin } from "@tokimo/ui";
import { ArrowLeft } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useAppleMusic } from "../AppleMusicProvider";
import { ArtworkImage } from "../components/ArtworkImage";
import { MediaItemCard } from "../components/MediaItemCard";
import { TrackList } from "../components/TrackList";

function getStorefront(): string {
  try {
    return MusicKit.getInstance().storefrontCountryCode || "us";
  } catch {
    return "us";
  }
}

interface ArtistData {
  id: string;
  name: string;
  genreNames: string[];
  artwork?: MusicKit.Artwork;
  topSongs: MusicKit.Resource[];
  albums: MusicKit.Resource[];
}

export default function ArtistPage() {
  const {
    api,
    currentPage,
    goBack,
    canGoBack,
    navigateTo,
    setQueue,
    setQueueFromTracks,
  } = useAppleMusic();
  const [artist, setArtist] = useState<ArtistData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const artistId = currentPage.type === "artist" ? currentPage.id : undefined;

  useEffect(() => {
    if (!artistId) return;
    let cancelled = false;

    async function fetchArtist() {
      setLoading(true);
      setError(null);
      try {
        let catalogArtistId = artistId;

        // Library IDs (r.xxx) can't be used with catalog endpoints — resolve first
        const isLibraryId = artistId ? /^[rl]\./.test(artistId) : false;
        if (isLibraryId) {
          const libRes = await api(`/v1/me/library/artists/${artistId}`, {
            include: "catalog",
          });
          if (cancelled) return;
          const libResource = libRes?.data?.data?.[0];
          const catalogData = libResource?.relationships?.catalog?.data?.[0];
          if (catalogData?.id) {
            catalogArtistId = catalogData.id;
          } else {
            // No catalog equivalent — show basic library info
            if (!libResource) {
              setError("Artist not found");
              return;
            }
            setArtist({
              id: libResource.id,
              name: libResource.attributes?.name ?? "Unknown Artist",
              genreNames: [],
              artwork: libResource.attributes?.artwork,
              topSongs: [],
              albums: [],
            });
            return;
          }
        }

        const sf = getStorefront();
        const res = await api(`/v1/catalog/${sf}/artists/${catalogArtistId}`, {
          include: "albums",
          views: "top-songs",
        });
        if (cancelled) return;

        const resource = res?.data?.data?.[0];
        if (!resource) {
          setError("Artist not found");
          return;
        }

        const attrs = resource.attributes;
        const albumsData = resource.relationships?.albums?.data ?? [];

        // top-songs come from the "views" key, not standard relationships
        const views = (resource as unknown as Record<string, unknown>).views as
          | Record<string, { data?: MusicKit.Resource[] }>
          | undefined;
        const topSongsData = views?.["top-songs"]?.data ?? [];

        setArtist({
          id: resource.id,
          name: attrs?.name ?? "Unknown Artist",
          genreNames: attrs?.genreNames ?? [],
          artwork: attrs?.artwork,
          topSongs: topSongsData,
          albums: albumsData,
        });
      } catch (err) {
        if (!cancelled) {
          setError(
            err instanceof Error ? err.message : "Failed to load artist",
          );
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    fetchArtist();
    return () => {
      cancelled = true;
    };
  }, [artistId, api]);

  const [showAllSongs, setShowAllSongs] = useState(false);

  const INITIAL_SONG_COUNT = 6;
  const visibleTopSongs = useMemo(
    () =>
      showAllSongs
        ? (artist?.topSongs ?? [])
        : (artist?.topSongs ?? []).slice(0, INITIAL_SONG_COUNT),
    [artist?.topSongs, showAllSongs],
  );

  const handlePlayTopSong = useCallback(
    async (index: number) => {
      if (visibleTopSongs.length === 0) return;
      await setQueueFromTracks(visibleTopSongs, index);
    },
    [visibleTopSongs, setQueueFromTracks],
  );

  const handlePlayAlbum = useCallback(
    async (albumId: string) => {
      await setQueue({ album: albumId, startPlaying: true });
    },
    [setQueue],
  );

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spin spinning tip="Loading artist…" />
      </div>
    );
  }

  if (error || !artist) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3">
        <p className="text-sm text-[var(--text-secondary)]">
          {error ?? "Artist not found"}
        </p>
        {canGoBack && (
          <Button
            variant="link"
            icon={<ArrowLeft size={16} />}
            onClick={goBack}
            style={{ color: "#FA2D48" }}
          >
            Go Back
          </Button>
        )}
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      {/* Hero Header */}
      <div className="relative">
        {artist.artwork ? (
          <div className="relative h-64 w-full overflow-hidden">
            <ArtworkImage
              artwork={artist.artwork}
              alt={artist.name}
              size={1200}
              className="h-full w-full object-cover !rounded-none"
            />
            <div className="absolute inset-0 bg-gradient-to-t from-black/70 to-transparent" />
            {canGoBack && (
              <div className="absolute top-3 left-4">
                <Button
                  variant="text"
                  size="small"
                  icon={<ArrowLeft size={16} />}
                  onClick={goBack}
                  style={{ color: "#fff" }}
                >
                  Back
                </Button>
              </div>
            )}
            <div className="absolute bottom-0 left-0 px-6 pb-6">
              <h1 className="text-4xl font-bold text-white drop-shadow-lg">
                {artist.name}
              </h1>
              {artist.genreNames.length > 0 && (
                <p className="mt-1 text-base text-white/80">
                  {artist.genreNames.join(", ")}
                </p>
              )}
            </div>
          </div>
        ) : (
          <div className="px-6 pt-4 pb-6">
            {canGoBack && (
              <Button
                variant="text"
                size="small"
                icon={<ArrowLeft size={16} />}
                onClick={goBack}
                style={{ color: "#FA2D48" }}
                className="mb-2"
              >
                Back
              </Button>
            )}
            <h1 className="text-4xl font-bold text-[var(--text-primary)]">
              {artist.name}
            </h1>
            {artist.genreNames.length > 0 && (
              <p className="mt-1 text-base text-[var(--text-secondary)]">
                {artist.genreNames.join(", ")}
              </p>
            )}
          </div>
        )}
      </div>

      {/* Top Songs */}
      {artist.topSongs.length > 0 && (
        <section className="px-6 pt-6 pb-8">
          <h2 className="mb-4 text-xl font-bold text-[var(--text-primary)]">
            Top Songs
          </h2>
          <TrackList
            tracks={visibleTopSongs}
            showArtwork
            onPlayTrack={handlePlayTopSong}
          />
          {artist.topSongs.length > INITIAL_SONG_COUNT && (
            <button
              type="button"
              className="mt-3 cursor-pointer text-sm font-medium hover:underline"
              style={{ color: "#FA2D48" }}
              onClick={() => setShowAllSongs((v) => !v)}
            >
              {showAllSongs
                ? "Show Less"
                : `Show All (${artist.topSongs.length})`}
            </button>
          )}
        </section>
      )}

      {/* Albums */}
      {artist.albums.length > 0 && (
        <section className="px-6 pb-8">
          <h2 className="mb-4 text-xl font-bold text-[var(--text-primary)]">
            Albums
          </h2>
          <div className="flex flex-wrap gap-4">
            {artist.albums.map((album) => (
              <MediaItemCard
                key={album.id}
                item={album}
                type="album"
                onClick={() => navigateTo({ type: "album", id: album.id })}
                onPlay={() => handlePlayAlbum(album.id)}
              />
            ))}
          </div>
        </section>
      )}

      {artist.topSongs.length === 0 && artist.albums.length === 0 && (
        <div className="flex h-40 items-center justify-center text-sm text-[var(--text-tertiary)]">
          No content available for this artist
        </div>
      )}
    </div>
  );
}
