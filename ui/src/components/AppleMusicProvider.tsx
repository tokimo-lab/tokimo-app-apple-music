import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { PlaybackStateData } from "../api-types/PlaybackStateData";
import * as centralEngine from "../shell/engine-ref";
import { useMediaSessionOptional } from "../shell/hooks";
import { useMessage } from "../shell/hooks";
import { getCatalogTrackId, resolveLibrarySongToCatalog } from "../proxy-utils";
import {
  getStoredAppleMusicVolume,
  saveStoredAppleMusicVolume,
} from "../shared-audio";
import { installAppleMusicFetchInterceptor } from "./apple-music-fetch-interceptor";
import type { AppleMusicPage } from "./types";
import { useAppleMusicSession } from "./use-apple-music-session";
import { useMusicKitLoader } from "./use-musickit";

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

  // Playback
  playbackState: MusicKit.PlaybackStates;
  nowPlayingItem: MusicKit.MediaItem | null;
  currentPlaybackTime: number;
  currentPlaybackDuration: number;
  volume: number;
  shuffleMode: boolean;
  repeatMode: number;
  queueItems: MusicKit.MediaItem[];
  queuePosition: number;
  /** True when MusicKit is loading/buffering a track */
  isBuffering: boolean;
  /** True once any song has ever started playing (keeps player bar visible) */
  hasEverPlayed: boolean;
  /** Last playback error message (cleared on successful play) */
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
  /** Build a local queue from pre-fetched tracks (bypasses MusicKit's broken batch queue). */
  setQueueFromTracks: (
    tracks: MusicKit.Resource[],
    startIndex: number,
  ) => Promise<void>;
  /** Jump to a specific index in the queue (local or MusicKit). */
  skipToQueueIndex: (index: number) => Promise<void>;
  playNext: (options: MusicKit.SetQueueOptions) => Promise<void>;
  playLater: (options: MusicKit.SetQueueOptions) => Promise<void>;

  // Navigation
  currentPage: AppleMusicPage;
  navigateTo: (page: AppleMusicPage) => void;
  goBack: () => void;
  canGoBack: boolean;

  // API helper
  api: (
    path: string,
    params?: Record<string, unknown>,
  ) => Promise<MusicKit.APIResponse>;
}

const AppleMusicContext = createContext<AppleMusicContextValue | null>(null);

const DEFAULT_PAGE: AppleMusicPage = { type: "browse" };

/**
 * Use backend audio decryption pipeline instead of MusicKit's native player.
 * MusicKit on non-apple.com domains only plays 30-second previews; the backend
 * endpoint downloads, decrypts (Widevine CDM), and streams full-length audio.
 */
const USE_BACKEND_AUDIO = true;

/** Fisher-Yates shuffle: returns an array of indices in random order, keeping currentIndex first. */
function buildShuffleOrder(length: number, currentIndex: number): number[] {
  const order: number[] = [];
  for (let i = 0; i < length; i++) {
    if (i !== currentIndex) order.push(i);
  }
  for (let i = order.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [order[i], order[j]] = [order[j], order[i]];
  }
  if (currentIndex >= 0 && currentIndex < length) {
    order.unshift(currentIndex);
  }
  return order;
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
  /** Initial page from persisted window metadata */
  initialPage?: AppleMusicPage;
  /** Callback to persist page changes to window metadata */
  onPageChange?: (page: AppleMusicPage) => void;
  children: React.ReactNode;
}

export function AppleMusicProvider({
  developerToken,
  initialPage,
  onPageChange,
  children,
}: AppleMusicProviderProps) {
  const { isLoaded, error: loadError } = useMusicKitLoader();
  const mediaSession = useMediaSessionOptional();
  const instanceRef = useRef<MusicKit.MusicKitInstance | null>(null);
  const musicUserTokenRef = useRef<string | null>(null);

  // State
  const [isConfigured, setIsConfigured] = useState(false);
  const [isAuthorized, setIsAuthorized] = useState(false);
  const [tokenExpired, setTokenExpired] = useState(false);
  const [playbackState, setPlaybackState] = useState<MusicKit.PlaybackStates>(
    0 as MusicKit.PlaybackStates,
  );
  const [nowPlayingItem, setNowPlayingItem] =
    useState<MusicKit.MediaItem | null>(null);
  const nowPlayingItemRef = useRef(nowPlayingItem);
  nowPlayingItemRef.current = nowPlayingItem;
  const [currentPlaybackTime, setCurrentPlaybackTime] = useState(0);
  const currentPlaybackTimeRef = useRef(0);
  currentPlaybackTimeRef.current = currentPlaybackTime;
  const [currentPlaybackDuration, setCurrentPlaybackDuration] = useState(0);
  const [volume, setVolumeState] = useState(getStoredAppleMusicVolume);
  const [shuffleMode, setShuffleMode] = useState(false);
  const [repeatMode, setRepeatMode] = useState(0);
  const [queueItems, setQueueItems] = useState<MusicKit.MediaItem[]>([]);
  const [queuePosition, setQueuePosition] = useState(0);
  const [hasEverPlayed, setHasEverPlayed] = useState(false);
  const [playbackError, setPlaybackError] = useState<string | null>(null);
  const message = useMessage();
  const messageRef = useRef(message);
  messageRef.current = message;
  const mediaSessionRef = useRef(mediaSession);
  mediaSessionRef.current = mediaSession;

  // ── Local queue ──
  // MusicKit JS can't build multi-song queues on non-Apple domains.
  // We manage the queue ourselves and play songs one at a time.
  const localQueueRef = useRef<MusicKit.Resource[]>([]);
  const localQueuePosRef = useRef(-1);
  const isLocalQueueActiveRef = useRef(false);

  // Shuffle order for local queue (backend audio mode)
  const shuffleOrderRef = useRef<number[]>([]);
  const shufflePosRef = useRef(0);

  // Refs for onEnded callback (avoids stale closures)
  const shuffleModeRef = useRef(shuffleMode);
  shuffleModeRef.current = shuffleMode;
  const repeatModeRef = useRef(repeatMode);
  repeatModeRef.current = repeatMode;

  // Navigation stack
  const [pageStack, setPageStack] = useState<AppleMusicPage[]>(() => {
    if (!initialPage) return [DEFAULT_PAGE];
    if (initialPage.type === "now-playing") {
      return [DEFAULT_PAGE, initialPage];
    }
    return [initialPage];
  });
  const currentPage = pageStack[pageStack.length - 1] ?? DEFAULT_PAGE;
  const canGoBack = pageStack.length > 1;

  // Sync React state from a live MusicKit instance (used on mount/reconnect)
  const syncStateFromInstance = useCallback((mk: MusicKit.MusicKitInstance) => {
    setPlaybackState(mk.playbackState);
    setCurrentPlaybackTime(mk.currentPlaybackTime);
    setCurrentPlaybackDuration(mk.currentPlaybackDuration);
    setVolumeState(mk.volume);
    setShuffleMode(mk.shuffleMode === MusicKit.PlayerShuffleMode.songs);
    setRepeatMode(mk.repeatMode);

    const items = [...mk.queue.items];
    setQueueItems(items);
    setQueuePosition(mk.queue.position);

    if (mk.nowPlayingItem) {
      setNowPlayingItem(mk.nowPlayingItem);
      setHasEverPlayed(true);
    }
  }, []);

  // ── Backend audio helpers ──

  const playTrackViaBackend = useCallback(
    async (trackId: string, startTime?: number) => {
      // Pause MusicKit's native player to prevent dual playback
      try {
        instanceRef.current?.pause();
      } catch {
        // ignore
      }

      // Notify media session — pauses all other sources (local music, video, etc.)
      mediaSessionRef.current?.requestPlay("music", "apple-music");

      const url = `/api/apps/apple-music/audio/${encodeURIComponent(trackId)}`;
      console.log(`[AppleMusic] Backend audio: loading ${trackId}`);
      setPlaybackState(8 as MusicKit.PlaybackStates); // waiting/buffering
      setPlaybackError(null);

      try {
        await centralEngine.loadAndPlay(url, {
          provider: "apple-music",
          startTime,
        });
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.error("[AppleMusic] Backend audio play failed:", msg);
        setPlaybackError(msg);
        messageRef.current.error(`Playback failed: ${msg}`);
      }
    },
    [],
  );

  // ── Configure MusicKit ──
  useEffect(() => {
    if (!isLoaded || !developerToken || isConfigured) return;

    let cancelled = false;
    (async () => {
      try {
        // Intercept all fetch calls to api.music.apple.com and route through
        // our backend proxy. Must be installed before MusicKit.configure().
        installAppleMusicFetchInterceptor();

        // MusicKit is a singleton that survives React unmount/remount.
        // Reuse the existing instance to avoid triggering a queue restore
        // (which fails with library IDs on non-Apple domains).
        let instance: MusicKit.MusicKitInstance | null = null;
        try {
          instance = MusicKit.getInstance();
        } catch {
          // Not yet configured — that's fine, will configure below
        }

        if (!instance) {
          instance = await MusicKit.configure({
            developerToken,
            app: { name: "Tokimo", build: "1.0.0" },
          });
        }
        if (cancelled) return;
        instanceRef.current = instance;
        instance.volume = getStoredAppleMusicVolume();

        // Restore React state from the live instance (e.g. if a song is
        // still playing from before the window was closed/reopened).
        syncStateFromInstance(instance);

        // Capture MusicKit's own token BEFORE any await — MusicKit on
        // non-apple.com origins clears musicUserToken almost immediately,
        // so we must read it synchronously right after configure.
        const mkToken = instance.musicUserToken;

        // Check if user has a stored music-user-token on the server.
        const hasServerToken = await checkServerToken();

        // MusicKit may have recovered the token from its own localStorage even
        // when our DB was cleared (e.g. after a previous session expiry event).
        // If so, save it back to the server so the proxy can use it.
        if (!hasServerToken && mkToken) {
          console.log(
            "[AppleMusic] No server token but MusicKit has one — restoring to server",
          );
          await saveTokenToServer(mkToken);
          musicUserTokenRef.current = mkToken;
          setIsAuthorized(true);
        } else if (hasServerToken) {
          console.log("[AppleMusic] Configured with server-stored token");
          // Guard against MusicKit's authorizationStatusDidChange(false)
          // which fires on non-apple.com domains. Without this, the
          // onAuthChange handler would immediately de-auth the user.
          musicUserTokenRef.current = "server-stored";
          setIsAuthorized(true);
        } else {
          console.log(
            "[AppleMusic] Configured without token, isAuthorized:",
            instance.isAuthorized,
          );
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
  }, [isLoaded, developerToken, isConfigured, syncStateFromInstance]);

  // ── Token-expired event (fired by fetch interceptor on x-apple-music-token-expired header) ──
  useEffect(() => {
    if (!isConfigured) return;

    const onTokenExpired = () => {
      console.warn(
        "[AppleMusic] Token expired signal received — clearing stored token, prompting reconnect",
      );
      musicUserTokenRef.current = null;
      // Keep isAuthorized=true so the app stays open; only Library/personal
      // content is gated behind tokenExpired. Catalog browsing still works.
      setTokenExpired(true);
      deleteTokenFromServer();
    };

    window.addEventListener("apple-music-token-expired", onTokenExpired);
    return () =>
      window.removeEventListener("apple-music-token-expired", onTokenExpired);
  }, [isConfigured]);

  // ── Event listeners ──
  useEffect(() => {
    const mk = instanceRef.current;
    if (!mk || !isConfigured) return;

    const onAuthChange = () => {
      console.log(
        "[AppleMusic] authorizationStatusDidChange:",
        mk.isAuthorized,
        "musicUserToken:",
        !!mk.musicUserToken,
      );
      if (mk.isAuthorized && mk.musicUserToken) {
        // Capture token during the brief authorized window and save to server
        musicUserTokenRef.current = mk.musicUserToken;
        // Fire-and-forget is acceptable here; the authorize() flow also saves
        // and awaits. This is a redundant backup path.
        saveTokenToServer(mk.musicUserToken);
        setIsAuthorized(true);
      } else if (!musicUserTokenRef.current) {
        // Only set unauthorized if we don't have our own stored token
        setIsAuthorized(false);
      }
      // If MusicKit says false but we captured a token, ignore it
    };
    const onPlaybackState = () => {
      if (
        USE_BACKEND_AUDIO &&
        centralEngine.getActiveProvider() === "apple-music"
      )
        return;
      setPlaybackState(mk.playbackState);
    };
    const onNowPlaying = () => {
      const item = mk.nowPlayingItem;
      if (item) {
        setNowPlayingItem(item);
        // isLocalQueueActiveRef guards against restore-triggered events
        // (same pattern as onQueueItems / onQueuePosition)
        if (!isLocalQueueActiveRef.current) {
          setHasEverPlayed(true);
        }
      }
      // Don't null out — keep last item visible during transitions
    };
    const onTimeChange = () => {
      if (
        USE_BACKEND_AUDIO &&
        centralEngine.getActiveProvider() === "apple-music"
      )
        return;
      setCurrentPlaybackTime(mk.currentPlaybackTime);
    };
    const onDurationChange = () => {
      if (
        USE_BACKEND_AUDIO &&
        centralEngine.getActiveProvider() === "apple-music"
      )
        return;
      setCurrentPlaybackDuration(mk.currentPlaybackDuration);
    };
    const onVolumeChange = () => {
      if (
        USE_BACKEND_AUDIO &&
        centralEngine.getActiveProvider() === "apple-music"
      )
        return;
      setVolumeState(mk.volume);
      saveStoredAppleMusicVolume(mk.volume);
    };
    const onQueueItems = () => {
      // When local queue is active, MusicKit only has 1 song — don't overwrite
      if (isLocalQueueActiveRef.current) return;
      const items = [...mk.queue.items];
      setQueueItems(items);
      // Eagerly preview next track from queue for instant UI feedback
      if (items.length > 0 && mk.queue.position >= 0) {
        const nextItem = items[mk.queue.position];
        if (nextItem) {
          setNowPlayingItem(nextItem);
          setHasEverPlayed(true);
        }
      }
    };
    const onQueuePosition = () => {
      if (isLocalQueueActiveRef.current) return;
      const pos = mk.queue.position;
      setQueuePosition(pos);
      // Update now-playing from queue immediately on position change
      const item = mk.queue.items[pos];
      if (item) {
        setNowPlayingItem(item);
        setHasEverPlayed(true);
      }
    };
    const onShuffleChange = () =>
      setShuffleMode(mk.shuffleMode === MusicKit.PlayerShuffleMode.songs);
    const onRepeatChange = () => setRepeatMode(mk.repeatMode);
    const onMediaPlaybackError = (...args: unknown[]) => {
      const errInfo =
        args[0] && typeof args[0] === "object"
          ? JSON.stringify(args[0])
          : String(args[0] ?? "Unknown playback error");
      console.error("[AppleMusic] mediaPlaybackError:", errInfo);
      setPlaybackError(errInfo);
      messageRef.current.error(`Playback failed: ${errInfo}`);
    };

    mk.addEventListener(
      MusicKit.Events.authorizationStatusDidChange,
      onAuthChange,
    );
    mk.addEventListener(
      MusicKit.Events.playbackStateDidChange,
      onPlaybackState,
    );
    mk.addEventListener(MusicKit.Events.nowPlayingItemDidChange, onNowPlaying);
    mk.addEventListener(MusicKit.Events.playbackTimeDidChange, onTimeChange);
    mk.addEventListener(
      MusicKit.Events.playbackDurationDidChange,
      onDurationChange,
    );
    mk.addEventListener(
      MusicKit.Events.playbackVolumeDidChange,
      onVolumeChange,
    );
    mk.addEventListener(MusicKit.Events.queueItemsDidChange, onQueueItems);
    mk.addEventListener(
      MusicKit.Events.queuePositionDidChange,
      onQueuePosition,
    );
    mk.addEventListener(MusicKit.Events.shuffleModeDidChange, onShuffleChange);
    mk.addEventListener(MusicKit.Events.repeatModeDidChange, onRepeatChange);
    mk.addEventListener(
      MusicKit.Events.mediaPlaybackError,
      onMediaPlaybackError,
    );

    return () => {
      mk.removeEventListener(
        MusicKit.Events.authorizationStatusDidChange,
        onAuthChange,
      );
      mk.removeEventListener(
        MusicKit.Events.playbackStateDidChange,
        onPlaybackState,
      );
      mk.removeEventListener(
        MusicKit.Events.nowPlayingItemDidChange,
        onNowPlaying,
      );
      mk.removeEventListener(
        MusicKit.Events.playbackTimeDidChange,
        onTimeChange,
      );
      mk.removeEventListener(
        MusicKit.Events.playbackDurationDidChange,
        onDurationChange,
      );
      mk.removeEventListener(
        MusicKit.Events.playbackVolumeDidChange,
        onVolumeChange,
      );
      mk.removeEventListener(MusicKit.Events.queueItemsDidChange, onQueueItems);
      mk.removeEventListener(
        MusicKit.Events.queuePositionDidChange,
        onQueuePosition,
      );
      mk.removeEventListener(
        MusicKit.Events.shuffleModeDidChange,
        onShuffleChange,
      );
      mk.removeEventListener(
        MusicKit.Events.repeatModeDidChange,
        onRepeatChange,
      );
      mk.removeEventListener(
        MusicKit.Events.mediaPlaybackError,
        onMediaPlaybackError,
      );
    };
  }, [isConfigured]);

  // ── Central engine subscription for Apple Music state ──
  useEffect(() => {
    if (!USE_BACKEND_AUDIO) return;

    const unsubscribe = centralEngine.subscribe(() => {
      if (centralEngine.getActiveProvider() !== "apple-music") return;
      const snap = centralEngine.getSnapshot();

      if (snap.isPlaying) {
        setCurrentPlaybackTime(snap.currentTime);
        setCurrentPlaybackDuration(centralEngine.getDuration());
        setPlaybackState(2 as MusicKit.PlaybackStates);
        setPlaybackError(null);
        setHasEverPlayed(true);
      } else if (snap.isBuffering) {
        setPlaybackState(8 as MusicKit.PlaybackStates);
      } else if (snap.error) {
        setPlaybackError(snap.error);
        setPlaybackState(0 as MusicKit.PlaybackStates);
      } else {
        setCurrentPlaybackTime(snap.currentTime);
        setPlaybackState(3 as MusicKit.PlaybackStates);
      }
      setVolumeState(snap.volume);
    });

    return unsubscribe;
  }, []);

  // ── Controls ──

  const authorize = useCallback(async () => {
    const mk = instanceRef.current;
    if (!mk) return;
    console.log("[AppleMusic] authorize() starting...");

    // Poll mk.musicUserToken aggressively during authorization. MusicKit on
    // non-apple.com origins revokes access almost immediately, so the event
    // handler might miss it. We poll every 100ms for up to 15s.
    let pollTimer: ReturnType<typeof setInterval> | null = null;
    let tokenSavePromise: Promise<void> | null = null;
    const captureToken = (source: string) => {
      const token = mk.musicUserToken;
      if (token && !musicUserTokenRef.current) {
        console.log(
          `[AppleMusic] Token captured via ${source} (${token.length} chars)`,
        );
        musicUserTokenRef.current = token;
        tokenSavePromise = saveTokenToServer(token);
        setIsAuthorized(true);
      }
    };
    pollTimer = setInterval(() => captureToken("poll"), 100);

    try {
      const result = await Promise.race([
        mk.authorize().catch(() => null),
        new Promise<null>((resolve) => setTimeout(() => resolve(null), 15000)),
      ]);
      if (result) {
        captureToken("authorize-result");
        if (!musicUserTokenRef.current) {
          musicUserTokenRef.current = result;
          tokenSavePromise = saveTokenToServer(result);
          setIsAuthorized(true);
        }
      }
    } catch {
      // Ignore — event handler or poll may have already captured the token
    } finally {
      if (pollTimer) clearInterval(pollTimer);
      // Ensure the token is persisted to the server before we return so the
      // proxy can use it on the very first playback request.
      if (tokenSavePromise) {
        await tokenSavePromise;
      }
    }

    console.log(
      "[AppleMusic] authorize() finished. hasToken:",
      !!musicUserTokenRef.current,
    );
    if (musicUserTokenRef.current) {
      setIsAuthorized(true);
      setTokenExpired(false);
    }
  }, []);

  const unauthorize = useCallback(async () => {
    const mk = instanceRef.current;
    if (!mk) return;
    try {
      await mk.unauthorize();
    } catch {
      // Ignore errors
    }
    musicUserTokenRef.current = null;
    deleteTokenFromServer();
    setIsAuthorized(false);
  }, []);

  const play = useCallback(async () => {
    if (USE_BACKEND_AUDIO) {
      // Central engine already active for Apple Music — resume it
      if (centralEngine.getActiveProvider() === "apple-music") {
        try {
          setPlaybackError(null);
          mediaSessionRef.current?.requestPlay("music", "apple-music");
          centralEngine.resume();
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          setPlaybackError(msg);
          message.error(`Playback failed: ${msg}`);
        }
        return;
      }

      // Central engine not yet active (e.g. after restore from refresh) —
      // resolve the current track and start playback via backend.
      let track: MusicKit.Resource | null = null;
      if (
        isLocalQueueActiveRef.current &&
        localQueuePosRef.current >= 0 &&
        localQueuePosRef.current < localQueueRef.current.length
      ) {
        track = localQueueRef.current[localQueuePosRef.current];
      } else {
        const mk = instanceRef.current;
        track = mk?.nowPlayingItem ?? mk?.queue?.items?.[0] ?? null;
      }

      if (track) {
        try {
          setPlaybackError(null);
          let catalogId = getCatalogTrackId(track);
          if (!catalogId) {
            const id = String(track.id);
            if (id.startsWith("i.")) {
              catalogId = await resolveLibrarySongToCatalog(id);
            } else {
              catalogId = id;
            }
          }
          if (catalogId) {
            // Preserve restored playback position when resuming after refresh
            const resumeTime = currentPlaybackTimeRef.current;
            await playTrackViaBackend(
              catalogId,
              resumeTime > 0 ? resumeTime : undefined,
            );
            return;
          }
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          console.error("[AppleMusic] play() backend start failed:", msg);
          setPlaybackError(msg);
          message.error(`Playback failed: ${msg}`);
          return;
        }
      }
    }

    const mk = instanceRef.current;
    if (!mk) return;
    try {
      setPlaybackError(null);
      await mk.play();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error("[AppleMusic] play() failed:", msg);
      setPlaybackError(msg);
      message.error(`Playback failed: ${msg}`);
    }
  }, [message, playTrackViaBackend]);

  const pause = useCallback(() => {
    if (
      USE_BACKEND_AUDIO &&
      centralEngine.getActiveProvider() === "apple-music"
    ) {
      centralEngine.pause();
      return;
    }
    instanceRef.current?.pause();
  }, []);

  const stop = useCallback(() => {
    if (
      USE_BACKEND_AUDIO &&
      centralEngine.getActiveProvider() === "apple-music"
    ) {
      centralEngine.stop();
      return;
    }
    instanceRef.current?.stop();
  }, []);

  // ── Local queue playback ──

  const playLocalQueueItem = useCallback(
    async (index: number) => {
      const queue = localQueueRef.current;
      if (index < 0 || index >= queue.length) return;
      const track = queue[index];
      const songId = track.attributes?.playParams?.catalogId ?? track.id;
      localQueuePosRef.current = index;
      setQueuePosition(index);
      setPlaybackError(null);

      // Update now-playing metadata and show buffering indicator immediately,
      // before any async network calls, so the UI reacts on click.
      setNowPlayingItem(track as MusicKit.MediaItem);
      setHasEverPlayed(true);
      setPlaybackState(8 as MusicKit.PlaybackStates);

      const mk = instanceRef.current;
      if (!mk) return;
      try {
        console.log(
          `[AppleMusic] Local queue: playing ${index + 1}/${queue.length} — ${track.attributes?.name ?? songId}`,
        );

        if (USE_BACKEND_AUDIO) {
          // In backend-audio mode, we already have the full track metadata in
          // localQueueRef, so there's no need to call mk.setQueue (which would
          // hit Apple's API and add ~1-2s of latency before playback starts).
          // Resolve library IDs (i.xxx) to catalog IDs for the backend endpoint.
          let catalogId = songId;
          if (typeof catalogId === "string" && catalogId.startsWith("i.")) {
            catalogId =
              (await resolveLibrarySongToCatalog(catalogId)) ?? catalogId;
          }
          await playTrackViaBackend(catalogId);
        } else {
          await mk.setQueue({ song: songId, startPlaying: true });
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.error("[AppleMusic] Local queue play failed:", msg);
        // Try next song if this one fails
        if (index + 1 < queue.length) {
          await playLocalQueueItem(index + 1);
        } else {
          setPlaybackError(msg);
        }
      }
    },
    [playTrackViaBackend],
  );

  const skipToNext = useCallback(async () => {
    if (isLocalQueueActiveRef.current) {
      const queue = localQueueRef.current;
      let nextIdx: number;

      if (shuffleModeRef.current) {
        const pos = shufflePosRef.current + 1;
        if (pos < shuffleOrderRef.current.length) {
          shufflePosRef.current = pos;
          nextIdx = shuffleOrderRef.current[pos];
        } else {
          // Wrap: rebuild shuffle order
          shuffleOrderRef.current = buildShuffleOrder(queue.length, -1);
          shufflePosRef.current = 0;
          nextIdx = shuffleOrderRef.current[0];
        }
      } else {
        nextIdx = (localQueuePosRef.current + 1) % queue.length;
      }

      if (nextIdx >= 0 && nextIdx < queue.length) {
        await playLocalQueueItem(nextIdx);
      }
      return;
    }
    const mk = instanceRef.current;
    if (!mk) return;
    try {
      await mk.skipToNextItem();
    } catch (err) {
      console.error("[AppleMusic] skipToNext failed:", err);
    }
  }, [playLocalQueueItem]);

  const skipToPrevious = useCallback(async () => {
    if (isLocalQueueActiveRef.current) {
      // If >3s into current track, restart it
      if (
        USE_BACKEND_AUDIO &&
        centralEngine.getActiveProvider() === "apple-music" &&
        centralEngine.getCurrentTime() > 3
      ) {
        centralEngine.seek(0);
        return;
      }

      const queue = localQueueRef.current;
      let prevIdx: number;

      if (shuffleModeRef.current) {
        const pos = shufflePosRef.current - 1;
        if (pos >= 0) {
          shufflePosRef.current = pos;
          prevIdx = shuffleOrderRef.current[pos];
        } else {
          prevIdx = localQueuePosRef.current; // Stay on current
        }
      } else {
        prevIdx = (localQueuePosRef.current - 1 + queue.length) % queue.length;
      }

      if (prevIdx >= 0 && prevIdx < queue.length) {
        await playLocalQueueItem(prevIdx);
      }
      return;
    }
    const mk = instanceRef.current;
    if (!mk) return;
    try {
      await mk.skipToPreviousItem();
    } catch (err) {
      console.error("[AppleMusic] skipToPrevious failed:", err);
    }
  }, [playLocalQueueItem]);

  const seekToTime = useCallback(
    async (time: number) => {
      // Always update our own cached position
      setCurrentPlaybackTime(time);

      if (USE_BACKEND_AUDIO) {
        if (centralEngine.getActiveProvider() === "apple-music") {
          // We own the engine — seek directly
          centralEngine.seek(time);
        } else {
          // Another provider owns the engine — preempt with playback from this position
          const track = nowPlayingItemRef.current;
          if (track) {
            let catalogId = getCatalogTrackId(track);
            if (!catalogId) {
              const id = String(track.id);
              catalogId = id.startsWith("i.")
                ? await resolveLibrarySongToCatalog(id)
                : id;
            }
            if (catalogId) {
              await playTrackViaBackend(catalogId, time);
            }
          }
        }
        return;
      }
      const mk = instanceRef.current;
      if (!mk) return;
      await mk.seekToTime(time);
    },
    [playTrackViaBackend],
  );

  const setVolume = useCallback((vol: number) => {
    const clamped = Math.max(0, Math.min(1, vol));
    centralEngine.setVolume(clamped);
    const mk = instanceRef.current;
    if (mk) mk.volume = clamped;
    saveStoredAppleMusicVolume(clamped);
  }, []);

  const toggleShuffle = useCallback(() => {
    const mk = instanceRef.current;
    if (!mk) return;
    const wasOff = mk.shuffleMode === MusicKit.PlayerShuffleMode.off;
    mk.shuffleMode = wasOff
      ? MusicKit.PlayerShuffleMode.songs
      : MusicKit.PlayerShuffleMode.off;

    // Build/clear shuffle order for local queue (backend audio mode)
    if (isLocalQueueActiveRef.current && USE_BACKEND_AUDIO) {
      if (wasOff) {
        shuffleOrderRef.current = buildShuffleOrder(
          localQueueRef.current.length,
          localQueuePosRef.current,
        );
        shufflePosRef.current = 0;
      } else {
        shuffleOrderRef.current = [];
        shufflePosRef.current = 0;
      }
    }
  }, []);

  const cycleRepeatMode = useCallback(() => {
    const mk = instanceRef.current;
    if (!mk) return;
    // none → all → one → none
    const next =
      mk.repeatMode === MusicKit.PlayerRepeatMode.none
        ? MusicKit.PlayerRepeatMode.all
        : mk.repeatMode === MusicKit.PlayerRepeatMode.all
          ? MusicKit.PlayerRepeatMode.one
          : MusicKit.PlayerRepeatMode.none;
    mk.repeatMode = next;
  }, []);

  const setQueueFn = useCallback(
    async (options: MusicKit.SetQueueOptions) => {
      const mk = instanceRef.current;
      if (!mk) return;

      // Clear local queue — caller is using MusicKit's native queue
      isLocalQueueActiveRef.current = false;
      localQueueRef.current = [];
      localQueuePosRef.current = -1;

      try {
        setPlaybackError(null);
        console.log("[AppleMusic] setQueue:", options);

        // Show buffering immediately so the UI reacts on click, before the
        // (potentially slow) mk.setQueue call hits Apple's servers.
        if (options.startPlaying !== false) {
          setPlaybackState(8 as MusicKit.PlaybackStates);
        }

        if (USE_BACKEND_AUDIO) {
          // Load metadata into MusicKit but DON'T let it play natively
          await mk.setQueue({ ...options, startPlaying: false });

          // Handle library song fallback
          if (mk.queue.isEmpty && options.song?.startsWith("i.")) {
            console.warn(
              "[AppleMusic] Queue empty after setQueue — trying catalog fallback...",
            );
            const catalogId = await resolveLibrarySongToCatalog(options.song);
            if (catalogId) {
              await mk.setQueue({ song: catalogId });
              if (mk.queue.isEmpty) {
                const msg = "This song is no longer available in Apple Music";
                setPlaybackError(msg);
                message.error(msg);
                return;
              }
            }
          }

          // Play via backend if originally requested
          if (options.startPlaying !== false) {
            const item = mk.nowPlayingItem ?? mk.queue.items[0];
            if (item) {
              setNowPlayingItem(item);
              setHasEverPlayed(true);
              let trackId = getCatalogTrackId(item);
              if (!trackId && options.song) {
                // Resolve library ID
                if (options.song.startsWith("i.")) {
                  trackId = await resolveLibrarySongToCatalog(options.song);
                } else {
                  trackId = options.song;
                }
              }
              if (trackId) {
                await playTrackViaBackend(trackId);
              }
            }
          }
        } else {
          await mk.setQueue(options);

          // MusicKit silently fails for some library songs (no catalog equivalent).
          if (mk.queue.isEmpty && options.song?.startsWith("i.")) {
            console.warn(
              "[AppleMusic] Queue empty after setQueue — trying catalog fallback...",
            );
            const catalogId = await resolveLibrarySongToCatalog(options.song);
            if (catalogId) {
              await mk.setQueue({
                song: catalogId,
                startPlaying: options.startPlaying,
              });
              if (!mk.queue.isEmpty) return;
            }
            const msg = "This song is no longer available in Apple Music";
            setPlaybackError(msg);
            message.error(msg);
          }
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.error("[AppleMusic] setQueue failed:", msg, options);
        setPlaybackError(msg);
        message.error(`Cannot play this item: ${msg}`);
      }
    },
    [message, playTrackViaBackend],
  );

  /** Build a local queue from pre-fetched tracks and start playback. */
  const setQueueFromTracks = useCallback(
    async (tracks: MusicKit.Resource[], startIndex: number) => {
      if (tracks.length === 0) return;

      // Activate local queue
      localQueueRef.current = tracks;
      localQueuePosRef.current = startIndex;
      isLocalQueueActiveRef.current = true;

      // Build shuffle order if shuffle is active
      if (shuffleModeRef.current) {
        shuffleOrderRef.current = buildShuffleOrder(tracks.length, startIndex);
        shufflePosRef.current = 0;
      } else {
        shuffleOrderRef.current = [];
        shufflePosRef.current = 0;
      }

      // Populate queueItems for MediaSession display
      setQueueItems(tracks as MusicKit.MediaItem[]);
      setQueuePosition(startIndex);
      setHasEverPlayed(true);

      await playLocalQueueItem(startIndex);
    },
    [playLocalQueueItem],
  );

  const skipToQueueIndex = useCallback(
    async (index: number) => {
      if (isLocalQueueActiveRef.current) {
        await playLocalQueueItem(index);
        return;
      }
      const mk = instanceRef.current;
      if (!mk) return;
      try {
        await mk.changeToMediaAtIndex(index);
      } catch (err) {
        console.error("[AppleMusic] changeToMediaAtIndex failed:", err);
      }
    },
    [playLocalQueueItem],
  );

  // Auto-advance: when a song completes, play next from local queue.
  // For backend audio, subscribe to CentralMusicEngine's onEnded event
  // (MusicKit's playbackState never reaches "completed" in this mode).
  const handleEndedRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    handleEndedRef.current = () => {
      const isActive = isLocalQueueActiveRef.current;
      const pos = localQueuePosRef.current;
      const queue = localQueueRef.current;
      const rm = repeatModeRef.current;
      const shuffle = shuffleModeRef.current;

      console.log(
        "[AppleMusic] onEnded fired — localQueue=%s, pos=%d/%d, repeat=%d, shuffle=%s",
        isActive,
        pos,
        queue.length,
        rm,
        shuffle,
      );

      if (!isActive) return;

      // Repeat one: replay current track
      if (rm === 2) {
        console.log("[AppleMusic] repeat-one → replaying index", pos);
        playLocalQueueItem(pos);
        return;
      }

      let nextIdx: number;
      if (shuffle) {
        const sPos = shufflePosRef.current + 1;
        if (sPos < shuffleOrderRef.current.length) {
          shufflePosRef.current = sPos;
          nextIdx = shuffleOrderRef.current[sPos];
        } else if (rm === 1) {
          shuffleOrderRef.current = buildShuffleOrder(queue.length, -1);
          shufflePosRef.current = 0;
          nextIdx = shuffleOrderRef.current[0];
        } else {
          console.log("[AppleMusic] shuffle queue ended, no repeat — stopping");
          return;
        }
      } else {
        nextIdx = pos + 1;
        if (nextIdx >= queue.length) {
          if (rm === 1) {
            nextIdx = 0;
          } else {
            console.log("[AppleMusic] queue ended, no repeat — stopping");
            return;
          }
        }
      }

      console.log("[AppleMusic] auto-advance → playing index", nextIdx);
      if (nextIdx >= 0 && nextIdx < queue.length) {
        playLocalQueueItem(nextIdx);
      }
    };
  }, [playLocalQueueItem]);

  useEffect(() => {
    if (!USE_BACKEND_AUDIO) return;
    const unsubscribe = centralEngine.onEnded(() => {
      const provider = centralEngine.getActiveProvider();
      console.log("[AppleMusic] centralEngine.onEnded — provider=%s", provider);
      if (provider === "apple-music") {
        handleEndedRef.current?.();
      }
    });
    return unsubscribe;
  }, []);

  // Fallback: non-backend-audio auto-advance via MusicKit playbackState
  useEffect(() => {
    if (USE_BACKEND_AUDIO) return;
    if (!isLocalQueueActiveRef.current) return;
    if (playbackState !== (10 as MusicKit.PlaybackStates)) return;

    const nextIdx = localQueuePosRef.current + 1;
    if (nextIdx < localQueueRef.current.length) {
      playLocalQueueItem(nextIdx);
    }
  }, [playbackState, playLocalQueueItem]);

  const playNext = useCallback(async (options: MusicKit.SetQueueOptions) => {
    const mk = instanceRef.current;
    if (!mk) return;
    await mk.queue.prepend(options);
  }, []);

  const playLater = useCallback(async (options: MusicKit.SetQueueOptions) => {
    const mk = instanceRef.current;
    if (!mk) return;
    await mk.queue.append(options);
  }, []);

  // ── Navigation ──

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

  // ── API helper ──

  const apiHelper = useCallback(
    async (
      path: string,
      params?: Record<string, unknown>,
    ): Promise<MusicKit.APIResponse> => {
      // Use backend proxy instead of MusicKit's api.music() because MusicKit
      // revokes authorization on non-apple.com origins (scraped token restriction).
      // The backend reads the music-user-token from the user's DB settings.

      // Detect full URLs vs relative paths
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
        // When the backend signals that the stored music-user-token was
        // explicitly rejected by Apple (401/403), clear auth state so the
        // user is prompted to re-authorize. This is the ONLY place we trigger
        // de-auth — doing it here (user-initiated API calls) avoids false
        // logouts from MusicKit's own background requests.
        if (resp.headers.get("x-apple-music-token-expired") === "true") {
          console.warn(
            "[AppleMusic] Token explicitly rejected by Apple — clearing auth state",
          );
          window.dispatchEvent(new CustomEvent("apple-music-token-expired"));
        }
        let detail = "";
        try {
          const text = await resp.text();
          detail = text.slice(0, 500);
        } catch {
          // ignore read errors
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

  const restorePlaybackState = useCallback(
    async (state: NonNullable<PlaybackStateData["appleMusic"]>) => {
      const mk = instanceRef.current;
      if (!mk) return;

      mk.shuffleMode = state.shuffleMode
        ? MusicKit.PlayerShuffleMode.songs
        : MusicKit.PlayerShuffleMode.off;
      mk.repeatMode =
        state.repeatMode === 1
          ? MusicKit.PlayerRepeatMode.one
          : state.repeatMode === 2
            ? MusicKit.PlayerRepeatMode.all
            : MusicKit.PlayerRepeatMode.none;
      setShuffleMode(state.shuffleMode);
      setRepeatMode(state.repeatMode);

      const savedQueueItems =
        state.queueItems?.map((item) => ({
          id: item.id,
          type: item.type,
          attributes: {
            name: item.attributes?.name,
            artistName: item.attributes?.artistName,
            albumName: item.attributes?.albumName,
            artwork: item.attributes
              ?.artwork as MusicKit.ResourceAttributes["artwork"],
            durationInMillis: item.attributes?.durationInMillis,
            playParams: item.attributes?.playParams,
          },
        })) ?? [];

      if (savedQueueItems.length > 0) {
        localQueueRef.current = savedQueueItems as MusicKit.Resource[];
        localQueuePosRef.current = Math.min(
          Math.max(state.currentIndex, 0),
          savedQueueItems.length - 1,
        );
        isLocalQueueActiveRef.current = true;

        const currentItem =
          (savedQueueItems[localQueuePosRef.current] as MusicKit.MediaItem) ??
          null;
        setQueueItems(savedQueueItems as MusicKit.MediaItem[]);
        setQueuePosition(localQueuePosRef.current);
        setNowPlayingItem(currentItem);
        setCurrentPlaybackTime(state.currentTime);
        setCurrentPlaybackDuration(
          currentItem?.attributes?.durationInMillis
            ? currentItem.attributes.durationInMillis / 1000
            : 0,
        );

        // Keep MusicKit's internal queue loosely aligned for metadata lookups,
        // but preserve our restored local queue for system controls.
        if (state.songIds.length > 0) {
          await mk
            .setQueue({ songs: state.songIds, startPlaying: false })
            .catch(() => {});
          if (
            state.currentIndex > 0 &&
            state.currentIndex < state.songIds.length
          ) {
            await mk.changeToMediaAtIndex(state.currentIndex).catch(() => {});
          }
        }
        return;
      }

      if (state.songIds.length === 0) return;

      await mk.setQueue({ songs: state.songIds, startPlaying: false });
      if (state.currentIndex > 0 && state.currentIndex < state.songIds.length) {
        await mk.changeToMediaAtIndex(state.currentIndex);
      }
      setCurrentPlaybackTime(state.currentTime);
    },
    [],
  );

  // ── MediaSession integration + state persistence ──

  useAppleMusicSession({
    isConfigured,
    playbackState,
    nowPlayingItem,
    currentPlaybackTime,
    currentPlaybackDuration,
    volume,
    shuffleMode,
    repeatMode,
    queueItems,
    queuePosition,
    hasEverPlayed,
    initialData: mediaSession?.rawPlaybackData ?? null,
    initialDataReady: mediaSession?.rawPlaybackDataReady ?? true,
    play,
    pause,
    seekToTime,
    setVolume,
    restorePlaybackState,
    skipToNext,
    skipToPrevious,
    skipToQueueIndex,
  });

  // ── Context value ──

  // MusicKit playback states: 0=none, 1=loading, 2=playing, 3=paused, 8=waiting, 9=stalled, 10=completed
  const isBuffering =
    playbackState === (1 as MusicKit.PlaybackStates) ||
    playbackState === (8 as MusicKit.PlaybackStates) ||
    playbackState === (9 as MusicKit.PlaybackStates);

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
      hasEverPlayed,
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
      isBuffering,
      hasEverPlayed,
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
