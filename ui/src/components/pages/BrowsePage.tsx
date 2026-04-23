import { Button, Spin } from "@tokimo/ui";
import { useEffect, useState } from "react";
import { useAppleMusic } from "../AppleMusicProvider";
import { MediaItemCard } from "../components/MediaItemCard";
import { TrackList } from "../components/TrackList";
import { useArtistNavigation } from "../hooks/useArtistNavigation";

function getStorefront(): string {
  try {
    return MusicKit.getInstance().storefrontCountryCode || "us";
  } catch {
    return "us";
  }
}

interface ChartsData {
  songs: MusicKit.Resource[];
  albums: MusicKit.Resource[];
  playlists: MusicKit.Resource[];
}

export default function BrowsePage() {
  const { api, navigateTo, setQueue, setQueueFromTracks } = useAppleMusic();
  const navigateToArtist = useArtistNavigation();
  const [charts, setCharts] = useState<ChartsData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function fetchCharts() {
      setLoading(true);
      setError(null);
      try {
        const sf = getStorefront();
        const res = await api(`/v1/catalog/${sf}/charts`, {
          types: "songs,albums,playlists",
          limit: 50,
        });

        if (cancelled) return;

        const results = res?.data?.results;
        // Charts API returns arrays of chart groups, e.g. results.songs = [{chart, data, name}]
        type ChartGroup = { data: MusicKit.Resource[] };
        const songsData =
          (results?.songs as unknown as ChartGroup[])?.[0]?.data ?? [];
        const albumsData =
          (results?.albums as unknown as ChartGroup[])?.[0]?.data ?? [];
        const playlistsData =
          (results?.playlists as unknown as ChartGroup[])?.[0]?.data ?? [];

        setCharts({
          songs: songsData,
          albums: albumsData,
          playlists: playlistsData,
        });
      } catch (err) {
        if (!cancelled) {
          setError(
            err instanceof Error ? err.message : "Failed to load charts",
          );
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    fetchCharts();
    return () => {
      cancelled = true;
    };
  }, [api]);

  async function handlePlayTrack(index: number): Promise<void> {
    if (!charts?.songs.length) return;
    await setQueueFromTracks(charts.songs, index);
  }

  async function handlePlayAlbum(albumId: string): Promise<void> {
    await setQueue({ album: albumId, startPlaying: true });
  }

  async function handlePlayPlaylist(playlistId: string): Promise<void> {
    await setQueue({ playlist: playlistId, startPlaying: true });
  }

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spin spinning tip="Loading charts…" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3">
        <p className="text-sm text-[var(--text-secondary)]">{error}</p>
        <Button
          variant="primary"
          shape="round"
          onClick={() => window.location.reload()}
          style={{ backgroundColor: "#FA2D48", borderColor: "#FA2D48" }}
        >
          Retry
        </Button>
      </div>
    );
  }

  if (!charts) return null;

  return (
    <div className="space-y-8 p-6">
      {/* Top Songs */}
      {charts.songs.length > 0 && (
        <section>
          <h2 className="mb-4 text-xl font-bold text-[var(--text-primary)]">
            Top Songs
          </h2>
          <TrackList
            tracks={charts.songs}
            showArtwork
            showAlbum
            onPlayTrack={handlePlayTrack}
          />
        </section>
      )}

      {/* Top Albums */}
      {charts.albums.length > 0 && (
        <section>
          <h2 className="mb-4 text-xl font-bold text-[var(--text-primary)]">
            Top Albums
          </h2>
          <div className="flex flex-wrap gap-4">
            {charts.albums.map((album) => (
              <MediaItemCard
                key={album.id}
                item={album}
                type="album"
                onClick={() => navigateTo({ type: "album", id: album.id })}
                onPlay={() => handlePlayAlbum(album.id)}
                onSubtitleClick={() => {
                  const name = album.attributes?.artistName;
                  if (name) navigateToArtist(name, album);
                }}
              />
            ))}
          </div>
        </section>
      )}

      {/* Top Playlists */}
      {charts.playlists.length > 0 && (
        <section>
          <h2 className="mb-4 text-xl font-bold text-[var(--text-primary)]">
            Top Playlists
          </h2>
          <div className="flex flex-wrap gap-4">
            {charts.playlists.map((playlist) => (
              <MediaItemCard
                key={playlist.id}
                item={playlist}
                type="playlist"
                onClick={() =>
                  navigateTo({ type: "playlist", id: playlist.id })
                }
                onPlay={() => handlePlayPlaylist(playlist.id)}
              />
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
