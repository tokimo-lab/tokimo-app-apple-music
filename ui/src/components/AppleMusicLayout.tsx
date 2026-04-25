import type { MenuBarConfig } from "@tokimo/sdk";
import { Spin } from "@tokimo/ui";
import { lazy, Suspense, useMemo } from "react";
import { useContainerWidth } from "../hooks/use-container-width";
import { useSidebarCollapsed } from "../hooks/use-sidebar-collapsed";
import { useMenuBar } from "../shell/hooks";
import { AppleMusicLogin } from "./AppleMusicLogin";
import { AppleMusicPlayer } from "./AppleMusicPlayer";
import { useAppleMusic } from "./AppleMusicProvider";
import { AppleMusicSidebar } from "./AppleMusicSidebar";

// Code-split each page
const BrowsePage = lazy(() => import("./pages/BrowsePage"));
const ForYouPage = lazy(() => import("./pages/ForYouPage"));
const SearchPage = lazy(() => import("./pages/SearchPage"));
const LibraryPage = lazy(() => import("./pages/LibraryPage"));
const AlbumPage = lazy(() => import("./pages/AlbumPage"));
const ArtistPage = lazy(() => import("./pages/ArtistPage"));
const PlaylistPage = lazy(() => import("./pages/PlaylistPage"));
const NowPlayingPage = lazy(() => import("./pages/NowPlayingPage"));

const PageFallback = (
  <div className="flex h-full items-center justify-center">
    <Spin />
  </div>
);

export function AppleMusicLayout() {
  const {
    currentPage,
    hasEverPlayed,
    isAuthorized,
    nowPlayingItem,
    playbackState,
    play,
    pause,
    skipToNext,
    skipToPrevious,
    toggleShuffle,
    cycleRepeatMode,
    shuffleMode,
    navigateTo,
    goBack,
    canGoBack,
  } = useAppleMusic();

  // Use numeric value (2) — MusicKit global may not be loaded yet
  const isPlaying = playbackState === 2;

  const trackName = nowPlayingItem?.attributes?.name;
  const trackArtist = nowPlayingItem?.attributes?.artistName;
  const nowPlayingLabel = trackName
    ? `♫ ${trackName}${trackArtist ? ` — ${trackArtist}` : ""}`
    : "Not Playing";

  const menuBarConfig = useMemo<MenuBarConfig | null>(() => {
    if (!isAuthorized) return null;
    return {
      menus: [
        {
          key: "now-playing",
          label: "Now Playing",
          items: [
            {
              key: "current-track",
              label: nowPlayingLabel,
              disabled: true,
            },
            { type: "divider" as const },
            {
              key: "show-now-playing",
              label: "Open Now Playing",
              onClick: () => navigateTo({ type: "now-playing" }),
              disabled: !nowPlayingItem,
            },
          ],
        },
        {
          key: "controls",
          label: "Controls",
          items: [
            {
              key: "play-pause",
              label: isPlaying ? "Pause" : "Play",
              shortcut: "Space",
              onClick: () => (isPlaying ? pause() : play()),
              disabled: !nowPlayingItem,
            },
            {
              key: "next",
              label: "Next",
              onClick: () => skipToNext(),
              disabled: !nowPlayingItem,
            },
            {
              key: "previous",
              label: "Previous",
              onClick: () => skipToPrevious(),
              disabled: !nowPlayingItem,
            },
            { type: "divider" as const },
            {
              key: "shuffle",
              label: shuffleMode ? "Shuffle: On" : "Shuffle: Off",
              onClick: toggleShuffle,
            },
            { key: "repeat", label: "Repeat", onClick: cycleRepeatMode },
          ],
        },
        {
          key: "navigate",
          label: "Navigate",
          items: [
            {
              key: "back",
              label: "Back",
              onClick: goBack,
              disabled: !canGoBack,
            },
            { type: "divider" as const },
            {
              key: "browse",
              label: "Browse",
              onClick: () => navigateTo({ type: "browse" }),
            },
            {
              key: "for-you",
              label: "For You",
              onClick: () => navigateTo({ type: "for-you" }),
            },
            {
              key: "search",
              label: "Search",
              onClick: () => navigateTo({ type: "search" }),
            },
            {
              key: "library",
              label: "Library",
              onClick: () => navigateTo({ type: "library" }),
            },
          ],
        },
      ],
      about: { description: "Apple Music", version: "1.0" },
    };
  }, [
    isAuthorized,
    isPlaying,
    nowPlayingItem,
    nowPlayingLabel,
    play,
    pause,
    skipToNext,
    skipToPrevious,
    toggleShuffle,
    cycleRepeatMode,
    shuffleMode,
    navigateTo,
    goBack,
    canGoBack,
  ]);

  useMenuBar(menuBarConfig);

  const [containerRef, containerWidth] = useContainerWidth();
  const { collapsed: sidebarCollapsed, onToggleCollapse } = useSidebarCollapsed(
    "apple-music",
    containerWidth > 0 && containerWidth < 720,
  );

  // Show login screen before user signs in with Apple ID
  if (!isAuthorized) {
    return <AppleMusicLogin />;
  }

  // Full-screen Now Playing view (no sidebar, no player bar)
  if (currentPage.type === "now-playing") {
    return (
      <div className="flex h-full flex-col">
        <Suspense fallback={PageFallback}>
          <NowPlayingPage />
        </Suspense>
      </div>
    );
  }

  return (
    <div ref={containerRef} className="flex h-full flex-col">
      <div className="relative flex flex-1 overflow-hidden">
        <AppleMusicSidebar
          collapsed={sidebarCollapsed}
          onToggleCollapse={onToggleCollapse}
        />
        <main className="flex-1 overflow-y-auto">
          <Suspense fallback={PageFallback}>
            <PageContent page={currentPage} />
          </Suspense>
        </main>
      </div>
      {hasEverPlayed && <AppleMusicPlayer />}
    </div>
  );
}

function PageContent({ page }: { page: { type: string } }) {
  switch (page.type) {
    case "browse":
      return <BrowsePage />;
    case "for-you":
      return <ForYouPage />;
    case "search":
      return <SearchPage />;
    case "library":
      return <LibraryPage />;
    case "album":
      return <AlbumPage />;
    case "artist":
      return <ArtistPage />;
    case "playlist":
      return <PlaylistPage />;
    default:
      return <BrowsePage />;
  }
}
