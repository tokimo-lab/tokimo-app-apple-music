/**
 * MenuBarLyricsWidget — Apple Music lyrics display in the menu bar.
 *
 * Registered via manifest.menuBarWidget; rendered by MenuBar alongside
 * other menu bar widgets without the shell importing this file directly.
 */

import { useEffect, useRef } from "react";
import { useMediaSessionOptional } from "../shell/hooks";
import { useAppleMusicLyrics } from "./use-apple-music-lyrics";

function useStickyLyricText(
  lines: Array<{ text: string }>,
  currentIdx: number,
  resetKey: string | null | undefined,
  enabled: boolean,
): string | null {
  const lastTextRef = useRef<string | null>(null);
  const prevResetKeyRef = useRef<string | null | undefined>(resetKey);

  if (prevResetKeyRef.current !== resetKey) {
    prevResetKeyRef.current = resetKey;
    lastTextRef.current = null;
  }

  useEffect(() => {
    if (!enabled || lines.length === 0) {
      lastTextRef.current = null;
      return;
    }
    if (currentIdx >= 0 && currentIdx < lines.length) {
      lastTextRef.current = lines[currentIdx].text;
    }
  }, [currentIdx, enabled, lines]);

  if (!enabled) return null;
  if (currentIdx >= 0 && currentIdx < lines.length) {
    return lines[currentIdx].text;
  }
  return lastTextRef.current;
}

function KaraokeText({
  text,
  progressRef,
}: {
  text: string;
  progressRef: React.RefObject<number>;
}) {
  const clipRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    let raf: number;
    const tick = () => {
      if (clipRef.current) {
        const p = progressRef.current ?? 0;
        clipRef.current.style.clipPath = `inset(0 ${(1 - p) * 100}% 0 0)`;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [progressRef]);

  return (
    <span className="relative inline-block max-w-full truncate text-xs">
      <span aria-hidden className="invisible">
        {text}
      </span>
      <span className="absolute inset-0 truncate text-[var(--text-muted)]">
        {text}
      </span>
      <span
        ref={clipRef}
        className="absolute inset-0 truncate text-[var(--accent)]"
      >
        {text}
      </span>
    </span>
  );
}

export default function MenuBarLyricsWidget() {
  const mediaSession = useMediaSessionOptional();
  const activeSource = mediaSession?.activeSource;

  const isAppleMusicActive =
    activeSource?.type === "music" && activeSource.provider === "apple-music";
  const appleTrackId = isAppleMusicActive ? activeSource.trackId : null;
  const isAppleMusicPlaying = isAppleMusicActive && !!activeSource?.isPlaying;

  const { lines, currentIdx, progressRef, hasSyncedLyrics } =
    useAppleMusicLyrics(
      appleTrackId,
      isAppleMusicActive ? activeSource.getCurrentTime : () => 0,
      isAppleMusicPlaying,
    );

  const stickyText = useStickyLyricText(
    lines,
    currentIdx,
    appleTrackId,
    hasSyncedLyrics,
  );

  const showAppleLyrics =
    isAppleMusicActive && !!activeSource?.isPlaying && hasSyncedLyrics;

  if (!showAppleLyrics || !activeSource) return null;

  const fallbackText = [activeSource.title, activeSource.artist]
    .filter(Boolean)
    .join(" · ");

  return (
    <button
      type="button"
      className="mx-1 flex h-5 max-w-[260px] shrink items-center cursor-pointer overflow-hidden rounded px-1.5 transition-colors hover:bg-white/10"
      onClick={() =>
        activeSource.isPlaying ? activeSource.pause() : activeSource.play()
      }
      title={activeSource.isPlaying ? "暂停" : "播放"}
    >
      {stickyText ? (
        <KaraokeText text={stickyText} progressRef={progressRef} />
      ) : (
        <span className="max-w-full truncate text-xs text-[var(--accent)]">
          {fallbackText}
        </span>
      )}
    </button>
  );
}
