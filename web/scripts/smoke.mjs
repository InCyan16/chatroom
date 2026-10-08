import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, cp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import WebSocket from "ws";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const binary = join(
  root,
  "target/debug",
  process.platform === "win32" ? "chat.exe" : "chat",
);
const temporary = await mkdtemp(join(tmpdir(), "commonroom-smoke-"));
const origin = "http://localhost:5173";
const publicOrigin = "https://chat.example.com";
let production = false;
const sockets = [];
let child;
let base;
let logs = "";
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(predicate, label) {
  const start = Date.now();
  while (!predicate()) {
    if (Date.now() - start > 10000)
      throw new Error(`Timed out: ${label}\n${logs}`);
    await pause(30);
  }
}
async function start(data, release = false, trust, baseUrl) {
  logs = "";
  production = release;
  const env = { ...process.env, CHAT_DATA: data, RUST_LOG: "chat=info" };
  delete env.CHAT_PRODUCTION;
  delete env.CHAT_ORIGIN;
  delete env.CHAT_BIND;
  delete env.CHAT_TRUST;
  delete env.CHAT_BASE_URL;
  if (baseUrl) env.CHAT_BASE_URL = baseUrl;
  if (trust) env.CHAT_TRUST = trust;
  if (release) {
    env.CHAT_ORIGIN = publicOrigin;
    env.CHAT_BIND = "127.0.0.1:0";
  }
  child = spawn(
    release
      ? join(root, process.platform === "win32" ? "chat.exe" : "chat")
      : binary,
    [],
    {
      cwd: release ? temporary : root,
      env,
      stdio: ["pipe", "pipe", "pipe"],
    },
  );
  let failure;
  child.on("error", (error) => {
    failure = error;
  });
  child.stdout.on("data", (data) => {
    logs += data.toString();
  });
  child.stderr.on("data", (data) => {
    logs += data.toString();
  });
  await until(
    () => failure || /Chat server ready/.test(logs) || child.exitCode !== null,
    "server startup",
  );
  if (failure) throw failure;
  assert.equal(child.exitCode, null, logs);
  const match = logs.match(/address=127\.0\.0\.1:(\d+)/);
  assert.ok(match, logs);
  base = `http://127.0.0.1:${match[1]}${(baseUrl || "/").replace(/\/$/, "")}`;
}
async function stop() {
  for (const socket of sockets.splice(0)) socket.ws.terminate();
  if (!child || child.exitCode !== null) return;
  const current = child;
  current.kill("SIGTERM");
  await until(
    () => current.exitCode !== null || current.signalCode !== null,
    "graceful shutdown",
  );
  assert.equal(current.exitCode, 0, logs);
  child = null;
}
async function request(
  path,
  cookie,
  body,
  requestOrigin = production ? publicOrigin : origin,
  protocol = production ? "https" : null,
) {
  return fetch(`${base}/api/${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: {
      Origin: requestOrigin,
      ...(protocol ? { "X-Forwarded-Proto": protocol } : {}),
      ...(cookie ? { Cookie: cookie } : {}),
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
}
async function login(username, password = `${username}-long-password`) {
  const response = await request("login", null, {
    username,
    password,
  });
  assert.equal(response.status, 200, await response.clone().text());
  const cookie = response.headers.get("set-cookie");
  assert.match(cookie, /HttpOnly/);
  return cookie.split(";")[0];
}
async function connect(cookie) {
  const client = {
    ws: new WebSocket(`${base.replace("http:", "ws:")}/ws`, {
      headers: {
        Origin: production ? publicOrigin : origin,
        Cookie: cookie,
        ...(production ? { "X-Forwarded-Proto": "https" } : {}),
      },
    }),
    frames: [],
    serial: 0,
    snapshot: null,
  };
  sockets.push(client);
  client.ws.on("message", (raw) => {
    const frame = JSON.parse(raw);
    client.frames.push(frame);
    if (frame.kind === "snapshot") client.snapshot = frame;
  });
  let error;
  client.ws.on("error", (e) => {
    error = e;
  });
  await until(() => error || client.snapshot, "initial WebSocket snapshot");
  if (error) throw error;
  return client;
}
async function send(client, text, room = "lobby", kind = "notice", escaped = false) {
    const id = ++client.serial;
  const frame = JSON.stringify({ id, room, text });
  client.ws.send(escaped ? frame.replaceAll("🙂", "\\ud83d\\ude42") : frame);
  await until(
    () => client.frames.some((f) => f.id === id),
    `acknowledgement for ${text}`,
  );
  const reply = client.frames.find((f) => f.id === id);
  assert.equal(reply.kind, kind, reply.text);
}

try {
  // Port zero avoids collisions. This setting persists through migration.
  const { mkdir, readFile, writeFile } = await import("node:fs/promises");
  const source = join(temporary, "original");
  await mkdir(source);
  await writeFile(
    join(source, "config.json"),
    JSON.stringify({
      bind: "127.0.0.1:0",
      origins: [origin],
      production: false,
      max_users: 4,
      max_rooms: 1,
    }),
  );
  await start(source);
  for (const [name, role] of [
    ["alice", "admin"],
    ["bob", "user"],
    ["eve", "user"],
  ]) {
    child.stdin.write(`/user ${name} ${name}-long-password ${role}\n`);
    await until(
      () => logs.includes(`Account ${name} created.`),
      `provision ${name}`,
    );
  }
  assert.equal((await request("me")).status, 401);
  assert.equal(
    (
      await request("login", null, {
        username: "alice",
        password: "wrong-password",
      })
    ).status,
    401,
  );
  assert.equal(
    (
      await request(
        "login",
        null,
        { username: "alice", password: "alice-long-password" },
        "https://evil.example",
      )
    ).status,
    403,
  );
  const cookies = {};
  for (const name of ["alice", "bob", "eve"]) cookies[name] = await login(name);
  const alice = await connect(cookies.alice),
    bob = await connect(cookies.bob),
    eve = await connect(cookies.eve);
  await send(bob, "/whoami");
  assert.match(
    bob.frames.find((f) => f.id === bob.serial).text,
    /Permission: user/,
  );
  await send(bob, "/grant bob", null, "error");
  await send(bob, "/configs", null, "error");
  await send(bob, "/clean 7d @all", null, "error");
  await send(alice, "/configs", null);
  const configs = JSON.parse(alice.frames.find((f) => f.id === alice.serial).text);
  assert.equal(configs.max_users, 4);
  assert.equal(configs.max_rooms, 1);
  assert.equal(configs.max_messages, 1000);
  await send(alice, "/configs max_users 9", null, "error");
  await send(alice, "/help");
  const help = alice.frames.find((f) => f.id === alice.serial).text;
  assert.ok(help.split("\n").length > 10);
  assert.ok(help.includes("/reset"));
  assert.deepEqual(bob.snapshot.rooms, [], "new users start without rooms");
  assert.deepEqual(bob.snapshot.private_peers, [], "unmessaged accounts are not private contacts");
  await send(bob, "/user denied abc", null, "error");
  await send(bob, "/reset alice abc", null, "error");
  await send(bob, "/disable alice", null, "error");
  await send(bob, "/enable alice", null, "error");
  await send(alice, "/user carol abc", null);
  await send(alice, "/user dave abc", null, "error");
  child.stdin.write("/user dave abc\n");
  await until(() => logs.includes("User limit reached (4)."), "stdin user limit");
  const carolCookie = await login("carol", "abc");
  const carol = await connect(carolCookie);
  assert.deepEqual(carol.snapshot.rooms, []);
  await send(alice, "/reset carol xyz", null);
  await until(() => carol.ws.readyState === WebSocket.CLOSED, "admin password reset revokes sessions");
  await login("carol", "xyz");
  const carol2 = await connect(await login("carol", "xyz"));
  const carol3Cookie = await login("carol", "xyz");
  const carol3 = await connect(carol3Cookie);
  await send(carol2, "/passwd wrong abc", null, "error");
  await send(carol2, "/passwd xyz abc", null);
  await until(() => carol3.ws.readyState === WebSocket.CLOSED, "password change revokes other sessions");
  assert.equal((await request("me", carol3Cookie)).status, 401);
  await send(carol2, "/whoami", null);
  await send(alice, "/disable carol", null);
  await send(alice, "/enable carol", null);
  await send(alice, "/disable alice", null, "error");
  await send(alice, "/grant bob");
  await until(() => bob.snapshot.admin, "live admin grant");
  await send(bob, "/whoami");
  assert.match(
    bob.frames.find((f) => f.id === bob.serial).text,
    /Permission: admin/,
  );
  await send(alice, "/revoke bob");
  await until(() => !bob.snapshot.admin, "live admin revoke");
  assert.ok(
    !eve.frames.some((f) => f.kind === "notice" || f.kind === "error"),
    "command replies must not reach other clients",
  );
  assert.equal(
    alice.snapshot.rooms.flatMap((r) => r.messages).length,
    0,
    "commands must not enter persisted chat history",
  );
  await send(bob, "/new forbidden", null, "error");
  await send(alice, "/new study");
  await send(alice, "/new overflow", null, "error");
  child.stdin.write("/new overflow\n");
  await until(() => logs.includes("Room limit reached (1)."), "stdin room limit");
  await send(bob, "/join study", null, "error");
  await send(alice, "/add bob study");
  await until(
    () => bob.snapshot.rooms.some((r) => r.name === "study"),
    "membership delivery",
  );
  await send(bob, "a retained room message", "study");
  const multilingual = "你好 日本語 한국어 مرحبا नमस्ते Привет שלום 🙂 e\u0301";
  await send(bob, multilingual, "study");
  await until(() => bob.snapshot.rooms.find(r => r.name === "study").messages.at(-1)?.text === multilingual, "Unicode room delivery");
  const roomMessageId = bob.snapshot.rooms.find(r => r.name === "study").messages.at(-1).id;
  await send(alice, `/react ${roomMessageId} 好👍`, "study");
  await until(() => bob.snapshot.rooms.find(r => r.name === "study").messages.find(m => m.id === roomMessageId)?.reactions["好👍"]?.includes("alice"), "reaction broadcast");
  await send(bob, `/react ${roomMessageId} 好👍`, "study");
  await until(() => alice.snapshot.rooms.find(r => r.name === "study").messages.find(m => m.id === roomMessageId)?.reactions["好👍"]?.length === 2, "shared reaction count");
  await send(alice, `/react ${roomMessageId} 好👍`, "study");
  await send(bob, `/reply ${roomMessageId} @alice 回答 🙂`, "study");
  await until(() => alice.snapshot.rooms.find(r => r.name === "study").messages.at(-1)?.reply?.id === roomMessageId, "reply delivery");
  const roomReply = alice.snapshot.rooms.find(r => r.name === "study").messages.at(-1);
  assert.deepEqual(roomReply.mentions, ["alice"]);
  assert.equal(roomReply.reply.text, multilingual);
  await send(eve, `/react ${roomMessageId} 👀`, "study", "error");
  await send(eve, `/reply ${roomMessageId} no access`, "study", "error");
  const longUnicode = "🙂".repeat(4000);
  await send(bob, `/tell alice ${longUnicode}`, null, "notice", true);
  await until(() => alice.snapshot.direct.some((m) => m.text === longUnicode), "full-length UTF-8 private message");
  await send(bob, "/tell alice a private message");
  await until(
    () => alice.snapshot.direct.some((m) => m.text === "a private message"),
    "private message delivery",
  );
  assert.ok(
    !eve.frames.some((frame) =>
      frame.direct?.some((message) => message.text === "a private message"),
    ),
  );
  assert.ok(
    !eve.frames.some((frame) =>
      frame.rooms?.some((room) =>
        room.messages.some(
          (message) => message.text === "a retained room message",
        ),
      ),
    ),
  );
  const dmId = alice.snapshot.direct.find(m => m.text === "a private message").id;
  await send(alice, `/react ${dmId} ❤️`, null);
  await send(alice, `/reply ${dmId} @bob 私信`, null);
  await until(() => bob.snapshot.direct.at(-1)?.reply?.id === dmId, "private reply delivery");
  assert.equal(bob.snapshot.direct.at(-1).to, "bob");
  assert.deepEqual(bob.snapshot.direct.at(-1).mentions, ["bob"]);
  await send(eve, `/react ${dmId} ❤️`, null, "error");
  await send(eve, `/reply ${dmId} leak`, null, "error");
  await send(alice, "/kick bob study");
  await until(
    () => !bob.snapshot.rooms.some((r) => r.name === "study"),
    "membership revocation",
  );
  await send(bob, "not allowed", "study", "error");
  await send(alice, "/clean 7d study", null);
  await send(alice, "/add bob study");
  await stop();
  const destination = join(temporary, "migrated");
  await cp(source, destination, { recursive: true });
  const migratedConfigPath = join(destination, "config.json");
  const migratedConfig = JSON.parse(await readFile(migratedConfigPath, "utf8"));
  migratedConfig.max_messages = 4;
  await writeFile(migratedConfigPath, JSON.stringify(migratedConfig));
  await start(destination);
  const resumed = await request("me", cookies.alice);
  assert.equal(resumed.status, 200);
  const state = await resumed.json();
  assert.equal(state.admin, true);
  assert.ok(
    state.rooms
      .find((r) => r.name === "study")
      .messages.some((m) => m.text === "a retained room message"),
  );
  assert.ok(state.direct.some((m) => m.text === "a private message"));
  assert.ok(state.rooms.find(r => r.name === "study").messages.some(m => m.reply?.text === multilingual && m.mentions.includes("alice")));
  assert.ok(state.direct.some(m => m.reply && m.mentions.includes("bob")));
  assert.ok(state.direct.find(m => m.text === "a private message").reactions["❤️"].includes("alice"));
  const migratedAlice = await connect(cookies.alice);
  await send(migratedAlice, "/configs", null);
  assert.equal(JSON.parse(migratedAlice.frames.find((f) => f.id === migratedAlice.serial).text).max_messages, 4);
  for (const text of ["ring first", "ring oldest", "ring next", "ring middle 你好", "ring newest 🙂"]) {
    await send(migratedAlice, text, "study");
  }
  await until(() => migratedAlice.snapshot.rooms.find((r) => r.name === "study").messages.at(-1)?.text === "ring newest 🙂", "bounded room delivery");
  assert.deepEqual(migratedAlice.snapshot.rooms.find((r) => r.name === "study").messages.map((m) => m.text), ["ring oldest", "ring next", "ring middle 你好", "ring newest 🙂"]);
  await send(migratedAlice, "/history 5", "study", "error");
  await send(migratedAlice, "/history", "study");
  assert.match(migratedAlice.frames.find((f) => f.id === migratedAlice.serial).text, /ring middle 你好/);
  const boundedState = await (await request("me", cookies.alice)).json();
  assert.equal(boundedState.rooms.find((r) => r.name === "study").messages.length, 4);
  assert.ok(boundedState.direct.some((m) => m.text === "a private message"));
  const migratedBob = await connect(cookies.bob);
  child.stdin.write("/reset bob changed-long-password\n");
  await until(
    () => logs.includes("Account bob password reset."),
    "password reset",
  );
  await until(
    () => migratedBob.ws.readyState === WebSocket.CLOSED,
    "reset disconnects old socket",
  );
  assert.equal((await request("me", cookies.bob)).status, 401);
  child.stdin.write("/disable eve\n");
  await until(() => logs.includes("Disabled eve."), "disable account");
  assert.equal((await request("me", cookies.eve)).status, 401);
  assert.equal((await request("logout", cookies.alice, {})).status, 200);
  assert.equal((await request("me", cookies.alice)).status, 401);
  assert.ok(
    !logs.includes("changed-long-password"),
    "passwords must not appear in logs",
  );
  await stop();
  await start(join(temporary, "production"), true, undefined, "/commonroom/");
  const bare = await fetch(base, { headers: { "X-Forwarded-Proto": "https" }, redirect: "manual" });
  assert.equal(bare.status, 308);
  assert.equal(bare.headers.get("location"), "/commonroom/");
  assert.equal((await fetch(new URL("/api/health", base), { headers: { "X-Forwarded-Proto": "https" } })).status, 404);
  assert.equal(
    (await request("health", null, undefined, publicOrigin, null)).status,
    400,
  );
  assert.equal(
    (await request("health", null, undefined, publicOrigin, "http")).status,
    400,
  );
  assert.equal((await request("health")).status, 200);
  const page = await fetch(`${base}/`, {
    headers: { "X-Forwarded-Proto": "https" },
  });
  assert.equal(page.status, 200);
  assert.match(page.headers.get("strict-transport-security"), /max-age=/);
  assert.match(
    page.headers.get("content-security-policy"),
    /wss:\/\/chat\.example\.com/,
  );
  const html = await page.text();
  const assets = [...html.matchAll(/(?:src|href)="(\.\/assets\/[^\"]+)"/g)];
  assert.ok(assets.length >= 2, "binary must serve embedded JS and CSS");
  assert.ok(html.includes('<base href="/commonroom/">'));
  for (const [, path] of assets) {
    const asset = await fetch(new URL(path, `${base}/`), {
      headers: { "X-Forwarded-Proto": "https" },
    });
    assert.equal(asset.status, 200);
    assert.ok((await asset.text()).length > 0);
  }
  child.stdin.write("/user alice alice-long-password admin\n");
  await until(
    () => logs.includes("Account alice created."),
    "production account provisioning",
  );
  const secureLogin = await request("login", null, {
    username: "alice",
    password: "alice-long-password",
  });
  assert.equal(secureLogin.status, 200);
  assert.match(secureLogin.headers.get("set-cookie"), /; Secure/);
  assert.match(secureLogin.headers.get("set-cookie"), /Path=\/commonroom\//);
  const secureCookie = secureLogin.headers.get("set-cookie").split(";")[0];
  const secureClient = await connect(secureCookie);
  await send(secureClient, "/new production", null);
  await send(secureClient, "production websocket works", "production");
  assert.equal(
    (await request("logout", secureCookie, {}, "https://evil.example")).status,
    403,
  );
  const rejectedSocket = new WebSocket(`${base.replace("http:", "ws:")}/ws`, {
    headers: {
      Cookie: secureCookie,
      Origin: "https://evil.example",
      "X-Forwarded-Proto": "https",
    },
  });
  let rejected;
  rejectedSocket.on("error", (error) => {
    rejected = error;
  });
  await until(() => rejected, "untrusted WebSocket origin rejection");
  assert.match(rejected.message, /403/);
  assert.equal((await request("logout", secureCookie, {})).status, 200);
  await until(
    () => secureClient.ws.readyState === WebSocket.CLOSED,
    "logout disconnects socket",
  );
  await stop();
  await start(join(temporary, "untrusted-proxy"), true, "10.0.0.2");
  const spoofed = await fetch(`${base}/api/health`, {
    headers: {
      "X-Forwarded-Proto": "https",
      "X-Forwarded-For": "10.0.0.2",
      Origin: publicOrigin,
    },
  });
  assert.equal(spoofed.status, 403, "forwarded headers cannot impersonate a trusted socket peer");
  console.log(
    "PASS: login, roles, room privacy, DMs, revocation, folder migration, embedded assets, production HTTPS checks, secure cookies, and proxy IP trust.",
  );
} finally {
  try {
    await stop();
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}
