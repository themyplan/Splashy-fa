import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import ro from "./locales/ro.json";
import fa from "./locales/fa.json";

// To add a new language:
// 1. Create src/locales/<code>.json (copy en.json and translate)
// 2. Import it here
// 3. Add it to `resources` below
// 4. Add it to LANGUAGES in src/views/SettingsView.tsx

export const LANGUAGES: { code: string; label: string }[] = [
  { code: "en", label: "English" },
  { code: "ro", label: "Română" },
  { code: "fa", label: "فارسی" },
];

const saved = localStorage.getItem("lang") ?? "en";

// The Persian font follows the LANGUAGE, not the page direction: fa.json
// stays Persian whether the user picks LTR (original layout) or RTL.
function applyDocumentLanguage(lng: string) {
  document.documentElement.lang = lng;
}

// Page direction is the USER's choice (Settings -> layout_direction),
// independent of language. Default "ltr": the original upstream layout.
// Stored in localStorage as a mirror so it applies at startup, before
// the backend settings are loaded (App.tsx re-applies the saved value
// from settings on every launch).
export function applyLayoutDirection(dir: string) {
  const d = dir === "rtl" ? "rtl" : "ltr";
  document.documentElement.dir = d;
  localStorage.setItem("layout_direction", d);
}

applyDocumentLanguage(saved);
applyLayoutDirection(localStorage.getItem("layout_direction") ?? "ltr");

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    ro: { translation: ro },
    fa: { translation: fa },
  },
  lng: saved,
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

i18n.on("languageChanged", applyDocumentLanguage);

export default i18n;
