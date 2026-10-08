export const defaultFonts = {chat:16, ui:14};
export function fontSettings(value) {
  const size = (key, min, max) => Number.isFinite(value?.[key]) ? Math.round(Math.max(min, Math.min(max, value[key]))) : defaultFonts[key];
  return {chat:size("chat",14,24), ui:size("ui",12,18)};
}
export function loadFonts(storage) {
  try { return fontSettings(JSON.parse(storage.getItem("commonroom-fonts"))); }
  catch { return {...defaultFonts}; }
}
export function saveFonts(storage, value) {
  try { storage.setItem("commonroom-fonts", JSON.stringify(fontSettings(value))); }
  catch { /* Font controls still work when browser storage is unavailable. */ }
}
