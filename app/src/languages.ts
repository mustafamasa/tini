// VoxCPM2's 30 languages, their flags, and formatting helpers.
const flagFiles = import.meta.glob("@resources/flags/*.svg", { eager: true, query: "?url", import: "default" }) as Record<string, string>;

export const LANGUAGES: { code: string; name: string; flag: string }[] = [
  { code: "tr", name: "Türkçe", flag: "tr" },
  { code: "en", name: "English", flag: "gb" },
  { code: "de", name: "Deutsch", flag: "de" },
  { code: "fr", name: "Français", flag: "fr" },
  { code: "es", name: "Español", flag: "es" },
  { code: "ru", name: "Русский", flag: "ru" },
  { code: "zh", name: "中文", flag: "cn" },
  { code: "ja", name: "日本語", flag: "jp" },
  { code: "it", name: "Italiano", flag: "it" },
  { code: "pt", name: "Português", flag: "pt" },
  { code: "pl", name: "Polski", flag: "pl" },
  { code: "nl", name: "Nederlands", flag: "nl" },
  { code: "da", name: "Dansk", flag: "dk" },
  { code: "nb", name: "Norsk", flag: "no" },
  { code: "sv", name: "Svenska", flag: "se" },
  { code: "fi", name: "Suomi", flag: "fi" },
  { code: "el", name: "Ελληνικά", flag: "gr" },
  { code: "ar", name: "العربية", flag: "sa" },
  { code: "he", name: "עברית", flag: "il" },
  { code: "hi", name: "हिन्दी", flag: "in" },
  { code: "ko", name: "한국어", flag: "kr" },
  { code: "th", name: "ไทย", flag: "th" },
  { code: "vi", name: "Tiếng Việt", flag: "vn" },
  { code: "id", name: "Bahasa Indonesia", flag: "id" },
  { code: "ms", name: "Bahasa Melayu", flag: "my" },
  { code: "tl", name: "Tagalog", flag: "ph" },
  { code: "my", name: "မြန်မာ", flag: "mm" },
  { code: "km", name: "ខ្មែរ", flag: "kh" },
  { code: "lo", name: "ລາວ", flag: "la" },
  { code: "sw", name: "Kiswahili", flag: "ke" },
];

export function languageName(code: string | null | undefined): string {
  return LANGUAGES.find((l) => l.code === code)?.name ?? (code ?? "").toUpperCase();
}

export function flagUrl(language: string | null | undefined): string | undefined {
  const flag = LANGUAGES.find((l) => l.code === language)?.flag;
  if (!flag) return undefined;
  const key = Object.keys(flagFiles).find((k) => k.endsWith(`/${flag}.svg`));
  return key ? flagFiles[key] : undefined;
}

/** Human-readable duration ("12,5 sn", "3 dk 04 sn", "1 sa 05 dk"). */
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !isFinite(seconds)) return "–";
  if (seconds < 60) return `${seconds.toLocaleString("tr-TR", { maximumFractionDigits: 1 })} sn`;
  const minutes = Math.floor(seconds / 60);
  const rest = Math.round(seconds % 60);
  if (minutes < 60) return `${minutes} dk ${rest.toString().padStart(2, "0")} sn`;
  return `${Math.floor(minutes / 60)} sa ${(minutes % 60).toString().padStart(2, "0")} dk`;
}

/** m:ss */
export function formatClock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function formatDate(value: string | null | undefined): string {
  if (!value) return "";
  const d = new Date(value.replace(" ", "T"));
  if (isNaN(d.getTime())) return value;
  const today = new Date();
  const isToday = d.toDateString() === today.toDateString();
  const isYesterday = new Date(today.getTime() - 86400000).toDateString() === d.toDateString();
  const time = d.toLocaleTimeString("tr-TR", { hour: "2-digit", minute: "2-digit" });
  if (isToday) return `Bugün ${time}`;
  if (isYesterday) return `Dün ${time}`;
  return d.toLocaleDateString("tr-TR", { day: "numeric", month: "short", year: d.getFullYear() === today.getFullYear() ? undefined : "numeric" }) + ` ${time}`;
}

export function formatBytes(n: number): string {
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export function voiceName(v: { alias: string | null; display_name: string }): string {
  return v.alias || v.display_name;
}
