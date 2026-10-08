import { mentionEvents } from "./interactions.js";
export function notifyMentions(previous, next, {baseline, enabled, NotificationClass, background, selected, open}) {
  if (baseline || !enabled || NotificationClass?.permission !== "granted") return 0;
  let delivered = 0;
  for (const {message, view} of mentionEvents(previous, next)) {
    if (!background && selected === view) continue;
    try {
      const notification = new NotificationClass(`${message.from} mentioned you`, {
        body: Array.from(message.text).slice(0, 200).join(""), tag: message.id,
      });
      notification.onclick = () => { open(view); notification.close(); };
      delivered++;
    } catch { /* A notification failure must never interrupt chat delivery. */ }
  }
  return delivered;
}
