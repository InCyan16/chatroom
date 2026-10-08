// No timeZone override: all visible dates follow the browser's timezone.
const clock = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
const calendar = new Intl.DateTimeFormat(undefined, { year: "numeric", month: "short", day: "numeric" });
const fullDate = new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "long" });
export function messageDate(seconds) {
  const date = new Date(seconds * 1000);
  return {
    key: `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`,
    label: calendar.format(date),
    time: clock.format(date),
    full: fullDate.format(date),
    iso: date.toISOString(),
  };
}
export function groupedMessage(message, previous) {
  return !!previous && message.from === previous.from &&
    message.time >= previous.time && message.time - previous.time < 300 &&
    messageDate(message.time).key === messageDate(previous.time).key;
}
