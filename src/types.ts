export type View = "home" | "collections" | "users" | "topics" | "queries" | "colors" | "related" | "settings";

export interface AppSettings {
  quality: string;
  orientation: string;
  wallpaper_cron: string;
  api_key: string | null;
}

/// One saved Unsplash API key as returned by `get_api_keys`.
/// `masked_key` shows only the last 4 characters (e.g. `••••••••ab12`);
/// the full key is never sent to the frontend.
export interface ApiKeyEntry {
  id: string;
  label: string;
  masked_key: string;
  is_active: boolean;
}

/// One entry of a full key list sent to `save_api_keys`.
/// `key` is empty for an already-saved entry (keep the stored key) and
/// holds the full key only when the user typed a new one.
export interface ApiKeySaveEntry {
  id: string;
  label: string;
  key: string;
}

/// Latest rate-limit status for the active key, from the most recent
/// api.unsplash.com response. Null before the first call with this key.
export interface RateLimitStatus {
  key_id: string | null;
  limit: number;
  remaining: number;
}

export interface CollectionSummary {
  id: string;
  title: string;
  description: string | null;
  count: number;
  cover_url: string | null;
  author_name: string | null;
  author_username: string | null;
  enabled: boolean;
}

export interface UserSummary {
  username: string;
  name: string;
  bio: string | null;
  total_photos: number;
  avatar_path: string | null;
  enabled: boolean;
}

export interface TopicWithEnabled {
  id: string;
  slug: string;
  title: string;
  total_photos: number;
  cover_url: string | null;
  enabled: boolean;
}

export interface ColorSource {
  color: string;
  enabled: boolean;
}

export interface QuerySummary {
  id: string;
  value: string;
  enabled: boolean;
  weight: number;
}

export interface RelatedSourceSummary {
  photo_id: string;
  slug: string;
  author_name: string;
  author_username: string;
  unsplash_url: string;
  cover_url: string | null;
  enabled: boolean;
}

export interface CurrentWallpaperInfo {
  path: string;
  author_name: string;
  author_username: string;
  unsplash_url: string;
  photo_id: string;
}

export interface AdjacentWallpapers {
  previous: CurrentWallpaperInfo | null;
  next: CurrentWallpaperInfo | null;
}

export interface TimeGroup {
  id: string;
  label: string;
  start_hour: number;
  end_hour: number;
  target_ids: string[];
}
