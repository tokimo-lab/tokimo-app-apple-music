/**
 * Registers Apple Music as a MediaSession source and handles
 * playback state persistence (save/restore across page reloads).
 *
 * Audio playback uses the backend decrypt pipeline (USE_BACKEND_AUDIO = true
 * in AppleMusicProvider) which streams full-length decrypted audio via
 * /api/apps/apple-music/audio/:track_id. MusicKit is used only for metadata and
 * queue management.
 *
 * Extracted from AppleMusicProvider to keep the provider ≤ 500 lines.
 */

import type { MediaSessionQueueItem } from "@tokimo/sdk";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PlaybackStateData } from "../api-types/PlaybackStateData";
import { usePlaybackStatePersistence } from "../hooks/usePlaybackStatePersistence";
import * as centralEngine from "../shell/engine-ref";
import {
  useMediaSessionOptional,
  useMediaSessionRegister,
} from "../shell/hooks";
import { formatArtworkUrl } from "./types";

interface AppleMusicSessionInput {
  isConfigured: boolean;
  playbackState: MusicKit.PlaybackStates;
  nowPlayingItem: MusicKit.MediaItem | null;
  currentPlaybackTime: number;
  currentPlaybackDuration: number;
  volume: number;
  shuffleMode: boolean;
  repeatMode: number;
  queueItems: MusicKit.MediaItem[];
  queuePosition: number;
  hasEverPlayed: boolean;
  /** Pre-fetched server state from MediaSessionContext (single GET point). */
  initialData: PlaybackStateData | null;
  /** True once initialData has been populated. */
  initialDataReady: boolean;
  play: () => Promise<void>;
  pause: () => void;
  seekToTime: (time: number) => Promise<void>;
  setVolume: (volume: number) => void;
  restorePlaybackState: (
    state: NonNullable<PlaybackStateData["appleMusic"]>,
  ) => Promise<void>;
  skipToNext: () => Promise<void>;
  skipToPrevious: () => Promise<void>;
  skipToQueueIndex: (index: number) => Promise<void>;
}

export function useAppleMusicSession(input: AppleMusicSessionInput): void {
  const {
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
    initialData,
    initialDataReady,
    play,
    pause,
    seekToTime,
    setVolume,
    restorePlaybackState,
    skipToNext,
    skipToPrevious,
    skipToQueueIndex,
  } = input;
  const [restoreComplete, setRestoreComplete] = useState(false);

  // True once restore fetch returned Apple Music data — used to show the
  // player UI after a refresh, without conflating "has restored" with
  // "user has actively played" (which is hasEverPlayed).
  const [hasRestoredState, setHasRestoredState] = useState(false);

  // For notifySaveNeeded — single write authority lives in MediaSessionContext
  const mediaSession = useMediaSessionOptional();
  const mediaSessionRef = useRef(mediaSession);
  mediaSessionRef.current = mediaSession;

  // ── Web Audio API analyser from central engine ──

  const getAnalyser = useCallback(() => centralEngine.getAnalyser(), []);

  // ── MediaSession registration ──

  const isPlaying = playbackState === (2 as MusicKit.PlaybackStates);

  const mediaSessionQueue = useMemo<MediaSessionQueueItem[]>(
    () =>
      queueItems.map((item) => ({
        id: item.id,
        title: item.attributes?.name ?? "Unknown",
        artist: item.attributes?.artistName ?? undefined,
        artwork: formatArtworkUrl(item.attributes?.artwork, 128) || undefined,
        duration: item.attributes?.durationInMillis
          ? item.attributes.durationInMillis / 1000
          : undefined,
      })),
    [queueItems],
  );

  const currentTimeRef = useRef(currentPlaybackTime);
  currentTimeRef.current = currentPlaybackTime;
  const durationRef = useRef(currentPlaybackDuration);
  durationRef.current = currentPlaybackDuration;

  // Each app owns its own progress. Just return stored values.
  const getCurrentTime = useCallback(() => {
    return currentTimeRef.current;
  }, []);
  const getDuration = useCallback(() => durationRef.current, []);

  const seek = useCallback(
    async (time: number) => {
      await seekToTime(time);
    },
    [seekToTime],
  );

  // ── Playback state persistence ──

  const saveQueueItemsRef = useRef(queueItems);
  saveQueueItemsRef.current = queueItems;
  const saveQueuePositionRef = useRef(queuePosition);
  saveQueuePositionRef.current = queuePosition;
  const saveShuffleModeRef = useRef(shuffleMode);
  saveShuffleModeRef.current = shuffleMode;
  const saveRepeatModeRef = useRef(repeatMode);
  saveRepeatModeRef.current = repeatMode;
  const saveNowPlayingRef = useRef(nowPlayingItem);
  saveNowPlayingRef.current = nowPlayingItem;

  const buildState = useCallback(() => {
    const items = saveQueueItemsRef.current;
    const np = saveNowPlayingRef.current;
    return {
      provider: "apple-music" as const,
      queue: [],
      songIds: items.map((item) => item.id),
      currentIndex: saveQueuePositionRef.current,
      currentTime: currentTimeRef.current,
      shuffleEnabled: saveShuffleModeRef.current,
      repeatMode: String(saveRepeatModeRef.current),
      repeatModeValue: saveRepeatModeRef.current,
      shuffleMode: saveShuffleModeRef.current,
      queueItems: items.map((item) => ({
        id: item.id,
        type: item.type,
        attributes: {
          name: item.attributes?.name,
          artistName: item.attributes?.artistName,
          albumName: item.attributes?.albumName,
          artwork: item.attributes?.artwork,
          durationInMillis: item.attributes?.durationInMillis,
          playParams: item.attributes?.playParams
            ? {
                id: item.attributes.playParams.id,
                catalogId: item.attributes.playParams.catalogId,
                kind: item.attributes.playParams.kind,
                isLibrary: item.attributes.playParams.isLibrary,
              }
            : undefined,
        },
      })),
      nowPlaying: np
        ? {
            title: np.attributes?.name ?? "",
            artistName: np.attributes?.artistName ?? "",
            albumName: np.attributes?.albumName ?? "",
            artworkUrl: formatArtworkUrl(np.attributes?.artwork, 256),
            duration: (np.attributes?.durationInMillis ?? 0) / 1000,
          }
        : undefined,
    };
  }, []);

  const appleMediaSource = useMemo(() => {
    if (!restoreComplete) return null;
    if (!nowPlayingItem && !hasRestoredState && !hasEverPlayed) return null;
    if (mediaSessionQueue.length === 0 && !nowPlayingItem) return null;

    const artwork =
      formatArtworkUrl(nowPlayingItem?.attributes?.artwork, 256) || undefined;
    const trackId =
      nowPlayingItem?.attributes?.playParams?.catalogId ?? nowPlayingItem?.id;

    return {
      id: "music" as const,
      type: "music" as const,
      provider: "apple-music" as const,
      trackId: trackId ? String(trackId) : undefined,
      label: "音乐",
      title: nowPlayingItem?.attributes?.name ?? "音乐",
      artist: nowPlayingItem?.attributes?.artistName ?? undefined,
      album:
        nowPlayingItem?.attributes?.albumName ??
        nowPlayingItem?.container?.name ??
        undefined,
      artwork,
      isPlaying,
      getCurrentTime,
      getDuration,
      volume,
      play: () => {
        play().catch(() => {});
      },
      pause,
      seek,
      setVolume,
      next: skipToNext,
      previous: skipToPrevious,
      queue: mediaSessionQueue,
      currentIndex: queuePosition,
      skipToIndex: skipToQueueIndex,
      getAnalyser,
      buildPersistState: buildState,
    };
  }, [
    nowPlayingItem,
    hasEverPlayed,
    isPlaying,
    getCurrentTime,
    getDuration,
    volume,
    play,
    pause,
    seek,
    setVolume,
    skipToNext,
    skipToPrevious,
    mediaSessionQueue,
    queuePosition,
    restoreComplete,
    skipToQueueIndex,
    getAnalyser,
    hasRestoredState,
    buildState,
  ]);

  useMediaSessionRegister(appleMediaSource);

  const { didRestoreRef } = usePlaybackStatePersistence({
    ready: isConfigured,
    initialData,
    initialDataReady,
    onRestore: async (data: PlaybackStateData) => {
      const musicState = data?.music;
      const am =
        musicState?.provider === "apple-music" &&
        Array.isArray(musicState.songIds) &&
        musicState.songIds.length > 0
          ? {
              songIds: musicState.songIds,
              currentIndex: musicState.currentIndex,
              currentTime: musicState.currentTime,
              shuffleMode: musicState.shuffleMode ?? musicState.shuffleEnabled,
              repeatMode:
                typeof musicState.repeatModeValue === "number"
                  ? musicState.repeatModeValue
                  : Number(musicState.repeatMode) || 0,
              queueItems: musicState.queueItems,
              nowPlaying: musicState.nowPlaying,
            }
          : data?.appleMusic;
      if (!am || !Array.isArray(am.songIds) || am.songIds.length === 0) {
        setRestoreComplete(true);
        return;
      }
      try {
        await restorePlaybackState(am);
        // Mark that we restored Apple Music data so the player UI appears
        // after a refresh, without triggering any save.
        setHasRestoredState(true);
      } catch (err) {
        console.warn("[AppleMusic] Failed to restore state:", err);
      } finally {
        setRestoreComplete(true);
      }
    },
  });

  // Queue, now-playing and control-state changes: notify MediaSessionContext
  // (single write authority) to schedule a save. Gated on hasEverPlayed so
  // restored-but-never-played state never overwrites another provider.
  // biome-ignore lint/correctness/useExhaustiveDependencies: intentionally triggers on queue/position changes
  useEffect(() => {
    if (!didRestoreRef.current) return;
    if (!hasEverPlayed) return;
    mediaSessionRef.current?.notifySaveNeeded("music", "apple-music");
  }, [
    queueItems,
    queuePosition,
    shuffleMode,
    repeatMode,
    nowPlayingItem,
    hasEverPlayed,
  ]);

  // Current playback time is high-frequency — still notify (MediaSessionContext debounces).
  // biome-ignore lint/correctness/useExhaustiveDependencies: currentPlaybackTime is used as a trigger only
  useEffect(() => {
    if (!didRestoreRef.current) return;
    if (!hasEverPlayed) return;
    mediaSessionRef.current?.notifySaveNeeded("music", "apple-music");
  }, [currentPlaybackTime, hasEverPlayed]);
}
