/**
 * AppleMusicProvider — thin compatibility layer over the host MediaCenter.
 *
 * Architecture (post-migration):
 *   - All playback goes through the system MediaCenter (ctx.shell.media).
 *     We register an "apple-music" MediaProvider whose resolveAudioUrl maps
 *     a track id to the backend audio endpoint
 *     (`/api/apps/apple-music/audio/:trackId`), which decrypts and streams
 *     the full song. The MediaCenter handles play/pause/seek/queue/shuffle/
 *     repeat/persistence/cross-provider preemption.
 *   - MusicKit JS is kept ONLY for: (a) the OAuth popup to obtain a
 *     MusicUserToken, and (b) resolving SetQueueOptions {song,songs,album,
 *     playlist} into a flat list of catalog track items. Native MusicKit
 *     playback (`mk.play/pause/seekToTime/queue.append/...`) is never used —
 *     it can only play 30-second previews on non-apple.com origins.
 *   - All catalog API reads go through `/api/apps/apple-music/proxy` because
 *     MusicKit revokes the scraped MusicUserToken on non-apple.com origins.
 *   - The provider exposes a context whose surface mirrors the legacy
 *     `useAppleMusic()` consumed by pages/components; values are derived
 *     from the MediaCenter snapshot so existing UI keeps working.
 */

import type { MediaTrack } from "@tokimo/sdk";
import { useMediaCenter } from "@tokimo/sdk/react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useAppCtx } from "../AppContext";
import { getCatalogTrackId, resolveLibrarySongToCatalog } from "../proxy-utils";
import { useMessage } from "../shell/hooks";
import { installAppleMusicFetchInterceptor } from "./apple-music-fetch-interceptor";
import type { AppleMusicPage } from "./types";
import { useMusicKitLoader } from "./use-musickit";

const PROVIDER_ID = "apple-music";

// ── Context value ──

export interface AppleMusicContextValue {
  // Instance state
  isReady: boolean;
  isConfigured: boolean;

  // Auth
  isAuthorized: boolean;
  /** True when the stored token was rejected by Apple and needs refresh. */
  tokenExpired: boolean;
  authorize: () => Promise<void>;
  unauthorize: () => Promise<void>;

  // Playback (MusicKit-compatible numerics: 0=none, 2=playing, 3=paused, 8=buffering)
  playbackState: MusicKit.PlaybackStates;
  nowPlayingItem: MusicKit.MediaItem | null;
  /** Seconds (matching legacy MusicKit API). */
  currentPlaybackTime: number;
  /** Seconds. */
  currentPlaybackDuration: number;
  volume: number;
  shuffleMode: boolean;
  /** 0=off, 1=all, 2=one (matching MusicKit.PlayerRepeatMode). */
  repeatMode: number;
  queueItems: MusicKit.MediaItem[];
  queuePosition: number;
  isBuffering: boolean;
  /** True once any song has ever started playing in this session. */
  hasEverPlayed: boolean;
  /** Last playback error message. */
  playbackError: string | null;

  // Controls
  play: () => Promise<void>;
  pause: () => void;
  stop: () => void;
  skipToNext: () => Promise<void>;
  skipToPrevious: () => Promise<void>;
  seekToTime: (time: number) => Promise<void>;
  setVolume: (vol: number) => void;
  toggleShuffle: () => void;
  cycleRepeatMode: () => void;
  setQueue: (options: MusicKit.SetQueueOptions) => Promise<void>;
  setQueueFromTracks: (
    tracks: MusicKit.Resource[],
    startIndex: number,
  ) => Promise<void>;
  skipToQueueIndex: (index: number) => Promise<void>;
  playNext: (options: MusicKit.SetQueueOptions) => Promise<void>;
  playLater: (options: MusicKit.SetQueueOptions) => Promise<void>;

  // Navigation
  currentPage: AppleMusicPage;
  navigateTo: (page: AppleMusicPage) => void;
  goBack: () => void;
  canGoBack: boolean;

  // API helper (catalog reads via backend proxy)
  api: (
    path: string,
    params?: Record<string, unknown>,
  ) => Promise<MusicKit.APIResponse>;
}

const AppleMusicContext = createContext<AppleMusicContextValue | null>(null);

const DEFAULT_PAGE: AppleMusicPage = { type: "browse" };

// ── Track conversion helpers ──

function formatArtwork(
  artwork: MusicKit.Artwork | undefined,
  size: number,
): string | undefined {
  if (!artwork?.url) return undefined;
  return artwork.url
    .replace("{w}", String(size))
    .replace("{h}", String(size))
    .replace("{f}", "jpg");
}

/**
 * Convert a MusicKit catalog/library item to a host MediaTrack. Returns null
 * if no catalog id can be derived synchronously (caller must resolve library
 * ids via `resolveLibrarySongToCatalog` before pushing into the queue, since
 * `resolveAudioUrl` is required to be sync).
 */
function toMediaTrackSync(
  item: MusicKit.Resource | MusicKit.MediaItem,
): MediaTrack | null {
  const attrs =
    (item as MusicKit.Resource).attributes ??
    ({} as NonNullable<MusicKit.Resource["attributes"]>);
  let catalogId = getCatalogTrackId(item as MusicKit.Resource);
  if (!catalogId) {
    const raw = String(item.id);
    if (!raw.startsWith("i.")) catalogId = raw;
  }
  if (!catalogId) return null;
  return {
    id: catalogId,
    title: attrs.name ?? "",
    artist: attrs.artistName,
    album: attrs.albumName,
    artworkUrl: formatArtwork(attrs.artwork, 300),
    durationMs: attrs.durationInMillis,
    meta: { original: item, originalId: String(item.id) },
  };
}

async function toMediaTrackResolving(
  item: MusicKit.Resource | MusicKit.MediaItem,
): Promise<MediaTrack | null> {
  const direct = toMediaTrackSync(item);
  if (direct) return direct;
  const raw = String(item.id);
  if (!raw.startsWith("i.")) return null;
  const catalogId = await resolveLibrarySongToCatalog(raw);
  if (!catalogId) return null;
  const attrs =
    (item as MusicKit.Resource).attributes ??
    ({} as NonNullable<MusicKit.Resource["attributes"]>);
  return {
    id: catalogId,
    title: attrs.name ?? "",
    artist: attrs.artistName,
    album: attrs.albumName,
    artworkUrl: formatArtwork(attrs.artwork, 300),
    durationMs: attrs.durationInMillis,
    meta: { original: item, originalId: raw },
  };
}

async function tracksFromItems(
  items: ReadonlyArray<MusicKit.Resource | MusicKit.MediaItem>,
): Promise<MediaTrack[]> {
  const resolved = await Promise.all(items.map(toMediaTrackResolving));
  return resolved.filter((t): t is MediaTrack => t !== null);
}

// ── Server-side token storage helpers ──

async function saveTokenToServer(token: string): Promise<void> {
  try {
    await fetch("/api/apps/apple-music/auth", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ musicUserToken: token }),
    });
  } catch (e) {
    console.warn("[AppleMusic] Failed to save token to server:", e);
  }
}

async function deleteTokenFromServer(): Promise<void> {
  try {
    await fetch("/api/apps/apple-music/auth", { method: "DELETE" });
  } catch (e) {
    console.warn("[AppleMusic] Failed to delete token from server:", e);
  }
}

async function checkServerToken(): Promise<boolean> {
  try {
    const resp = await fetch("/api/apps/apple-music/auth");
    if (!resp.ok) return false;
    const json = await resp.json();
    return json?.data?.hasToken === true;
  } catch {
    return false;
  }
}

// ── Provider ──

interface AppleMusicProviderProps {
  developerToken: string;
  /** Initial page from persisted window metadata. */
  initialPage?: AppleMusicPage;
  /** Callback to persist page changes to window metadata. */
  onPageChange?: (page: AppleMusicPage) => void;
  children: React.ReactNode;
}

export function AppleMusicProvider({
  developerToken,
  initialPage,
  onPageChange,
  children,
}: AppleMusicProviderProps) {
  const ctx = useAppCtx();
  const message = useMessage();
  const messageRef = useRef(message);
  messageRef.current = message;

  const { isLoaded, error: loadError } = useMusicKitLoader();
  const instanceRef = useRef<MusicKit.MusicKitInstance | null>(null);
  const musicUserTokenRef = useRef<string | null>(null);

  const [isConfigured, setIsConfigured] = useState(false);
  const [isAuthorized, setIsAuthorized] = useState(false);
  const [tokenExpired, setTokenExpired] = useState(false);
  const [playbackError, setPlaybackError] = useState<string | null>(null);
  const [hasEverPlayedLocal, setHasEverPlayedLocal] = useState(false);

  // ── MediaCenter snapshot ──
  const { snapshot, api: mediaApi } = useMediaCenter(ctx);
  const mediaApiRef = useRef(mediaApi);
  mediaApiRef.current = mediaApi;

  const isAppleMusicActive = snapshot?.providerId === PROVIDER_ID;
  const activeSnapshot = isAppleMusicActive ? snapshot : null;

  // ── Provider registration ──
  useEffect(() => {
    const dispose = mediaApi.registerProvider(PROVIDER_ID, {
      displayName: "Apple Music",
      resolveAudioUrl: (track) =>
        `/api/apps/apple-music/audio/${encodeURIComponent(track.id)}`,
      onTrackChanged: () => {
        setPlaybackError(null);
        setHasEverPlayedLocal(true);
      },
    });
    return dispose;
  }, [mediaApi]);

  // Sticky hasEverPlayed: once we see an apple-music snapshot with a queue,
  // keep the player bar visible even after stopping.
  useEffect(() => {
    if (
      activeSnapshot &&
      activeSnapshot.queue.length > 0 &&
      !hasEverPlayedLocal
    ) {
      setHasEverPlayedLocal(true);
    }
  }, [activeSnapshot, hasEverPlayedLocal]);

  // ── Derived MusicKit-compatible projection ──
  const nowPlayingItem = useMemo<MusicKit.MediaItem | null>(() => {
    if (!activeSnapshot) return null;
    const track = activeSnapshot.queue[activeSnapshot.currentIndex];
    if (!track) return null;
    return (track.meta?.original as MusicKit.MediaItem | undefined) ?? null;
  }, [activeSnapshot]);

  const queueItems = useMemo<MusicKit.MediaItem[]>(() => {
    if (!activeSnapshot) return [];
    return activeSnapshot.queue
      .map((t) => t.meta?.original as MusicKit.MediaItem | undefined)
      .filter((x): x is MusicKit.MediaItem => !!x);
  }, [activeSnapshot]);

  const currentPlaybackTime = activeSnapshot
    ? activeSnapshot.currentTimeMs / 1000
    : 0;
  const currentPlaybackDuration = activeSnapshot
    ? activeSnapshot.durationMs / 1000
    : 0;
  const volume = snapshot?.volume ?? 1;
  const shuffleMode = activeSnapshot?.shuffle ?? false;
  const repeatMode = (() => {
    if (!activeSnapshot) return 0;
    switch (activeSnapshot.repeatMode) {
      case "one":
        return 2;
      case "all":
        return 1;
      default:
        return 0;
    }
  })();
  const queuePosition = activeSnapshot?.currentIndex ?? 0;
  // MusicKit playback states: 0=none, 2=playing, 3=paused.
  const playbackState = (
    !activeSnapshot ? 0 : activeSnapshot.isPlaying ? 2 : 3
  ) as MusicKit.PlaybackStates;
  const isBuffering = false;

  // ── Navigation ──
  const [pageStack, setPageStack] = useState<AppleMusicPage[]>(() => {
    if (!initialPage) return [DEFAULT_PAGE];
    if (initialPage.type === "now-playing") {
      return [DEFAULT_PAGE, initialPage];
    }
    return [initialPage];
  });
  const currentPage = pageStack[pageStack.length - 1] ?? DEFAULT_PAGE;
  const canGoBack = pageStack.length > 1;

  const navigateTo = useCallback(
    (page: AppleMusicPage) => {
      setPageStack((prev) => [...prev, page]);
      onPageChange?.(page);
    },
    [onPageChange],
  );

  const goBack = useCallback(() => {
    setPageStack((prev) => {
      if (prev.length <= 1) return prev;
      const next = prev.slice(0, -1);
      onPageChange?.(next[next.length - 1]);
      return next;
    });
  }, [onPageChange]);

  // ── Configure MusicKit ──
  useEffect(() => {
    if (!isLoaded || !developerToken || isConfigured) return;

    let cancelled = false;
    (async () => {
      try {
        // Install fetch interceptor before configure() so subsequent
        // MusicKit-internal catalog reads route through our proxy.
        installAppleMusicFetchInterceptor();

        let instance: MusicKit.MusicKitInstance | null = null;
        try {
          instance = MusicKit.getInstance();
        } catch {
          // Not yet configured — that's fine, will configure below.
        }
        if (!instance) {
          instance = await MusicKit.configure({
            developerToken,
            app: { name: "Tokimo", build: "1.0.0" },
          });
        }
        if (cancelled) return;
        instanceRef.current = instance;

        // Capture MusicKit's own token BEFORE any await — MusicKit on
        // non-apple.com origins clears musicUserToken almost immediately.
        const mkToken = instance.musicUserToken;
        const hasServerToken = await checkServerToken();

        if (!hasServerToken && mkToken) {
          console.log(
            "[AppleMusic] No server token but MusicKit has one — restoring to server",
          );
          await saveTokenToServer(mkToken);
          musicUserTokenRef.current = mkToken;
          setIsAuthorized(true);
        } else if (hasServerToken) {
          musicUserTokenRef.current = "server-stored";
          setIsAuthorized(true);
        } else {
          setIsAuthorized(instance.isAuthorized);
        }
        setIsConfigured(true);
      } catch (err) {
        console.error("[AppleMusic] configure failed:", err);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [isLoaded, developerToken, isConfigured]);

  // ── MusicKit authorization-status listener (login state only) ──
  useEffect(() => {
    if (!isConfigured) return;
    const mk = instanceRef.current;
    if (!mk) return;

    const onAuthChange = () => {
      if (mk.isAuthorized && mk.musicUserToken) {
        musicUserTokenRef.current = mk.musicUserToken;
        saveTokenToServer(mk.musicUserToken);
        setIsAuthorized(true);
      } else if (!musicUserTokenRef.current) {
        setIsAuthorized(false);
      }
    };
    mk.addEventListener(
      MusicKit.Events.authorizationStatusDidChange,
      onAuthChange,
    );
    return () =>
      mk.removeEventListener(
        MusicKit.Events.authorizationStatusDidChange,
        onAuthChange,
      );
  }, [isConfigured]);

  // ── Token-expired event (fired by fetch interceptor) ──
  useEffect(() => {
    if (!isConfigured) return;
    const onTokenExpired = () => {
      console.warn(
        "[AppleMusic] Token expired signal received — clearing stored token",
      );
      musicUserTokenRef.current = null;
      setTokenExpired(true);
      deleteTokenFromServer();
    };
    window.addEventListener("apple-music-token-expired", onTokenExpired);
    return () =>
      window.removeEventListener("apple-music-token-expired", onTokenExpired);
  }, [isConfigured]);

  // ── Auth controls ──
  const authorize = useCallback(async () => {
    const mk = instanceRef.current;
    if (!mk) return;

    // Poll musicUserToken aggressively during authorization. MusicKit on
    // non-apple.com origins revokes access almost immediately.
    let pollTimer: ReturnType<typeof setInterval> | null = null;
    let tokenSavePromise: Promise<void> | null = null;
    const captureToken = () => {
      const token = mk.musicUserToken;
      if (token && !musicUserTokenRef.current) {
        musicUserTokenRef.current = token;
        tokenSavePromise = saveTokenToServer(token);
        setIsAuthorized(true);
      }
    };
    pollTimer = setInterval(captureToken, 100);

    try {
      const result = await Promise.race([
        mk.authorize().catch(() => null),
        new Promise<null>((resolve) => setTimeout(() => resolve(null), 15000)),
      ]);
      if (result) {
        captureToken();
        if (!musicUserTokenRef.current) {
          musicUserTokenRef.current = result;
          tokenSavePromise = saveTokenToServer(result);
          setIsAuthorized(true);
        }
      }
    } catch {
      // Ignore — poll/event may have already captured the token.
    } finally {
      if (pollTimer) clearInterval(pollTimer);
      if (tokenSavePromise) await tokenSavePromise;
    }

    if (musicUserTokenRef.current) {
      setIsAuthorized(true);
      setTokenExpired(false);
    }
  }, []);

  const unauthorize = useCallback(async () => {
    const mk = instanceRef.current;
    if (mk) {
      try {
        await mk.unauthorize();
      } catch {
        // Ignore.
      }
    }
    musicUserTokenRef.current = null;
    deleteTokenFromServer();
    setIsAuthorized(false);
  }, []);

  // ── API helper (catalog reads via backend proxy) ──
  const apiHelper = useCallback(
    async (
      path: string,
      params?: Record<string, unknown>,
    ): Promise<MusicKit.APIResponse> => {
      const isFullUrl = path.startsWith("https://");
      const body: Record<string, unknown> = {
        ...(isFullUrl ? { targetUrl: path } : { path }),
        params: params
          ? Object.fromEntries(
              Object.entries(params).map(([k, v]) => [k, String(v)]),
            )
          : {},
      };

      const resp = await fetch("/api/apps/apple-music/proxy", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });

      if (!resp.ok) {
        if (resp.headers.get("x-apple-music-token-expired") === "true") {
          console.warn(
            "[AppleMusic] Token explicitly rejected by Apple — clearing auth state",
          );
          window.dispatchEvent(new CustomEvent("apple-music-token-expired"));
        }
        let detail = "";
        try {
          detail = (await resp.text()).slice(0, 500);
        } catch {
          // ignore
        }
        const errMsg = `Apple Music API error: ${resp.status} ${resp.statusText} — ${body.path ?? body.targetUrl}\n${detail}`;
        console.error("[AppleMusic]", errMsg);
        throw new Error(errMsg);
      }

      const data = await resp.json();
      return { data } as MusicKit.APIResponse;
    },
    [],
  );

  // ── Playback controls (delegate to MediaCenter) ──

  const playMediaTracks = useCallback(
    async (tracks: MediaTrack[], startIndex: number) => {
      if (tracks.length === 0) return;
      const idx = Math.max(0, Math.min(startIndex, tracks.length - 1));
      setPlaybackError(null);
      try {
        await mediaApiRef.current.play({
          providerId: PROVIDER_ID,
          queue: tracks,
          startIndex: idx,
        });
        setHasEverPlayedLocal(true);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.error("[AppleMusic] play() failed:", msg);
        setPlaybackError(msg);
        messageRef.current.error(`Playback failed: ${msg}`);
      }
    },
    [],
  );

  const play = useCallback(async () => {
    setPlaybackError(null);
    const snap = mediaApiRef.current.getSnapshot();
    if (snap?.providerId === PROVIDER_ID && snap.queue.length > 0) {
      mediaApiRef.current.resume();
      return;
    }
    // No active apple-music queue and nothing to resume.
    if (snap?.providerId === PROVIDER_ID) return;
  }, []);

  const pause = useCallback(() => {
    mediaApiRef.current.pause();
  }, []);

  const stop = useCallback(() => {
    mediaApiRef.current.pause();
  }, []);

  const skipToNext = useCallback(async () => {
    mediaApiRef.current.next();
  }, []);

  const skipToPrevious = useCallback(async () => {
    mediaApiRef.current.previous();
  }, []);

  const seekToTime = useCallback(async (time: number) => {
    mediaApiRef.current.seek(Math.max(0, time * 1000));
  }, []);

  const setVolume = useCallback((vol: number) => {
    const clamped = Math.max(0, Math.min(1, vol));
    mediaApiRef.current.setVolume(clamped);
  }, []);

  const toggleShuffle = useCallback(() => {
    const snap = mediaApiRef.current.getSnapshot();
    const cur = snap?.providerId === PROVIDER_ID ? snap.shuffle : false;
    mediaApiRef.current.setShuffle(!cur);
  }, []);

  const cycleRepeatMode = useCallback(() => {
    const snap = mediaApiRef.current.getSnapshot();
    const cur =
      snap?.providerId === PROVIDER_ID ? snap.repeatMode : ("off" as const);
    const next = cur === "off" ? "all" : cur === "all" ? "one" : "off";
    mediaApiRef.current.setRepeat(next);
  }, []);

  /**
   * Expand a `SetQueueOptions` into a list of catalog tracks using MusicKit
   * to talk to the Apple catalog API (via our fetch interceptor), then hand
   * the resolved queue to the host MediaCenter.
   */
  const setQueueFn = useCallback(
    async (options: MusicKit.SetQueueOptions) => {
      const mk = instanceRef.current;
      if (!mk) return;
      setPlaybackError(null);

      try {
        await mk.setQueue({ ...options, startPlaying: false });

        // Library-only song with no catalog mapping — try the explicit
        // catalog fallback so backend audio decryption succeeds.
        if (mk.queue.isEmpty && options.song?.startsWith("i.")) {
          const catalogId = await resolveLibrarySongToCatalog(options.song);
          if (catalogId) {
            await mk.setQueue({ song: catalogId });
          }
        }

        if (mk.queue.isEmpty) {
          const msg = "This item is no longer available in Apple Music";
          setPlaybackError(msg);
          messageRef.current.error(msg);
          return;
        }

        const tracks = await tracksFromItems(mk.queue.items);
        if (tracks.length === 0) {
          const msg = "No playable tracks found";
          setPlaybackError(msg);
          messageRef.current.error(msg);
          return;
        }

        await playMediaTracks(tracks, 0);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.error("[AppleMusic] setQueue failed:", msg, options);
        setPlaybackError(msg);
        messageRef.current.error(`Cannot play this item: ${msg}`);
      }
    },
    [playMediaTracks],
  );

  const setQueueFromTracks = useCallback(
    async (tracks: MusicKit.Resource[], startIndex: number) => {
      if (tracks.length === 0) return;
      const mediaTracks = await tracksFromItems(tracks);
      if (mediaTracks.length === 0) {
        const msg = "No playable tracks in this list";
        setPlaybackError(msg);
        messageRef.current.error(msg);
        return;
      }
      // If filtering dropped earlier tracks, clamp the start index.
      const clamped = Math.min(startIndex, mediaTracks.length - 1);
      await playMediaTracks(mediaTracks, Math.max(0, clamped));
    },
    [playMediaTracks],
  );

  const skipToQueueIndex = useCallback(async (index: number) => {
    mediaApiRef.current.skipToIndex(index);
  }, []);

  /**
   * Resolve `options` into tracks, then insert them right after the current
   * playing index (play next) or at the end of the queue (play later). If no
   * apple-music queue is active, falls back to starting a new queue.
   */
  const insertIntoQueue = useCallback(
    async (options: MusicKit.SetQueueOptions, where: "next" | "later") => {
      const mk = instanceRef.current;
      if (!mk) return;

      try {
        await mk.setQueue({ ...options, startPlaying: false });
        if (mk.queue.isEmpty && options.song?.startsWith("i.")) {
          const catalogId = await resolveLibrarySongToCatalog(options.song);
          if (catalogId) await mk.setQueue({ song: catalogId });
        }
        if (mk.queue.isEmpty) return;

        const additions = await tracksFromItems(mk.queue.items);
        if (additions.length === 0) return;

        const snap = mediaApiRef.current.getSnapshot();
        if (snap?.providerId !== PROVIDER_ID || snap.queue.length === 0) {
          // No active apple-music queue — start a new one.
          await playMediaTracks(additions, 0);
          return;
        }

        const cur = snap.queue;
        const idx = snap.currentIndex;
        const newQueue =
          where === "next"
            ? [...cur.slice(0, idx + 1), ...additions, ...cur.slice(idx + 1)]
            : [...cur, ...additions];
        mediaApiRef.current.setQueue(newQueue, idx);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.error(`[AppleMusic] ${where} failed:`, msg, options);
        messageRef.current.error(`Cannot queue this item: ${msg}`);
      }
    },
    [playMediaTracks],
  );

  const playNext = useCallback(
    (options: MusicKit.SetQueueOptions) => insertIntoQueue(options, "next"),
    [insertIntoQueue],
  );

  const playLater = useCallback(
    (options: MusicKit.SetQueueOptions) => insertIntoQueue(options, "later"),
    [insertIntoQueue],
  );

  // ── Context value ──
  const value = useMemo<AppleMusicContextValue>(
    () => ({
      isReady: isConfigured && !loadError,
      isConfigured,
      isAuthorized,
      tokenExpired,
      authorize,
      unauthorize,
      playbackState,
      nowPlayingItem,
      currentPlaybackTime,
      currentPlaybackDuration,
      volume,
      shuffleMode,
      repeatMode,
      queueItems,
      queuePosition,
      isBuffering,
      hasEverPlayed: hasEverPlayedLocal,
      playbackError,
      play,
      pause,
      stop,
      skipToNext,
      skipToPrevious,
      seekToTime,
      setVolume,
      toggleShuffle,
      cycleRepeatMode,
      setQueue: setQueueFn,
      setQueueFromTracks,
      skipToQueueIndex,
      playNext,
      playLater,
      currentPage,
      navigateTo,
      goBack,
      canGoBack,
      api: apiHelper,
    }),
    [
      isConfigured,
      loadError,
      isAuthorized,
      tokenExpired,
      authorize,
      unauthorize,
      playbackState,
      nowPlayingItem,
      currentPlaybackTime,
      currentPlaybackDuration,
      volume,
      shuffleMode,
      repeatMode,
      queueItems,
      queuePosition,
      hasEverPlayedLocal,
      playbackError,
      play,
      pause,
      stop,
      skipToNext,
      skipToPrevious,
      seekToTime,
      setVolume,
      toggleShuffle,
      cycleRepeatMode,
      setQueueFn,
      setQueueFromTracks,
      skipToQueueIndex,
      playNext,
      playLater,
      currentPage,
      navigateTo,
      goBack,
      canGoBack,
      apiHelper,
    ],
  );

  return (
    <AppleMusicContext.Provider value={value}>
      {children}
    </AppleMusicContext.Provider>
  );
}

export function useAppleMusic(): AppleMusicContextValue {
  const ctx = useContext(AppleMusicContext);
  if (!ctx) {
    throw new Error("useAppleMusic must be used within <AppleMusicProvider>");
  }
  return ctx;
}
