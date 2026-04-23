import { Button, Spin } from "@tokimo/ui";
import { Music } from "lucide-react";
import { useState } from "react";
import { useAppleMusic } from "./AppleMusicProvider";

const APPLE_MUSIC_RED = "#FA2D48";

/**
 * Full-screen login prompt shown when the user hasn't signed in
 * with their Apple ID yet. Clicking "Sign In with Apple" triggers
 * MusicKit JS's OAuth popup.
 */
export function AppleMusicLogin() {
  const { authorize, isReady } = useAppleMusic();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSignIn(): Promise<void> {
    setLoading(true);
    setError(null);
    try {
      await authorize();
    } catch (e) {
      setError(
        e instanceof Error ? e.message : "Sign in failed. Please try again.",
      );
    } finally {
      setLoading(false);
    }
  }

  if (!isReady) {
    return (
      <div className="flex h-full items-center justify-center bg-transparent">
        <Spin />
      </div>
    );
  }

  return (
    <div className="flex h-full items-center justify-center bg-transparent">
      <div className="flex max-w-sm flex-col items-center gap-6 px-8 text-center">
        {/* Icon */}
        <div
          className="flex h-20 w-20 items-center justify-center rounded-[22px] shadow-lg"
          style={{
            background: `linear-gradient(135deg, ${APPLE_MUSIC_RED}, #8C1D30)`,
          }}
        >
          <Music className="h-10 w-10 text-white" />
        </div>

        {/* Title */}
        <div className="space-y-2">
          <h1 className="text-2xl font-bold text-[var(--text-primary)]">
            Apple Music
          </h1>
          <p className="text-sm leading-relaxed text-[var(--text-tertiary)]">
            Sign in with your Apple ID to listen to over 100 million songs,
            access your library, and get personalized recommendations.
          </p>
        </div>

        {/* Error */}
        {error && <p className="text-sm text-red-500">{error}</p>}

        {/* Sign In button */}
        <Button
          variant="primary"
          size="large"
          onClick={handleSignIn}
          loading={loading}
          icon={<Music className="h-4 w-4" />}
          style={{
            backgroundColor: APPLE_MUSIC_RED,
            borderColor: APPLE_MUSIC_RED,
            paddingLeft: 32,
            paddingRight: 32,
          }}
        >
          Sign In with Apple
        </Button>

        {/* Note */}
        <p className="text-xs text-[var(--text-tertiary)]">
          Requires an Apple Music subscription.
        </p>
      </div>
    </div>
  );
}
