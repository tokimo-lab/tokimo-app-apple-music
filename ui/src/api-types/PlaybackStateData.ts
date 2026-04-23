// Copied from packages/web/src/generated/rust-api/playback.ts
// Only the PlaybackStateData type is needed here
export interface PlaybackStateData {
  music?: {
    provider?: "local" | "apple-music";
    queue: unknown[];
    currentIndex: number;
    currentTime: number;
    repeatMode: string;
    shuffleEnabled: boolean;
    repeatModeValue?: number;
    songIds?: string[];
    queueItems?: Array<{
      id: string;
      type: string;
      attributes?: {
        name?: string;
        artistName?: string;
        albumName?: string;
        artwork?: unknown;
        durationInMillis?: number;
        playParams?: { id?: string; catalogId?: string; kind?: string; isLibrary?: boolean };
      };
    }>;
    nowPlaying?: { title: string; artistName: string; albumName: string; artworkUrl: string; duration: number };
    shuffleMode?: boolean;
  };
  appleMusic?: {
    songIds: string[];
    currentIndex: number;
    currentTime: number;
    shuffleMode: boolean;
    repeatMode: number;
    queueItems?: Array<{
      id: string;
      type: string;
      attributes?: {
        name?: string;
        artistName?: string;
        albumName?: string;
        artwork?: unknown;
        durationInMillis?: number;
        playParams?: { id?: string; catalogId?: string; kind?: string; isLibrary?: boolean };
      };
    }>;
    nowPlaying?: { title: string; artistName: string; albumName: string; artworkUrl: string; duration: number };
  };
}
