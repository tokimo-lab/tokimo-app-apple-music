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

interface PlaylistData {
  id: string;
  name: string;
  curatorName: string;
  description: string;
  artwork?: MusicKit.Artwork;
  trackCount: number;
  durationMs: number;
  tracks: MusicKit.Resource[];
}

export default function PlaylistPage() {
  const { api, currentPage, goBack, canGoBack, setQueueFromTracks } =
    useAppleMusic();
  const [playlist, setPlaylist] = useState<PlaylistData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const playlistId =
    currentPage.type === "playlist" ? currentPage.id : undefined;
  const isLibrary =
    currentPage.type === "playlist" ? currentPage.isLibrary : false;

  useEffect(() => {
    if (!playlistId) return;
    let cancelled = false;

    async function fetchPlaylist() {
      setLoading(true);
      setError(null);
      try {
        const path = isLibrary
          ? `/v1/me/library/playlists/${playlistId}`
          : `/v1/catalog/${getStorefront()}/playlists/${playlistId}`;

        const res = await api(path, { include: "tracks" });
        const resource = res?.data?.data?.[0] ?? null;

        if (cancelled) return;

        if (!resource) {
          setError("Playlist not found");
          return;
        }

        const attrs = resource.attributes;
        const trackList = resource.relationships?.tracks?.data ?? [];

        const totalDuration = trackList.reduce((sum, t) => {
          return sum + (t.attributes?.durationInMillis ?? 0);
        }, 0);

        setPlaylist({
          id: resource.id,
          name: attrs?.name ?? "Unknown Playlist",
          curatorName: attrs?.curatorName ?? "",
          description:
            attrs?.editorialNotes?.short ??
            attrs?.editorialNotes?.standard ??
            attrs?.description?.short ??
            attrs?.description?.standard ??
            "",
          artwork: attrs?.artwork,
          trackCount: trackList.length,
          durationMs: totalDuration,
          tracks: trackList,
        });
      } catch (err) {
        if (!cancelled) {
          setError(
            err instanceof Error ? err.message : "Failed to load playlist",
          );
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    fetchPlaylist();
    return () => {
      cancelled = true;
    };
  }, [playlistId, isLibrary, api]);

  async function handlePlayAll(): Promise<void> {
    if (!playlist) return;
    await setQueueFromTracks(playlist.tracks, 0);
  }

  async function handleShuffle(): Promise<void> {
    if (!playlist) return;
    try {
      const mk = MusicKit.getInstance();
      mk.shuffleMode = MusicKit.PlayerShuffleMode.songs;
    } catch {
      // ignore if shuffle mode API unavailable
    }
    await setQueueFromTracks(playlist.tracks, 0);
  }

  async function handlePlayTrack(index: number): Promise<void> {
    if (!playlist) return;
    await setQueueFromTracks(playlist.tracks, index);
  }

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spin spinning tip="Loading playlist…" />
      </div>
    );
  }

  if (error || !playlist) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3">
        <p className="text-sm text-[var(--text-secondary)]">
          {error ?? "Playlist not found"}
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
        <ArtworkImage
          artwork={playlist.artwork}
          size={250}
          alt={playlist.name}
        />

        <div className="flex min-w-0 flex-col justify-end gap-2">
          <h1 className="text-3xl font-bold text-[var(--text-primary)]">
            {playlist.name}
          </h1>

          {playlist.curatorName && (
            <p className="text-base text-[var(--text-secondary)]">
              {playlist.curatorName}
            </p>
          )}

          <div className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
            <span>
              {playlist.trackCount}{" "}
              {playlist.trackCount === 1 ? "song" : "songs"}
            </span>
            {playlist.durationMs > 0 && (
              <>
                <span>·</span>
                <span>{formatDuration(playlist.durationMs)}</span>
              </>
            )}
          </div>

          {playlist.description && (
            <p className="mt-1 line-clamp-3 text-sm text-[var(--text-tertiary)]">
              {playlist.description}
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
          tracks={playlist.tracks}
          showArtwork
          onPlayTrack={handlePlayTrack}
          containerType="playlist"
          containerId={playlist.id}
        />
      </div>
    </div>
  );
}
