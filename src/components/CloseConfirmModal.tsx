import { useState } from "react";
import { useTranslation } from "react-i18next";

// Shown when the user presses the window's close button while
// Settings.close_behavior is "ask" (the default). Glass style, like
// the Active Sources modal. Two choices — minimize to the tray (keep
// running) or close completely — plus a "remember my choice" checkbox
// and Cancel. Choosing with the checkbox remembered saves the choice
// as close_behavior, so the dialog stops appearing.
interface Props {
  onMinimize: (remember: boolean) => void;
  onExit: (remember: boolean) => void;
  onCancel: () => void;
}

export default function CloseConfirmModal({ onMinimize, onExit, onCancel }: Props) {
  const { t } = useTranslation();
  const [remember, setRemember] = useState(false);

  return (
    <div
      className="fixed inset-0 bg-black/40 backdrop-blur-md z-[70] flex items-center justify-center p-6"
      onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}
    >
      <div className="w-full max-w-sm">
        <div className="relative rounded-3xl overflow-hidden border border-white/[0.12] shadow-2xl shadow-black/40">
          {/* Emulated liquid glass */}
          <div className="absolute inset-0 z-0 pointer-events-none backdrop-blur-2xl bg-white/[0.04]" />
          <div className="absolute inset-0 z-0 pointer-events-none bg-gradient-to-br from-white/[0.08] via-transparent to-white/[0.03]" />
          <div className="absolute inset-0 z-0 pointer-events-none bg-black/35" />

          <div className="relative z-[2] p-5 flex flex-col gap-2.5">
            <h3 className="text-base font-semibold text-white mb-1">{t("closeConfirm.title")}</h3>

            <button
              onClick={() => onMinimize(remember)}
              className="w-full flex items-center gap-3 px-4 py-3 rounded-2xl border border-white/[0.06] bg-white/[0.03] hover:bg-white/[0.06] hover:border-white/[0.1] transition-all cursor-pointer text-start"
            >
              <div className="flex-1 min-w-0">
                <span className="text-sm font-medium text-white block">{t("closeConfirm.minimize")}</span>
                <span className="text-[11px] text-white/40 block mt-0.5">{t("closeConfirm.minimizeDesc")}</span>
              </div>
              <span className="text-white/30 shrink-0">›</span>
            </button>

            <button
              onClick={() => onExit(remember)}
              className="w-full flex items-center gap-3 px-4 py-3 rounded-2xl border border-red-500/15 bg-red-500/10 hover:bg-red-500/20 hover:border-red-500/30 transition-all cursor-pointer text-start"
            >
              <div className="flex-1 min-w-0">
                <span className="text-sm font-medium text-red-300 block">{t("closeConfirm.exit")}</span>
                <span className="text-[11px] text-red-300/50 block mt-0.5">{t("closeConfirm.exitDesc")}</span>
              </div>
              <span className="text-red-400/60 shrink-0">›</span>
            </button>

            <label className="flex items-center gap-2 px-1 pt-1 text-xs text-white/60 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={remember}
                onChange={(e) => setRemember(e.target.checked)}
                className="accent-white cursor-pointer"
              />
              {t("closeConfirm.remember")}
            </label>

            <button
              onClick={onCancel}
              className="mt-1 px-4 py-2 rounded-xl border border-white/10 bg-white/5 hover:bg-white/10 text-sm text-white/70 hover:text-white transition-all cursor-pointer"
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
