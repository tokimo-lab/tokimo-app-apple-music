import {
  AppSidebar,
  type AppSidebarSection,
  Button,
  Tooltip,
} from "@tokimo/ui";
import {
  Compass,
  Disc3,
  ListMusic,
  MicVocal,
  Music,
  PanelLeft,
  PanelLeftClose,
  Search,
  Star,
} from "lucide-react";
import { useMemo } from "react";
import { useThemeCore } from "../shell/hooks";
import { useAppleMusic } from "./AppleMusicProvider";
import type { AppleMusicPage } from "./types";

const APPLE_MUSIC_RED = "#FA2D48";

function getSelectedKey(page: AppleMusicPage): string {
  if (page.type === "search") return "search";
  if (page.type === "browse") return "browse";
  if (page.type === "for-you") return "for-you";
  if (page.type === "library") return `library-${page.tab ?? "songs"}`;
  return "";
}

function keyToPage(key: string): AppleMusicPage | null {
  switch (key) {
    case "search":
      return { type: "search" };
    case "browse":
      return { type: "browse" };
    case "for-you":
      return { type: "for-you" };
    case "library-songs":
      return { type: "library", tab: "songs" };
    case "library-albums":
      return { type: "library", tab: "albums" };
    case "library-artists":
      return { type: "library", tab: "artists" };
    case "library-playlists":
      return { type: "library", tab: "playlists" };
    default:
      return null;
  }
}

export function AppleMusicSidebar({
  collapsed = false,
  onToggleCollapse,
}: {
  collapsed?: boolean;
  onToggleCollapse?: () => void;
}) {
  const { currentPage, navigateTo, isAuthorized, authorize } = useAppleMusic();
  const { isMacStyle } = useThemeCore();

  const selectedKey = getSelectedKey(currentPage);

  function handleNavigate(key: string) {
    const page = keyToPage(key);
    if (page) navigateTo(page);
  }

  const sections: AppSidebarSection[] = useMemo(
    () => [
      {
        items: [
          {
            key: "search",
            label: "Search",
            icon: <Search className="h-4 w-4" />,
          },
        ],
      },
      {
        label: "Apple Music",
        items: [
          {
            key: "browse",
            label: "Browse",
            icon: <Compass className="h-4 w-4" />,
          },
          ...(isAuthorized
            ? [
                {
                  key: "for-you",
                  label: "For You",
                  icon: <Star className="h-4 w-4" />,
                },
              ]
            : []),
        ],
      },
      ...(isAuthorized
        ? [
            {
              label: "Library",
              items: [
                {
                  key: "library-songs",
                  label: "Songs",
                  icon: <Music className="h-4 w-4" />,
                },
                {
                  key: "library-albums",
                  label: "Albums",
                  icon: <Disc3 className="h-4 w-4" />,
                },
                {
                  key: "library-artists",
                  label: "Artists",
                  icon: <MicVocal className="h-4 w-4" />,
                },
                {
                  key: "library-playlists",
                  label: "Playlists",
                  icon: <ListMusic className="h-4 w-4" />,
                },
              ],
            },
          ]
        : []),
    ],
    [isAuthorized],
  );

  return (
    <AppSidebar
      width={240}
      collapsed={collapsed}
      topInset={isMacStyle ? 36 : undefined}
      style={
        {
          "--accent": APPLE_MUSIC_RED,
          "--accent-subtle": "rgba(250, 45, 72, 0.12)",
          "--accent-subtle-hover": "rgba(250, 45, 72, 0.16)",
        } as React.CSSProperties
      }
      sections={sections}
      activeKey={selectedKey}
      onSelect={handleNavigate}
      footer={
        !isAuthorized ? (
          collapsed ? (
            <div className="flex flex-col items-center gap-1">
              <Tooltip title="Sign In" placement="right">
                <button
                  type="button"
                  onClick={() => authorize()}
                  className="flex h-9 w-9 cursor-pointer items-center justify-center rounded-lg transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                  style={{ color: APPLE_MUSIC_RED }}
                >
                  <Music className="h-4 w-4" />
                </button>
              </Tooltip>
              <Tooltip title="展开侧边栏" placement="right">
                <button
                  type="button"
                  onClick={onToggleCollapse}
                  className="flex h-9 w-9 cursor-pointer items-center justify-center rounded-lg text-fg-muted transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                >
                  <PanelLeft className="h-4 w-4" />
                </button>
              </Tooltip>
            </div>
          ) : (
            <div className="flex items-center gap-1">
              <Button
                variant="primary"
                block
                icon={<Music className="h-4 w-4" />}
                onClick={() => authorize()}
                style={{
                  backgroundColor: APPLE_MUSIC_RED,
                  borderColor: APPLE_MUSIC_RED,
                }}
              >
                Sign In
              </Button>
              <Tooltip title="收起侧边栏">
                <button
                  type="button"
                  onClick={onToggleCollapse}
                  className="flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-lg text-fg-muted transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                >
                  <PanelLeftClose className="h-4 w-4" />
                </button>
              </Tooltip>
            </div>
          )
        ) : collapsed ? (
          <div className="flex justify-center">
            <Tooltip title="展开侧边栏" placement="right">
              <button
                type="button"
                onClick={onToggleCollapse}
                className="flex h-9 w-9 cursor-pointer items-center justify-center rounded-lg text-fg-muted transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              >
                <PanelLeft className="h-4 w-4" />
              </button>
            </Tooltip>
          </div>
        ) : (
          <div className="flex justify-end">
            <Tooltip title="收起侧边栏">
              <button
                type="button"
                onClick={onToggleCollapse}
                className="flex h-8 w-8 cursor-pointer items-center justify-center rounded-lg text-fg-muted transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              >
                <PanelLeftClose className="h-4 w-4" />
              </button>
            </Tooltip>
          </div>
        )
      }
    />
  );
}
