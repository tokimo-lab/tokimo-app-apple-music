/**
 * Intercepts all `fetch()` calls to Apple Music service domains and routes
 * them through our backend proxy at `/api/apps/apple-music/proxy`. This is
 * necessary because MusicKit.js makes internal API calls (setQueue, play,
 * activity tracking, etc.) that fail on non-apple.com origins due to CORS
 * and token origin restrictions.
 */

const APPLE_DOMAINS = [
  "api.music.apple.com",
  "universal-activity-service.itunes.apple.com",
  "play.itunes.apple.com",
  "buy.itunes.apple.com",
  "amp-api.music.apple.com",
  "amp-api-edge.music.apple.com",
];

function isAppleDomain(url: string): boolean {
  return APPLE_DOMAINS.some((d) => url.includes(d));
}

/** Extract Media-User-Token from the original request headers (if MusicKit set it). */
function extractMediaUserToken(
  input: RequestInfo | URL,
  init?: RequestInit,
): string | undefined {
  const raw =
    init?.headers ?? (input instanceof Request ? input.headers : undefined);
  if (!raw) return undefined;

  if (raw instanceof Headers) {
    return (
      raw.get("Media-User-Token") ?? raw.get("media-user-token") ?? undefined
    );
  }
  if (Array.isArray(raw)) {
    const found = raw.find(([k]) => k.toLowerCase() === "media-user-token");
    return found?.[1];
  }
  if (typeof raw === "object") {
    const h = raw as Record<string, string>;
    return h["Media-User-Token"] ?? h["media-user-token"];
  }
  return undefined;
}

let installed = false;

export function installAppleMusicFetchInterceptor(): void {
  if (installed) return;
  installed = true;

  const originalFetch = window.fetch;

  window.fetch = async function patchedFetch(
    input: RequestInfo | URL,
    init?: RequestInit,
  ): Promise<Response> {
    const url =
      input instanceof Request
        ? input.url
        : input instanceof URL
          ? input.href
          : String(input);

    if (!isAppleDomain(url)) {
      return originalFetch.call(window, input, init);
    }

    // Parse the full Apple URL — send origin + path + query to proxy
    const parsed = new URL(url);
    const targetUrl = `${parsed.origin}${parsed.pathname}`;
    const params: Record<string, string> = {};
    for (const [k, v] of parsed.searchParams.entries()) {
      params[k] = v;
    }

    const method =
      init?.method ?? (input instanceof Request ? input.method : "GET");

    // Forward any Media-User-Token from MusicKit's original headers so the
    // backend can use it even before the async DB save has completed.
    const musicUserToken = extractMediaUserToken(input, init);

    // Extract body for non-GET requests
    let body: unknown;
    if (init?.body) {
      try {
        body =
          typeof init.body === "string" ? JSON.parse(init.body) : init.body;
      } catch {
        body = init.body;
      }
    } else if (input instanceof Request && method !== "GET") {
      try {
        const cloned = input.clone();
        const text = await cloned.text();
        if (text) body = JSON.parse(text);
      } catch {
        // non-JSON body — skip
      }
    }

    const proxyResp = await originalFetch.call(
      window,
      "/api/apps/apple-music/proxy",
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        credentials: "same-origin",
        body: JSON.stringify({
          targetUrl,
          method,
          params,
          body,
          musicUserToken,
        }),
      },
    );

    // Do NOT dispatch a de-auth event here. MusicKit makes many background
    // requests (activity tracking, queue sync, etc.) to Apple domains. These
    // go through this interceptor. If MusicKit has restored a stale token from
    // localStorage, those background calls may 403, but dispatching de-auth
    // from background requests would kick the user to the login screen while
    // they're still actively using the app.
    //
    // De-auth is instead handled in apiHelper (AppleMusicProvider) which only
    // fires for explicit user-initiated API calls.

    // MusicKit.js calls .json() on all responses. If the proxy returned an
    // empty body (e.g. activity tracking endpoints), synthesize a `{}`
    // response so MusicKit doesn't choke on an empty string.
    const contentLength = proxyResp.headers.get("content-length");
    if (contentLength === "0" || proxyResp.status === 204) {
      return new Response("{}", {
        status: proxyResp.status === 204 ? 200 : proxyResp.status,
        statusText: proxyResp.statusText,
        headers: { "content-type": "application/json" },
      });
    }

    return proxyResp;
  };
}

export function uninstallAppleMusicFetchInterceptor(): void {
  // We can't truly restore since other code may have also patched fetch.
  // In practice this interceptor lives for the app's lifetime.
  installed = false;
}
