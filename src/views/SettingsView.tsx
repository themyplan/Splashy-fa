import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { useTranslation } from "react-i18next";
import { ApiKeyEntry, ApiKeySaveEntry, AppSettings, RateLimitStatus } from "../types";
import { LANGUAGES, applyLayoutDirection } from "../i18n";
import { DEFAULT_API_BASE_URL } from "../constants";
import CronScheduler from "../components/CronScheduler";
import Toggle from "../components/Toggle";

function getOSStartLabel(): "startWithWindows" | "startWithMac" | "startWithLinux" {
  const p = navigator.platform.toLowerCase();
  if (p.startsWith("mac")) return "startWithMac";
  if (p.startsWith("win")) return "startWithWindows";
  return "startWithLinux";
}

const BASIC_CRONS = new Set([
  "@startup",
  "*/30 * * * *",
  "0 * * * *",
  "0 */2 * * *",
  "0 */4 * * *",
  "0 */6 * * *",
  "0 */12 * * *",
]);

function isOnceDailyCron(expr: string): boolean {
  return /^\d+ \d+ \* \* \*$/.test(expr);
}



const ALL_HOURS = Array.from({ length: 24 }, (_, i) => i);
const ALL_MINUTES = Array.from({ length: 60 }, (_, i) => i);

function SectionHeader({ children }: { children: React.ReactNode }) {
  return (
    <p className="text-[11px] font-semibold uppercase tracking-widest text-white/50 mb-3">
      {children}
    </p>
  );
}

// One row of the API-key list. `keyInput` holds text the user typed for
// this row only (a new key, or a replacement for a saved row); saved rows
// display `maskedKey` and never hold the full key.
interface ApiKeyRow {
  id: string;
  label: string;
  maskedKey: string;
  isActive: boolean;
  keyInput: string;
  isNew: boolean;
  verifying: boolean;
  error: string | null;
}

function toRows(keys: ApiKeyEntry[]): ApiKeyRow[] {
  return keys.map((k) => ({
    id: k.id,
    label: k.label,
    maskedKey: k.masked_key,
    isActive: k.is_active,
    keyInput: "",
    isNew: false,
    verifying: false,
    error: null,
  }));
}

export default function SettingsView({ onApiKeyChange }: { onApiKeyChange: () => void }) {
  const { t, i18n } = useTranslation();
  const hourLabel12 = (h: number): string => {
    const disp = h === 0 ? 12 : h <= 12 ? h : h - 12;
    return `${disp} ${h < 12 ? t("cron.am") : t("cron.pm")}`;
  };
  const [quality, setQuality] = useState("regular");
  const [orientation, setOrientation] = useState("landscape");
  const [cron, setCron] = useState("0 * * * *");
  const [scheduleTab, setScheduleTab] = useState<"basic" | "advanced">("basic");
  const [startOnLogin, setStartOnLogin] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saved, setSaved] = useState(false);
  const savedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [apiKeys, setApiKeys] = useState<ApiKeyRow[]>([]);
  const [rateLimit, setRateLimit] = useState<RateLimitStatus | null>(null);
  const [apiKeysError, setApiKeysError] = useState<string | null>(null);
  const [apiKeySaved, setApiKeySaved] = useState(false);
  const [apiBaseUrl, setApiBaseUrl] = useState(DEFAULT_API_BASE_URL);
  const [layoutDirection, setLayoutDirection] = useState<"ltr" | "rtl">("ltr");
  const [wallpaperFit, setWallpaperFit] = useState("fill");
  const [saveWallpapers, setSaveWallpapers] = useState(false);
  const [wallpaperSaveFolder, setWallpaperSaveFolder] = useState("");
  const [prefetchEnabled, setPrefetchEnabled] = useState(false);
  const [prefetchCount, setPrefetchCount] = useState("5");
  const [changeOnLaunch, setChangeOnLaunch] = useState(false);
  const [closeBehavior, setCloseBehavior] = useState("ask");
  const [version, setVersion] = useState("");

  const BASIC_OPTIONS = [
    { cron: "@startup",     label: t("settings.scheduleOnStartup"),  desc: t("settings.scheduleOnStartupDesc") },
    { cron: "*/30 * * * *", label: t("settings.scheduleEvery30"),    desc: "" },
    { cron: "0 * * * *",    label: t("settings.scheduleEveryHour"),  desc: "" },
    { cron: "0 */2 * * *",  label: t("settings.scheduleEvery2Hours"), desc: "" },
    { cron: "0 */4 * * *",  label: t("settings.scheduleEvery4Hours"), desc: "" },
    { cron: "0 */6 * * *",  label: t("settings.scheduleEvery6Hours"), desc: "" },
    { cron: "0 */12 * * *", label: t("settings.scheduleEvery12Hours"), desc: "" },
  ];

  const QUALITY_OPTIONS = [
    { value: "raw",     label: t("settings.qualityRaw"),     description: t("settings.qualityRawDesc") },
    { value: "full",    label: t("settings.qualityFull"),    description: t("settings.qualityFullDesc") },
    { value: "regular", label: t("settings.qualityRegular"), description: t("settings.qualityRegularDesc") },
    { value: "small",   label: t("settings.qualitySmall"),   description: t("settings.qualitySmallDesc") },
    { value: "thumb",   label: t("settings.qualityThumb"),   description: t("settings.qualityThumbDesc") },
  ];

  const ORIENTATION_OPTIONS = [
    { value: "landscape", label: t("settings.orientationLandscape"), description: t("settings.orientationLandscapeDesc") },
    { value: "portrait",  label: t("settings.orientationPortrait"),  description: t("settings.orientationPortraitDesc") },
    { value: "squarish",  label: t("settings.orientationSquarish"),  description: t("settings.orientationSquarishDesc") },
    { value: "all",       label: t("settings.orientationAll"),       description: t("settings.orientationAllDesc") },
  ];

  const FIT_OPTIONS = [
    { value: "fill",    label: t("settings.fitFill"),    description: t("settings.fitFillDesc") },
    { value: "fit",     label: t("settings.fitFit"),     description: t("settings.fitFitDesc") },
    { value: "stretch", label: t("settings.fitStretch"), description: t("settings.fitStretchDesc") },
    { value: "tile",    label: t("settings.fitTile"),    description: t("settings.fitTileDesc") },
    { value: "center",  label: t("settings.fitCenter"),  description: t("settings.fitCenterDesc") },
    { value: "span",    label: t("settings.fitSpan"),    description: t("settings.fitSpanDesc") },
  ];

  useEffect(() => {
    Promise.all([
      invoke<AppSettings>("get_settings"),
      isEnabled(),
      invoke<ApiKeyEntry[]>("get_api_keys"),
      invoke<RateLimitStatus | null>("get_rate_limit_status"),
    ])
      .then(([s, autostart, keys, status]) => {
        setQuality(s.quality);
        setOrientation(s.orientation);
        setCron(s.wallpaper_cron);
        setApiBaseUrl(s.api_base_url || DEFAULT_API_BASE_URL);
        setLayoutDirection(s.layout_direction === "rtl" ? "rtl" : "ltr");
        setWallpaperFit(["fill", "fit", "stretch", "tile", "center", "span"].includes(s.wallpaper_fit) ? s.wallpaper_fit : "fill");
        setSaveWallpapers(s.save_wallpapers === true);
        setWallpaperSaveFolder(s.wallpaper_save_folder || "");
        setPrefetchEnabled(s.prefetch_enabled === true);
        setPrefetchCount(String(s.prefetch_count > 0 ? s.prefetch_count : 5));
        setChangeOnLaunch(s.change_on_launch === true);
        setCloseBehavior(["ask", "minimize", "exit"].includes(s.close_behavior) ? s.close_behavior : "ask");
        setScheduleTab(BASIC_CRONS.has(s.wallpaper_cron) || isOnceDailyCron(s.wallpaper_cron) ? "basic" : "advanced");
        setStartOnLogin(autostart);
        setApiKeys(toRows(keys));
        setRateLimit(status);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
    getVersion().then(setVersion).catch(() => {});
  }, []);

  const refreshRateLimit = useCallback(async () => {
    try {
      setRateLimit(await invoke<RateLimitStatus | null>("get_rate_limit_status"));
    } catch {
      /* counter stays as-is */
    }
  }, []);

  const reloadApiKeys = useCallback(async () => {
    const keys = await invoke<ApiKeyEntry[]>("get_api_keys");
    setApiKeys(toRows(keys));
    await refreshRateLimit();
    return keys;
  }, [refreshRateLimit]);

  const flashSaved = () => {
    setApiKeySaved(true);
    setTimeout(() => setApiKeySaved(false), 1500);
  };

  const setRow = (id: string, patch: Partial<ApiKeyRow>) =>
    setApiKeys((rows) => rows.map((r) => (r.id === id ? { ...r, ...patch } : r)));

  // Persist labels + active selection for SAVED rows only (unsaved new
  // rows are not sent — they only enter via Verify & Save). The key is
  // always sent empty here, which means "keep the stored key": a typed
  // replacement key is only ever sent through the verify commands.
  const persistSavedKeys = async (rows: ApiKeyRow[], activeId: string | null) => {
    const keys: ApiKeySaveEntry[] = rows
      .filter((r) => !r.isNew)
      .map((r) => ({ id: r.id, label: r.label, key: "" }));
    await invoke("save_api_keys", { keys, activeId });
  };

  const handleVerifyRow = async (row: ApiKeyRow) => {
    setRow(row.id, { verifying: true, error: null });
    setApiKeysError(null);
    try {
      if (row.isNew) {
        if (!row.keyInput.trim()) return;
        const beforeIds = new Set(apiKeys.filter((r) => !r.isNew).map((r) => r.id));
        await invoke("verify_and_save_api_key", { key: row.keyInput.trim() });
        const keys = await reloadApiKeys();
        // Keep a label the user typed for the new row: verify creates it
        // as "کلید N+1", so re-apply the typed label right after.
        const typedLabel = row.label.trim();
        if (typedLabel) {
          const added = keys.find((k) => !beforeIds.has(k.id));
          if (added) {
            await invoke("save_api_keys", {
              keys: keys.map((k) => ({
                id: k.id,
                label: k.id === added.id ? typedLabel : k.label,
                key: "",
              })),
              activeId: keys.find((k) => k.is_active)?.id ?? added.id,
            });
            await reloadApiKeys();
          }
        }
      } else if (row.keyInput.trim()) {
        await invoke("verify_and_update_api_key", { id: row.id, key: row.keyInput.trim() });
        await reloadApiKeys();
      } else {
        // No new key typed: just persist the (possibly edited) label.
        await persistSavedKeys(apiKeys, apiKeys.find((r) => r.isActive)?.id ?? null);
        await reloadApiKeys();
      }
      flashSaved();
      onApiKeyChange();
    } catch (e) {
      setRow(row.id, { error: String(e) });
    } finally {
      setRow(row.id, { verifying: false });
    }
  };

  const handleSelectActive = async (row: ApiKeyRow) => {
    if (row.isActive || row.isNew) return;
    setApiKeysError(null);
    try {
      await persistSavedKeys(apiKeys, row.id);
      await reloadApiKeys();
      onApiKeyChange();
    } catch (e) {
      setApiKeysError(String(e));
    }
  };

  const handleDeleteRow = async (row: ApiKeyRow) => {
    // Unsaved new rows only exist locally.
    if (row.isNew) {
      setApiKeys((rows) => rows.filter((r) => r.id !== row.id));
      return;
    }
    if (apiKeys.filter((r) => !r.isNew).length <= 1) {
      setApiKeysError(t("settings.apiKeys.cannotDeleteLast"));
      return;
    }
    if (!window.confirm(t("settings.apiKeys.deleteConfirm", { label: row.label || row.maskedKey }))) return;
    setApiKeysError(null);
    try {
      const remaining = apiKeys.filter((r) => r.id !== row.id && !r.isNew);
      const activeId = row.isActive
        ? remaining[0]?.id ?? null
        : apiKeys.find((r) => r.isActive)?.id ?? null;
      await invoke("save_api_keys", {
        keys: remaining.map((r) => ({ id: r.id, label: r.label, key: "" })),
        activeId,
      });
      await reloadApiKeys();
      onApiKeyChange();
    } catch (e) {
      setApiKeysError(String(e));
    }
  };

  const handleAddKey = () => {
    setApiKeysError(null);
    setApiKeys((rows) => [
      ...rows,
      {
        id: `new-${Date.now()}`,
        label: "",
        maskedKey: "",
        isActive: false,
        keyInput: "",
        isNew: true,
        verifying: false,
        error: null,
      },
    ]);
  };

  // Usage counter for one row, shown under the ACTIVE row only as a bare
  // `used/limit` number. The limit comes from the response header once a
  // call has been made with this key; before the first call it is the
  // Demo-tier default (50) with 0 used.
  const usage = (row: ApiKeyRow): { used: number; limit: number } => {
    if (rateLimit && rateLimit.key_id === row.id) {
      return {
        used: Math.max(0, rateLimit.limit - rateLimit.remaining),
        limit: rateLimit.limit,
      };
    }
    return { used: 0, limit: 50 };
  };

  const handleStartOnLoginToggle = async (checked: boolean) => {
    try {
      if (checked) await enable(); else await disable();
      setStartOnLogin(checked);
    } catch (e) {
      setError(String(e));
    }
  };

  const persistSettings = useCallback(async (q: string, o: string, c: string, base: string, dir: string, fit: string, save: boolean, folder: string, pe: boolean, pc: string, col: boolean, cb: string) => {
    try {
      const prefetchCount = Math.min(20, Math.max(1, parseInt(pc, 10) || 5));
      await invoke("update_settings", { quality: q, orientation: o, wallpaperCron: c, apiBaseUrl: base, layoutDirection: dir, wallpaperFit: fit, saveWallpapers: save, wallpaperSaveFolder: folder, prefetchEnabled: pe, prefetchCount, changeOnLaunch: col, closeBehavior: cb });
      if (savedTimerRef.current) clearTimeout(savedTimerRef.current);
      setSaved(true);
      savedTimerRef.current = setTimeout(() => setSaved(false), 1500);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleQualityChange     = (v: string) => { setQuality(v);      persistSettings(v, orientation, cron, apiBaseUrl, layoutDirection, wallpaperFit, saveWallpapers, wallpaperSaveFolder, prefetchEnabled, prefetchCount, changeOnLaunch, closeBehavior); };
  const handleOrientationChange = (v: string) => { setOrientation(v);  persistSettings(quality, v, cron, apiBaseUrl, layoutDirection, wallpaperFit, saveWallpapers, wallpaperSaveFolder, prefetchEnabled, prefetchCount, changeOnLaunch, closeBehavior); };
  const handleCronChange        = (expr: string) => { setCron(expr);   persistSettings(quality, orientation, expr, apiBaseUrl, layoutDirection, wallpaperFit, saveWallpapers, wallpaperSaveFolder, prefetchEnabled, prefetchCount, changeOnLaunch, closeBehavior); };

  // Server address: empty means the relay default. Applies on blur/Enter.
  const handleApiBaseUrlSave = () => {
    const v = apiBaseUrl.trim();
    if (!v) setApiBaseUrl(DEFAULT_API_BASE_URL);
    persistSettings(quality, orientation, cron, v, layoutDirection, wallpaperFit, saveWallpapers, wallpaperSaveFolder, prefetchEnabled, prefetchCount, changeOnLaunch, closeBehavior);
  };

  // Page direction: applies immediately, no restart needed.
  const handleLayoutDirectionChange = (dir: "ltr" | "rtl") => {
    setLayoutDirection(dir);
    applyLayoutDirection(dir);
    persistSettings(quality, orientation, cron, apiBaseUrl, dir, wallpaperFit, saveWallpapers, wallpaperSaveFolder, prefetchEnabled, prefetchCount, changeOnLaunch, closeBehavior);
  };

  // Wallpaper fit: saved, then applied at once to the current desktop
  // wallpaper too, so the user sees the change immediately instead of
  // waiting for the next wallpaper change. Best-effort: fails silently
  // when no wallpaper is set yet.
  const handleWallpaperFitChange = async (v: string) => {
    setWallpaperFit(v);
    await persistSettings(quality, orientation, cron, apiBaseUrl, layoutDirection, v, saveWallpapers, wallpaperSaveFolder, prefetchEnabled, prefetchCount, changeOnLaunch, closeBehavior);
    try { await invoke("apply_wallpaper_fit"); } catch { /* no wallpaper set yet */ }
  };

  // Save wallpapers to a folder: the toggle and the folder are saved
  // together; without a folder the feature stays inactive.
  const handleSaveWallpapersToggle = (checked: boolean) => {
    setSaveWallpapers(checked);
    persistSettings(quality, orientation, cron, apiBaseUrl, layoutDirection, wallpaperFit, checked, wallpaperSaveFolder, prefetchEnabled, prefetchCount, changeOnLaunch, closeBehavior);
  };

  const handleChooseSaveFolder = async () => {
    const folder = await open({ directory: true, multiple: false, title: t("home.chooseFolderTitle") });
    if (!folder) return;
    const path = Array.isArray(folder) ? folder[0] : folder;
    if (!path) return;
    setWallpaperSaveFolder(path);
    persistSettings(quality, orientation, cron, apiBaseUrl, layoutDirection, wallpaperFit, saveWallpapers, path, prefetchEnabled, prefetchCount, changeOnLaunch, closeBehavior);
  };

  // Prefetch upcoming wallpapers: the toggle and the count are saved
  // together. Turning it on fills the queue at once (and turning it off
  // collapses it back to a single next wallpaper) via refill_prefetch_queue.
  const handlePrefetchToggle = async (checked: boolean) => {
    setPrefetchEnabled(checked);
    await persistSettings(quality, orientation, cron, apiBaseUrl, layoutDirection, wallpaperFit, saveWallpapers, wallpaperSaveFolder, checked, prefetchCount, changeOnLaunch, closeBehavior);
    try { await invoke("refill_prefetch_queue"); } catch { /* nothing queued yet */ }
  };

  // Number box: digits only; auto-saves on blur/Enter (no save button),
  // clamped to 1..=20 — same pattern as the server-address field.
  const handlePrefetchCountSave = async () => {
    const n = Math.min(20, Math.max(1, parseInt(prefetchCount, 10) || 5));
    setPrefetchCount(String(n));
    await persistSettings(quality, orientation, cron, apiBaseUrl, layoutDirection, wallpaperFit, saveWallpapers, wallpaperSaveFolder, prefetchEnabled, String(n), changeOnLaunch, closeBehavior);
    try { await invoke("refill_prefetch_queue"); } catch { /* nothing queued yet */ }
  };

  // Change once when the app opens (off by default): with a timed
  // schedule and this off, opening the app does not change the
  // wallpaper — it waits for the next scheduled time.
  const handleChangeOnLaunchToggle = (checked: boolean) => {
    setChangeOnLaunch(checked);
    persistSettings(quality, orientation, cron, apiBaseUrl, layoutDirection, wallpaperFit, saveWallpapers, wallpaperSaveFolder, prefetchEnabled, prefetchCount, checked, closeBehavior);
  };

  // What the close button does: ask (dialog), minimize to tray, or exit.
  const handleCloseBehaviorChange = (v: string) => {
    setCloseBehavior(v);
    persistSettings(quality, orientation, cron, apiBaseUrl, layoutDirection, wallpaperFit, saveWallpapers, wallpaperSaveFolder, prefetchEnabled, prefetchCount, changeOnLaunch, v);
  };

  const handleLanguageChange = (code: string) => {
    i18n.changeLanguage(code);
    localStorage.setItem("lang", code);
  };

  // Once-a-day derived state
  const isOnceDailySelected = isOnceDailyCron(cron);
  const dailyCronParts = isOnceDailySelected ? cron.split(" ") : ["0", "8"];
  const dailyMinute = parseInt(dailyCronParts[0]);
  const dailyHour   = parseInt(dailyCronParts[1]);

  const handleOnceDailyClick = () => {
    if (!isOnceDailySelected) handleCronChange("0 8 * * *");
  };
  const handleDailyHour   = (h: number) => handleCronChange(`${dailyMinute} ${h} * * *`);
  const handleDailyMinute = (m: number) => handleCronChange(`${m} ${dailyHour} * * *`);

  if (loading) {
    return <div className="flex-1 flex items-center justify-center text-white/50 text-sm">{t("common.loading")}</div>;
  }

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="p-6">
      <div className="max-w-lg mx-auto">
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-lg font-semibold text-white">{t("settings.title")}</h2>
        {saved && <span className="text-xs text-emerald-400 animate-card-in">{t("settings.saved")}</span>}
      </div>

      {error && (
        <div className="mb-4 text-sm px-3 py-2 rounded-lg bg-red-900/30 text-red-300">{error}</div>
      )}

      <div className="flex flex-col gap-8">

        {/* ── Photo ─────────────────────────────────────────────────────── */}
        <section>
          <SectionHeader>{t("settings.sectionPhoto")}</SectionHeader>
          <div className="flex flex-col gap-4 bg-white/4 rounded-2xl border border-white/8 p-4">

            {/* Orientation */}
            <div>
              <p className="text-sm font-medium text-white/70 mb-2">{t("settings.orientation")}</p>
              <div className="grid grid-cols-2 gap-2">
                {ORIENTATION_OPTIONS.map(opt => (
                  <button
                    key={opt.value}
                    onClick={() => handleOrientationChange(opt.value)}
                    className={`flex flex-col items-center gap-1 py-3 px-2 rounded-xl border text-center cursor-pointer transition-all ${
                      orientation === opt.value
                        ? "border-white/25 bg-white/10 text-white"
                        : "border-white/8 bg-white/4 text-white/60 hover:border-white/18 hover:text-white"
                    }`}
                  >
                    <span className="text-sm font-medium">{opt.label}</span>
                    <span className="text-[11px] text-white/50">{opt.description}</span>
                  </button>
                ))}
              </div>
            </div>

            <div className="border-t border-white/8" />

            {/* Quality */}
            <div>
              <p className="text-sm font-medium text-white/70 mb-2">{t("settings.quality")}</p>
              <div className="grid grid-cols-5 gap-1.5">
                {QUALITY_OPTIONS.map(opt => (
                  <button
                    key={opt.value}
                    onClick={() => handleQualityChange(opt.value)}
                    title={opt.description}
                    className={`py-2 px-1 rounded-lg border text-xs font-medium cursor-pointer transition-all ${
                      quality === opt.value
                        ? "border-white/25 bg-white/10 text-white"
                        : "border-white/8 bg-white/4 text-white/60 hover:border-white/18 hover:text-white"
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
              <p className="text-[11px] text-white/50 mt-2">
                {QUALITY_OPTIONS.find(o => o.value === quality)?.description}
              </p>
            </div>

            <div className="border-t border-white/8" />

            {/* Wallpaper fit */}
            <div>
              <p className="text-sm font-medium text-white/70 mb-2">{t("settings.wallpaperFit")}</p>
              <div className="grid grid-cols-3 gap-2">
                {FIT_OPTIONS.map(opt => (
                  <button
                    key={opt.value}
                    onClick={() => handleWallpaperFitChange(opt.value)}
                    className={`flex flex-col items-center gap-1 py-3 px-2 rounded-xl border text-center cursor-pointer transition-all ${
                      wallpaperFit === opt.value
                        ? "border-white/25 bg-white/10 text-white"
                        : "border-white/8 bg-white/4 text-white/60 hover:border-white/18 hover:text-white"
                    }`}
                  >
                    <span className="text-sm font-medium">{opt.label}</span>
                    <span className="text-[11px] text-white/50">{opt.description}</span>
                  </button>
                ))}
              </div>
            </div>

            <div className="border-t border-white/8" />

            {/* Save wallpapers to a folder */}
            <div>
              <div className="flex items-center justify-between gap-3">
                <div>
                  <p className="text-sm font-medium text-white/70">{t("settings.saveWallpapers")}</p>
                  <p className="text-xs text-white/50 mt-0.5">{t("settings.saveWallpapersDesc")}</p>
                </div>
                <Toggle enabled={saveWallpapers} onChange={handleSaveWallpapersToggle} />
              </div>
              {saveWallpapers && (
                <div className="flex items-center gap-2 mt-3">
                  <p className="flex-1 truncate text-xs text-white/60 px-3 py-2 rounded-lg bg-white/4 border border-white/8" dir="auto">
                    {wallpaperSaveFolder || t("settings.noFolderChosen")}
                  </p>
                  <button
                    onClick={handleChooseSaveFolder}
                    className="px-3 py-2 rounded-lg border border-white/25 bg-white/10 text-xs font-medium text-white cursor-pointer transition-all hover:border-white/40 shrink-0"
                  >
                    {t("settings.chooseFolder")}
                  </button>
                </div>
              )}
            </div>

            <div className="border-t border-white/8" />

            {/* Prefetch upcoming wallpapers */}
            <div>
              <div className="flex items-center justify-between gap-3">
                <div>
                  <p className="text-sm font-medium text-white/70">{t("settings.prefetch")}</p>
                  <p className="text-xs text-white/50 mt-0.5">{t("settings.prefetchDesc")}</p>
                </div>
                <Toggle enabled={prefetchEnabled} onChange={handlePrefetchToggle} />
              </div>
              <div className={`flex items-center gap-2 mt-3 transition-opacity ${prefetchEnabled ? "" : "opacity-40"}`}>
                <label htmlFor="prefetch-count" className="text-xs text-white/60 shrink-0">{t("settings.prefetchCount")}</label>
                <input
                  id="prefetch-count"
                  type="text"
                  inputMode="numeric"
                  disabled={!prefetchEnabled}
                  value={prefetchCount}
                  onChange={(e) => setPrefetchCount(e.target.value.replace(/[^0-9]/g, ""))}
                  onBlur={handlePrefetchCountSave}
                  onKeyDown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); }}
                  className={`w-16 px-3 py-2 rounded-lg border text-xs text-white text-center outline-none disabled:cursor-not-allowed transition-all ${
                    prefetchEnabled
                      ? "border-white/25 bg-white/10 focus:border-white/40"
                      : "border-white/8 bg-white/4"
                  }`}
                />
              </div>
            </div>
          </div>
        </section>

        {/* ── Schedule ──────────────────────────────────────────────────── */}
        <section>
          <div className="flex items-center justify-between mb-3">
            <SectionHeader>{t("settings.sectionSchedule")}</SectionHeader>
            <div className="flex rounded-lg overflow-hidden border border-white/10 mb-3">
              {(["basic", "advanced"] as const).map(tab => (
                <button
                  key={tab}
                  onClick={() => setScheduleTab(tab)}
                  className={`px-3 py-1 text-xs font-medium capitalize transition-colors cursor-pointer ${
                    scheduleTab === tab
                      ? "bg-white/15 text-white"
                      : "bg-white/4 text-white/60 hover:text-white"
                  }`}
                >
                  {tab === "basic" ? t("settings.scheduleBasic") : t("settings.scheduleAdvanced")}
                </button>
              ))}
            </div>
          </div>

          {scheduleTab === "basic" ? (
            <div className="flex flex-col gap-1.5">
              {BASIC_OPTIONS.map(opt => (
                <button
                  key={opt.cron}
                  onClick={() => handleCronChange(opt.cron)}
                  className={`flex items-center justify-between w-full px-3 py-2.5 rounded-xl border text-start cursor-pointer transition-colors ${
                    cron === opt.cron
                      ? "border-white/25 bg-white/8"
                      : "border-white/8 bg-white/4 hover:border-white/16"
                  }`}
                >
                  <span className={`text-sm font-medium ${cron === opt.cron ? "text-white" : "text-white/70"}`}>
                    {opt.label}
                  </span>
                  {opt.desc && <span className="text-xs text-white/50 ms-2 shrink-0">{opt.desc}</span>}
                </button>
              ))}

              {/* ── Once a day (with inline time picker) ─────────────── */}
              <div>
                <button
                  onClick={handleOnceDailyClick}
                  className={`flex items-center justify-between w-full px-3 py-2.5 rounded-xl border text-start cursor-pointer transition-colors ${
                    isOnceDailySelected
                      ? "border-white/25 bg-white/8 rounded-b-none border-b-0"
                      : "border-white/8 bg-white/4 hover:border-white/16"
                  }`}
                >
                  <span className={`text-sm font-medium ${isOnceDailySelected ? "text-white" : "text-white/70"}`}>
                    {t("settings.scheduleOnceADay")}
                  </span>
                  {isOnceDailySelected && (
                    <span className="text-xs text-white/60 ms-2 shrink-0 font-mono">
                      {hourLabel12(dailyHour)}:{String(dailyMinute).padStart(2, "0")}
                    </span>
                  )}
                </button>

                {isOnceDailySelected && (
                  <div className="border border-white/25 border-t-0 rounded-xl rounded-t-none bg-white/4 px-3 pt-3 pb-3 flex flex-col gap-3">
                    {/* Hours grid: AM top row, PM bottom row */}
                    <div>
                      <p className="text-[10px] text-white/50 uppercase tracking-widest mb-1.5">{t("settings.scheduleHour")}</p>
                      <div className="grid grid-cols-12 gap-1">
                        {ALL_HOURS.map(h => (
                          <button
                            key={h}
                            onClick={() => handleDailyHour(h)}
                            className={`py-1 rounded text-[10px] font-medium cursor-pointer transition-colors border ${
                              h === dailyHour
                                ? "bg-white/20 border-white/30 text-white"
                                : "bg-white/4 border-white/8 text-white/60 hover:border-white/20 hover:text-white"
                            }`}
                          >
                            {hourLabel12(h)}
                          </button>
                        ))}
                      </div>
                    </div>

                    {/* Minutes */}
                    <div>
                      <p className="text-[10px] text-white/50 uppercase tracking-widest mb-1.5">{t("settings.scheduleMinute")}</p>
                      <div className="grid grid-cols-12 gap-1">
                        {ALL_MINUTES.map(m => (
                          <button
                            key={m}
                            onClick={() => handleDailyMinute(m)}
                            className={`py-1 rounded text-[10px] font-medium cursor-pointer transition-colors border ${
                              m === dailyMinute
                                ? "bg-white/20 border-white/30 text-white"
                                : "bg-white/4 border-white/8 text-white/60 hover:border-white/20 hover:text-white"
                            }`}
                          >
                            :{String(m).padStart(2, "0")}
                          </button>
                        ))}
                      </div>
                    </div>
                  </div>
                )}
              </div>

              {!BASIC_CRONS.has(cron) && !isOnceDailyCron(cron) && (
                <div className="mt-1 px-3 py-2 rounded-xl border border-amber-600/30 bg-amber-900/10 text-xs text-amber-400">
                  {t("settings.scheduleCustomActive")}
                </div>
              )}
            </div>
          ) : (
            <CronScheduler value={cron} onChange={handleCronChange} />
          )}

          {/* Change once when the app opens (off by default): with a
              timed schedule and this off, opening the app does not
              change the wallpaper — it waits for the next scheduled
              time. Startup-only mode always changes on launch. */}
          <div className="flex items-center justify-between gap-3 mt-4 bg-white/4 rounded-2xl border border-white/8 p-4">
            <div>
              <p className="text-sm font-medium text-white/70">{t("settings.changeOnLaunch")}</p>
              <p className="text-xs text-white/50 mt-0.5">{t("settings.changeOnLaunchDesc")}</p>
            </div>
            <Toggle enabled={changeOnLaunch} onChange={handleChangeOnLaunchToggle} />
          </div>
        </section>

        {/* ── App ───────────────────────────────────────────────────────── */}
        <section>
          <SectionHeader>{t("settings.sectionApp")}</SectionHeader>
          <div className="bg-white/4 rounded-2xl border border-white/8 divide-y divide-white/6">
            <div className="flex items-center justify-between px-4 py-3.5">
              <div>
                <p className="text-sm font-medium text-white">{t(`settings.${getOSStartLabel()}`)}</p>
                <p className="text-xs text-white/60 mt-0.5">{t("settings.startWithWindowsDesc")}</p>
              </div>
              <Toggle enabled={startOnLogin} onChange={handleStartOnLoginToggle} />
            </div>
            <div className="flex items-center justify-between gap-3 px-4 py-3.5">
              <div>
                <p className="text-sm font-medium text-white">{t("settings.closeBehavior")}</p>
                <p className="text-xs text-white/60 mt-0.5">{t("settings.closeBehaviorDesc")}</p>
              </div>
              <select
                value={closeBehavior}
                onChange={(e) => handleCloseBehaviorChange(e.target.value)}
                className="shrink-0 bg-white/6 border border-white/10 rounded-lg px-3 py-2 text-sm text-white outline-none focus:border-white/30 transition-colors cursor-pointer [&>option]:bg-[#171923] [&>option]:text-white"
              >
                <option value="ask">{t("settings.closeAsk")}</option>
                <option value="minimize">{t("settings.closeMinimize")}</option>
                <option value="exit">{t("settings.closeExit")}</option>
              </select>
            </div>
          </div>
        </section>

        {/* ── Language ──────────────────────────────────────────────────── */}
        <section>
          <SectionHeader>{t("settings.sectionLanguage")}</SectionHeader>
          <div className="bg-white/4 rounded-2xl border border-white/8 p-4 flex flex-col gap-4">
            <div className="flex gap-2 flex-wrap">
              {LANGUAGES.map(lang => (
                <button
                  key={lang.code}
                  onClick={() => handleLanguageChange(lang.code)}
                  className={`px-4 py-2 rounded-xl border text-sm font-medium transition-all cursor-pointer ${
                    i18n.language === lang.code
                      ? "border-white/25 bg-white/10 text-white"
                      : "border-white/8 bg-white/4 text-white/60 hover:border-white/18 hover:text-white"
                  }`}
                >
                  {lang.label}
                </button>
              ))}
            </div>

            {/* Page direction: LTR = original upstream layout (default),
                RTL = mirrored. Applies immediately. */}
            <div>
              <p className="text-sm font-medium text-white/70 mb-2">{t("settings.layoutDirection")}</p>
              <div className="flex gap-2 flex-wrap">
                {(["ltr", "rtl"] as const).map(dir => (
                  <button
                    key={dir}
                    onClick={() => handleLayoutDirectionChange(dir)}
                    className={`px-4 py-2 rounded-xl border text-sm font-medium transition-all cursor-pointer ${
                      layoutDirection === dir
                        ? "border-white/25 bg-white/10 text-white"
                        : "border-white/8 bg-white/4 text-white/60 hover:border-white/18 hover:text-white"
                    }`}
                  >
                    {dir === "ltr" ? t("settings.directionLtr") : t("settings.directionRtl")}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </section>

        {/* ── API Keys ──────────────────────────────────────────────────── */}
        <section>
          <SectionHeader>{t("settings.sectionApiKey")}</SectionHeader>
          <div className="flex flex-col gap-2 bg-white/4 rounded-2xl border border-white/8 p-4">
            <p className="text-xs text-white/60">{t("settings.apiKeyDesc")}</p>
            <p className="text-sm font-medium text-white/70">{t("settings.apiKeys.title")}</p>
            {apiKeysError && (
              <p className="text-xs text-red-400">{apiKeysError}</p>
            )}
            {apiKeys.map((row) => {
              const u = usage(row);
              return (
                <div key={row.id} className="flex flex-col gap-2 rounded-xl border border-white/8 bg-white/4 p-3">
                  <div className="flex items-center gap-2">
                    <input
                      type="radio"
                      name="active-api-key"
                      checked={row.isActive}
                      disabled={row.isNew}
                      onChange={() => handleSelectActive(row)}
                      title={t("settings.apiKeys.activeKey")}
                      className="shrink-0 accent-white cursor-pointer"
                    />
                    <input
                      type="text"
                      value={row.label}
                      onChange={(e) => setRow(row.id, { label: e.target.value, error: null })}
                      onBlur={() => {
                        if (!row.isNew) {
                          persistSavedKeys(apiKeys, apiKeys.find((r) => r.isActive)?.id ?? null)
                            .then(() => reloadApiKeys())
                            .catch((e) => setApiKeysError(String(e)));
                        }
                      }}
                      placeholder={t("settings.apiKeys.keyLabelPlaceholder")}
                      className="flex-1 min-w-0 bg-white/6 border border-white/10 rounded-lg px-3 py-2 text-sm text-white placeholder-white/50 outline-none focus:border-white/30 transition-colors"
                    />
                    <button
                      onClick={() => handleDeleteRow(row)}
                      disabled={apiKeys.filter((r) => !r.isNew).length <= 1 && !row.isNew}
                      title={t("settings.apiKeys.deleteKey")}
                      className="shrink-0 text-xs px-3 py-2 rounded-lg bg-white/6 hover:bg-red-900/40 border border-white/10 disabled:opacity-50 disabled:cursor-not-allowed text-white/70 hover:text-red-300 transition-colors cursor-pointer"
                    >
                      {t("settings.apiKeys.deleteKey")}
                    </button>
                  </div>
                  {!row.isNew && (
                    <p className="text-xs text-white/50 font-mono px-1" title={t("settings.apiKeys.lastFour")}>
                      {row.maskedKey}
                    </p>
                  )}
                  <div className="flex items-center gap-2">
                    <input
                      type="text"
                      value={row.keyInput}
                      onChange={(e) => setRow(row.id, { keyInput: e.target.value, error: null })}
                      onKeyDown={(e) => e.key === "Enter" && handleVerifyRow(row)}
                      placeholder={t("settings.apiKeys.keyPlaceholder")}
                      className="flex-1 min-w-0 bg-white/6 border border-white/10 rounded-lg px-3 py-2 text-sm text-white placeholder-white/50 outline-none focus:border-white/30 transition-colors font-mono"
                    />
                    <button
                      onClick={() => handleVerifyRow(row)}
                      disabled={row.verifying || (row.isNew && !row.keyInput.trim())}
                      className="shrink-0 text-sm px-4 py-2 rounded-lg bg-white/10 hover:bg-white/16 border border-white/15 disabled:opacity-50 disabled:cursor-not-allowed text-white transition-colors cursor-pointer"
                    >
                      {row.verifying ? t("apiSetup.verifying") : t("settings.apiKeys.verifySave")}
                    </button>
                  </div>
                  {row.error && (
                    <p className="text-xs text-red-400">{row.error}</p>
                  )}
                  {row.isActive && (
                    <p
                      title={t("settings.apiKeys.usageTitle")}
                      className="text-sm font-mono text-white/80 px-1"
                    >
                      {u.used}/{u.limit}
                    </p>
                  )}
                </div>
              );
            })}
            <div className="flex items-center justify-between">
              {apiKeySaved && <span className="text-xs text-emerald-400">{t("settings.apiKeys.verified")}</span>}
              <button
                onClick={handleAddKey}
                className="ms-auto text-sm px-4 py-1.5 rounded-lg bg-white/10 hover:bg-white/16 border border-white/15 text-white transition-colors cursor-pointer"
              >
                {t("settings.apiKeys.addKey")}
              </button>
            </div>

            {/* Server address (Sonora-style): requests go through this
                server instead of Unsplash directly, so the app works
                without a VPN. Empty = default relay. */}
            <div className="border-t border-white/8 pt-3">
              <p className="text-sm font-medium text-white/70 mb-1">{t("settings.serverAddress")}</p>
              <p className="text-xs text-white/60 mb-2">{t("settings.serverAddressDesc")}</p>
              <input
                type="text"
                dir="ltr"
                value={apiBaseUrl}
                onChange={(e) => setApiBaseUrl(e.target.value)}
                onBlur={handleApiBaseUrlSave}
                onKeyDown={(e) => e.key === "Enter" && handleApiBaseUrlSave()}
                placeholder={DEFAULT_API_BASE_URL}
                className="w-full min-w-0 bg-white/6 border border-white/10 rounded-lg px-3 py-2 text-sm text-white placeholder-white/50 outline-none focus:border-white/30 transition-colors font-mono"
              />
            </div>
          </div>
        </section>

        {/* ── About ─────────────────────────────────────────────────────── */}
        {version && (
          <p className="text-center text-[11px] text-white/25 -mt-4">Splashy v{version}</p>
        )}

      </div>
      </div>
      </div>
    </div>
  );
}
