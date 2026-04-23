import { Dropdown, Tooltip } from "@tokimo/ui";
import { Ellipsis, Pause, Play, Volume2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useAppleMusic } from "../AppleMusicProvider";
import { useArtistNavigation } from "../hooks/useArtistNavigation";
import { formatArtworkUrl, formatDuration } from "../types";

function useContainerWidth(
  ref: React.RefObject<HTMLDivElement | null>,
): number {
  const [width, setWidth] = useState(9999);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      setWidth(entries[0].contentRect.width);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);
  return width;
}

interface TrackListProps {
  tracks: MusicKit.Resource[];
  showArtwork?: boolean;
  showAlbum?: boolean;
  showTrackNumber?: boolean;
  onPlayTrack?: (index: number) => void;
  containerType?: string;
  containerId?: string;
}

const APPLE_MUSIC_RED = "#FA2D48";

function isCurrentTrack(
  track: MusicKit.Resource,
  nowPlaying: MusicKit.MediaItem | null,
): boolean {
  if (!nowPlaying) return false;
  if (track.id === nowPlaying.id) return true;
  // When playing via catalogId, nowPlaying.id is the catalog ID
  // but the track in the list may use a library ID (i.xxx)
  const catalogId = track.attributes?.playParams?.catalogId;
  if (catalogId && catalogId === nowPlaying.id) return true;
  return false;
}

export function TrackList({
  tracks,
  showArtwork = false,
  showAlbum = false,
  showTrackNumber = false,
  onPlayTrack,
  containerType,
  containerId,
}: TrackListProps) {
  const {
    nowPlayingItem,
    playbackState,
    playNext,
    playLater,
    play,
    pause,
    navigateTo,
    api,
  } = useAppleMusic();

  const containerRef = useRef<HTMLDivElement>(null);
  const containerWidth = useContainerWidth(containerRef);

  // Responsive column visibility based on container width
  const effectiveShowAlbum = showAlbum && containerWidth >= 500;
  const effectiveShowDuration = containerWidth >= 320;

  const navigateToArtist = useArtistNavigation();

  const handleNavigateToAlbum = useCallback(
    async (albumName: string, track: MusicKit.Resource) => {
      // Try relationships first
      const albumRel = (track as unknown as Record<string, unknown>)
        .relationships as { albums?: { data?: { id: string }[] } } | undefined;
      const albumId = albumRel?.albums?.data?.[0]?.id;
      if (albumId) {
        navigateTo({ type: "album", id: albumId });
        return;
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

  return (
    <div className="w-full" ref={containerRef}>
      {/* Header */}
      <div
        className="grid items-center gap-3 border-b border-border-base px-3 py-1.5 text-xs font-medium uppercase tracking-wider text-[var(--text-tertiary)]"
        style={{
          gridTemplateColumns: buildGridColumns(
            showTrackNumber,
            showArtwork,
            effectiveShowAlbum,
            effectiveShowDuration,
          ),
        }}
      >
        {showTrackNumber && <span className="text-center">#</span>}
        {showArtwork && <span />}
        <span>Title</span>
        {effectiveShowAlbum && <span>Album</span>}
        {effectiveShowDuration && <span className="text-right">Duration</span>}
        <span />
      </div>

      {/* Tracks */}
      {tracks.map((track, index) => (
        <TrackRow
          key={track.id}
          track={track}
          index={index}
          showArtwork={showArtwork}
          showAlbum={effectiveShowAlbum}
          showTrackNumber={showTrackNumber}
          showDuration={effectiveShowDuration}
          isCurrent={isCurrentTrack(track, nowPlayingItem)}
          isPlaying={
            isCurrentTrack(track, nowPlayingItem) && playbackState === 2
          }
          onPlay={() => onPlayTrack?.(index)}
          onPause={pause}
          onResume={play}
          onPlayNext={playNext}
          onPlayLater={playLater}
          onArtistClick={navigateToArtist}
          onAlbumClick={handleNavigateToAlbum}
          containerType={containerType}
          containerId={containerId}
        />
      ))}
    </div>
  );
}

function buildGridColumns(
  showNumber: boolean,
  showArt: boolean,
  showAlbum: boolean,
  showDuration: boolean,
): string {
  const cols: string[] = [];
  if (showNumber) cols.push("40px");
  if (showArt) cols.push("40px");
  cols.push("1fr");
  if (showAlbum) cols.push("minmax(100px, 0.5fr)");
  if (showDuration) cols.push("60px");
  cols.push("40px");
  return cols.join(" ");
}

interface TrackRowProps {
  track: MusicKit.Resource;
  index: number;
  showArtwork: boolean;
  showAlbum: boolean;
  showTrackNumber: boolean;
  showDuration: boolean;
  isCurrent: boolean;
  isPlaying: boolean;
  onPlay: () => void;
  onPause: () => void;
  onResume: () => Promise<void>;
  onPlayNext: (options: MusicKit.SetQueueOptions) => Promise<void>;
  onPlayLater: (options: MusicKit.SetQueueOptions) => Promise<void>;
  onArtistClick: (name: string, track: MusicKit.Resource) => void;
  onAlbumClick: (name: string, track: MusicKit.Resource) => void;
  containerType?: string;
  containerId?: string;
}

function TrackRow({
  track,
  index,
  showArtwork,
  showAlbum,
  showTrackNumber,
  showDuration,
  isCurrent,
  isPlaying,
  onPlay,
  onPause,
  onResume,
  onPlayNext,
  onPlayLater,
  onArtistClick,
  onAlbumClick,
}: TrackRowProps) {
  const [hovered, setHovered] = useState(false);

  const attrs = track.attributes;
  const name = attrs?.name ?? "Unknown";
  const artistName = attrs?.artistName ?? "";
  const albumName = attrs?.albumName ?? "";
  const artwork = attrs?.artwork;
  const durationMs = attrs?.durationInMillis ?? 0;
  const contentRating = attrs?.contentRating;
  const trackNumber = attrs?.trackNumber;

  const handleClick = useCallback(() => {
    if (isCurrent && isPlaying) {
      onPause();
    } else if (isCurrent) {
      onResume();
    } else {
      onPlay();
    }
  }, [isCurrent, isPlaying, onPause, onResume, onPlay]);

  const handlePlayNext = useCallback(
    (e?: React.MouseEvent) => {
      e?.stopPropagation();
      onPlayNext({ song: track.id } as MusicKit.SetQueueOptions);
    },
    [onPlayNext, track.id],
  );

  const handlePlayLater = useCallback(
    (e?: React.MouseEvent) => {
      e?.stopPropagation();
      onPlayLater({ song: track.id } as MusicKit.SetQueueOptions);
    },
    [onPlayLater, track.id],
  );

  const artworkUrl = formatArtworkUrl(artwork, 40);

  return (
    // biome-ignore lint/a11y/useSemanticElements: Grid row with nested interactive controls prevents using <button>
    <div
      role="button"
      tabIndex={0}
      className={`group grid cursor-pointer items-center gap-3 rounded-md px-3 py-1.5 transition-colors ${
        isCurrent ? "bg-[#FA2D48]/10" : "hover:bg-[var(--fill-tertiary)]"
      }`}
      style={{
        gridTemplateColumns: buildGridColumns(
          showTrackNumber,
          showArtwork,
          showAlbum,
          showDuration,
        ),
      }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onDoubleClick={handleClick}
      onKeyDown={(e) => {
        if (e.key === "Enter") handleClick();
      }}
    >
      {/* Track number / Play indicator */}
      {showTrackNumber && (
        <div className="flex items-center justify-center">
          {hovered ? (
            <button
              type="button"
              onClick={handleClick}
              className="flex cursor-pointer items-center justify-center"
            >
              {isCurrent && isPlaying ? (
                <Pause
                  size={14}
                  fill={APPLE_MUSIC_RED}
                  color={APPLE_MUSIC_RED}
                />
              ) : (
                <Play
                  size={14}
                  fill={isCurrent ? APPLE_MUSIC_RED : "currentColor"}
                  color={isCurrent ? APPLE_MUSIC_RED : "currentColor"}
                />
              )}
            </button>
          ) : isCurrent ? (
            <Volume2 size={14} color={APPLE_MUSIC_RED} />
          ) : (
            <span className="text-sm text-[var(--text-tertiary)]">
              {trackNumber ?? index + 1}
            </span>
          )}
        </div>
      )}

      {/* Artwork */}
      {showArtwork && (
        <div className="relative h-10 w-10 shrink-0">
          {artworkUrl ? (
            <img
              src={artworkUrl}
              alt={name}
              width={40}
              height={40}
              className="h-10 w-10 rounded object-cover"
              loading="lazy"
            />
          ) : (
            <div className="flex h-10 w-10 items-center justify-center rounded bg-[var(--fill-tertiary)]">
              <Play size={14} className="text-[var(--text-tertiary)]" />
            </div>
          )}
          {hovered && !showTrackNumber && (
            <button
              type="button"
              onClick={handleClick}
              className="absolute inset-0 flex cursor-pointer items-center justify-center rounded bg-black/40"
            >
              {isCurrent && isPlaying ? (
                <Pause size={16} fill="white" color="white" />
              ) : (
                <Play size={16} fill="white" color="white" className="ml-0.5" />
              )}
            </button>
          )}
        </div>
      )}

      {/* Title + Artist */}
      <div className="min-w-0">
        <div className="flex items-center gap-1.5">
          <span
            className={`truncate text-sm font-medium ${
              isCurrent ? "text-[#FA2D48]" : "text-[var(--text-primary)]"
            }`}
          >
            {name}
          </span>
          {contentRating === "explicit" && (
            <span className="shrink-0 rounded bg-[var(--fill-tertiary)] px-1 py-px text-[10px] font-bold uppercase text-[var(--text-tertiary)]">
              E
            </span>
          )}
        </div>
        {artistName && (
          <button
            type="button"
            className="truncate text-left text-xs text-[var(--text-secondary)] cursor-pointer hover:underline"
            onClick={(e) => {
              e.stopPropagation();
              onArtistClick(artistName, track);
            }}
          >
            {artistName}
          </button>
        )}
      </div>

      {/* Album */}
      {showAlbum && (
        <button
          type="button"
          className="truncate text-left text-xs text-[var(--text-secondary)] cursor-pointer hover:underline"
          onClick={(e) => {
            e.stopPropagation();
            if (albumName) onAlbumClick(albumName, track);
          }}
        >
          {albumName}
        </button>
      )}

      {/* Duration */}
      {showDuration && (
        <span className="text-right text-xs text-[var(--text-tertiary)]">
          {durationMs > 0 ? formatDuration(durationMs) : "—"}
        </span>
      )}

      {/* Actions */}
      <div className="flex items-center justify-center">
        <Dropdown
          menu={{
            items: [
              {
                key: "play-next",
                label: "Play Next",
                onClick: () => handlePlayNext(),
              },
              {
                key: "play-later",
                label: "Play Later",
                onClick: () => handlePlayLater(),
              },
            ],
          }}
          trigger={["click"]}
          placement="bottomRight"
        >
          <Tooltip title="More">
            <button
              type="button"
              className={`cursor-pointer rounded p-1 text-[var(--text-tertiary)] transition-colors hover:text-[var(--text-primary)] ${
                hovered ? "opacity-100" : "opacity-0"
              }`}
              onClick={(e) => e.stopPropagation()}
            >
              <Ellipsis size={16} />
            </button>
          </Tooltip>
        </Dropdown>
      </div>
    </div>
  );
}
