import { Input, Spin, Tabs } from "@tokimo/ui";
import { Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
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

interface SearchResults {
  songs: MusicKit.Resource[];
  albums: MusicKit.Resource[];
  artists: MusicKit.Resource[];
  playlists: MusicKit.Resource[];
}

const EMPTY_RESULTS: SearchResults = {
  songs: [],
  albums: [],
  artists: [],
  playlists: [],
};

type TabKey = "songs" | "albums" | "artists" | "playlists";

const TABS: { key: TabKey; label: string }[] = [
  { key: "songs", label: "Songs" },
  { key: "albums", label: "Albums" },
  { key: "artists", label: "Artists" },
  { key: "playlists", label: "Playlists" },
];

export default function SearchPage() {
  const { api, navigateTo, currentPage, setQueue, setQueueFromTracks } =
    useAppleMusic();
  const navigateToArtist = useArtistNavigation();
  const initialQuery =
    currentPage.type === "search" ? (currentPage.query ?? "") : "";
  const [query, setQuery] = useState(initialQuery);
  const [results, setResults] = useState<SearchResults>(EMPTY_RESULTS);
  const [loading, setLoading] = useState(false);
  const [activeTab, setActiveTab] = useState<TabKey>("songs");
  const [searched, setSearched] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Run initial search if restoring from persisted query
  // biome-ignore lint/correctness/useExhaustiveDependencies: run once on mount only
  useEffect(() => {
    if (initialQuery) void doSearch(initialQuery);
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, []);

  async function doSearch(term: string): Promise<void> {
    if (!term.trim()) {
      setResults(EMPTY_RESULTS);
      setSearched(false);
      return;
    }

    setLoading(true);
    setSearched(true);
    try {
      const sf = getStorefront();
      const res = await api(`/v1/catalog/${sf}/search`, {
        term: term.trim(),
        types: "songs,albums,artists,playlists",
        limit: 25,
      });

      const r = res?.data?.results;
      setResults({
        songs: r?.songs?.data ?? [],
        albums: r?.albums?.data ?? [],
        artists: r?.artists?.data ?? [],
        playlists: r?.playlists?.data ?? [],
      });
    } catch {
      setResults(EMPTY_RESULTS);
    } finally {
      setLoading(false);
    }
  }

  function handleInputChange(e: React.ChangeEvent<HTMLInputElement>): void {
    const value = e.target.value;
    setQuery(value);
    navigateTo({ type: "search", query: value || undefined });

    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      void doSearch(value);
    }, 400);
  }

  function clearQuery(): void {
    setQuery("");
    setResults(EMPTY_RESULTS);
    setSearched(false);
    navigateTo({ type: "search" });
    inputRef.current?.focus();
  }

  async function handlePlaySong(index: number): Promise<void> {
    if (results.songs.length === 0) return;
    const song = results.songs[index];
    if (!song) return;
    await setQueueFromTracks([song], 0);
  }

  const hasResults =
    results.songs.length > 0 ||
    results.albums.length > 0 ||
    results.artists.length > 0 ||
    results.playlists.length > 0;

  const tabContent: Record<TabKey, React.ReactNode> = {
    songs: (
      <TrackList
        tracks={results.songs}
        showArtwork
        showAlbum
        onPlayTrack={handlePlaySong}
      />
    ),
    albums: (
      <div className="flex flex-wrap gap-4">
        {results.albums.map((album) => (
          <MediaItemCard
            key={album.id}
            item={album}
            type="album"
            onClick={() => navigateTo({ type: "album", id: album.id })}
            onPlay={() => setQueue({ album: album.id, startPlaying: true })}
            onSubtitleClick={
              album.attributes?.artistName
                ? () => navigateToArtist(album.attributes!.artistName!, album)
                : undefined
            }
          />
        ))}
      </div>
    ),
    artists: (
      <div className="flex flex-wrap gap-4">
        {results.artists.map((artist) => (
          <MediaItemCard
            key={artist.id}
            item={artist}
            type="artist"
            onClick={() => navigateTo({ type: "artist", id: artist.id })}
          />
        ))}
      </div>
    ),
    playlists: (
      <div className="flex flex-wrap gap-4">
        {results.playlists.map((playlist) => (
          <MediaItemCard
            key={playlist.id}
            item={playlist}
            type="playlist"
            onClick={() => navigateTo({ type: "playlist", id: playlist.id })}
            onPlay={() =>
              setQueue({ playlist: playlist.id, startPlaying: true })
            }
          />
        ))}
      </div>
    ),
  };

  const visibleTabItems = TABS.filter((tab) => results[tab.key].length > 0).map(
    (tab) => ({
      key: tab.key,
      label: tab.label,
      children: tabContent[tab.key],
    }),
  );

  return (
    <div className="flex h-full flex-col">
      {/* Search Input */}
      <div className="border-b border-border-base px-6 py-4">
        <div className="mx-auto max-w-2xl">
          <Input
            ref={inputRef}
            value={query}
            onChange={handleInputChange}
            placeholder="Search Apple Music…"
            size="large"
            prefix={<Search size={18} />}
            suffix={
              query ? (
                <button
                  type="button"
                  onClick={clearQuery}
                  className="cursor-pointer text-[var(--text-tertiary)] hover:text-[var(--text-primary)]"
                >
                  <X size={14} />
                </button>
              ) : undefined
            }
            className="w-full"
          />
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto">
        {!searched && !loading && (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-[var(--text-tertiary)]">
            <Search size={48} strokeWidth={1} />
            <p className="text-sm">
              Search for songs, albums, artists, and playlists
            </p>
          </div>
        )}

        {loading && (
          <div className="flex h-full items-center justify-center">
            <Spin spinning tip="Searching…" />
          </div>
        )}

        {searched && !loading && !hasResults && (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-[var(--text-tertiary)]">
            <p className="text-sm">No results found for "{query}"</p>
          </div>
        )}

        {searched && !loading && hasResults && (
          <div className="p-6">
            <Tabs
              type="pill"
              activeKey={activeTab}
              onChange={(key) => setActiveTab(key as TabKey)}
              items={visibleTabItems}
              className="[--accent:#FA2D48] mb-6"
              destroyInactiveTabPane
            />
          </div>
        )}
      </div>
    </div>
  );
}
