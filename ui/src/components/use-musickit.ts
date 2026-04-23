import { useEffect, useState } from "react";

const MUSICKIT_CDN = "https://js-cdn.music.apple.com/musickit/v3/musickit.js";

// Module-level singleton promise so the script is only loaded once
let loaderPromise: Promise<void> | null = null;

function loadMusicKitScript(): Promise<void> {
  if (loaderPromise) return loaderPromise;

  loaderPromise = new Promise<void>((resolve, reject) => {
    // Already loaded (e.g. via a <script> tag in HTML)
    if (typeof window !== "undefined" && window.MusicKit) {
      resolve();
      return;
    }

    const existing = document.querySelector(`script[src="${MUSICKIT_CDN}"]`);
    if (existing) {
      existing.addEventListener("load", () => resolve());
      existing.addEventListener("error", () =>
        reject(new Error("Failed to load MusicKit JS")),
      );
      return;
    }

    const script = document.createElement("script");
    script.src = MUSICKIT_CDN;
    script.async = true;
    script.crossOrigin = "anonymous";
    script.addEventListener("load", () => resolve());
    script.addEventListener("error", () =>
      reject(new Error("Failed to load MusicKit JS from Apple CDN")),
    );
    document.head.appendChild(script);
  });

  return loaderPromise;
}

/**
 * React hook that dynamically loads MusicKit JS v3 from Apple's CDN.
 * Only loads once — subsequent calls share the same loader promise.
 */
export function useMusicKitLoader(): {
  isLoaded: boolean;
  error: string | null;
} {
  const [isLoaded, setIsLoaded] = useState(
    () => typeof window !== "undefined" && !!window.MusicKit,
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (isLoaded) return;

    let cancelled = false;
    loadMusicKitScript()
      .then(() => {
        if (!cancelled) setIsLoaded(true);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(
            err instanceof Error ? err.message : "Failed to load MusicKit JS",
          );
        }
      });

    return () => {
      cancelled = true;
    };
  }, [isLoaded]);

  return { isLoaded, error };
}
