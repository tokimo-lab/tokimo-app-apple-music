import { Slider, Tooltip } from "@tokimo/ui";
import {
  ChevronDown,
  Disc3,
  List,
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
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useThemeCore } from "../../shell/hooks";
import { useAppleMusic } from "../AppleMusicProvider";
import { LyricsView } from "../components/LyricsView";
import { QueueView } from "../components/QueueView";
import { useAlbumNavigation } from "../hooks/useAlbumNavigation";
import { useArtistNavigation } from "../hooks/useArtistNavigation";
import { formatArtworkUrl, formatDurationSeconds } from "../types";

const APPLE_MUSIC_RED = "#FA2D48";

type NowPlayingPalette = {
  bg: string;
  text1: string;
  text2: string;
  control: string;
  controlTrack: string;
  progressFill: string;
};

// ── Color helpers (Apple Music style) ────────────────────────────────────────

function hexToRgb(hex: string): [number, number, number] {
  const n = Number.parseInt(hex.replace("#", ""), 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

function rgbToHex(r: number, g: number, b: number): string {
  return `#${((1 << 24) | (r << 16) | (g << 8) | b).toString(16).slice(1)}`;
}

/** Relative luminance (WCAG formula) */
function luminance([r, g, b]: [number, number, number]): number {
  const [rs, gs, bs] = [r, g, b].map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * rs + 0.7152 * gs + 0.0722 * bs;
}

/** WCAG contrast ratio between two colors */
function contrastRatio(
  c1: [number, number, number],
  c2: [number, number, number],
): number {
  const l1 = luminance(c1);
  const l2 = luminance(c2);
  const lighter = Math.max(l1, l2);
  const darker = Math.min(l1, l2);
  return (lighter + 0.05) / (darker + 0.05);
}

/** Lighten or darken a color by mixing with white/black */
function adjustBrightness(
  [r, g, b]: [number, number, number],
  factor: number,
): [number, number, number] {
  if (factor > 0) {
    // Lighten: mix towards white
    return [
      Math.round(r + (255 - r) * factor),
      Math.round(g + (255 - g) * factor),
      Math.round(b + (255 - b) * factor),
    ];
  }
  // Darken: mix towards black
  const f = 1 + factor;
  return [Math.round(r * f), Math.round(g * f), Math.round(b * f)];
}

const DEFAULT_COLORS: NowPlayingPalette = {
  bg: "var(--bg-base)",
  text1: "var(--text-primary)",
  text2: "var(--text-secondary)",
  control: "rgba(255, 255, 255, 0.88)",
  controlTrack: "rgba(255, 255, 255, 0.16)",
  progressFill: "rgba(255, 255, 255, 0.96)",
};

async function loadProxiedArtwork(url: string): Promise<Blob> {
  console.log("[AppleMusicColors] artwork proxy start", { url });
  const response = await fetch("/api/apps/apple-music/proxy", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ targetUrl: url }),
  });

  if (!response.ok) {
    throw new Error(`artwork proxy request failed: ${response.status}`);
  }

  const blob = await response.blob();
  console.log("[AppleMusicColors] artwork proxy success", {
    url,
    status: response.status,
    size: blob.size,
    type: blob.type,
  });
  return blob;
}

async function loadBlobImage(blob: Blob): Promise<HTMLImageElement> {
  const objectUrl = URL.createObjectURL(blob);

  return new Promise<HTMLImageElement>((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      resolve(img);
    };
    img.onerror = () => {
      reject(new Error("proxied artwork image load failed"));
    };
    img.src = objectUrl;
  }).finally(() => {
    URL.revokeObjectURL(objectUrl);
  });
}

/** Is this color near-black? (luminance < 0.03) */
function isNearBlack(hex: string | undefined): boolean {
  if (!hex) return true;
  return luminance(hexToRgb(hex)) < 0.03;
}

/**
 * Build final NowPlaying color palette from a resolved background RGB.
 * Derives text colors with WCAG contrast enforcement.
 */
function normalizeBackgroundTone(
  bg: [number, number, number],
): [number, number, number] {
  let next = bg;
  let bgLuminance = luminance(next);

  // Keep the page in a dark, tinted range instead of letting vibrant covers
  // turn the whole surface bright.
  while (bgLuminance > 0.1) {
    next = adjustBrightness(next, -0.08);
    bgLuminance = luminance(next);
  }

  while (bgLuminance < 0.045) {
    next = adjustBrightness(next, 0.05);
    bgLuminance = luminance(next);
  }

  return next;
}

function buildPalette(bg: [number, number, number]): NowPlayingPalette {
  bg = normalizeBackgroundTone(bg);

  let text1: [number, number, number] = [245, 245, 248];
  let text2: [number, number, number] = [188, 188, 198];
  let control = adjustBrightness(bg, 0.1);
  let controlTrack = adjustBrightness(bg, 0.04);
  let progressFill = adjustBrightness(bg, 0.18);

  const minContrast = 4.5;
  if (contrastRatio(bg, text1) < minContrast) {
    for (let i = 0; i < 20; i++) {
      text1 = adjustBrightness(text1, 0.04);
      if (contrastRatio(bg, text1) >= minContrast) break;
    }
  }
  if (contrastRatio(bg, text2) < 3) {
    for (let i = 0; i < 20; i++) {
      text2 = adjustBrightness(text2, 0.04);
      if (contrastRatio(bg, text2) >= 3) break;
    }
  }

  if (contrastRatio(bg, control) < 1.12) {
    control = adjustBrightness(bg, 0.12);
  }
  if (contrastRatio(bg, controlTrack) < 1.04) {
    controlTrack = adjustBrightness(bg, 0.06);
  }
  if (contrastRatio(bg, progressFill) < 1.2) {
    progressFill = adjustBrightness(bg, 0.22);
  }

  return {
    bg: rgbToHex(...bg),
    text1: rgbToHex(...text1),
    text2: rgbToHex(...text2),
    control: rgbToHex(...control),
    controlTrack: rgbToHex(...controlTrack),
    progressFill: rgbToHex(...progressFill),
  };
}

/**
 * Derive colors from artwork metadata.
 * Near-black backgrounds are gently lifted so we always have a usable fallback.
 */
function deriveFromMetadata(artwork: {
  bgColor?: string;
  textColor1?: string;
  textColor2?: string;
}): NowPlayingPalette {
  if (!artwork.bgColor) return DEFAULT_COLORS;

  const bg = hexToRgb(artwork.bgColor);
  return buildPalette(bg);
}

/**
 * Extract dominant color from artwork via backend proxy and a small off-screen canvas.
 * Samples a 50×50 scaled-down version and picks the most common non-grey color bucket.
 */
async function extractDominantColor(
  url: string,
): Promise<[number, number, number]> {
  const blob = await loadProxiedArtwork(url);
  const img = await loadBlobImage(blob);
  const size = 50;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    throw new Error("no 2d context");
  }

  ctx.drawImage(img, 0, 0, size, size);
  const { data } = ctx.getImageData(0, 0, size, size);

  const buckets = new Map<
    string,
    { r: number; g: number; b: number; count: number }
  >();
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i];
    const g = data[i + 1];
    const b = data[i + 2];
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    if (max < 30) continue;
    if (min > 225) continue;
    const saturation = max === 0 ? 0 : (max - min) / max;
    const key = `${(r >> 3) << 3},${(g >> 3) << 3},${(b >> 3) << 3}`;
    const existing = buckets.get(key);
    const weight = saturation > 0.15 ? 3 : 1;
    if (existing) {
      existing.r += r * weight;
      existing.g += g * weight;
      existing.b += b * weight;
      existing.count += weight;
    } else {
      buckets.set(key, {
        r: r * weight,
        g: g * weight,
        b: b * weight,
        count: weight,
      });
    }
  }

  if (buckets.size === 0) {
    throw new Error("no usable artwork colors found");
  }

  let best = { r: 40, g: 40, b: 40, count: 0 };
  for (const bucket of buckets.values()) {
    if (bucket.count > best.count) best = bucket;
  }

  return [
    Math.round(best.r / best.count),
    Math.round(best.g / best.count),
    Math.round(best.b / best.count),
  ];
}

/** Hook: derive NowPlaying colors, falling back to image extraction when metadata is near-black */
function useNowPlayingColors(
  artwork: MusicKit.Artwork | undefined,
): NowPlayingPalette {
  const [imageColors, setImageColors] = useState<{
    bg: string;
    text1: string;
    text2: string;
    control: string;
    controlTrack: string;
    progressFill: string;
  } | null>(null);

  // Try metadata first
  const bgHex = artwork?.bgColor;
  const t1Hex = artwork?.textColor1;
  const t2Hex = artwork?.textColor2;
  const metadataColors = useMemo(
    () =>
      deriveFromMetadata({
        bgColor: bgHex,
        textColor1: t1Hex,
        textColor2: t2Hex,
      }),
    [bgHex, t1Hex, t2Hex],
  );

  // Try artwork extraction when Apple omits colors entirely or resolves to near-black.
  const needsExtraction = !!artwork && (!bgHex || isNearBlack(bgHex));
  const artworkUrl = needsExtraction ? formatArtworkUrl(artwork, 100) : "";

  useEffect(() => {
    if (!artwork) return;
    console.log("[AppleMusicColors] metadata", {
      bgColor: bgHex ?? null,
      textColor1: t1Hex ?? null,
      textColor2: t2Hex ?? null,
      artworkUrl: artworkUrl || formatArtworkUrl(artwork, 100),
      needsExtraction,
    });
  }, [artwork, artworkUrl, bgHex, t1Hex, t2Hex, needsExtraction]);

  useEffect(() => {
    if (!artworkUrl) {
      setImageColors(null);
      return;
    }

    console.log("[AppleMusicColors] artwork extraction start", {
      artworkUrl,
      metadataColors,
    });
    setImageColors(null);
    let cancelled = false;

    extractDominantColor(artworkUrl)
      .then((dominant) => {
        if (cancelled) return;
        // Ensure extracted color isn't too dark either
        if (luminance(dominant) < 0.03) {
          dominant = adjustBrightness(dominant, 0.15);
        }
        const resolved = buildPalette(dominant);
        console.log("[AppleMusicColors] artwork", {
          artworkUrl,
          dominant: rgbToHex(...dominant),
          resolved,
        });
        setImageColors(resolved);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        console.warn("[AppleMusicColors] artwork extraction failed", {
          artworkUrl,
          error,
          metadataColors,
        });
        setImageColors(null);
      });

    return () => {
      cancelled = true;
    };
  }, [artworkUrl, metadataColors]);

  useEffect(() => {
    const resolved = imageColors ?? metadataColors;
    console.log("[AppleMusicColors] applied", {
      source: imageColors ? "artwork" : "metadata",
      resolved,
    });
  }, [imageColors, metadataColors]);

  return imageColors ?? metadataColors;
}

export default function NowPlayingPage() {
  const {
    nowPlayingItem,
    playbackState,
    currentPlaybackTime,
    currentPlaybackDuration,
    volume,
    shuffleMode,
    repeatMode,
    play,
    pause,
    skipToNext,
    skipToPrevious,
    seekToTime,
    setVolume,
    toggleShuffle,
    cycleRepeatMode,
    goBack,
    canGoBack,
  } = useAppleMusic();
  const { isMacStyle } = useThemeCore();
  const navigateToArtist = useArtistNavigation();
  const navigateToAlbum = useAlbumNavigation();

  const [showQueue, setShowQueue] = useState(false);

  // ResizeObserver-based responsive layout (breakpoints based on window width, not viewport)
  const containerRef = useRef<HTMLDivElement>(null);
  const [isNarrow, setIsNarrow] = useState(false);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      setIsNarrow((entries[0]?.contentRect.width ?? 800) < 720);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const isPlaying = playbackState === MusicKit.PlaybackStates.playing;
  const attrs = nowPlayingItem?.attributes;
  const artwork = attrs?.artwork;
  const artworkUrl = formatArtworkUrl(artwork, 600);
  const title = attrs?.name ?? "Not Playing";
  const artist = attrs?.artistName ?? "";
  const albumName = attrs?.albumName ?? "";
  const progress =
    currentPlaybackDuration > 0
      ? (currentPlaybackTime / currentPlaybackDuration) * 100
      : 0;

  const {
    bg: bgColor,
    text1: textColor1,
    text2: textColor2,
    controlTrack,
    progressFill,
  } = useNowPlayingColors(artwork);

  const handlePlayPause = useCallback(() => {
    if (isPlaying) pause();
    else play();
  }, [isPlaying, pause, play]);

  const handleBack = useCallback(() => {
    if (canGoBack) goBack();
  }, [canGoBack, goBack]);

  return (
    <div
      ref={containerRef}
      className="relative flex h-full flex-col overflow-hidden transition-colors duration-700"
      style={{ backgroundColor: bgColor }}
    >
      {/* Blurred artwork background */}
      {artworkUrl && (
        <div
          className="pointer-events-none absolute inset-0 scale-110 bg-cover bg-center opacity-50 blur-md transition-all duration-700"
          style={{ backgroundImage: `url(${artworkUrl})` }}
        />
      )}
      {/* Darkening overlay so text stays readable */}
      <div className="pointer-events-none absolute inset-0 bg-black/40" />

      {/* All content sits above the background layers */}
      <div className="relative z-10 flex flex-1 flex-col overflow-hidden">
        {/* Top bar — spacer for macOS traffic lights */}
        {isMacStyle && <div className="h-10 px-6 py-3" />}

        {/* Main content */}
        {showQueue ? (
          <div className="flex-1 overflow-hidden">
            <QueueView />
          </div>
        ) : (
          <div className="flex flex-1 gap-6 overflow-hidden px-6 pb-2">
            {/* Spinning disc + artwork (left side on wide, hidden on narrow) */}
            <div
              className={`flex shrink-0 items-center justify-center ${isNarrow ? "hidden" : ""}`}
            >
              <SpinningDisc
                artworkUrl={artworkUrl}
                isPlaying={isPlaying}
                textColor2={textColor2}
                size={280}
              />
            </div>

            {/* Lyrics (right side on wide, full-height on narrow) */}
            <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
              <div className="flex-1 overflow-hidden">
                <LyricsView />
              </div>
            </div>
          </div>
        )}

        {/* Bottom controls */}
        <div className="px-6 pb-5 pt-3">
          {/* Song info */}
          <div className="mb-2 text-center">
            <h2
              className="truncate text-lg font-bold"
              style={{ color: textColor1 }}
            >
              {title}
            </h2>
            <p className="truncate text-sm" style={{ color: textColor2 }}>
              {artist && (
                <button
                  type="button"
                  className="cursor-pointer hover:underline"
                  onClick={() =>
                    navigateToArtist(
                      artist,
                      nowPlayingItem as MusicKit.Resource | undefined,
                    )
                  }
                >
                  {artist}
                </button>
              )}
              {artist && albumName && " — "}
              {albumName && (
                <button
                  type="button"
                  className="cursor-pointer hover:underline"
                  onClick={() =>
                    navigateToAlbum(
                      albumName,
                      nowPlayingItem as MusicKit.Resource | undefined,
                    )
                  }
                >
                  {albumName}
                </button>
              )}
            </p>
          </div>

          {/* Progress */}
          <div className="mb-3 flex items-center gap-3">
            <span
              className="w-10 text-right text-xs tabular-nums"
              style={{ color: textColor2 }}
            >
              {formatDurationSeconds(currentPlaybackTime)}
            </span>
            <NowPlayingProgressBar
              progress={progress}
              duration={currentPlaybackDuration}
              onSeek={seekToTime}
              trackColor={controlTrack}
              fillColor={progressFill}
            />
            <span
              className="w-10 text-xs tabular-nums"
              style={{ color: textColor2 }}
            >
              {formatDurationSeconds(currentPlaybackDuration)}
            </span>
          </div>

          {/* Transport controls */}
          <div className="flex items-center justify-center gap-5">
            <Tooltip title="Shuffle">
              <button
                type="button"
                onClick={toggleShuffle}
                className="cursor-pointer rounded p-1.5 transition-colors hover:bg-white/10"
              >
                <Shuffle
                  size={18}
                  style={{
                    color: shuffleMode ? APPLE_MUSIC_RED : textColor2,
                  }}
                />
              </button>
            </Tooltip>

            <button
              type="button"
              onClick={() => skipToPrevious()}
              className="cursor-pointer rounded p-1.5 transition-colors hover:bg-white/10"
            >
              <SkipBack
                size={24}
                fill={textColor1}
                style={{ color: textColor1 }}
              />
            </button>

            <button
              type="button"
              onClick={handlePlayPause}
              className="flex h-14 w-14 cursor-pointer items-center justify-center rounded-full transition-transform hover:scale-105"
              style={{ backgroundColor: textColor1 }}
            >
              {isPlaying ? (
                <Pause size={24} fill={bgColor} style={{ color: bgColor }} />
              ) : (
                <Play
                  size={24}
                  fill={bgColor}
                  style={{ color: bgColor }}
                  className="translate-x-0.5"
                />
              )}
            </button>

            <button
              type="button"
              onClick={() => skipToNext()}
              className="cursor-pointer rounded p-1.5 transition-colors hover:bg-white/10"
            >
              <SkipForward
                size={24}
                fill={textColor1}
                style={{ color: textColor1 }}
              />
            </button>

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
                className="cursor-pointer rounded p-1.5 transition-colors hover:bg-white/10"
              >
                {repeatMode === 1 ? (
                  <Repeat1 size={18} style={{ color: APPLE_MUSIC_RED }} />
                ) : (
                  <Repeat
                    size={18}
                    style={{
                      color: repeatMode === 2 ? APPLE_MUSIC_RED : textColor2,
                    }}
                  />
                )}
              </button>
            </Tooltip>
          </div>

          {/* Volume + Queue toggle */}
          <div className="mt-2 flex items-center justify-between gap-2">
            <div className="flex items-center gap-1">
              <Tooltip title="Close Now Playing">
                <button
                  type="button"
                  onClick={handleBack}
                  className="cursor-pointer rounded-full p-1.5 transition-colors hover:bg-white/10"
                  style={{ color: textColor1 }}
                >
                  <ChevronDown size={16} />
                </button>
              </Tooltip>
              <Tooltip title={showQueue ? "Now Playing" : "Queue"}>
                <button
                  type="button"
                  onClick={() => setShowQueue((v) => !v)}
                  className="cursor-pointer rounded-full p-1.5 transition-colors hover:bg-white/10"
                  style={{
                    backgroundColor: showQueue ? controlTrack : "transparent",
                    color: textColor1,
                  }}
                >
                  <List size={16} />
                </button>
              </Tooltip>
            </div>
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => setVolume(volume > 0 ? 0 : 0.5)}
                className="cursor-pointer rounded p-1 hover:bg-white/10"
              >
                {volume === 0 ? (
                  <VolumeX size={16} style={{ color: textColor2 }} />
                ) : (
                  <Volume2 size={16} style={{ color: textColor2 }} />
                )}
              </button>
              <Slider
                min={0}
                max={1}
                step={0.01}
                value={volume}
                onChange={(v) => setVolume(v)}
                accentColor="#FA2D48"
                className="w-32"
                style={{ backgroundColor: controlTrack }}
              />
            </div>
          </div>
        </div>
      </div>
      {/* /z-10 content wrapper */}
    </div>
  );
}

function SpinningDisc({
  artworkUrl,
  isPlaying,
  textColor2,
  size,
}: {
  artworkUrl: string;
  isPlaying: boolean;
  textColor2: string;
  size: number;
}) {
  const innerSize = Math.round(size * 0.52);

  return (
    <div
      className="relative shrink-0 rounded-full shadow-2xl"
      style={{ width: size, height: size }}
    >
      {/* Vinyl grooves background */}
      <div
        className={`absolute inset-0 rounded-full bg-gradient-to-br from-zinc-900 via-zinc-800 to-zinc-900 ${isPlaying ? "animate-[spin_4s_linear_infinite]" : ""}`}
      >
        {/* Groove rings */}
        <div className="absolute inset-[12%] rounded-full border border-white/[0.06]" />
        <div className="absolute inset-[20%] rounded-full border border-white/[0.04]" />
        <div className="absolute inset-[28%] rounded-full border border-white/[0.06]" />

        {/* Center artwork */}
        <div
          className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 overflow-hidden rounded-full shadow-inner"
          style={{ width: innerSize, height: innerSize }}
        >
          {artworkUrl ? (
            <img
              src={artworkUrl}
              alt=""
              className="h-full w-full object-cover"
            />
          ) : (
            <div className="flex h-full w-full items-center justify-center bg-white/10">
              <Disc3 size={innerSize * 0.5} style={{ color: textColor2 }} />
            </div>
          )}
        </div>

        {/* Center hole */}
        <div className="absolute left-1/2 top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-zinc-900 ring-1 ring-white/10" />
      </div>
    </div>
  );
}

function NowPlayingProgressBar({
  progress,
  duration,
  onSeek,
  trackColor,
  fillColor,
}: {
  progress: number;
  duration: number;
  onSeek: (time: number) => Promise<void>;
  trackColor: string;
  fillColor: string;
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
    // biome-ignore lint/a11y/useKeyWithClickEvents: progress bar is mouse-only
    <div
      ref={barRef}
      role="slider"
      tabIndex={0}
      aria-valuenow={Math.round(progress)}
      aria-valuemin={0}
      aria-valuemax={100}
      className="relative h-1.5 flex-1 cursor-pointer rounded-full"
      style={{ backgroundColor: trackColor }}
      onClick={handleClick}
    >
      <div
        className="absolute left-0 top-0 h-full rounded-full"
        style={{ width: `${progress}%`, backgroundColor: fillColor }}
      />
    </div>
  );
}
