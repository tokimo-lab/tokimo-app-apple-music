import { type RefObject, useEffect, useMemo, useRef, useState } from "react";
import { resolveLibrarySongToCatalog } from "../proxy-utils";

export interface AppleMusicLyricLine {
  begin: number;
  end: number;
  text: string;
}

interface UseAppleMusicLyricsResult {
  lines: AppleMusicLyricLine[];
  currentIdx: number;
  progressRef: RefObject<number>;
  isLoading: boolean;
  noLyrics: boolean;
  hasSyncedLyrics: boolean;
}

function parseTTMLTime(ts: string): number {
  const parts = ts.split(":");
  if (parts.length === 2) {
    return Number.parseFloat(parts[0]) * 60 + Number.parseFloat(parts[1]);
  }
  return Number.parseFloat(parts[0]);
}

function parseTTML(ttml: string): AppleMusicLyricLine[] {
  const lines: AppleMusicLyricLine[] = [];
  const parser = new DOMParser();
  const doc = parser.parseFromString(ttml, "text/xml");
  const pElements = doc.querySelectorAll("p");

  for (const p of pElements) {
    const begin = p.getAttribute("begin");
    const end = p.getAttribute("end");
    const text = p.textContent?.trim();
    if (begin && end && text) {
      lines.push({
        begin: parseTTMLTime(begin),
        end: parseTTMLTime(end),
        text,
      });
    }
  }

  return lines;
}

function getStorefront(): string {
  try {
    return MusicKit.getInstance().storefrontCountryCode || "us";
  } catch {
    return "us";
  }
}

function currentLyricIndex(lines: AppleMusicLyricLine[], time: number): number {
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (time >= line.begin && time < line.end) {
      return i;
    }
  }
  return -1;
}

function lyricProgress(
  lines: AppleMusicLyricLine[],
  index: number,
  time: number,
): number {
  if (index < 0 || index >= lines.length) return 0;
  const line = lines[index];
  const duration = Math.max(line.end - line.begin, 0.001);
  const progress = (time - line.begin) / duration;
  return Math.max(0, Math.min(1, progress));
}

export function useAppleMusicLyrics(
  trackId: string | null | undefined,
  getCurrentTime: () => number,
  enabled = true,
): UseAppleMusicLyricsResult {
  const [lines, setLines] = useState<AppleMusicLyricLine[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [noLyrics, setNoLyrics] = useState(false);
  const [currentIdx, setCurrentIdx] = useState(-1);
  const progressRef = useRef(0);
  const prevIdxRef = useRef(-1);
  const getTimeRef = useRef(getCurrentTime);
  getTimeRef.current = getCurrentTime;

  const resolvedTrackId = useMemo(() => trackId ?? null, [trackId]);

  useEffect(() => {
    const initialTrackId = resolvedTrackId;
    if (!enabled || !initialTrackId) {
      setLines([]);
      setIsLoading(false);
      setNoLyrics(false);
      return;
    }
    const assuredTrackId: string = initialTrackId;

    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 15_000);

    setIsLoading(true);
    setNoLyrics(false);
    setLines([]);

    async function fetchLyrics() {
      try {
        let catalogId: string = assuredTrackId;
        if (catalogId.startsWith("i.")) {
          const resolved = await resolveLibrarySongToCatalog(catalogId);
          if (controller.signal.aborted) return;
          if (!resolved) {
            setNoLyrics(true);
            setIsLoading(false);
            return;
          }
          catalogId = resolved;
        }

        const sf = getStorefront();
        const targetUrl = `https://amp-api-edge.music.apple.com/v1/catalog/${sf}/songs/${catalogId}/lyrics`;
        const resp = await fetch("/api/apps/apple-music/proxy", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ targetUrl }),
          signal: controller.signal,
        });

        if (!resp.ok) {
          throw new Error(`Lyrics API ${resp.status}`);
        }

        const json = (await resp.json()) as {
          data?: Array<{ attributes?: { ttml?: string } }>;
        };
        if (controller.signal.aborted) return;

        const ttml = json.data?.[0]?.attributes?.ttml;
        if (!ttml) {
          setNoLyrics(true);
          return;
        }

        setLines(parseTTML(ttml));
      } catch {
        if (!controller.signal.aborted) {
          setNoLyrics(true);
        }
      } finally {
        if (!controller.signal.aborted) {
          setIsLoading(false);
        }
      }
    }

    void fetchLyrics();

    return () => {
      clearTimeout(timeout);
      controller.abort();
    };
  }, [enabled, resolvedTrackId]);

  useEffect(() => {
    if (!enabled || lines.length === 0) {
      prevIdxRef.current = -1;
      setCurrentIdx(-1);
      progressRef.current = 0;
      return;
    }

    let raf = 0;
    const tick = () => {
      const time = getTimeRef.current();
      const idx = currentLyricIndex(lines, time);
      if (idx !== prevIdxRef.current) {
        prevIdxRef.current = idx;
        setCurrentIdx(idx);
      }
      progressRef.current = lyricProgress(lines, idx, time);
      raf = requestAnimationFrame(tick);
    };

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [enabled, lines]);

  return {
    lines,
    currentIdx,
    progressRef,
    isLoading,
    noLyrics,
    hasSyncedLyrics: lines.length > 0,
  };
}
