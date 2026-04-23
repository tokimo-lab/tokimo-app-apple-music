import { ListMusic, Pause, Play, Volume2 } from "lucide-react";
import { useCallback } from "react";
import { useAppleMusic } from "../AppleMusicProvider";
import { formatArtworkUrl, formatDuration } from "../types";

export function QueueView() {
  const {
    queueItems,
    queuePosition,
    nowPlayingItem,
    playbackState,
    skipToQueueIndex,
  } = useAppleMusic();

  const isPlaying = playbackState === MusicKit.PlaybackStates.playing;

  const handlePlayIndex = useCallback(
    async (index: number) => {
      await skipToQueueIndex(index);
    },
    [skipToQueueIndex],
  );

  if (queueItems.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-[var(--text-tertiary)]">
        <ListMusic size={40} strokeWidth={1} />
        <p className="text-sm">Queue is empty</p>
      </div>
    );
  }

  const upNext = queueItems.slice(queuePosition + 1);

  return (
    <div className="h-full overflow-y-auto px-4 py-4">
      {/* Now Playing */}
      {nowPlayingItem && (
        <section className="mb-6">
          <h3 className="mb-2 px-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-tertiary)]">
            Now Playing
          </h3>
          <QueueItem
            item={nowPlayingItem}
            isCurrent
            isPlaying={isPlaying}
            onClick={() => {}}
          />
        </section>
      )}

      {/* Up Next */}
      {upNext.length > 0 && (
        <section>
          <h3 className="mb-2 px-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-tertiary)]">
            Up Next · {upNext.length} {upNext.length === 1 ? "song" : "songs"}
          </h3>
          {upNext.map((item, i) => (
            <QueueItem
              key={item.id}
              item={item}
              isCurrent={false}
              isPlaying={false}
              onClick={() => handlePlayIndex(queuePosition + 1 + i)}
            />
          ))}
        </section>
      )}
    </div>
  );
}

function QueueItem({
  item,
  isCurrent,
  isPlaying,
  onClick,
}: {
  item: MusicKit.MediaItem;
  isCurrent: boolean;
  isPlaying: boolean;
  onClick: () => void;
}) {
  const attrs = item.attributes;
  const name = attrs?.name ?? "Unknown";
  const artist = attrs?.artistName ?? "";
  const artwork = attrs?.artwork;
  const artworkUrl = formatArtworkUrl(artwork, 48);
  const durationMs = attrs?.durationInMillis ?? 0;

  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full cursor-pointer items-center gap-3 rounded-lg px-2 py-2 text-left transition-colors ${
        isCurrent ? "bg-white/10" : "hover:bg-white/5"
      }`}
    >
      <div className="relative h-10 w-10 shrink-0">
        {artworkUrl ? (
          <img
            src={artworkUrl}
            alt={name}
            className="h-10 w-10 rounded object-cover"
            loading="lazy"
          />
        ) : (
          <div className="flex h-10 w-10 items-center justify-center rounded bg-[var(--fill-tertiary)]">
            <Play size={14} className="text-[var(--text-tertiary)]" />
          </div>
        )}
        {isCurrent && (
          <div className="absolute inset-0 flex items-center justify-center rounded bg-black/40">
            {isPlaying ? (
              <Volume2 size={14} color="#FA2D48" />
            ) : (
              <Pause size={14} color="#FA2D48" />
            )}
          </div>
        )}
      </div>

      <div className="min-w-0 flex-1">
        <p
          className={`truncate text-sm font-medium ${
            isCurrent ? "text-[#FA2D48]" : "text-[var(--text-primary)]"
          }`}
        >
          {name}
        </p>
        {artist && (
          <p className="truncate text-xs text-[var(--text-secondary)]">
            {artist}
          </p>
        )}
      </div>

      {durationMs > 0 && (
        <span className="shrink-0 text-xs tabular-nums text-[var(--text-tertiary)]">
          {formatDuration(durationMs)}
        </span>
      )}
    </button>
  );
}
