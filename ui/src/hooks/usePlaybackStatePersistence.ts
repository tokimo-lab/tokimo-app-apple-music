import type { MutableRefObject } from "react";
import { useEffect, useRef } from "react";
import type { PlaybackStateData } from "../api-types/PlaybackStateData";

export interface UsePlaybackStatePersistenceOptions {
  onRestore: (data: PlaybackStateData) => void | Promise<void>;
  initialData: PlaybackStateData | null;
  initialDataReady: boolean;
  ready?: boolean;
}

export interface UsePlaybackStatePersistenceResult {
  didRestoreRef: MutableRefObject<boolean>;
}

export function usePlaybackStatePersistence(
  options: UsePlaybackStatePersistenceOptions,
): UsePlaybackStatePersistenceResult {
  const { onRestore, initialData, initialDataReady, ready = true } = options;
  const didRestoreRef = useRef(false);
  const onRestoreRef = useRef(onRestore);
  onRestoreRef.current = onRestore;
  const initialDataRef = useRef(initialData);
  initialDataRef.current = initialData;

  useEffect(() => {
    if (!ready || !initialDataReady || didRestoreRef.current) return;
    didRestoreRef.current = true;
    const data = initialDataRef.current;
    if (data) {
      const result = onRestoreRef.current(data);
      if (result instanceof Promise) result.catch(() => {});
    }
  }, [ready, initialDataReady]);

  return { didRestoreRef };
}
