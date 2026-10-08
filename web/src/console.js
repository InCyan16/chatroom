// Commands and output live only in this tab; chat comes from server snapshots.
export function ingest(
  snapshot,
  ledger = { sequence: 0, messages: new Map() },
) {
  const messages = [
    ...snapshot.rooms.flatMap((r) => r.messages),
    ...snapshot.direct,
  ].sort((a, b) => a.time - b.time);
  const current = new Set(messages.map((message) => message.id));
  for (const key of ledger.messages.keys()) {
    if (!current.has(key)) ledger.messages.delete(key);
  }
  for (const message of messages) {
    if (!ledger.messages.has(message.id))
      ledger.messages.set(message.id, ++ledger.sequence);
  }
  return ledger;
}

export function timeline(messages, output, ledger, cleared = 0, view) {
  return [
    ...messages.map((message) => ({
      kind: "message",
      key: message.id,
      message,
      order: ledger.messages.get(message.id),
    })),
    ...output.filter((entry) => view === undefined || entry.room === view),
  ]
    .filter((entry) => entry.order > cleared)
    .sort((a, b) => a.order - b.order);
}

export function privatePeers(snapshot) {
  if (snapshot.private_peers) return [...snapshot.private_peers].sort();
  return [...new Set([
    ...snapshot.direct.map((message) => message.from === snapshot.username ? message.to : message.from),
  ].filter(Boolean))].sort();
}

export function privateMessages(messages, self, peer) {
  return messages.filter((message) =>
    (message.from === self && message.to === peer) ||
    (message.from === peer && message.to === self));
}
export function redactCommand(text) {
  if (/^\/passwd(?:\s|$)/.test(text)) return "/passwd •••• ••••";
  return text.replace(/^(\/(?:user|reset)\s+\S+\s+)\S+/, "$1••••");
}
export function helpSections(text) {
  const groups = [];
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    if (/^\[.+\]$/.test(line)) groups.push({title:line.slice(1,-1),commands:[]});
    else {
      if (!groups.length) groups.push({title:"Commands",commands:[]});
      const [usage,description] = line.split(" — ");
      groups.at(-1).commands.push({usage,description});
    }
  }
  return groups;
}

export function suggestions(draft, commands, users, rooms) {
  if (!draft.startsWith("/") || draft.includes("\n")) return [];
  const parts = draft.split(/\s+/);
  const name = parts[0];
  if (parts.length === 1) {
    return commands
      .filter((c) => c.name.startsWith(name))
      .map((c) => ({
        label: c.usage,
        description: c.description,
        value: c.name + (c.usage === c.name ? "" : " "),
      }));
  }
  const spec = commands.find((c) => c.name === name);
  if (!spec) return [];
  const position = parts.length - 1;
  const prefix = parts.at(-1);
  let values = [];
  if (
    ["/tell", "/grant", "/revoke", "/add", "/kick", "/reset", "/disable", "/enable"].includes(name) &&
    position === 1
  )
    values = users;
  if (
    ["/join", "/leave", "/delete", "/members"].includes(name) &&
    position === 1
  )
    values = rooms;
  if (["/add", "/kick"].includes(name) && position === 2) values = rooms;
  if (name === "/clean" && position === 2) values = [...rooms, "@private", "@all"];
  if (values.length)
    return values
      .filter((value) => value.startsWith(prefix))
      .map((value) => ({
        label: value,
        description: spec.usage,
        value: [...parts.slice(0, -1), value].join(" ") + " ",
      }));
  return [{ label: spec.usage, description: spec.description, value: null }];
}
