interface ResourceWithPlayParams {
  id: string;
  attributes?: {
    playParams?: {
      catalogId?: string;
    };
  };
}

export function getCatalogTrackId(item: ResourceWithPlayParams): string | null {
  const catalogId = item.attributes?.playParams?.catalogId;
  if (catalogId) return String(catalogId);
  const id = String(item.id);
  if (/^\d+$/.test(id)) return id;
  return null;
}

export function getAppleMusicArtworkUrl(
  artwork: unknown,
  size: number,
): string | undefined {
  if (typeof artwork === "string") return artwork;
  if (!artwork || typeof artwork !== "object") return undefined;
  const url = (artwork as { url?: unknown }).url;
  if (typeof url !== "string" || url.length === 0) return undefined;
  return url.replace(/\{w\}/g, String(size)).replace(/\{h\}/g, String(size));
}

const _catalogIdCache = new Map<string, string>();

/** Try to resolve a library song ID to a catalog song ID via Apple Music proxy. */
export async function resolveLibrarySongToCatalog(
  librarySongId: string,
): Promise<string | null> {
  const cached = _catalogIdCache.get(librarySongId);
  if (cached !== undefined) return cached;

  try {
    const infoResp = await fetch("/api/apps/apple-music/proxy", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "same-origin",
      body: JSON.stringify({
        targetUrl: `https://api.music.apple.com/v1/me/library/songs/${librarySongId}`,
        params: { include: "catalog" },
      }),
    });
    if (!infoResp.ok) return null;
    const info = (await infoResp.json()) as {
      data?: Array<{
        attributes?: { name?: string; artistName?: string };
        relationships?: { catalog?: { data?: Array<{ id: string }> } };
      }>;
    };
    const song = info.data?.[0];
    if (!song) return null;

    const catalogData = song.relationships?.catalog?.data;
    if (Array.isArray(catalogData) && catalogData.length > 0) {
      const id = catalogData[0].id;
      _catalogIdCache.set(librarySongId, id);
      return id;
    }

    const name = song.attributes?.name;
    const artist = song.attributes?.artistName;
    if (!name) return null;

    const searchResp = await fetch("/api/apps/apple-music/proxy", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "same-origin",
      body: JSON.stringify({
        path: "/v1/catalog/us/search",
        params: {
          types: "songs",
          term: `${name} ${artist ?? ""}`.trim(),
          limit: "10",
        },
      }),
    });
    if (!searchResp.ok) return null;
    const searchData = (await searchResp.json()) as {
      results?: {
        songs?: {
          data?: Array<{ id: string; attributes?: { name?: string } }>;
        };
      };
    };
    const results = searchData.results?.songs?.data;
    if (!Array.isArray(results) || results.length === 0) return null;

    const loweredName = name.toLowerCase();
    const exact = results.find(
      (result) => result.attributes?.name?.toLowerCase() === loweredName,
    );
    const resolved = exact?.id ?? results[0].id;
    if (resolved) _catalogIdCache.set(librarySongId, resolved);
    return resolved ?? null;
  } catch (error) {
    console.warn("[AppleMusic] resolveLibrarySongToCatalog failed:", error);
    return null;
  }
}
