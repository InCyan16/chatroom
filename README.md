# Commonroom

A small, command-driven chatroom: Rust + Axum, a declarative Preact frontend,
and one portable SQLite data folder. No external database, public account signup,
third-party fonts, or runtime Node server. Built for a small, trusted circle.

## Development

Requires Rust 1.89+ (edition 2024), Node 20.19+ or 22.12+, pnpm, and RTK.
All shell commands below use RTK.

```sh
rtk pnpm --dir web install
rtk cargo run
```

In the Rust process's stdin, create accounts:

```text
/user alice a-long-unique-password admin
/user bob another-unique-password user
```

In another terminal:

```sh
rtk pnpm --dir web dev
```

Open **http://localhost:5173**. Vite proxies HTTP authentication and plain
WebSockets to Rust at `127.0.0.1:3000`. Account names are case sensitive.
Passwords require at least 3 characters and at most 128 bytes; stdin account commands use a single password
token without whitespace. Credentials and message bodies are never logged.

To inspect a built debug artifact without Vite:

```sh
rtk proxy ./build.sh debug
rtk proxy ./debug.sh
```

Open **http://localhost:3000**. `debug.sh` runs `target/debug/chat` and keeps
stdin available for superuser commands. The debug build embeds a development
web bundle from `web/dist-debug`; release builds use the production bundle
from `web/dist`. Both use `data/` by default and respect `CHAT_DATA` and saved
settings. A data folder configured for production still requires its HTTPS
proxy, even when started with a debug binary.

## Commands

Admins start in a local Command view and can register or manage accounts there. Rooms and private conversations use foldable navigation lists.
New accounts have no room memberships. Type messages or commands in the composer. Enter sends; Shift + Enter adds
a line. Use `/help` for commands grouped into General, Account, Rooms, Messages, and Server, filtered by your role. Typing `/`
shows contextual command and argument hints; arrow keys select a hint, Tab
completes it, and Escape closes the hints. Enter executes what you typed.
The message area is a console: commands and replies are printed alongside
chat messages, only in your current tab and the view where the command ran.
The Command view has its own local history. Private messages are grouped by
person; only people with saved message history appear in the list. Start a
new private conversation with `/tell person message`; typing in a private conversation is equivalent to `/tell person message`. Reloading clears command output and
fetches the latest 50 chat messages per room, plus up to 50 of your private
messages. `/history 200` reads more retained messages on demand. `/clear`
clears the local display without deleting server history; refresh restores it.
The URL remembers the selected room, so refresh returns to the same conversation.
Every permission check happens on the server.

| Command | Who | Behavior |
| --- | --- | --- |
| `/help` | Everyone | Show the command reference |
| `/whoami` | Everyone | Show your name and permission (`user` or `admin`) |
| `/passwd old new` | Web users | Verify your old password, change it, and sign out other sessions; current session stays logged in |
| `/rooms` | Everyone | List your rooms; admins can discover all rooms |
| `/users` | Everyone | List active account names and permissions |
| `/members [room]` | Everyone | List members of a room you belong to |
| `/history [count] [user]` | Everyone | Read retained messages (default up to 50; room limit from config, private limit 200); private view uses the selected person |
| `/join room` | Everyone | Open a room you already belong to; admins can join any room |
| `/leave [room]` | Everyone | Leave the specified or selected room; an admin must add regular users back |
| `/tell user message` | Everyone | Private message, visible only to sender and recipient |
| `/new room` | Admin or stdin | Create a room; the web admin becomes its first member |
| `/add user [room]` | Admin or stdin | Add an existing account; defaults to the selected room in the web UI |
| `/kick user [room]` | Admin or stdin | Revoke room access immediately |
| `/delete room` | Admin or stdin | Delete room and history |
| `/grant user` | Admin or stdin | Grant administrator permission |
| `/revoke user` | Admin or stdin | Restore user permission; web cannot revoke the last active admin |
| `/configs` | Admin or stdin | Print the active configuration; values require restart to change |
| `/clean age [room\|@private\|@all]` | Admin or stdin | Delete messages older than an age; units `s`, `m`, `h`, `d`, `w` |
| `/clear` | Web only | Clear local console output and visible chat without deleting persisted messages |
| `/logout` | Web only | Sign out |
| `/user name password [admin\|user]` | Admin or stdin | Create an account; default role is user |
| `/reset name password` | Admin or stdin | Reset password and revoke all login sessions |
| `/disable name` | Admin or stdin | Disable account, revoke sessions, and remove memberships |
| `/enable name` | Admin or stdin | Enable account without restoring room memberships; old sessions stay revoked |

There is no default room. Existing rooms are preserved, including rooms named
`lobby`, which can be deleted like any other room. Rooms are invitation-only for regular users;
admins explicitly control membership. Stdin is the superuser and can manage
all rooms; specify the room on stdin. Account command passwords, including both `/passwd` arguments, are masked in local output and never logged.
`/clean 7d room-name` cleans one room; `/clean 24h @private` cleans private messages;
`/clean 7d @all` cleans all rooms and private messages. Without a scope it uses
the selected room, or all messages in Command and stdin. Cleanup keeps messages
at or newer than the cutoff and reports the number removed. It preserves rooms,
accounts, and memberships. Cleanup is manual and uses stored UTC timestamps.
There is no runtime configuration setter: `/configs` shows the loaded settings,
and editing `data/config.json` takes effect after restart.
Web admins do not receive private
messages between other users. `/tell` opens the private-message view in the UI.
Names contain 1–32 ASCII letters, digits, `_`, or `-`.

## Build and run

To build manually:

```sh
rtk pnpm --dir web install
rtk pnpm --dir web build
rtk cargo build --release
rtk proxy cp target/release/chat ./chat
```

Builds can also use `rtk proxy ./build.sh`. The script generates
missing lockfiles on its first run and uses frozen/locked dependencies on
subsequent runs. Both lockfiles are included for reproducible builds. The script
itself uses ordinary commands; RTK only wraps your invocation. Release compilation requires a built
frontend. Assets are embedded into `chat`, so neither `web/` nor Node
is needed on the destination server. A debug binary embeds assets if they
exist at compile time; normally development uses Vite instead.

First production start:

```sh
rtk proxy env CHAT_ORIGIN=https://chat.example.com ./chat
```

Put the server behind Nginx; [deploy/nginx.conf](deploy/nginx.conf) is a
location-only include for `/commonroom/`. Place it inside your existing HTTPS
`server {}` block (under `http {}`). Nginx does not allow `location` directly
inside `http`. Your existing server supplies TLS certificates and HTTP-to-HTTPS
redirection. Match `base_url` to the location prefix. The example preserves
the prefix when forwarding; do not add a trailing slash to `proxy_pass`.
For root hosting, set `base_url` to `/` and use a single `location /` proxy.
Validate with `rtk proxy nginx -t` before reloading Nginx.

Release binaries require HTTPS, check request origins, set secure HttpOnly
cookies scoped to `base_url`, send HSTS and CSP, and reject requests without
`X-Forwarded-Proto: https`. The frontend redirects HTTP to HTTPS and uses
`wss:` in production. The proxy must overwrite forwarded headers. Production
requests must arrive from the exact socket-peer IP configured in `trust`;
forwarded headers cannot override that check. Keep the backend listener private
and restrict remote proxy deployments to the configured source IP. The Rust
backend deliberately does not terminate TLS.

The proxy upgrade headers follow the [official Nginx WebSocket guidance](https://nginx.org/en/docs/http/websocket.html).

Settings are saved as key-value pairs in `data/config.json`. Edit this file
while the service is stopped, then restart to apply changes. Example for Nginx
on the same machine:

```json
{
  "production": true,
  "bind": "127.0.0.1:3000",
  "trust": "127.0.0.1",
  "base_url": "/commonroom/",
  "max_users": 64,
  "max_rooms": 64,
  "max_messages": 1000,
  "origins": ["https://domain.com"]
}
```

`bind` is the single HTTP listener: it serves the embedded frontend, `/api/`
endpoints, and WebSockets at `/ws` on the same port (3000 by default).
`base_url` prefixes all these paths at runtime without rebuilding the frontend:
`/commonroom/`, `/commonroom/api/login`, `/commonroom/ws`. It defaults to `/`
and accepts path segments containing letters, digits, `-`, and `_`; a trailing
slash is added automatically. It is a path, not a full URL. Nginx
exposes HTTPS and WSS on port 443 and forwards both to this listener.
`trust` is a single proxy IP address, defaulting to `127.0.0.1`; use the load
balancer's actual source IP for a remote proxy, and bind to a reachable private
address. Trust is enforced in production. Existing config files without
`trust` receive the default automatically. Release builds always enforce
production mode; debug builds use the saved `production` setting.
`max_users` and `max_rooms` are positive integers, each defaulting to 64.
They limit new account and room creation through both web commands and stdin.
Disabled accounts count toward `max_users`. Lowering a limit preserves existing
accounts and rooms while preventing new creation until the count is below it.
`max_messages` is a positive integer, defaulting to 1000 per room. Each new
message evicts the oldest when the room is full, keeping the newest messages.
Lowering this limit trims and saves existing room histories at startup.
Restart after editing these values.

### Cloudflare Tunnel (without Nginx)

Cloudflare Tunnel can connect directly to this HTTP server and supports the
same WebSocket endpoint. For a named tunnel, map your public hostname to
`http://127.0.0.1:3000`. Use [deploy/cloudflared.yml](deploy/cloudflared.yml)
for a locally managed tunnel, or enter that service URL in the Cloudflare dashboard.
Run `cloudflared` on the same machine with:

```json
{
  "production": true,
  "bind": "127.0.0.1:3000",
  "trust": "127.0.0.1",
  "base_url": "/",
  "origins": ["https://chat.example.com"],
  "max_users": 64,
  "max_rooms": 64,
  "max_messages": 1000
}
```

Set the public hostname to your actual domain. Enable HTTPS redirects at
Cloudflare (Always Use HTTPS) and keep WebSockets enabled. The backend checks
`X-Forwarded-Proto: https`, which Cloudflare supplies for HTTPS visitors;
its own connection from `cloudflared` remains HTTP. No separate WebSocket route
or TLS certificate is needed on this server. Use `127.0.0.1` rather than
`localhost` in the service URL so the socket peer matches `trust`. If your
connector is on another machine or in a container, adjust `bind` and `trust`
to the reachable address and actual connector source IP.

Cloudflare Tunnel preserves request paths. For `/commonroom/`, set that
`base_url` and forward the path unchanged. Do not cache `/api/*` or `/ws`
(or their base-path equivalents) using custom Cloudflare cache rules.
The app reconnects if a tunnel restart interrupts an existing WebSocket.

Protocol compatibility is verified with production HTTP/WebSocket checks;
a real public tunnel still needs an end-to-end check on your own hostname.
See Cloudflare's [routing documentation](https://developers.cloudflare.com/tunnel/concepts/routing/),
[WebSocket support](https://developers.cloudflare.com/cloudflare-one/faq/cloudflare-tunnels-faq/),
and [forwarded headers](https://developers.cloudflare.com/fundamentals/reference/http-headers/).

`origins` checks the browser Origin header for login, logout, and WebSockets.
It contains scheme + host + optional port, never a path; for
`https://domain.com/commonroom/`, the origin is `https://domain.com`.
Production uses one HTTPS origin. `trust` checks the proxy IP separately.

Subsequent starts need only:

```sh
rtk proxy ./chat
```

Environment settings:

| Variable | Purpose |
| --- | --- |
| `CHAT_DATA` | Data-folder location; defaults to `./data` relative to the working directory |
| `CHAT_ORIGIN` | Public HTTPS origin, no path or trailing slash; required on first production start and saved |
| `CHAT_BIND` | Listener; defaults to `127.0.0.1:3000`, saved when overridden |
| `CHAT_TRUST` | Trusted proxy IP; defaults to `127.0.0.1`, saved when overridden |
| `CHAT_BASE_URL` | Hosting path; defaults to `/`, saved when overridden |
| `CHAT_PRODUCTION=1` | Exercise production security in a debug binary |
| `RUST_LOG` | Log filter; defaults to `chat=info,tower_http=info` |

Changing `CHAT_BIND`, `CHAT_TRUST`, `CHAT_BASE_URL`, or `CHAT_ORIGIN` persists the change.
Environment overrides take precedence over saved values. Production settings
cannot be silently downgraded by running a debug binary. A release binary
always enforces production security, including when upgrading a development
data folder; provide `CHAT_ORIGIN` the first time you upgrade it.

Logs go to stdout. To redirect them while preserving the stdin console:

```sh
rtk proxy ./chat > chat.log 2>&1
```

Closing stdin leaves the web service running. Ctrl + C or SIGTERM stops it
gracefully and checkpoints SQLite. No default account or password is shipped.

## Move to another machine

1. Stop the old server with Ctrl + C or SIGTERM. Wait for it to exit.
2. Copy the **entire `data/` folder** to the new machine, preserving it privately.
3. Place a compatible `chat` binary built for that machine's OS/architecture
   beside `data/`. Start it from that directory with `rtk proxy ./chat`.
4. Route the same public HTTPS hostname to the new machine and keep the proxy
   configuration equivalent. If the hostname changes, set `CHAT_ORIGIN` on
   the next start; browser cookies are tied to the original hostname.

```text
service/
├── chat                 # destination-platform binary, includes frontend
└── data/
    ├── config.json      # listener, public origin, production mode
    ├── chat.sqlite      # accounts, password hashes, rooms, memberships,
    │                    # messages, and unexpired login sessions
    └── chat.lock        # exclusive OS lock; safe to move after shutdown
```

SQLite may also have `chat.sqlite-wal` and `chat.sqlite-shm`; copy the whole
folder, never just the database while it is running. The stored format uses
portable SQLite and JSON, with no machine-specific paths or secrets outside
the data folder. Only one process can use a folder at once. Compatible newer
binaries can read schema v1; older binaries reject unknown newer schema
versions. Browser login sessions keep their original 12-hour expiry across
restart and migration. Accounts, roles, memberships, message timestamps and
IDs remain the same. Proxy certificates, DNS, and OS-specific binaries are
deployment infrastructure and must be supplied on the destination.

On Unix, startup restricts the data directory to its owner (`0700`). This
folder contains private messages and active session credentials; keep
backups private as well.

## Verification

```sh
rtk cargo fmt --all --check
rtk cargo test
rtk cargo clippy --all-targets -- -D warnings
rtk pnpm --dir web test
rtk proxy ./build.sh
rtk cargo build
rtk pnpm --dir web test:integration
```

Unit tests cover room authorization, private-message visibility, password
hashes, bounded history, account disabling, persistence, session revocation,
and moving state to a new directory. The integration script starts a fresh
debug server with an isolated temporary data folder and verifies HTTP login,
WebSocket commands, privacy, origin checks, revocation, and restart after
migration. It also starts the release binary from a directory without web
assets and checks embedded HTML/JS/CSS, HTTPS enforcement, secure cookies,
security headers, WebSocket origin rejection, and logout disconnection.
It never uses your real `data/`.

## Layout and protocol

```text
server/
  build.rs               # embed the frontend at compile time
  src/main.rs            # process lifecycle and stdin console
  src/config.rs          # saved settings, defaults, environment overrides
  src/engine.rs          # commands, permissions, portable storage
  src/commands.rs        # role-aware help and shared hint definitions
  src/web.rs             # HTTP auth, WebSockets, deployment security
web/
  src/main.jsx           # declarative screens and WebSocket client
  src/console.js         # local transcript ordering and argument completion
  src/style.css          # responsive, light-themed UI; no remote assets
  scripts/smoke.mjs      # end-to-end server checks
build.sh                 # release build and binary packaging
debug.sh                 # run the built debug artifact
deploy/nginx.conf         # Nginx HTTPS / WebSocket proxy example
```

HTTP endpoints: `POST /api/login`, `POST /api/logout`, `GET /api/me`, and
`GET /api/health`. Authentication uses same-origin cookies; no bearer tokens
are exposed to JavaScript. `GET /ws` requires a valid session and an allowed
Origin. Client frames are JSON `{ "id": 1, "room": "lobby", "text": "hello" }`.
The server acknowledges the request ID with `notice` or `error`, then sends
an authorization-filtered `snapshot` containing account metadata, accessible
rooms and the latest 50 messages, only the user's private messages, and
role-appropriate command metadata for hints. Replies go only to the requesting
socket and are never stored in chat history. The browser keeps at most 200
local command entries in memory; it does not use localStorage or sessionStorage
for them. Reconnect fetches
the current state. The client preserves an unacknowledged draft and asks the
user to check history before resending it.

History is deliberately bounded: `max_messages` messages per room (1000 by default) and the last 200
private messages across the service. Accounts and rooms are bounded by the saved `max_users` and `max_rooms`
settings (64 each by default). There are at most 256 sessions and 128
simultaneous sockets. UTF-8 messages allow 4000 Unicode characters, including
emoji and combining marks;
sockets are limited to 30 submissions per 10 seconds and use heartbeats.
Login attempts are limited to 10 per minute per peer IP (shared by users
behind the same proxy). Snapshots and transactional SQLite writes keep the
implementation small. Room histories use a deque: append and oldest-message
eviction are amortized O(1), with O(1) eviction once full. The entire send is
not O(1): rollback copies, JSON serialization, and SQLite state writes still
scale with the total retained data. This is a single-process service for a few users,
not a distributed chat platform. Multiple application replicas must not
share the data folder. Replacing the frontend requires rebuilding the binary.

## Current validation status

Verified on October 7, 2026:

- Rust formatting passes, and all 12 unit tests pass.
- Frontend console ordering, refresh/clear behavior, and hint completion tests pass.
- Clippy passes for all targets with warnings denied.
- Web production build and Rust debug/release builds pass.
- HTTP/WebSocket integration tests pass, including data-folder migration and
  the production security checks described above.
- Browser checks pass for login, room creation and selection, message delivery,
  desktop layout, and the mobile navigation drawer. No browser warnings or
  errors were observed during these checks.
- Build-script shell syntax passes. Nginx configuration validation is deferred
  to deployment because Nginx is not installed in the verification environment;
  run `nginx -t` after setting the hostname and certificate paths.

Read-only commands avoid copying or writing stored state and do not broadcast
snapshots. Successful mutations notify connected clients. The client memoizes
conversation ordering and rendered messages, so typing does not repeatedly sort
and format the transcript. Password checks and hashing run off the async thread.
