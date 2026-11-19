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

// Keep <html> in sync with the active language: Persian reads right-to-left,
// every other language here reads left-to-right. This runs once at startup
// and again on every language change (e.g. from the Settings picker, which
// calls i18n.changeLanguage), so no component needs to handle direction.
function applyDocumentDirection(lng: string) {
  const rtl = lng === "fa";
  document.documentElement.dir = rtl ? "rtl" : "ltr";
  document.documentElement.lang = rtl ? "fa" : lng;
}

applyDocumentDirection(saved);

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

i18n.on("languageChanged", applyDocumentDirection);

export default i18n;
