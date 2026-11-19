import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { ApiKeyEntry, ApiKeySaveEntry, AppSettings, RateLimitStatus } from "../types";
import { LANGUAGES } from "../i18n";
import CronScheduler from "../components/CronScheduler";
import Toggle from "../components/Toggle";

const BMC_URL = "https://buymeacoffee.com/halfaxa";

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

  const persistSettings = useCallback(async (q: string, o: string, c: string) => {
    try {
      await invoke("update_settings", { quality: q, orientation: o, wallpaperCron: c });
      if (savedTimerRef.current) clearTimeout(savedTimerRef.current);
      setSaved(true);
      savedTimerRef.current = setTimeout(() => setSaved(false), 1500);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleQualityChange     = (v: string) => { setQuality(v);      persistSettings(v, orientation, cron); };
  const handleOrientationChange = (v: string) => { setOrientation(v);  persistSettings(quality, v, cron); };
  const handleCronChange        = (expr: string) => { setCron(expr);   persistSettings(quality, orientation, expr); };

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
              <div className="grid grid-cols-3 gap-2">
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
          </div>
        </section>

        {/* ── Language ──────────────────────────────────────────────────── */}
        <section>
          <SectionHeader>{t("settings.sectionLanguage")}</SectionHeader>
          <div className="bg-white/4 rounded-2xl border border-white/8 p-4">
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
          </div>
        </section>

        {/* ── Support ───────────────────────────────────────────────────── */}
        <section className="pb-2">
          <SectionHeader>{t("settings.sectionSupport")}</SectionHeader>
          <button
            onClick={() => openUrl(BMC_URL)}
            className="w-full flex items-center justify-center gap-2.5 py-3 rounded-2xl border border-yellow-500/20 bg-yellow-500/6 hover:bg-yellow-500/12 hover:border-yellow-500/35 text-yellow-300/80 hover:text-yellow-200 transition-all cursor-pointer"
          >
            <span className="text-lg leading-none">☕</span>
            <span className="text-sm font-medium">{t("settings.buyMeACoffee")}</span>
          </button>
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
