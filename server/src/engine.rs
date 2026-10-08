use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use rand_core::OsRng;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const PRIVATE_HISTORY: usize = 200;
const VISIBLE_HISTORY: usize = 50;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub fn hash_password(password: &str) -> Result<String, String> {
    if password.chars().count() < 3 || password.len() > 128 {
        return Err("Passwords must contain at least 3 characters and at most 128 bytes.".into());
    }
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|h| h.to_string())
        .map_err(|_| "Password hashing failed.".into())
}
pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub hash: String,
    pub admin: bool,
    pub disabled: bool,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub from: String,
    pub to: Option<String>,
    pub text: String,
    pub time: u64,
}
#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Room {
    pub members: BTreeSet<String>,
    pub messages: VecDeque<Message>,
}
#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Data {
    pub users: BTreeMap<String, Account>,
    pub rooms: BTreeMap<String, Room>,
    pub direct: Vec<Message>,
    #[serde(default)]
    pub sessions: BTreeMap<String, Session>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub username: String,
    pub expires: u64,
}
#[derive(Serialize)]
pub struct RoomView {
    pub name: String,
    pub members: BTreeSet<String>,
    pub messages: Vec<Message>,
}
#[derive(Serialize)]
pub struct Snapshot {
    pub kind: &'static str,
    pub username: String,
    pub admin: bool,
    pub users: Vec<String>,
    pub rooms: Vec<RoomView>,
    pub direct: Vec<Message>,
    pub private_peers: Vec<String>,
    pub commands: Vec<crate::commands::Command>,
    pub available_rooms: Vec<String>,
}

pub struct Engine {
    pub data: Data,
    pub revision: u64,
    max_users: usize,
    max_rooms: usize,
    max_messages: usize,
    db: Connection,
    _lock: Option<std::fs::File>,
}
impl Engine {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let lock = if path == Path::new(":memory:") {
            None
        } else {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(path.with_extension("lock"))
                .map_err(|e| e.to_string())?;
            file.try_lock()
                .map_err(|_| "Another server is already using this data folder.".to_string())?;
            Some(file)
        };
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        let version: u32 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > 1 {
            return Err("This data folder was written by a newer, incompatible server.".into());
        }
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS state (id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL);").map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA user_version=1;")
            .map_err(|e| e.to_string())?;
        let raw = db.query_row("SELECT json FROM state WHERE id=1", [], |r| {
            r.get::<_, String>(0)
        });
        let data = match raw {
            Ok(raw) => serde_json::from_str(&raw).map_err(|e| e.to_string())?,
            Err(rusqlite::Error::QueryReturnedNoRows) => Data::default(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Self {
            data,
            revision: 0,
            max_users: 64,
            max_rooms: 64,
            max_messages: 1000,
            db,
            _lock: lock,
        })
    }
    pub fn set_limits(
        &mut self,
        max_users: usize,
        max_rooms: usize,
        max_messages: usize,
    ) -> Result<(), String> {
        if max_users == 0 || max_rooms == 0 || max_messages == 0 {
            return Err("max_users, max_rooms and max_messages must be positive integers.".into());
        }
        if self
            .data
            .rooms
            .values()
            .any(|room| room.messages.len() > max_messages)
        {
            let previous = self.data.clone();
            for room in self.data.rooms.values_mut() {
                room.messages
                    .drain(..room.messages.len().saturating_sub(max_messages));
            }
            if let Err(error) = self.save() {
                self.data = previous;
                return Err(error);
            }
        }
        self.max_users = max_users;
        self.max_rooms = max_rooms;
        self.max_messages = max_messages;
        Ok(())
    }
    fn save(&mut self) -> Result<(), String> {
        let json = serde_json::to_string(&self.data).map_err(|e| e.to_string())?;
        self.db.execute("INSERT INTO state(id,json) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET json=excluded.json", params![json]).map_err(|e| { tracing::error!(error = %e, "Storage write failed"); "Storage unavailable; change was not applied.".to_string() })?;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn is_admin(&self, name: &str) -> bool {
        self.data
            .users
            .get(name)
            .is_some_and(|u| u.admin && !u.disabled)
    }
    pub fn change_password(
        &mut self,
        user: &str,
        expected_hash: &str,
        hash: String,
        keep: &str,
    ) -> Result<String, String> {
        if !self
            .data
            .users
            .get(user)
            .is_some_and(|u| !u.disabled && u.hash == expected_hash)
            || self.session(keep).as_deref() != Some(user)
        {
            return Err("Account or session changed. Please log in again.".into());
        }
        let before = self.data.clone();
        self.data.users.get_mut(user).unwrap().hash = hash;
        self.data
            .sessions
            .retain(|token, session| session.username != user || token == keep);
        if let Err(error) = self.save() {
            self.data = before;
            return Err(error);
        }
        Ok("Password changed. Other sessions signed out.".into())
    }
    pub fn checkpoint(&self) -> Result<(), String> {
        self.db
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| e.to_string())
    }
    pub fn active(&self, name: &str) -> bool {
        self.data.users.get(name).is_some_and(|u| !u.disabled)
    }
    pub fn session(&self, token: &str) -> Option<String> {
        self.data
            .sessions
            .get(token)
            .filter(|s| s.expires > now() && self.active(&s.username))
            .map(|s| s.username.clone())
    }
    pub fn login(
        &mut self,
        token: String,
        username: &str,
        expected_hash: &str,
        old: Option<&str>,
    ) -> Result<(), String> {
        if !self
            .data
            .users
            .get(username)
            .is_some_and(|u| !u.disabled && u.hash == expected_hash)
        {
            return Err("Account changed. Please try again.".into());
        }
        let before = self.data.clone();
        self.data.sessions.retain(|_, s| s.expires > now());
        if let Some(old) = old {
            self.data.sessions.remove(old);
        }
        if self.data.sessions.len() >= 256 {
            self.data = before;
            return Err("Session limit reached.".into());
        }
        self.data.sessions.insert(
            token,
            Session {
                username: username.into(),
                expires: now() + 43200,
            },
        );
        if let Err(e) = self.save() {
            self.data = before;
            return Err(e);
        }
        Ok(())
    }
    pub fn logout(&mut self, token: &str) -> Result<(), String> {
        let before = self.data.clone();
        self.data.sessions.remove(token);
        if let Err(e) = self.save() {
            self.data = before;
            return Err(e);
        }
        Ok(())
    }
    pub fn snapshot(&self, name: &str) -> Option<Snapshot> {
        let user = self.data.users.get(name).filter(|u| !u.disabled)?;
        Some(Snapshot {
            kind: "snapshot",
            username: name.into(),
            admin: user.admin,
            users: self
                .data
                .users
                .iter()
                .filter(|(_, u)| !u.disabled)
                .map(|(n, _)| n.clone())
                .collect(),
            rooms: self
                .data
                .rooms
                .iter()
                .filter(|(_, r)| r.members.contains(name))
                .map(|(n, r)| RoomView {
                    name: n.clone(),
                    members: r.members.clone(),
                    messages: r
                        .messages
                        .iter()
                        .skip(r.messages.len().saturating_sub(VISIBLE_HISTORY))
                        .cloned()
                        .collect(),
                })
                .collect(),
            direct: self
                .data
                .direct
                .iter()
                .filter(|m| m.from == name || m.to.as_deref() == Some(name))
                .rev()
                .take(VISIBLE_HISTORY)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            private_peers: self
                .data
                .direct
                .iter()
                .filter_map(|m| {
                    if m.from == name {
                        m.to.clone()
                    } else if m.to.as_deref() == Some(name) {
                        Some(m.from.clone())
                    } else {
                        None
                    }
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            commands: crate::commands::available(user.admin, false),
            available_rooms: self
                .data
                .rooms
                .iter()
                .filter(|(_, r)| user.admin || r.members.contains(name))
                .map(|(n, _)| n.clone())
                .collect(),
        })
    }
    pub fn provision(
        &mut self,
        name: &str,
        hash: String,
        admin: bool,
        reset: bool,
    ) -> Result<String, String> {
        if !valid_name(name) {
            return Err("Invalid username.".into());
        }
        let before = self.data.clone();
        if reset {
            let user = self.data.users.get_mut(name).ok_or("User not found.")?;
            user.hash = hash;
            self.data.sessions.retain(|_, s| s.username != name);
        } else {
            if self.data.users.contains_key(name) {
                return Err("Username already exists.".into());
            }
            if self.data.users.len() >= self.max_users {
                return Err(format!("User limit reached ({}).", self.max_users));
            }
            self.data.users.insert(
                name.into(),
                Account {
                    hash,
                    admin,
                    disabled: false,
                },
            );
        }
        if let Err(e) = self.save() {
            self.data = before;
            return Err(e);
        }
        Ok(format!(
            "Account {name} {}.",
            if reset { "password reset" } else { "created" }
        ))
    }
    pub fn execute(
        &mut self,
        actor: Option<&str>,
        room: Option<&str>,
        input: &str,
    ) -> Result<String, String> {
        if matches!(
            input.split_whitespace().next(),
            Some("/help" | "/whoami" | "/rooms" | "/users" | "/members" | "/history")
        ) {
            return self.apply(actor, room, input);
        }
        let before = self.data.clone();
        let result = self.apply(actor, room, input);
        match result {
            Ok(reply) => {
                if self.data == before {
                    return Ok(reply);
                }
                if let Err(e) = self.save() {
                    self.data = before;
                    return Err(e);
                }
                Ok(reply)
            }
            Err(e) => {
                self.data = before;
                Err(e)
            }
        }
    }
    fn apply(
        &mut self,
        actor: Option<&str>,
        room: Option<&str>,
        input: &str,
    ) -> Result<String, String> {
        let input = input.trim();
        if input.is_empty() || input.chars().count() > 4100 {
            return Err("Enter a message of up to 4000 characters or a command.".into());
        }
        let admin = match actor {
            None => true,
            Some(name) => {
                self.data
                    .users
                    .get(name)
                    .filter(|u| !u.disabled)
                    .ok_or("Account unavailable.")?
                    .admin
            }
        };
        let author = actor.unwrap_or("console");
        let parts: Vec<&str> = input.split_whitespace().collect();
        if !input.starts_with('/') {
            if input.chars().count() > 4000 {
                return Err("Messages support at most 4000 characters.".into());
            }
            let room = room.ok_or("Select a room first.")?;
            let target = self.data.rooms.get_mut(room).ok_or("Room not found.")?;
            if actor.is_some() && !target.members.contains(author) {
                return Err("You are not a member of this room.".into());
            }
            if target.messages.len() == self.max_messages {
                target.messages.pop_front();
            }
            target.messages.push_back(new_message(author, None, input));
            return Ok(String::new());
        }
        match parts[0] {
            "/help" => {
                require_len(&parts, 1, "/help")?;
                Ok(crate::commands::help(admin, actor.is_none()))
            }
            "/clean" => {
                require_admin(admin)?;
                if !(2..=3).contains(&parts.len()) {
                    return Err(
                        "Usage: /clean age [room|@private|@all], e.g. /clean 7d @all".into(),
                    );
                }
                let age = parse_age(parts[1])?;
                let cutoff = now()
                    .checked_sub(age)
                    .ok_or("Age is older than the clock permits.")?;
                let scope = parts.get(2).copied().or(room).unwrap_or("@all");
                let mut removed = 0;
                if scope == "@all" || scope == "@private" {
                    let before = self.data.direct.len();
                    self.data.direct.retain(|m| m.time >= cutoff);
                    removed += before - self.data.direct.len();
                }
                if scope == "@all" {
                    for target in self.data.rooms.values_mut() {
                        let before = target.messages.len();
                        target.messages.retain(|m| m.time >= cutoff);
                        removed += before - target.messages.len();
                    }
                } else if scope != "@private" {
                    let target = self.data.rooms.get_mut(scope).ok_or("Room not found.")?;
                    let before = target.messages.len();
                    target.messages.retain(|m| m.time >= cutoff);
                    removed += before - target.messages.len();
                }
                Ok(format!(
                    "Deleted {removed} messages older than {} from {scope}.",
                    parts[1]
                ))
            }
            "/whoami" => {
                require_len(&parts, 1, "/whoami")?;
                Ok(format!(
                    "Name: {author}\nPermission: {}",
                    if actor.is_none() {
                        "superuser"
                    } else if admin {
                        "admin"
                    } else {
                        "user"
                    }
                ))
            }
            "/rooms" => {
                require_len(&parts, 1, "/rooms")?;
                Ok(format!(
                    "Your rooms: {}",
                    self.data
                        .rooms
                        .iter()
                        .filter(|(_, r)| admin || r.members.contains(author))
                        .map(|(n, _)| format!("#{n}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
            }
            "/users" => {
                require_len(&parts, 1, "/users")?;
                Ok(format!(
                    "Users:\n{}",
                    self.data
                        .users
                        .iter()
                        .filter(|(_, u)| admin || !u.disabled)
                        .map(|(n, u)| format!(
                            "{n} — {}{}",
                            if u.admin { "admin" } else { "user" },
                            if u.disabled { " (disabled)" } else { "" }
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
            }
            "/members" => {
                if parts.len() > 2 {
                    return Err("Usage: /members [room]".into());
                }
                let name = parts
                    .get(1)
                    .copied()
                    .or(room)
                    .ok_or("Select or specify a room.")?;
                let target = self.data.rooms.get(name).ok_or("Room not found.")?;
                if actor.is_some() && !target.members.contains(author) {
                    return Err("You are not a member of this room.".into());
                }
                Ok(target
                    .members
                    .iter()
                    .map(|n| {
                        format!(
                            "{n} — {}",
                            if self.data.users[n].admin {
                                "admin"
                            } else {
                                "user"
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "/history" => {
                if parts.len() > 3 || (room.is_some() && parts.len() > 2) {
                    return Err(
                        "Usage: /history [count] [user] (user is for private history)".into(),
                    );
                }
                let limit = if room.is_some() {
                    self.max_messages
                } else {
                    PRIVATE_HISTORY
                };
                let count_error = format!("Count must be between 1 and {limit}.");
                let count = parts
                    .get(1)
                    .map(|n| n.parse::<usize>())
                    .transpose()
                    .map_err(|_| count_error.clone())?
                    .unwrap_or(VISIBLE_HISTORY.min(limit));
                if !(1..=limit).contains(&count) {
                    return Err(count_error);
                }
                let messages: Vec<_> = if let Some(name) = room {
                    let target = self.data.rooms.get(name).ok_or("Room not found.")?;
                    if actor.is_some() && !target.members.contains(author) {
                        return Err("You are not a member of this room.".into());
                    }
                    target.messages.iter().collect()
                } else if actor.is_some() {
                    self.data
                        .direct
                        .iter()
                        .filter(|m| m.from == author || m.to.as_deref() == Some(author))
                        .filter(|m| {
                            parts.get(2).is_none_or(|peer| {
                                (m.from == author && m.to.as_deref() == Some(*peer))
                                    || (m.from == *peer && m.to.as_deref() == Some(author))
                            })
                        })
                        .collect()
                } else {
                    return Err("Specify a room via a web session to read history.".into());
                };
                let text = messages
                    .iter()
                    .skip(messages.len().saturating_sub(count))
                    .map(|m| {
                        format!(
                            "{} {}{}: {}",
                            m.time,
                            m.from,
                            m.to.as_ref()
                                .map(|to| format!(" → {to}"))
                                .unwrap_or_default(),
                            m.text
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(if text.is_empty() {
                    "No messages.".into()
                } else {
                    text
                })
            }
            "/grant" | "/revoke" => {
                require_admin(admin)?;
                require_len(&parts, 2, "/grant user or /revoke user")?;
                let name = parts[1];
                if !self.active(name) {
                    return Err("User not found.".into());
                }
                let grant = parts[0] == "/grant";
                if !grant
                    && actor.is_some()
                    && self.data.users[name].admin
                    && self
                        .data
                        .users
                        .values()
                        .filter(|u| u.admin && !u.disabled)
                        .count()
                        == 1
                {
                    return Err("Cannot revoke the last active administrator.".into());
                }
                self.data.users.get_mut(name).unwrap().admin = grant;
                Ok(format!(
                    "{name}: permission {}.",
                    if grant { "admin" } else { "user" }
                ))
            }
            "/new" => {
                require_admin(admin)?;
                require_len(&parts, 2, "/new room")?;
                let name = parts[1];
                if !valid_name(name) {
                    return Err("Invalid room name.".into());
                }
                if self.data.rooms.contains_key(name) {
                    return Err("Room already exists.".into());
                }
                if self.data.rooms.len() >= self.max_rooms {
                    return Err(format!("Room limit reached ({}).", self.max_rooms));
                }
                let mut r = Room::default();
                if actor.is_some() {
                    r.members.insert(author.into());
                }
                self.data.rooms.insert(name.into(), r);
                Ok(format!("Created #{name}."))
            }
            "/add" | "/kick" => {
                require_admin(admin)?;
                if !(2..=3).contains(&parts.len()) {
                    return Err("Usage: /add user [room] or /kick user [room]".into());
                }
                let name = parts[1];
                if !self.active(name) {
                    return Err("User not found.".into());
                }
                let room = parts.get(2).copied().or(room).ok_or("Specify a room.")?;
                let target = self.data.rooms.get_mut(room).ok_or("Room not found.")?;
                if parts[0] == "/add" {
                    target.members.insert(name.into());
                } else {
                    target.members.remove(name);
                }
                Ok(format!(
                    "{name} {} #{room}.",
                    if parts[0] == "/add" {
                        "added to"
                    } else {
                        "removed from"
                    }
                ))
            }
            "/join" | "/leave" => {
                if parts[0] == "/join" {
                    require_len(&parts, 2, "/join room")?;
                } else if parts.len() > 2 {
                    return Err("Usage: /leave [room]".into());
                }
                if actor.is_none() {
                    return Err("Use /add user room from the console.".into());
                }
                let name = parts
                    .get(1)
                    .copied()
                    .or(room)
                    .ok_or("Select or specify a room.")?;
                let r = self.data.rooms.get_mut(name).ok_or("Room not found.")?;
                if parts[0] == "/join" {
                    if !admin && !r.members.contains(author) {
                        return Err("An admin must add you to this room first.".into());
                    }
                    r.members.insert(author.into());
                } else {
                    r.members.remove(author);
                }
                Ok(format!(
                    "{} #{}.",
                    if parts[0] == "/join" {
                        "Joined"
                    } else {
                        "Left"
                    },
                    name
                ))
            }
            "/tell" => {
                if actor.is_none() {
                    return Err("Direct messages require a user account.".into());
                }
                let content = input.strip_prefix("/tell").unwrap().trim_start();
                let (recipient, text) = content
                    .split_once(char::is_whitespace)
                    .ok_or("Usage: /tell user message")?;
                let text = text.trim();
                if text.is_empty() {
                    return Err("Usage: /tell user message".into());
                }
                if text.chars().count() > 4000 {
                    return Err("Messages support at most 4000 characters.".into());
                }
                if !self.active(recipient) {
                    return Err("User not found.".into());
                }
                self.data
                    .direct
                    .push(new_message(author, Some(recipient), text));
                trim(&mut self.data.direct);
                Ok(format!("Private message sent to {recipient}."))
            }
            "/delete" => {
                require_admin(admin)?;
                require_len(&parts, 2, "/delete room")?;
                self.data.rooms.remove(parts[1]).ok_or("Room not found.")?;
                Ok(format!("Deleted #{}.", parts[1]))
            }
            "/enable" => {
                require_admin(admin)?;
                require_len(&parts, 2, "/enable user")?;
                self.data
                    .users
                    .get_mut(parts[1])
                    .ok_or("User not found.")?
                    .disabled = false;
                Ok(format!("Enabled {}.", parts[1]))
            }
            "/disable" => {
                require_admin(admin)?;
                require_len(&parts, 2, "/disable user")?;
                if actor.is_some()
                    && self
                        .data
                        .users
                        .get(parts[1])
                        .is_some_and(|u| u.admin && !u.disabled)
                    && self
                        .data
                        .users
                        .values()
                        .filter(|u| u.admin && !u.disabled)
                        .count()
                        == 1
                {
                    return Err("Cannot disable the last active admin.".into());
                }
                self.data
                    .users
                    .get_mut(parts[1])
                    .ok_or("User not found.")?
                    .disabled = true;
                self.data.sessions.retain(|_, s| s.username != parts[1]);
                for room in self.data.rooms.values_mut() {
                    room.members.remove(parts[1]);
                }
                Ok(format!("Disabled {}.", parts[1]))
            }
            "/user" | "/reset" => Err("Account provisioning is console-only.".into()),
            _ => Err("Unknown command. Try /help.".into()),
        }
    }
}
fn require_admin(admin: bool) -> Result<(), String> {
    if admin {
        Ok(())
    } else {
        Err("Admin permission required.".into())
    }
}
fn parse_age(value: &str) -> Result<u64, String> {
    let unit = value
        .chars()
        .last()
        .ok_or("Age must use s, m, h, d or w, e.g. 7d.")?;
    let digits = value.strip_suffix(unit).unwrap();
    let multiplier = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        'w' => 604800,
        _ => return Err("Age must use s, m, h, d or w, e.g. 7d.".into()),
    };
    digits
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .and_then(|n| n.checked_mul(multiplier))
        .ok_or_else(|| "Age must be a positive duration, e.g. 7d.".into())
}
fn require_len(parts: &[&str], len: usize, usage: &str) -> Result<(), String> {
    if parts.len() == len {
        Ok(())
    } else {
        Err(format!("Usage: {usage}"))
    }
}
fn new_message(from: &str, to: Option<&str>, text: &str) -> Message {
    Message {
        id: uuid::Uuid::new_v4().to_string(),
        from: from.into(),
        to: to.map(String::from),
        text: text.into(),
        time: now(),
    }
}
fn trim(messages: &mut Vec<Message>) {
    if messages.len() > PRIVATE_HISTORY {
        messages.drain(..messages.len() - PRIVATE_HISTORY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn engine() -> Engine {
        let mut e = Engine::open(Path::new(":memory:")).unwrap();
        for (name, admin) in [("alice", true), ("bob", false), ("eve", false)] {
            e.provision(name, "test-hash".into(), admin, false).unwrap();
        }
        e.execute(Some("alice"), None, "/new lobby").unwrap();
        e.execute(Some("alice"), None, "/add bob lobby").unwrap();
        e.execute(Some("alice"), None, "/add eve lobby").unwrap();
        e
    }
    #[test]
    fn cleanup_respects_age_scope_permissions_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.sqlite");
        {
            let mut e = Engine::open(&path).unwrap();
            e.provision("alice", "hash".into(), true, false).unwrap();
            e.provision("bob", "hash".into(), false, false).unwrap();
            e.execute(Some("alice"), None, "/new one").unwrap();
            e.execute(Some("alice"), None, "/new two").unwrap();
            for room in ["one", "two"] {
                e.execute(Some("alice"), Some(room), "old").unwrap();
                e.data.rooms.get_mut(room).unwrap().messages[0].time = now() - 8 * 86400;
                e.execute(Some("alice"), Some(room), "new").unwrap();
            }
            e.execute(Some("alice"), None, "/tell bob old private")
                .unwrap();
            e.data.direct[0].time = now() - 8 * 86400;
            e.execute(Some("alice"), None, "/tell bob new private")
                .unwrap();
            assert!(e.execute(Some("bob"), None, "/clean 7d @all").is_err());
            for age in ["0d", "你好", "-1d", "99999999999999999999w"] {
                assert!(
                    e.execute(Some("alice"), None, &format!("/clean {age} @all"))
                        .is_err()
                );
            }
            e.execute(Some("alice"), Some("one"), "/clean 7d").unwrap();
            assert_eq!(e.data.rooms["one"].messages.len(), 1);
            assert_eq!(e.data.rooms["two"].messages.len(), 2);
            assert_eq!(e.data.direct.len(), 2);
            e.execute(None, None, "/clean 7d @all").unwrap();
        }
        let e = Engine::open(&path).unwrap();
        assert_eq!(e.data.rooms["two"].messages[0].text, "new");
        assert_eq!(e.data.direct[0].text, "new private");
        assert_eq!(e.data.direct.len(), 1);
    }
    #[test]
    fn password_change_keeps_current_session_and_rejects_stale_hashes() {
        let mut e = engine();
        e.login("current".into(), "bob", "test-hash", None).unwrap();
        e.login("other".into(), "bob", "test-hash", None).unwrap();
        e.change_password("bob", "test-hash", "new-hash".into(), "current")
            .unwrap();
        assert_eq!(e.session("current").as_deref(), Some("bob"));
        assert!(e.session("other").is_none());
        assert!(
            e.change_password("bob", "test-hash", "bad".into(), "current")
                .is_err()
        );
        assert_eq!(e.data.users["bob"].hash, "new-hash");
        let revision = e.revision;
        e.execute(Some("bob"), None, "/help").unwrap();
        assert_eq!(e.revision, revision);
    }
    #[test]
    fn private_contacts_come_only_from_the_users_retained_messages() {
        let mut e = engine();
        assert!(e.snapshot("alice").unwrap().private_peers.is_empty());
        e.execute(Some("bob"), None, "/tell alice older").unwrap();
        for _ in 0..60 {
            e.execute(Some("eve"), None, "/tell alice recent").unwrap();
        }
        let snapshot = e.snapshot("alice").unwrap();
        assert_eq!(snapshot.private_peers, vec!["bob", "eve"]);
        assert!(!snapshot.direct.iter().any(|m| m.from == "bob"));
        assert_eq!(e.snapshot("bob").unwrap().private_peers, vec!["alice"]);
    }
    #[test]
    fn configurable_limits_preserve_existing_accounts_and_rooms() {
        let mut e = Engine::open(Path::new(":memory:")).unwrap();
        e.set_limits(2, 1, 1000).unwrap();
        e.provision("alice", "hash".into(), true, false).unwrap();
        e.provision("bob", "hash".into(), false, false).unwrap();
        assert!(e.provision("eve", "hash".into(), false, false).is_err());
        e.execute(Some("alice"), None, "/new one").unwrap();
        assert!(e.execute(None, None, "/new two").is_err());
        e.set_limits(1, 1, 1000).unwrap();
        e.provision("bob", "replacement".into(), false, true)
            .unwrap();
        assert!(e.active("bob"));
        assert!(e.data.rooms.contains_key("one"));
        assert!(e.set_limits(0, 1, 1000).is_err());
    }
    #[test]
    fn unicode_messages_round_trip_and_count_characters() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.sqlite");
        let sample = "你好 日本語 한국어 مرحبا नमस्ते Привет שלום 🙂 e\u{301}\nsecond line";
        {
            let mut e = Engine::open(&path).unwrap();
            e.provision("alice", "hash".into(), true, false).unwrap();
            e.provision("bob", "hash".into(), false, false).unwrap();
            e.execute(Some("alice"), None, "/new languages").unwrap();
            e.execute(Some("alice"), Some("languages"), sample).unwrap();
            e.execute(Some("alice"), None, &format!("/tell bob {sample}"))
                .unwrap();
            e.execute(Some("alice"), Some("languages"), &"你".repeat(4000))
                .unwrap();
            e.execute(
                Some("alice"),
                None,
                &format!("/tell bob {}", "🙂".repeat(4000)),
            )
            .unwrap();
            assert!(
                e.execute(Some("alice"), Some("languages"), &"你".repeat(4001))
                    .is_err()
            );
            assert!(
                e.execute(
                    Some("alice"),
                    None,
                    &format!("/tell bob {}", "🙂".repeat(4001))
                )
                .is_err()
            );
        }
        let e = Engine::open(&path).unwrap();
        assert_eq!(
            e.snapshot("alice").unwrap().rooms[0].messages[0].text,
            sample
        );
        assert_eq!(e.snapshot("bob").unwrap().direct[0].text, sample);
        assert_eq!(
            e.snapshot("bob").unwrap().direct[1].text.chars().count(),
            4000
        );
    }
    #[test]
    fn new_accounts_have_no_rooms() {
        let mut e = Engine::open(Path::new(":memory:")).unwrap();
        e.provision("admin", "hash".into(), true, false).unwrap();
        e.provision("bob", "hash".into(), false, false).unwrap();
        assert!(e.data.rooms.is_empty());
        assert!(e.snapshot("bob").unwrap().rooms.is_empty());
        e.execute(Some("admin"), None, "/new lobby").unwrap();
        e.provision("alice", "hash".into(), false, false).unwrap();
        assert!(e.snapshot("alice").unwrap().rooms.is_empty());
        assert!(e.execute(Some("admin"), None, "/disable admin").is_err());
        e.execute(Some("admin"), None, "/delete lobby").unwrap();
        assert!(e.data.rooms.is_empty());
    }
    #[test]
    fn permissions_and_private_delivery() {
        let mut e = engine();
        assert!(e.execute(Some("bob"), None, "/new secret").is_err());
        e.execute(Some("alice"), None, "/new secret").unwrap();
        assert!(e.execute(Some("bob"), None, "/join secret").is_err());
        e.execute(Some("alice"), None, "/add bob secret").unwrap();
        e.execute(Some("bob"), Some("secret"), "hello").unwrap();
        assert!(
            !e.snapshot("eve")
                .unwrap()
                .rooms
                .iter()
                .any(|r| r.name == "secret")
        );
        e.execute(Some("bob"), None, "/tell alice private").unwrap();
        e.execute(Some("bob"), None, "/tell eve other-private")
            .unwrap();
        let history = e.execute(Some("bob"), None, "/history 20 alice").unwrap();
        assert!(history.contains("private"));
        assert!(!history.contains("other-private"));
        assert_eq!(e.snapshot("alice").unwrap().direct.len(), 1);
        assert_eq!(e.snapshot("eve").unwrap().direct.len(), 1);
        e.execute(Some("alice"), None, "/kick bob secret").unwrap();
        assert!(e.execute(Some("bob"), Some("secret"), "blocked").is_err());
        assert!(e.execute(Some("eve"), None, "/disable bob").is_err());
    }
    #[test]
    fn persists_accounts_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        {
            let mut e = Engine::open(&path).unwrap();
            e.provision("alice", "hash".into(), true, false).unwrap();
            e.execute(Some("alice"), None, "/new lobby").unwrap();
            e.execute(Some("alice"), Some("lobby"), "retained").unwrap();
        }
        let e = Engine::open(&path).unwrap();
        assert!(e.active("alice"));
        assert_eq!(
            e.snapshot("alice").unwrap().rooms[0].messages[0].text,
            "retained"
        );
    }
    #[test]
    fn bounded_history_and_disabled_users() {
        let mut e = engine();
        e.set_limits(64, 64, 200).unwrap();
        for _ in 0..210 {
            e.execute(Some("bob"), Some("lobby"), "hello").unwrap();
        }
        assert_eq!(e.data.rooms["lobby"].messages.len(), 200);
        e.execute(None, None, "/disable bob").unwrap();
        assert!(e.snapshot("bob").is_none());
    }
    #[test]
    fn room_ring_keeps_newest_messages_across_restart_and_limit_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.sqlite");
        {
            let mut e = Engine::open(&path).unwrap();
            e.set_limits(64, 64, 3).unwrap();
            e.execute(None, None, "/new one").unwrap();
            e.execute(None, None, "/new two").unwrap();
            e.execute(None, Some("two"), "independent").unwrap();
            for n in 0..10 {
                e.execute(None, Some("one"), &format!("消息 {n} 🙂"))
                    .unwrap();
            }
            let texts: Vec<_> = e.data.rooms["one"]
                .messages
                .iter()
                .map(|m| m.text.as_str())
                .collect();
            assert_eq!(texts, ["消息 7 🙂", "消息 8 🙂", "消息 9 🙂"]);
            assert_eq!(e.data.rooms["two"].messages.len(), 1);
            assert!(e.execute(None, Some("one"), "/history 4").is_err());
            assert!(
                e.execute(None, Some("one"), "/history")
                    .unwrap()
                    .contains("消息 7")
            );
            // The on-disk representation stays a JSON array, compatible with older data folders.
            let raw: String =
                e.db.query_row("SELECT json FROM state", [], |r| r.get(0))
                    .unwrap();
            assert!(serde_json::from_str::<serde_json::Value>(&raw).unwrap()["rooms"]["one"]["messages"].is_array());
        }
        {
            let mut e = Engine::open(&path).unwrap();
            assert_eq!(
                e.data.rooms["one"].messages.front().unwrap().text,
                "消息 7 🙂"
            );
            e.set_limits(64, 64, 1).unwrap();
            assert_eq!(e.data.rooms["one"].messages.len(), 1);
            assert_eq!(e.data.rooms["one"].messages[0].text, "消息 9 🙂");
            e.execute(None, Some("one"), "newest").unwrap();
            assert_eq!(e.data.rooms["one"].messages[0].text, "newest");
            assert!(e.set_limits(64, 64, 0).is_err());
            assert_eq!(e.max_messages, 1);
        }
        let e = Engine::open(&path).unwrap();
        assert_eq!(e.data.rooms["one"].messages.len(), 1);
        assert_eq!(e.data.rooms["one"].messages[0].text, "newest");
    }
    #[test]
    fn failed_writes_restore_evicted_messages_and_limits() {
        let mut e = engine();
        e.set_limits(64, 64, 2).unwrap();
        e.execute(Some("alice"), Some("lobby"), "oldest").unwrap();
        e.execute(Some("alice"), Some("lobby"), "latest").unwrap();
        let previous = e.data.clone();
        let revision = e.revision;
        e.db.execute_batch("PRAGMA query_only=ON;").unwrap();
        assert!(e.execute(Some("alice"), Some("lobby"), "unsaved").is_err());
        assert!(e.data == previous);
        assert!(e.set_limits(1, 1, 1).is_err());
        assert!(e.data == previous);
        assert_eq!(e.max_messages, 2);
        assert_eq!(e.max_users, 64);
        assert_eq!(e.revision, revision);
    }
    #[test]
    fn password_hashing() {
        let hash = hash_password("correct-horse").unwrap();
        assert!(verify_password("correct-horse", &hash));
        assert!(!verify_password("wrong", &hash));
        assert!(hash_password("ab").is_err());
        assert!(hash_password("你好吗").is_ok());
        assert!(hash_password("你好").is_err());
        let short_hash = hash_password("abc").unwrap();
        assert!(verify_password("abc", &short_hash));
        assert!(hash_password(&"a".repeat(129)).is_err());
    }
    #[test]
    fn moving_folder_preserves_sessions_and_permissions() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let path = source.path().join("chat.sqlite");
        {
            let mut e = Engine::open(&path).unwrap();
            e.provision("alice", "hash".into(), true, false).unwrap();
            e.login("session-token".into(), "alice", "hash", None)
                .unwrap();
            e.execute(Some("alice"), None, "/new team").unwrap();
            e.execute(Some("alice"), Some("team"), "before migration")
                .unwrap();
            e.checkpoint().unwrap();
        }
        std::fs::copy(&path, destination.path().join("chat.sqlite")).unwrap();
        let mut e = Engine::open(&destination.path().join("chat.sqlite")).unwrap();
        assert_eq!(e.session("session-token"), Some("alice".into()));
        assert!(e.snapshot("alice").unwrap().admin);
        assert_eq!(e.data.rooms["team"].messages[0].text, "before migration");
        e.provision("alice", "new-hash".into(), false, true)
            .unwrap();
        assert!(e.session("session-token").is_none());
        assert!(e.login("new-token".into(), "alice", "hash", None).is_err());
        e.login("new-token".into(), "alice", "new-hash", None)
            .unwrap();
        e.logout("new-token").unwrap();
        assert!(e.session("new-token").is_none());
    }
    #[test]
    fn rejects_two_servers_and_unknown_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.sqlite");
        let e = Engine::open(&path).unwrap();
        assert!(Engine::open(&path).is_err());
        e.db.execute_batch("PRAGMA user_version=99;").unwrap();
        drop(e);
        assert!(Engine::open(&path).is_err());
    }
    #[test]
    fn failed_storage_rolls_back_changes() {
        let mut e = engine();
        e.db.execute_batch("DROP TABLE state;").unwrap();
        assert!(e.execute(Some("alice"), None, "/new lost").is_err());
        assert!(!e.data.rooms.contains_key("lost"));
    }
    #[test]
    fn role_commands_are_authorized_and_not_chat_messages() {
        let mut e = engine();
        assert_eq!(
            e.execute(Some("bob"), None, "/whoami").unwrap(),
            "Name: bob\nPermission: user"
        );
        assert!(e.execute(Some("bob"), None, "/grant bob").is_err());
        assert!(e.execute(Some("alice"), None, "/revoke alice").is_err());
        let help = e.execute(Some("bob"), None, "/help").unwrap();
        assert!(help.lines().count() >= 10);
        assert!(!help.contains("/grant"));
        assert!(!help.contains("/reset"));
        e.execute(Some("alice"), None, "/grant bob").unwrap();
        assert!(e.snapshot("bob").unwrap().admin);
        assert!(
            e.snapshot("bob")
                .unwrap()
                .commands
                .iter()
                .any(|c| c.name == "/grant")
        );
        assert!(
            e.execute(Some("bob"), None, "/whoami")
                .unwrap()
                .contains("Permission: admin")
        );
        e.execute(Some("bob"), None, "/new promoted").unwrap();
        e.execute(Some("alice"), None, "/revoke bob").unwrap();
        assert!(e.execute(Some("bob"), None, "/new denied").is_err());
        assert!(e.data.rooms["lobby"].messages.is_empty());
        assert!(e.execute(Some("alice"), None, "/grant missing").is_err());
    }
    #[test]
    fn history_tail_members_and_account_recovery() {
        let mut e = engine();
        for i in 0..80 {
            e.execute(Some("bob"), Some("lobby"), &format!("message-{i}"))
                .unwrap();
        }
        let snapshot = e.snapshot("bob").unwrap();
        assert_eq!(snapshot.rooms[0].messages.len(), 50);
        assert_eq!(snapshot.rooms[0].messages[0].text, "message-30");
        assert_eq!(
            e.execute(Some("bob"), Some("lobby"), "/history 60")
                .unwrap()
                .lines()
                .count(),
            60
        );
        assert!(
            e.execute(Some("bob"), Some("lobby"), "/history 1001")
                .is_err()
        );
        assert!(
            e.execute(Some("bob"), Some("lobby"), "/members")
                .unwrap()
                .contains("alice — admin")
        );
        e.execute(Some("alice"), None, "/new private").unwrap();
        assert!(e.execute(Some("bob"), Some("private"), "/history").is_err());
        assert!(
            e.execute(Some("bob"), Some("lobby"), "/members private")
                .is_err()
        );
        e.execute(Some("bob"), None, "/tell   alice   spaced text")
            .unwrap();
        assert_eq!(e.snapshot("alice").unwrap().direct[0].text, "spaced text");
        e.execute(None, None, "/disable bob").unwrap();
        assert!(e.execute(Some("eve"), None, "/enable bob").is_err());
        e.execute(Some("alice"), None, "/enable bob").unwrap();
        assert!(e.active("bob"));
        assert!(e.snapshot("bob").unwrap().rooms.is_empty());
    }
    #[test]
    fn granted_role_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.sqlite");
        {
            let mut e = Engine::open(&path).unwrap();
            e.provision("alice", "hash".into(), true, false).unwrap();
            e.provision("bob", "hash".into(), false, false).unwrap();
            e.execute(Some("alice"), None, "/grant bob").unwrap();
        }
        assert!(Engine::open(&path).unwrap().snapshot("bob").unwrap().admin);
    }
}
