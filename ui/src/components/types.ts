// ── MusicKit JS v3 type declarations ──

declare global {
  namespace MusicKit {
    function configure(config: Configuration): Promise<MusicKitInstance>;
    function getInstance(): MusicKitInstance;
    function formatArtworkURL(
      artwork: Artwork,
      width: number,
      height?: number,
    ): string;

    interface Configuration {
      developerToken: string;
      app: { name: string; build: string; icon?: string };
      bitrate?: PlaybackBitrate;
    }

    interface MusicKitInstance {
      authorize(): Promise<string>;
      unauthorize(): Promise<void>;
      isAuthorized: boolean;
      developerToken: string;
      musicUserToken: string;
      storefrontId: string;
      storefrontCountryCode: string;

      // Playback
      play(): Promise<void>;
      pause(): void;
      stop(): void;
      skipToNextItem(): Promise<void>;
      skipToPreviousItem(): Promise<void>;
      seekToTime(time: number): Promise<void>;
      setQueue(options: SetQueueOptions): Promise<Queue>;
      changeToMediaAtIndex(index: number): Promise<void>;

      volume: number;
      currentPlaybackTime: number;
      currentPlaybackDuration: number;
      currentPlaybackTimeRemaining: number;
      playbackState: PlaybackStates;
      repeatMode: PlayerRepeatMode;
      shuffleMode: PlayerShuffleMode;
      nowPlayingItem: MediaItem | null;
      queue: Queue;

      // API
      api: API;

      // Events
      addEventListener(
        name: string,
        callback: (...args: unknown[]) => void,
      ): void;
      removeEventListener(
        name: string,
        callback: (...args: unknown[]) => void,
      ): void;
    }

    interface API {
      music(
        path: string,
        queryParameters?: Record<string, unknown>,
      ): Promise<APIResponse>;
    }

    interface APIResponse {
      data: {
        results?: Record<string, { data: Resource[] }>;
        data?: Resource[];
        next?: string;
      };
    }

    interface Resource {
      id: string;
      type: string;
      href?: string;
      attributes?: ResourceAttributes;
      relationships?: Record<
        string,
        { data: Resource[]; next?: string; href?: string }
      >;
    }

    interface ResourceAttributes {
      name?: string;
      artistName?: string;
      albumName?: string;
      artwork?: Artwork;
      durationInMillis?: number;
      url?: string;
      playParams?: PlayParameters;
      contentRating?: string;
      genreNames?: string[];
      releaseDate?: string;
      trackNumber?: number;
      discNumber?: number;
      description?: { standard?: string; short?: string };
      editorialNotes?: { standard?: string; short?: string };
      curatorName?: string;
      lastModifiedDate?: string;
      isChart?: boolean;
      trackCount?: number;
      copyright?: string;
      [key: string]: unknown;
    }

    interface Artwork {
      url: string;
      width: number;
      height: number;
      bgColor?: string;
      textColor1?: string;
      textColor2?: string;
      textColor3?: string;
      textColor4?: string;
    }

    interface PlayParameters {
      id: string;
      kind: string;
      catalogId?: string;
      isLibrary?: boolean;
    }

    interface MediaItem extends Resource {
      container?: { id: string; type: string; name?: string };
    }

    interface Queue {
      items: MediaItem[];
      position: number;
      length: number;
      isEmpty: boolean;
      nextPlayableItemIndex?: number;
      previousPlayableItemIndex?: number;
      append(options: SetQueueOptions): Promise<void>;
      prepend(options: SetQueueOptions): Promise<void>;
    }

    interface SetQueueOptions {
      album?: string;
      song?: string;
      songs?: string[];
      playlist?: string;
      station?: string;
      url?: string;
      startWith?: number;
      startPlaying?: boolean;
    }

    enum PlaybackStates {
      none = 0,
      loading = 1,
      playing = 2,
      paused = 3,
      stopped = 4,
      ended = 5,
      seeking = 6,
      waiting = 8,
      stalled = 9,
      completed = 10,
    }

    enum PlaybackBitrate {
      HIGH = 256,
      STANDARD = 64,
    }

    enum PlayerRepeatMode {
      none = 0,
      one = 1,
      all = 2,
    }

    enum PlayerShuffleMode {
      off = 0,
      songs = 1,
    }

    enum Events {
      authorizationStatusDidChange = "authorizationStatusDidChange",
      playbackStateDidChange = "playbackStateDidChange",
      nowPlayingItemDidChange = "nowPlayingItemDidChange",
      playbackTimeDidChange = "playbackTimeDidChange",
      playbackDurationDidChange = "playbackDurationDidChange",
      playbackVolumeDidChange = "playbackVolumeDidChange",
      queueItemsDidChange = "queueItemsDidChange",
      queuePositionDidChange = "queuePositionDidChange",
      shuffleModeDidChange = "shuffleModeDidChange",
      repeatModeDidChange = "repeatModeDidChange",
      storefrontCountryCodeDidChange = "storefrontCountryCodeDidChange",
      mediaPlaybackError = "mediaPlaybackError",
    }
  }
}

// ── Exported utility types ──

export type AppleMusicPage =
  | { type: "browse" }
  | { type: "for-you" }
  | { type: "search"; query?: string }
  | { type: "library"; tab?: "songs" | "albums" | "artists" | "playlists" }
  | { type: "album"; id: string; isLibrary?: boolean }
  | { type: "artist"; id: string }
  | { type: "playlist"; id: string; isLibrary?: boolean }
  | { type: "now-playing" }
  | { type: "setup" };

/**
 * Replaces `{w}x{h}` tokens in an Apple Music artwork URL template.
 * Returns empty string if artwork is undefined.
 */
export function formatArtworkUrl(
  artwork: MusicKit.Artwork | undefined,
  size: number,
): string {
  if (!artwork?.url) return "";
  return artwork.url.replace("{w}", String(size)).replace("{h}", String(size));
}

/** Format milliseconds → "m:ss" */
export function formatDuration(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

/** Format seconds → "m:ss" */
export function formatDurationSeconds(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}
