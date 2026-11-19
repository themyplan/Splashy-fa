export const APP_NAME = "Splashy";

/** Cloudflare Unsplash relay (same Worker as the Resplash app). Cover
 *  URLs stored by older builds point at images.unsplash.com directly;
 *  rewrite them to the relay (/img) so thumbnails load without a VPN.
 *  Idempotent: relay URLs and non-Unsplash URLs pass through unchanged. */
export const UNSPLASH_RELAY_BASE = "https://unsplash-relay.a44410527.workers.dev";
export const DEFAULT_API_BASE_URL = UNSPLASH_RELAY_BASE;
export function relayImgUrl(
  url: string | null | undefined,
  base: string = UNSPLASH_RELAY_BASE,
): string | undefined {
  if (!url) return undefined;
  if (url.startsWith("https://images.unsplash.com/")) {
    return base.replace(/\/$/, "") + "/img/" + url.slice("https://images.unsplash.com/".length);
  }
  return url;
}
