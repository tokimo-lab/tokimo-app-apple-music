import { Button, Spin } from "@tokimo/ui";
import { LogIn, Sparkles } from "lucide-react";
import { useEffect, useState } from "react";
import { useAppleMusic } from "../AppleMusicProvider";
import { MediaItemCard } from "../components/MediaItemCard";
import { useArtistNavigation } from "../hooks/useArtistNavigation";

interface RecommendationGroup {
  id: string;
  title: string;
  items: MusicKit.Resource[];
}

export default function ForYouPage() {
  const { api, isAuthorized, authorize, navigateTo, setQueue } =
    useAppleMusic();
  const navigateToArtist = useArtistNavigation();
  const [recentlyPlayed, setRecentlyPlayed] = useState<MusicKit.Resource[]>([]);
  const [recommendations, setRecommendations] = useState<RecommendationGroup[]>(
    [],
  );
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isAuthorized) {
      setLoading(false);
      return;
    }
    let cancelled = false;

    async function fetchForYou() {
      setLoading(true);
      setError(null);
      try {
        const [recentRes, recsRes] = await Promise.allSettled([
          api("/v1/me/recent/played"),
          api("/v1/me/recommendations"),
        ]);

        if (cancelled) return;

        if (recentRes.status === "fulfilled") {
          setRecentlyPlayed(recentRes.value?.data?.data ?? []);
        }

        if (recsRes.status === "fulfilled") {
          const data = recsRes.value?.data?.data ?? [];
          const groups: RecommendationGroup[] = data
            .map((rec) => {
              const attrs = rec.attributes;
              // title may be a string or { stringForDisplay: string }
              const rawTitle = attrs?.name ?? "For You";
              const contents = rec.relationships?.contents?.data ?? [];

              return {
                id: rec.id,
                title: rawTitle,
                items: contents,
              };
            })
            .filter((g) => g.items.length > 0);

          setRecommendations(groups);
        }
      } catch (err) {
        if (!cancelled) {
          setError(
            err instanceof Error
              ? err.message
              : "Failed to load recommendations",
          );
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    fetchForYou();
    return () => {
      cancelled = true;
    };
  }, [isAuthorized, api]);

  if (!isAuthorized) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-4 text-[var(--text-secondary)]">
        <Sparkles size={48} strokeWidth={1} />
        <p className="text-base font-medium">
          Sign in for personalized recommendations
        </p>
        <p className="text-sm text-[var(--text-tertiary)]">
          Connect your Apple Music account to see music picked just for you
        </p>
        <Button
          variant="primary"
          shape="round"
          icon={<LogIn size={16} />}
          onClick={authorize}
          style={{ backgroundColor: "#FA2D48", borderColor: "#FA2D48" }}
        >
          Sign In
        </Button>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spin spinning tip="Loading recommendations…" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3">
        <p className="text-sm text-[var(--text-secondary)]">{error}</p>
      </div>
    );
  }

  const handlePlay = (item: MusicKit.Resource) => {
    const t = item.type ?? "";
    if (t.includes("album")) {
      setQueue({ album: item.id, startPlaying: true });
    } else if (t.includes("playlist")) {
      setQueue({ playlist: item.id, startPlaying: true });
    } else if (t.includes("station")) {
      setQueue({ station: item.id, startPlaying: true });
    }
  };

  const handleNavigate = (item: MusicKit.Resource) => {
    const t = item.type ?? "";
    if (t.includes("album")) {
      navigateTo({ type: "album", id: item.id });
    } else if (t.includes("playlist")) {
      navigateTo({ type: "playlist", id: item.id });
    } else if (t.includes("artist")) {
      navigateTo({ type: "artist", id: item.id });
    } else if (t.includes("station")) {
      setQueue({ station: item.id, startPlaying: true });
    }
  };

  const resolveCardType = (
    item: MusicKit.Resource,
  ): "album" | "playlist" | "artist" => {
    const t = item.type ?? "";
    if (t.includes("artist")) return "artist";
    if (t.includes("playlist")) return "playlist";
    return "album";
  };

  const getSubtitleClick = (item: MusicKit.Resource) => {
    const t = item.type ?? "";
    if (t.includes("album")) {
      const name = item.attributes?.artistName;
      return name ? () => navigateToArtist(name, item) : undefined;
    }
    return undefined;
  };

  const hasContent = recentlyPlayed.length > 0 || recommendations.length > 0;

  if (!hasContent) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[var(--text-tertiary)]">
        No recommendations available yet. Listen to more music!
      </div>
    );
  }

  return (
    <div className="h-full space-y-8 overflow-y-auto p-6">
      {recentlyPlayed.length > 0 && (
        <section>
          <h2 className="mb-4 text-xl font-bold text-[var(--text-primary)]">
            Recently Played
          </h2>
          <div className="flex gap-4 overflow-x-auto pb-2">
            {recentlyPlayed.map((item) => (
              <MediaItemCard
                key={item.id}
                item={item}
                type={resolveCardType(item)}
                onClick={() => handleNavigate(item)}
                onPlay={() => handlePlay(item)}
                onSubtitleClick={getSubtitleClick(item)}
              />
            ))}
          </div>
        </section>
      )}

      {recommendations.map((group) => (
        <section key={group.id}>
          <h2 className="mb-4 text-xl font-bold text-[var(--text-primary)]">
            {group.title}
          </h2>
          <div className="flex gap-4 overflow-x-auto pb-2">
            {group.items.map((item) => (
              <MediaItemCard
                key={item.id}
                item={item}
                type={resolveCardType(item)}
                onClick={() => handleNavigate(item)}
                onPlay={() => handlePlay(item)}
                onSubtitleClick={getSubtitleClick(item)}
              />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
