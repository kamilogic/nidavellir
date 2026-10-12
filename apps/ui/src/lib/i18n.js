import { writable, derived } from "svelte/store";

/** Available UI languages. English is the default; more can be added later. */
export const locales = [
  { id: "en", label: "English" },
];

function initialLocale() {
  try {
    const saved = localStorage.getItem("nidavellir-locale");
    if (saved && locales.some((l) => l.id === saved)) return saved;
  } catch {}
  return "en";
}

export const locale = writable(initialLocale());
locale.subscribe((v) => {
  try {
    localStorage.setItem("nidavellir-locale", v);
  } catch {}
});

// Strings from the removed legacy screens went with them; the Portuguese option adds keys here.
const dict = {
  en: {
    "app.tagline": "Where silicon is forged to its prime.",
  },
  pt: {
    "app.tagline": "Onde o silício é forjado ao seu auge.",
  },
};

/** Reactive translator: `$t('key', { var: value })`. Falls back to en, then key. */
export const t = derived(locale, ($l) => (key, vars) => {
  let s = (dict[$l] && dict[$l][key]) ?? dict.en[key] ?? key;
  if (vars) {
    for (const k in vars) s = s.replaceAll(`{${k}}`, vars[k]);
  }
  return s;
});
