export function mentionSuggestions(draft, users) {
  if (draft.startsWith("/")) return [];
  const match = draft.match(/(?:^|[^\p{L}\p{N}_@-])@([A-Za-z0-9_-]*)$/u);
  if (!match) return [];
  const prefix = match[1];
  return users.filter(name => name.startsWith(prefix)).map(name => ({
    label: `@${name}`, description: "Mention", value: draft.slice(0, draft.lastIndexOf("@")) + `@${name} `,
  }));
}
export function mentionEvents(previous, next) {
  const seen = new Set([...previous.rooms.flatMap(r => r.messages), ...previous.direct].map(m => m.id));
  const previousRooms = new Map(previous.rooms.map(r => [r.name, r]));
  const events = [];
  function add(messages, older, view) {
    const newest = Math.max(0, ...older.map(m => m.time));
    for (const message of messages) {
      if (!seen.has(message.id) && message.time >= newest && message.from !== next.username && message.mentions?.includes(next.username)) events.push({message, view});
    }
  }
  for (const room of next.rooms) {
    if (previousRooms.has(room.name)) add(room.messages, previousRooms.get(room.name).messages, room.name);
  }
  add(next.direct, previous.direct, null);
  return events.map(e => ({...e, view: e.view || `@direct:${e.message.from}`}));
}
export function mentionedText(text, mentions = []) {
  const names = new Set(mentions);
  return text.split(/(@[A-Za-z0-9_-]+)/).map((part, index, parts) => ({
    text: part,
    mention: part.startsWith("@") && names.has(part.slice(1)) && (!index || !/[\p{L}\p{N}_@-]$/u.test(parts[index - 1])),
  }));
}
