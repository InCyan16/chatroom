use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Command {
    pub name: &'static str,
    pub usage: &'static str,
    pub description: &'static str,
    pub section: &'static str,
    pub admin: bool,
    pub console: bool,
}

pub fn available(admin: bool, console: bool) -> Vec<Command> {
    let definitions = [
        ("General", "/help", "/help", "List commands", false),
        (
            "General",
            "/whoami",
            "/whoami",
            "Show your name and permission",
            false,
        ),
        (
            "Account",
            "/passwd",
            "/passwd old new",
            "Change your password; sign out other sessions",
            false,
        ),
        (
            "Account",
            "/users",
            "/users",
            "List accounts and permissions",
            false,
        ),
        (
            "Account",
            "/user",
            "/user name password [admin|user]",
            "Create an account",
            true,
        ),
        (
            "Account",
            "/reset",
            "/reset user password",
            "Reset a password and revoke sessions",
            true,
        ),
        (
            "Account",
            "/disable",
            "/disable user",
            "Disable an account and revoke sessions",
            true,
        ),
        (
            "Account",
            "/enable",
            "/enable user",
            "Enable an account",
            true,
        ),
        (
            "Account",
            "/grant",
            "/grant user",
            "Grant administrator permission",
            true,
        ),
        (
            "Account",
            "/revoke",
            "/revoke user",
            "Restore user permission",
            true,
        ),
        ("Rooms", "/rooms", "/rooms", "List rooms", false),
        (
            "Rooms",
            "/members",
            "/members [room]",
            "List room members",
            false,
        ),
        (
            "Rooms",
            "/join",
            "/join room",
            "Switch rooms; users must be invited",
            false,
        ),
        ("Rooms", "/leave", "/leave [room]", "Leave a room", false),
        ("Rooms", "/new", "/new room", "Create a room", true),
        (
            "Rooms",
            "/add",
            "/add user [room]",
            "Invite a user to a room",
            true,
        ),
        (
            "Rooms",
            "/kick",
            "/kick user [room]",
            "Remove a user from a room",
            true,
        ),
        (
            "Rooms",
            "/delete",
            "/delete room",
            "Delete a room and its messages",
            true,
        ),
        (
            "Messages",
            "/tell",
            "/tell user message",
            "Send a private message",
            false,
        ),
        (
            "Messages",
            "/react",
            "/react message-id reaction",
            "Toggle an emoji or text reaction",
            false,
        ),
        (
            "Messages",
            "/reply",
            "/reply message-id message",
            "Reply to a retained message",
            false,
        ),
        (
            "Messages",
            "/history",
            "/history [count] [user]",
            "Read retained messages (default up to 50)",
            false,
        ),
        (
            "Server",
            "/configs",
            "/configs",
            "Show active configuration; changes require restart",
            true,
        ),
        (
            "Server",
            "/clean",
            "/clean age [room|@private|@all]",
            "Delete messages older than an age, e.g. 7d or 24h",
            true,
        ),
    ];
    let mut result: Vec<_> = definitions
        .into_iter()
        .filter(|(_, name, _, _, privileged)| {
            (!privileged || admin)
                && (!console
                    || ![
                        "/passwd", "/join", "/leave", "/tell", "/history", "/react", "/reply",
                    ]
                    .contains(name))
        })
        .map(|(section, name, usage, description, admin)| Command {
            name,
            usage,
            description,
            section,
            admin,
            console: false,
        })
        .collect();
    if !console {
        result.push(Command {
            name: "/clear",
            usage: "/clear",
            description: "Clear this view locally; refresh restores messages",
            section: "General",
            admin: false,
            console: false,
        });
        result.push(Command {
            name: "/logout",
            usage: "/logout",
            description: "Sign out",
            section: "Account",
            admin: false,
            console: false,
        });
    }
    result
}

pub fn help(admin: bool, console: bool) -> String {
    let commands = available(admin, console);
    ["General", "Account", "Rooms", "Messages", "Server"]
        .into_iter()
        .filter_map(|section| {
            let lines: Vec<_> = commands
                .iter()
                .filter(|c| c.section == section)
                .map(|c| format!("{} — {}", c.usage, c.description))
                .collect();
            (!lines.is_empty()).then(|| format!("[{section}]\n{}", lines.join("\n")))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
