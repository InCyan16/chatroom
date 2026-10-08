import { render } from "preact";
import { useEffect, useRef, useState, useMemo } from "preact/hooks";
import "./style.css";
import { messageDate, groupedMessage } from "./messages.js";
import { mentionSuggestions, mentionedText } from "./interactions.js";
import { notifyMentions } from "./notifications.js";
import { snapshotSync } from "./sync.js";
import { defaultFonts, loadFonts, saveFonts } from "./settings.js";
import { ingest, timeline, suggestions, privatePeers, privateMessages, redactCommand, helpSections } from "./console.js";

if (import.meta.env.PROD && location.protocol !== "https:") {
  location.replace(
    `https://${location.host}${location.pathname}${location.search}${location.hash}`,
  );
} else {
  render(<App />, document.getElementById("app"));
}

async function api(path, body) {
  const response = await fetch(new URL(`api/${path}`, document.baseURI), {
    method: body === undefined ? "GET" : "POST",
    credentials: "same-origin",
    cache: "no-store",
    ...(body === undefined
      ? {}
      : {
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        }),
  });
  const value = await response.json();
  if (!response.ok)
    throw new Error(value.error || "Something went wrong. Please try again.");
  return value;
}

function App() {
  const [account, setAccount] = useState(undefined);
  const [initialError, setInitialError] = useState("");
  useEffect(() => {
    api("me")
      .then(setAccount)
      .catch((error) => {
        if (
          error.message !== "Please log in." &&
          error.message !== "Account unavailable."
        )
          setInitialError(error.message);
        setAccount(null);
      });
  }, []);
  if (account === undefined)
    return <div class="loading">Opening Commonroom…</div>;
  return account ? (
    <Chat initial={account} onLogout={() => setAccount(null)} />
  ) : (
    <Login initialError={initialError} onLogin={setAccount} />
  );
}

function Brand() {
  return (
    <div class="brand">
      <svg class="brand-mark" viewBox="0 0 36 36" fill="none" aria-hidden="true">
        <path d="M28 7H8v22h20v-6" />
        <path d="M15 12h15v9H19l-4 4V12Z" />
        <path d="M20 16.5h5" />
      </svg>
      <span>CommonRoom</span>
    </div>
  );
}

function Login({ onLogin, initialError }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState(initialError);
  const [busy, setBusy] = useState(false);
  async function submit(event) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api("login", { username, password });
      onLogin(await api("me"));
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <main class="login-page">
      <div class="login-top">
        <Brand />
      </div>
      <section class="login-card">
        <h1>Sign in.</h1>
        <form onSubmit={submit}>
          <label for="username">Username</label>
          <input
            id="username"
            autoComplete="username"
            autoFocus
            maxLength={32}
            value={username}
            onInput={(e) => setUsername(e.currentTarget.value)}
            required
            placeholder="Your username"
          />
          <label for="password">Password</label>
          <input
            id="password"
            type="password"
            autoComplete="current-password"
            maxLength={128}
            value={password}
            onInput={(e) => setPassword(e.currentTarget.value)}
            required
            placeholder="Your password"
          />
          {error && (
            <div class="form-error" role="alert">
              {error}
            </div>
          )}
          <button class="primary" disabled={busy}>
            {busy ? "Signing in…" : "Sign in"}
            <span aria-hidden="true">↗</span>
          </button>
        </form>
      </section>
    </main>
  );
}

function Chat({ initial, onLogout }) {
  const [fonts, setFonts] = useState(() => { try { return loadFonts(localStorage); } catch { return {...defaultFonts}; } });
  const [settingsOpen, setSettingsOpen] = useState(false);
  useEffect(() => {
    document.documentElement.style.setProperty("--chat-font-size", `${fonts.chat}px`);
    document.documentElement.style.setProperty("--ui-font-size", `${fonts.ui}px`);
    try { saveFonts(localStorage, fonts); } catch { /* Storage may be blocked. */ }
  }, [fonts]);
  const [state, setState] = useState(initial);
  const currentState = useRef(initial);
  const [selected, setSelected] = useState(() => {
    const requested = new URLSearchParams(location.hash.slice(1)).get("room");
    return (requested?.startsWith("@direct:") && privatePeers(initial).includes(requested.slice(8))) || (requested === "@command" && initial.admin) ||
      initial.rooms.some((room) => room.name === requested)
      ? requested
      : initial.admin ? "@command" : initial.rooms[0]?.name || "@direct";
  });
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const [status, setStatus] = useState("connecting");
  const [draft, setDraft] = useState("");
  const [replyTarget, setReplyTarget] = useState(null);
  const [notifications, setNotifications] = useState(() => {
    try { return typeof Notification !== "undefined" && Notification.permission === "granted" && localStorage.getItem("commonroom-notifications") === "on"; } catch { return false; }
  });
  const notificationsRef = useRef(notifications);
  useEffect(() => { setReplyTarget(null); }, [selected]);
  async function toggleNotifications() {
    if (!("Notification" in window) || !window.isSecureContext) {
      append("Notifications", "Desktop notifications require a supported browser and HTTPS (or localhost).", true); return;
    }
    try {
      const enabled = !notifications && (Notification.permission === "granted" || await Notification.requestPermission() === "granted");
      notificationsRef.current = enabled;
      setNotifications(enabled);
      localStorage.setItem("commonroom-notifications", enabled ? "on" : "off");
      if (!enabled && Notification.permission === "denied") append("Notifications", "Notifications are blocked. Change permission in Chrome site settings.", true);
    } catch { append("Notifications", "This browser cannot enable notifications.", true); }
  }
  function reactTo(message, value) {
    if (!value.trim() || request.current || socket.current?.readyState !== WebSocket.OPEN) return;
    const id = ++serial.current;
    const text = `/react ${message.id} ${value.trim()}`;
    request.current = {id, text, room:selected, key:null, action:true};
    setPending(true);
    socket.current.send(JSON.stringify({id, room: direct || consoleView ? null : selected, text}));
  }
  const [output, setOutput] = useState([]);
  const [cleared, setCleared] = useState({});
  const [pending, setPending] = useState(false);
  const [menu, setMenu] = useState(() => window.matchMedia("(min-width: 701px)").matches);
  const [composerHeight, setComposerHeight] = useState(24);
  const minimumComposerHeight = Math.ceil(fonts.chat * 1.5 + 6);
  const actualComposerHeight = Math.max(minimumComposerHeight, composerHeight);
  const resizeStart = useRef(null);
  const menuButton = useRef(null);
  function closeMenu() {
    setMenu(false);
    menuButton.current?.focus();
  }
  useEffect(() => {
    if (!menu || settingsOpen) return;
    const close = (event) => { if (event.key === "Escape") closeMenu(); };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [menu, settingsOpen]);
  function resizeComposer(height) {
    setComposerHeight(Math.max(minimumComposerHeight, Math.min(240, window.innerHeight * 0.4, height)));
  }
  function startResize(event) {
    if (event.button !== 0) return;
    event.preventDefault();
    resizeStart.current = { y: event.clientY, height: input.current.clientHeight };
    event.currentTarget.setPointerCapture(event.pointerId);
  }
  function moveResize(event) {
    if (resizeStart.current) resizeComposer(resizeStart.current.height + resizeStart.current.y - event.clientY);
  }
  function endResize() { resizeStart.current = null; }
  const [hintIndex, setHintIndex] = useState(0);
  const [hintDismissed, setHintDismissed] = useState(false);
  const ledger = useRef(null);
  if (!ledger.current) ledger.current = ingest(initial);
  const socket = useRef(null);
  const request = useRef(null);
  const serial = useRef(0);
  const desiredRoom = useRef(null);
  const end = useRef(null);
  const input = useRef(null);
  const room = state.rooms.find((r) => r.name === selected);
  const peers = useMemo(() => privatePeers(state), [state.private_peers, state.direct, state.username]);
  const direct = selected === "@direct" || selected.startsWith("@direct:");
  const peer = selected.startsWith("@direct:") ? selected.slice(8) : null;
  const consoleView = selected === "@command";
  const messages = useMemo(() => direct ? privateMessages(state.direct, state.username, peer) : room?.messages || [], [direct, peer, state.direct, state.username, room?.messages]);
  const entries = useMemo(() => timeline(messages, output, ledger.current, cleared[selected] || 0, selected), [messages, output, cleared, selected]);
  const renderedEntries = useMemo(() => {
    let lastDay = null;
    return entries.map((entry, index) => {
      if (entry.kind !== "message") return <ConsoleOutput key={entry.key} entry={entry} />;
      const date = messageDate(entry.message.time);
      const newDay = lastDay !== date.key;
      lastDay = date.key;
      const previous = entries[index - 1];
      const grouped = previous?.kind === "message" && groupedMessage(entry.message, previous.message);
      return <div key={entry.key}>
        {newDay && <div class="message-day"><time dateTime={date.iso}>{date.label}</time></div>}
        <ConsoleMessage message={entry.message} self={state.username} date={date} grouped={grouped && !entry.message.reply} pending={pending}
          onReact={(value) => reactTo(entry.message, value)} onReply={() => { setReplyTarget({...entry.message, view:selected}); input.current?.focus(); }} />
      </div>;
    });
  }, [entries, state.username, selected, pending]);
  const hints =
    hintDismissed || pending
      ? []
      : (draft.startsWith("/") ? suggestions(
          draft,
          state.commands || [],
          state.users,
          state.available_rooms || state.rooms.map((r) => r.name),
        ) : mentionSuggestions(draft, direct ? [state.username, ...(peer ? [peer] : [])] : [...(room?.members || [])])).slice(0, 6);
  const activeHint = Math.min(hintIndex, Math.max(0, hints.length - 1));

  function append(command, result = null, error = false, view = selected) {
    command = redactCommand(command);
    const order = ++ledger.current.sequence;
    const entry = {
      kind: "command",
      key: `local-${order}`,
      order,
      command,
      result,
      error,
      room: view,
    };
    setOutput((previous) => [...previous, entry].slice(-200));
    return entry.key;
  }
  function complete(key, result, error = false) {
    setOutput((previous) =>
      previous.map((entry) =>
        entry.key === key ? { ...entry, result, error } : entry,
      ),
    );
  }

  useEffect(() => {
    let stopped = false;
    let retry;
    let attempts = 0;
    const sync = snapshotSync(() => api("me"), (data, baseline) => {
      notifyMentions(currentState.current, data, {
        baseline, enabled: notificationsRef.current,
        NotificationClass: window.Notification,
        background: document.hidden || !document.hasFocus(), selected: selectedRef.current,
        open: view => { window.focus(); setSelected(view); },
      });
      ingest(data, ledger.current);
      currentState.current = data;
      setState(data);
      if (desiredRoom.current && data.rooms.some(r => r.name === desiredRoom.current)) {
        setSelected(desiredRoom.current);
        desiredRoom.current = null;
      }
    }, error => {
      if (["Please log in.", "Account unavailable."].includes(error.message)) onLogout();
    });
    const resume = () => { if (!document.hidden) sync.refresh(); };
    document.addEventListener("visibilitychange", resume);
    window.addEventListener("focus", resume);
    window.addEventListener("pageshow", resume);
    window.addEventListener("online", resume);
    async function connect() {
      if (stopped) return;
      setStatus("connecting");
      const url = new URL("ws", document.baseURI);
      url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
      const ws = new WebSocket(url);
      socket.current = ws;
      ws.onopen = () => {
        attempts = 0;
        setStatus("online");
        sync.refresh();
      };
      let baseline = true;
      ws.onmessage = (event) => {
        let data;
        try {
          data = JSON.parse(event.data);
        } catch {
          return;
        }
        if (data.kind === "snapshot") {
          sync.receive(data, baseline);
          baseline = false;
        } else if (["notice", "error"].includes(data.kind)) {
          const current = request.current;
          if (!current || data.id !== current.id) return;
          request.current = null;
          setPending(false);
          const error = data.kind === "error";
          if (current.key) complete(current.key, data.text || "Done.", error);
          else if (error) append(current.text, data.text, true, current.room);
          if (!error && !current.action) {
            setDraft("");
            if (current.reply) setReplyTarget(null);
            const [command, target] = current.text.trim().split(/\s+/);
            if (["/new", "/join"].includes(command) && target) {
              desiredRoom.current = target;
              if (currentState.current.rooms.some((room) => room.name === target)) {
                setSelected(target);
                desiredRoom.current = null;
              }
            }
            if (command === "/tell" && target) setSelected(`@direct:${target}`);
          }
        }
      };
      ws.onclose = async () => {
        if (stopped) return;
        setStatus("offline");
        if (request.current) {
          const current = request.current;
          const error =
            "Connection lost before confirmation. Check history before sending again.";
          if (current.key) complete(current.key, error, true);
          else append(current.text, error, true, current.room);
          request.current = null;
          setPending(false);
        }
        await sync.refresh();
        if (!stopped) retry = setTimeout(connect, Math.min(1000 * 2 ** attempts++, 15000));
      };
      ws.onerror = () => ws.close();
    }
    connect();
    return () => {
      stopped = true;
      sync.stop();
      document.removeEventListener("visibilitychange", resume);
      window.removeEventListener("focus", resume);
      window.removeEventListener("pageshow", resume);
      window.removeEventListener("online", resume);
      clearTimeout(retry);
      socket.current?.close();
    };
  }, []);

  useEffect(() => {
    if (!direct && !(consoleView && state.admin) && !state.rooms.some((r) => r.name === selected))
      setSelected(state.admin ? "@command" : state.rooms[0]?.name || "@direct");
  }, [state.rooms, state.admin, selected]);
  useEffect(() => {
    history.replaceState(null, "", `#room=${encodeURIComponent(selected)}`);
  }, [selected]);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [messages.at(-1)?.id, selected, output, cleared]);
  useEffect(() => {
    if (!pending) input.current?.focus();
  }, [pending]);

  async function logout() {
    try {
      await api("logout", {});
      onLogout();
    } catch (error) {
      append("/logout", error.message, true);
    }
  }
  function choose(name) {
    setSelected(name);
    if (window.matchMedia("(max-width: 700px)").matches) setMenu(false);
    input.current?.focus();
  }
  function edit(value) {
    setDraft(value);
    setHintIndex(0);
    setHintDismissed(false);
  }
  function acceptHint(hint) {
    if (!hint?.value) return;
    edit(hint.value);
    input.current?.focus();
  }
  function send(event) {
    event.preventDefault();
    const text = draft.trim();
    if (!text || pending) return;
    if (!text.startsWith("/") && Array.from(text).length > 4000) {
      append(text, "Messages support at most 4000 characters.", true);
      return;
    }
    if (text === "/clear") {
      setOutput((previous) => previous.filter((entry) => entry.room !== selected));
      setCleared((previous) => ({...previous, [selected]: ledger.current.sequence}));
      setDraft("");
      return;
    }
    if (text === "/logout") {
      logout();
      return;
    }
    if (socket.current?.readyState !== WebSocket.OPEN) {
      append(text, "Disconnected. Wait for the connection to recover.", true);
      return;
    }
    if (direct && !peer && !text.startsWith("/")) {
      append(text, "Use /tell user message for private messages.", true);
      return;
    }
    if (consoleView && !text.startsWith("/")) {
      append(text, "Use a command or select a room to send a message.", true);
      return;
    }
    const id = ++serial.current;
    request.current = {
      id,
      text,
      room: selected,
      key: text.startsWith("/") ? append(text) : null,
      reply: !!replyTarget && !text.startsWith("/"),
    };
    setPending(true);
    setHintDismissed(true);
    let wireText = peer && !text.startsWith("/") ? `/tell ${peer} ${text}` : draft;
    if (replyTarget && !text.startsWith("/")) wireText = `/reply ${replyTarget.id} ${draft}`;
    if (peer && /^\/history(?:\s+\d+)?$/.test(text))
      wireText = `${text === "/history" ? "/history 50" : text} ${peer}`;
    socket.current.send(
      JSON.stringify({ id, room: direct || consoleView ? null : selected, text: wireText }),
    );
  }
  function keydown(event) {
    if (event.isComposing) return;
    if (event.key === "Escape") {
      setHintDismissed(true);
      return;
    }
    if (hints.length && ["ArrowUp", "ArrowDown"].includes(event.key)) {
      event.preventDefault();
      setHintIndex(
        (activeHint + (event.key === "ArrowDown" ? 1 : hints.length - 1)) %
          hints.length,
      );
      return;
    }
    if (event.key === "Tab" && hints[activeHint]?.value) {
      event.preventDefault();
      acceptHint(hints[activeHint]);
      return;
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send(event);
    }
  }
  return (
    <div class="chat-layout">
      <aside id="navigation" class={`sidebar ${menu ? "visible" : ""}`}>
        <div class="sidebar-heading">
          <Brand />
          <button class="sidebar-close" aria-label="Close navigation" onClick={closeMenu}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
          </button>
        </div>
        <nav class="navigation" aria-label="Conversations">
          {state.admin && <button class={`room-link command-link ${consoleView ? "selected" : ""}`} onClick={() => choose("@command")}><span class="hash">&gt;</span>Command</button>}
          <details class="nav-group" open>
            <summary>Rooms<span class="group-count">{state.rooms.length}</span></summary>
            <div class="nav-items">
              {state.rooms.map((r) => <button key={r.name} class={`room-link ${selected === r.name ? "selected" : ""}`} onClick={() => choose(r.name)}><span class="hash">#</span>{r.name}<span class="room-count">{r.members.length}</span></button>)}
              {!state.rooms.length && <p class="no-rooms">No rooms</p>}
            </div>
          </details>
          <details class="nav-group" open>
            <summary>Private messages<span class="group-count">{peers.length}</span></summary>
            <div class="nav-items">
              {peers.map((name) => <button key={name} class={`room-link ${peer === name ? "selected" : ""}`} onClick={() => choose(`@direct:${name}`)}><span class="hash">↗</span>{name}</button>)}
              {!peers.length && <p class="no-rooms">No conversations</p>}
            </div>
          </details>
        </nav>
        <div class="sidebar-bottom">
          <div class="profile">
            <div class="profile-name">
              <strong>{state.username}</strong>
              <small>{state.admin ? "admin" : "user"}</small>
            </div>
            <button class="settings-toggle" onClick={() => setSettingsOpen(true)} aria-label="Settings" title="Settings">
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 3-.6 2.2-1.7 1L4.5 6l-2 3.5L4 11v2l-1.5 1.5 2 3.5 2.2-.2 1.7 1L9 21h4l.6-2.2 1.7-1 2.2.2 2-3.5L18 13v-2l1.5-1.5-2-3.5-2.2.2-1.7-1L13 3H9Z" /><circle cx="11" cy="12" r="3" /></svg>
            </button>
            <button class="notification-toggle" onClick={toggleNotifications} aria-pressed={notifications} aria-label={notifications ? "Disable mention notifications" : "Enable mention notifications"} title={notifications ? "Mention notifications on" : "Enable mention notifications"}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 17h14l-2-3V9a5 5 0 0 0-10 0v5l-2 3Zm5 3h4" /></svg>
            </button>
            <button
              class="signout"
              onClick={logout}
              aria-label="Sign out"
              title="Sign out"
            >
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 5H5v14h5M9 12h11m-4-4 4 4-4 4" /></svg>
            </button>
          </div>
        </div>
      </aside>
      {settingsOpen && <Settings fonts={fonts} onChange={setFonts} onClose={() => setSettingsOpen(false)} />}
      {menu && (
        <button
          class="scrim"
          aria-label="Close navigation"
          onClick={closeMenu}
        />
      )}
      <main class="conversation">
        <header class="chat-header">
          <button
            class="mobile-menu"
            ref={menuButton}
            aria-label={menu ? "Close navigation" : "Open navigation"}
            aria-expanded={menu}
            aria-controls="navigation"
            onClick={() => menu ? closeMenu() : setMenu(true)}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 6h14M5 12h14M5 18h14" /></svg>
          </button>
          <div class="room-title">
            <span>{consoleView ? ">" : direct ? "↗" : "#"}</span>
            <h1>{consoleView ? "Command" : direct ? peer || "Private messages" : selected}</h1>
          </div>
          <span class={`connection ${status}`}>
            <i />
            {status === "online"
              ? "Connected"
              : status === "connecting"
                ? "Connecting"
                : "Reconnecting"}
          </span>
        </header>
        <section
          class="message-list console-history"
          aria-label="Conversation history"
          aria-live="polite"
        >
          {renderedEntries}
          <div ref={end} />
        </section>
        <div class="composer-area">
          <div class="composer-resize" role="separator" tabIndex={0}
            aria-label="Resize message input" aria-orientation="horizontal"
            aria-valuemin={minimumComposerHeight} aria-valuemax={Math.floor(Math.min(240, window.innerHeight * 0.4))} aria-valuenow={actualComposerHeight}
            onPointerDown={startResize} onPointerMove={moveResize} onPointerUp={endResize} onPointerCancel={endResize} onLostPointerCapture={endResize}
            onKeyDown={(event) => {
              if (["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) {
                event.preventDefault();
                resizeComposer(event.key === "Home" ? minimumComposerHeight : event.key === "End" ? 240 : actualComposerHeight + (event.key === "ArrowUp" ? 16 : -16));
              }
            }}><span /></div>
          {hints.length > 0 && (
            <ul
              class="command-hints"
              id="command-hints"
              role="listbox"
              aria-label="Command suggestions"
            >
              {hints.map((hint, index) => (
                <li
                  id={`hint-${index}`}
                  key={hint.label}
                  role="option"
                  aria-selected={index === activeHint}
                >
                  <button
                    type="button"
                    tabIndex={-1}
                    class={index === activeHint ? "active" : ""}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => acceptHint(hint)}
                    disabled={!hint.value}
                  >
                    <code>{hint.label}</code>
                    <span>{hint.description}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {replyTarget && <div class="reply-composer"><div><strong>Reply to {replyTarget.from}</strong><p dir="auto">{Array.from(replyTarget.text).slice(0,160).join("")}</p></div><button type="button" aria-label="Cancel reply" onClick={() => setReplyTarget(null)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button></div>}
          <form class="composer" onSubmit={send}>
            <span class="prompt" aria-hidden="true">
              ›
            </span>
            <textarea
              ref={input}
              dir="auto"
              rows={1}
              style={{ height: `${actualComposerHeight}px` }}
              aria-label="Message or command"
              aria-autocomplete="list"
              aria-controls={hints.length ? "command-hints" : undefined}
              aria-activedescendant={
                hints.length ? `hint-${activeHint}` : undefined
              }
              placeholder={consoleView ? "Command" : "Message or command"}
              value={draft}
              disabled={pending}
              onInput={(event) => edit(event.currentTarget.value)}
              onKeyDown={keydown}
            />
            <button
              class="send-button"
              disabled={pending || !draft.trim()}
              aria-label="Send message"
            >
              {pending ? "…" : <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 20V4m-6 6 6-6 6 6" /></svg>}
            </button>
          </form>
        </div>
      </main>
    </div>
  );
}
function Settings({fonts, onChange, onClose}) {
  const dialog = useRef(null);
  useEffect(() => {
    const element = dialog.current;
    element.showModal();
    return () => element.close();
  }, []);
  return <dialog class="settings-dialog" ref={dialog} aria-labelledby="settings-title" onCancel={event => {event.preventDefault();onClose();}}>
    <header class="settings-heading"><h2 id="settings-title">Settings</h2><button class="settings-close" aria-label="Close settings" onClick={onClose}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button></header>
    <div class="font-setting"><label for="chat-font-size">Chat font size</label><output for="chat-font-size">{fonts.chat}px</output><input id="chat-font-size" type="range" min="14" max="24" step="1" value={fonts.chat} onInput={event => onChange({...fonts,chat:Number(event.currentTarget.value)})} /></div>
    <div class="font-setting"><label for="ui-font-size">UI font size</label><output for="ui-font-size">{fonts.ui}px</output><input id="ui-font-size" type="range" min="12" max="18" step="1" value={fonts.ui} onInput={event => onChange({...fonts,ui:Number(event.currentTarget.value)})} /></div>
    <p class="font-preview">The quick brown fox · 你好 · مرحبا 👋</p>
    <button class="settings-reset" onClick={() => onChange({...defaultFonts})}>Reset defaults</button>
  </dialog>;
}

function ConsoleOutput({ entry }) {
  const help = entry.command === "/help" && !entry.error && entry.result;
  return (
    <article
      class={`console-output ${entry.error ? "error" : ""}`}
      data-local-output="true"
    >
      <div class="console-input">
        <span aria-hidden="true">›</span>
        <code>{entry.command}</code>
      </div>
      {help ? (
        <div class="help-sections">
          {helpSections(entry.result).map((group) => <section key={group.title}>
            <h2>{group.title}</h2>
            <ul class="help-list" aria-label={`${group.title} commands`}>
              {group.commands.map(({usage, description}) => <li key={usage}><code>{usage}</code><span>{description}</span></li>)}
            </ul>
          </section>)}
        </div>
      ) : (
        <pre>{entry.result ?? "…"}</pre>
      )}
    </article>
  );
}
function ConsoleMessage({ message, self, date, grouped, pending, onReact, onReply }) {
  const [reaction, setReaction] = useState("");
  const [reactionOpen, setReactionOpen] = useState(false);
  const text = mentionedText(message.text, message.mentions);
  return (
    <article id={`message-${message.id}`} class={`chat-message ${message.from === self ? "own" : ""} ${grouped ? "grouped" : ""}`} title={date.full}>
      {grouped && <time class="continuation-time" dateTime={date.iso} aria-label={date.full}>{date.time}</time>}
      <div class="message-content">
        {!grouped && <header class="message-meta"><strong class="message-author">{message.from}</strong><time dateTime={date.iso} title={date.full} aria-label={date.full}>{date.time}</time></header>}
        {message.reply && <button class="reply-quote" onClick={() => document.getElementById(`message-${message.reply.id}`)?.scrollIntoView({block:"center"})} title="Jump to original message if it is loaded"><strong>↳ {message.reply.from}</strong><span dir="auto">{message.reply.text}</span></button>}
        <p dir="auto">{text.map((part,index) => part.mention ? <mark key={index} class="mention">{part.text}</mark> : part.text)}</p>
        <div class="message-reactions">{Object.entries(message.reactions || {}).map(([value, users]) => <button key={value} disabled={pending} aria-pressed={users.includes(self)} title={users.join(", ")} aria-label={`React ${value}: ${users.length}`} onClick={() => onReact(value)}>{value}<span>{users.length}</span></button>)}</div>
      </div>
      <div class="message-actions">
        <button type="button" disabled={pending} onClick={onReply} aria-label={`Reply to ${message.from}'s message`}>Reply</button>
        <details open={reactionOpen} onToggle={event => setReactionOpen(event.currentTarget.open)}><summary aria-label={`Add reaction to ${message.from}'s message`}>+</summary><div class="reaction-picker">
          <div class="reaction-choices">{["👍","❤️","😂","🎉","👀"].map(value => <button key={value} type="button" disabled={pending} aria-label={`Add reaction ${value}`} onClick={() => { onReact(value); setReactionOpen(false); }}>{value}</button>)}</div>
          <form onSubmit={(event) => { event.preventDefault(); onReact(reaction); setReaction(""); setReactionOpen(false); }}><input aria-label="Custom reaction" placeholder="Emoji or text" value={reaction} onInput={e => setReaction(e.currentTarget.value)} /><button disabled={pending || !reaction.trim()}>Add</button></form>
        </div></details>
      </div>
    </article>
  );
}
