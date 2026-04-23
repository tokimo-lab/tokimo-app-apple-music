import { Button, Spin } from "@tokimo/ui";
import { ArrowLeft, Play, Shuffle } from "lucide-react";
import { useEffect, useState } from "react";
import { useAppleMusic } from "../AppleMusicProvider";
import { ArtworkImage } from "../components/ArtworkImage";
import { TrackList } from "../components/TrackList";
import { formatDuration } from "../types";

function getStorefront(): string {
  try {
    return MusicKit.getInstance().storefrontCountryCode || "us";
  } catch {
    return "us";
  }
}

interface AlbumData {
  id: string;
  name: string;
  artistName: string;
  artistId?: string;
  artwork?: MusicKit.Artwork;
  releaseDate?: string;
  genreNames: string[];
  trackCount: number;
  durationMs: number;
  tracks: MusicKit.Resource[];
  editorialNotes?: string;
}

export default function AlbumPage() {
  const {
    api,
    currentPage,
    goBack,
    canGoBack,
    setQueueFromTracks,
    navigateTo,
  } = useAppleMusic();
  const [album, setAlbum] = useState<AlbumData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const albumId = currentPage.type === "album" ? currentPage.id : undefined;
  const isLibrary =
    currentPage.type === "album" ? currentPage.isLibrary : false;

  useEffect(() => {
    if (!albumId) return;
    let cancelled = false;

    async function fetchAlbum() {
      setLoading(true);
      setError(null);
      try {
        const path = isLibrary
          ? `/v1/me/library/albums/${albumId}`
          : `/v1/catalog/${getStorefront()}/albums/${albumId}`;

        const res = await api(path, { include: "tracks,artists" });
        if (cancelled) return;

        const resource = res?.data?.data?.[0];
        if (!resource) {
          setError("Album not found");
          return;
        }

        const attrs = resource.attributes;
        const trackList = resource.relationships?.tracks?.data ?? [];
        const artistId = resource.relationships?.artists?.data?.[0]?.id;

        const totalDuration = trackList.reduce((sum, t) => {
          return sum + (t.attributes?.durationInMillis ?? 0);
        }, 0);

        setAlbum({
          id: resource.id,
          name: attrs?.name ?? "Unknown Album",
          artistName: attrs?.artistName ?? "",
          artistId,
          artwork: attrs?.artwork,
          releaseDate: attrs?.releaseDate,
          genreNames: attrs?.genreNames ?? [],
          trackCount: trackList.length,
          durationMs: totalDuration,
          tracks: trackList,
          editorialNotes: attrs?.editorialNotes?.short,
        });
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Failed to load album");
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    fetchAlbum();
    return () => {
      cancelled = true;
    };
  }, [albumId, isLibrary, api]);

  async function handlePlayAll(): Promise<void> {
    if (!album) return;
    await setQueueFromTracks(album.tracks, 0);
  }

  async function handleShuffle(): Promise<void> {
    if (!album) return;
    try {
      const mk = MusicKit.getInstance();
      mk.shuffleMode = MusicKit.PlayerShuffleMode.songs;
    } catch {
      // ignore if shuffle mode API unavailable
    }
    await setQueueFromTracks(album.tracks, 0);
  }

  async function handlePlayTrack(index: number): Promise<void> {
    if (!album) return;
    await setQueueFromTracks(album.tracks, index);
  }

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spin spinning tip="Loading album…" />
      </div>
    );
  }

  if (error || !album) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3">
        <p className="text-sm text-[var(--text-secondary)]">
          {error ?? "Album not found"}
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

  const year = album.releaseDate?.split("-")[0];

  return (
    <div className="h-full overflow-y-auto">
      {canGoBack && (
        <div className="px-6 pt-4">
          <Button
            variant="text"
            size="small"
            icon={<ArrowLeft size={16} />}
            onClick={goBack}
            style={{ color: "#FA2D48" }}
          >
            Back
          </Button>
        </div>
      )}

      {/* Header */}
      <div className="flex gap-6 px-6 pt-4 pb-6">
        <ArtworkImage artwork={album.artwork} size={250} alt={album.name} />

        <div className="flex min-w-0 flex-col justify-end gap-2">
          <h1 className="text-3xl font-bold text-[var(--text-primary)]">
            {album.name}
          </h1>

          {album.artistId ? (
            <button
              type="button"
              className="text-left text-xl text-[#FA2D48] cursor-pointer hover:underline"
              onClick={() =>
                navigateTo({ type: "artist", id: album.artistId! })
              }
            >
              {album.artistName}
            </button>
          ) : (
            <p className="text-xl text-[#FA2D48]">{album.artistName}</p>
          )}

          <div className="flex flex-wrap items-center gap-2 text-sm text-[var(--text-secondary)]">
            {album.genreNames[0] && <span>{album.genreNames[0]}</span>}
            {year && (
              <>
                <span>·</span>
                <span>{year}</span>
              </>
            )}
            <span>·</span>
            <span>
              {album.trackCount} {album.trackCount === 1 ? "song" : "songs"}
            </span>
            {album.durationMs > 0 && (
              <>
                <span>·</span>
                <span>{formatDuration(album.durationMs)}</span>
              </>
            )}
          </div>

          {album.editorialNotes && (
            <p className="mt-1 line-clamp-2 text-sm text-[var(--text-tertiary)]">
              {album.editorialNotes}
            </p>
          )}

          <div className="mt-3 flex gap-3">
            <Button
              variant="primary"
              shape="round"
              icon={<Play size={16} fill="white" />}
              onClick={handlePlayAll}
              style={{ backgroundColor: "#FA2D48", borderColor: "#FA2D48" }}
            >
              Play
            </Button>
            <Button
              variant="default"
              shape="round"
              icon={<Shuffle size={16} />}
              onClick={handleShuffle}
              style={{ borderColor: "#FA2D48", color: "#FA2D48" }}
            >
              Shuffle
            </Button>
          </div>
        </div>
      </div>

      {/* Tracks */}
      <div className="px-6 pb-8">
        <TrackList
          tracks={album.tracks}
          showTrackNumber
          onPlayTrack={handlePlayTrack}
          containerType="album"
          containerId={album.id}
        />
      </div>
    </div>
  );
}
