import { Slider, Tooltip } from "@tokimo/ui";
import {
  Loader2,
  Pause,
  Play,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useAppleMusic } from "./AppleMusicProvider";
import { formatArtworkUrl, formatDurationSeconds } from "./types";

const APPLE_MUSIC_RED = "#FA2D48";

export function AppleMusicPlayer() {
  const {
    nowPlayingItem,
    playbackState,
    currentPlaybackTime,
    currentPlaybackDuration,
    volume,
    shuffleMode,
    repeatMode,
    isBuffering,
    play,
    pause,
    skipToNext,
    skipToPrevious,
    seekToTime,
    setVolume,
    toggleShuffle,
    cycleRepeatMode,
    navigateTo,
  } = useAppleMusic();

  const isPlaying = playbackState === MusicKit.PlaybackStates.playing;
  const artwork = nowPlayingItem?.attributes?.artwork;
  const artworkUrl = formatArtworkUrl(artwork, 96);
  const title = nowPlayingItem?.attributes?.name ?? "Not Playing";
  const artist = nowPlayingItem?.attributes?.artistName ?? "";
  const progress =
    currentPlaybackDuration > 0
      ? (currentPlaybackTime / currentPlaybackDuration) * 100
      : 0;

  const handlePlayPause = useCallback(() => {
    if (isPlaying) {
      pause();
    } else {
      play();
    }
  }, [isPlaying, pause, play]);

  const playerRef = useRef<HTMLDivElement>(null);
  const [isNarrow, setIsNarrow] = useState(false);
  const [isMini, setIsMini] = useState(false);

  useEffect(() => {
    const el = playerRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 800;
      setIsNarrow(w < 720);
      setIsMini(w < 400);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  return (
    <div
      ref={playerRef}
      className="flex h-20 flex-shrink-0 items-center border-t border-border-base bg-[var(--bg-glass)] px-4"
    >
      {" "}
      {/* Left: Now playing info — click to expand */}
      <button
        type="button"
        className={`flex ${isMini ? "flex-shrink-0" : isNarrow ? "w-36 flex-shrink-0" : "w-56 flex-shrink-0"} cursor-pointer items-center gap-3 rounded-lg p-1 transition-colors hover:bg-[var(--fill-tertiary)]`}
        onClick={() => navigateTo({ type: "now-playing" })}
      >
        {artworkUrl ? (
          <img
            src={artworkUrl}
            alt=""
            className="h-12 w-12 flex-shrink-0 rounded-md object-cover"
          />
        ) : (
          <div className="flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-md bg-[var(--fill-tertiary)]">
            <Play className="h-5 w-5 text-[var(--text-tertiary)]" />
          </div>
        )}
        <div className={`min-w-0 text-left ${isMini ? "hidden" : ""}`}>
          <div className="truncate text-sm font-medium text-[var(--text-primary)]">
            {title}
          </div>
          {artist && (
            <div className="truncate text-xs text-[var(--text-tertiary)]">
              {isBuffering ? (
                <span className="animate-pulse">Loading…</span>
              ) : (
                artist
              )}
            </div>
          )}
        </div>
      </button>
      {/* Center: Controls + progress */}
      <div className="flex flex-1 flex-col items-center gap-1">
        <div className="flex items-center gap-3">
          {!isNarrow && (
            <Tooltip title="Shuffle">
              <button
                type="button"
                onClick={toggleShuffle}
                className="cursor-pointer rounded p-1 transition-colors hover:bg-[var(--fill-tertiary)]"
                style={shuffleMode ? { color: APPLE_MUSIC_RED } : undefined}
              >
                <Shuffle
                  className="h-4 w-4 text-[var(--text-secondary)]"
                  style={shuffleMode ? { color: APPLE_MUSIC_RED } : undefined}
                />
              </button>
            </Tooltip>
          )}

          <Tooltip title="Previous">
            <button
              type="button"
              onClick={() => skipToPrevious()}
              className="cursor-pointer rounded p-1 text-[var(--text-primary)] transition-colors hover:bg-[var(--fill-tertiary)]"
            >
              <SkipBack className="h-5 w-5" />
            </button>
          </Tooltip>

          <button
            type="button"
            onClick={handlePlayPause}
            className="flex h-9 w-9 cursor-pointer items-center justify-center rounded-full text-white transition-transform hover:scale-105"
            style={{ backgroundColor: APPLE_MUSIC_RED }}
          >
            {isBuffering ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : isPlaying ? (
              <Pause className="h-4 w-4" />
            ) : (
              <Play className="h-4 w-4 translate-x-0.5" />
            )}
          </button>

          <Tooltip title="Next">
            <button
              type="button"
              onClick={() => skipToNext()}
              className="cursor-pointer rounded p-1 text-[var(--text-primary)] transition-colors hover:bg-[var(--fill-tertiary)]"
            >
              <SkipForward className="h-5 w-5" />
            </button>
          </Tooltip>

          {!isNarrow && (
            <Tooltip
              title={
                repeatMode === 1
                  ? "Repeat One"
                  : repeatMode === 2
                    ? "Repeat All"
                    : "Repeat"
              }
            >
              <button
                type="button"
                onClick={cycleRepeatMode}
                className="cursor-pointer rounded p-1 transition-colors hover:bg-[var(--fill-tertiary)]"
              >
                {repeatMode === 1 ? (
                  <Repeat1
                    className="h-4 w-4"
                    style={{ color: APPLE_MUSIC_RED }}
                  />
                ) : (
                  <Repeat
                    className="h-4 w-4 text-[var(--text-secondary)]"
                    style={
                      repeatMode === 2 ? { color: APPLE_MUSIC_RED } : undefined
                    }
                  />
                )}
              </button>
            </Tooltip>
          )}
        </div>

        {/* Progress bar */}
        {!isMini && (
          <div className="flex w-full max-w-lg items-center gap-2">
            <span className="w-10 text-right text-xs tabular-nums text-[var(--text-tertiary)]">
              {formatDurationSeconds(currentPlaybackTime)}
            </span>
            <ProgressBar
              progress={progress}
              duration={currentPlaybackDuration}
              onSeek={seekToTime}
            />
            <span className="w-10 text-xs tabular-nums text-[var(--text-tertiary)]">
              {formatDurationSeconds(currentPlaybackDuration)}
            </span>
          </div>
        )}
      </div>
      {/* Right: Volume — hidden on narrow */}
      {!isNarrow && (
        <div className="flex w-40 items-center justify-end gap-2">
          <button
            type="button"
            onClick={() => setVolume(volume > 0 ? 0 : 0.5)}
            className="cursor-pointer rounded p-1 text-[var(--text-secondary)] hover:bg-[var(--fill-tertiary)]"
          >
            {volume === 0 ? (
              <VolumeX className="h-4 w-4" />
            ) : (
              <Volume2 className="h-4 w-4" />
            )}
          </button>
          <Slider
            min={0}
            max={1}
            step={0.01}
            value={volume}
            onChange={(v) => setVolume(v)}
            accentColor="#FA2D48"
            size="small"
            className="w-24"
          />
        </div>
      )}
    </div>
  );
}

function ProgressBar({
  progress,
  duration,
  onSeek,
}: {
  progress: number;
  duration: number;
  onSeek: (time: number) => Promise<void>;
}) {
  const barRef = useRef<HTMLDivElement>(null);

  const handleClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      const bar = barRef.current;
      if (!bar || duration <= 0) return;
      const rect = bar.getBoundingClientRect();
      const ratio = Math.max(
        0,
        Math.min(1, (e.clientX - rect.left) / rect.width),
      );
      onSeek(ratio * duration);
    },
    [duration, onSeek],
  );

  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: progress bar interaction is mouse-only
    <div
      ref={barRef}
      role="slider"
      tabIndex={0}
      aria-valuenow={Math.round(progress)}
      aria-valuemin={0}
      aria-valuemax={100}
      className="relative h-1 flex-1 cursor-pointer rounded-full bg-[var(--fill-tertiary)]"
      onClick={handleClick}
    >
      <div
        className="absolute left-0 top-0 h-full rounded-full"
        style={{
          width: `${progress}%`,
          backgroundColor: APPLE_MUSIC_RED,
        }}
      />
    </div>
  );
}
