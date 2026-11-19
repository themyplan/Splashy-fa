// Translate a raw backend error into what the UI shows. A rate-limit
// error (Unsplash's 50/hour demo limit, reported as HTTP 403 and mapped
// by the client to ERR_RATE_LIMITED) becomes a plain Persian sentence
// telling the user to switch keys or wait; everything else is shown
// as-is.
export function friendlyErrorText(raw: string, rateLimited: string): string {
  return raw.includes("ERR_RATE_LIMITED") ? rateLimited : raw;
}
