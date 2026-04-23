import { Spin } from "@tokimo/ui";
import { useCallback, useEffect, useRef } from "react";
import * as centralEngine from "../../shell/engine-ref";
import { useAppleMusic } from "../AppleMusicProvider";
import { useAppleMusicLyrics } from "../use-apple-music-lyrics";

export function LyricsView() {
  const { nowPlayingItem, currentPlaybackTime, seekToTime } = useAppleMusic();
  const containerRef = useRef<HTMLDivElement>(null);
  const activeLineRef = useRef<HTMLButtonElement>(null);
  const userScrolledRef = useRef(false);
  const scrollTimeoutRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const songId = nowPlayingItem?.id;
  const catalogId = nowPlayingItem?.attributes?.playParams?.catalogId ?? songId;

  // Read directly from central engine for smoother lyrics sync (60fps RAF),
  // falling back to React state for non-engine playback.
  const getTime = useCallback(() => {
    if (centralEngine.getActiveProvider() === "apple-music") {
      return centralEngine.getCurrentTime();
    }
    return currentPlaybackTime;
  }, [currentPlaybackTime]);

  const {
    lines: lyrics,
    currentIdx: activeIndex,
    isLoading: loading,
    noLyrics,
  } = useAppleMusicLyrics(catalogId, getTime);

  // Auto-scroll to active line
  useEffect(() => {
    if (
      activeIndex >= 0 &&
      activeLineRef.current &&
      containerRef.current &&
      !userScrolledRef.current
    ) {
      activeLineRef.current.scrollIntoView({
        behavior: "smooth",
        block: "center",
      });
    }
  }, [activeIndex]);

  // Detect user scroll
  const handleScroll = useCallback(() => {
    userScrolledRef.current = true;
    clearTimeout(scrollTimeoutRef.current);
    scrollTimeoutRef.current = setTimeout(() => {
      userScrolledRef.current = false;
    }, 5000);
  }, []);

  const handleLineClick = useCallback(
    (line: { begin: number }) => {
      seekToTime(line.begin);
    },
    [seekToTime],
  );

  if (!songId) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[var(--text-tertiary)]">
        Play a song to see lyrics
      </div>
    );
  }

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spin spinning tip="Loading lyrics…" />
      </div>
    );
  }

  if (noLyrics || lyrics.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[var(--text-tertiary)]">
        Lyrics not available
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="h-full overflow-y-auto px-6 py-8"
      onScroll={handleScroll}
    >
      <div className="mx-auto flex max-w-lg flex-col gap-1">
        {lyrics.map((line, i) => {
          const isActive = i === activeIndex;
          const isPast = activeIndex >= 0 && i < activeIndex;
          return (
            <button
              key={line.begin}
              ref={isActive ? activeLineRef : undefined}
              type="button"
              onClick={() => handleLineClick(line)}
              className={`cursor-pointer rounded-md px-3 py-2 text-left text-lg font-semibold leading-relaxed transition-all duration-300 hover:bg-white/5 ${
                isActive
                  ? "text-[var(--text-primary)] scale-[1.02]"
                  : isPast
                    ? "text-[var(--text-tertiary)] opacity-60"
                    : "text-[var(--text-secondary)] opacity-80"
              }`}
            >
              {line.text}
            </button>
          );
        })}
        <div className="h-40" />
      </div>
    </div>
  );
}
