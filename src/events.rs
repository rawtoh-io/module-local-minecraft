// Minecraft → hub. RCON is one-way, but the server logs everything to
// logs/latest.log in English whatever the client locale: tail it and turn
// chat, joins, advancements and deaths into events.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

pub const EVENTS: [&str; 5] = [
    "minecraft.chat",
    "minecraft.join",
    "minecraft.leave",
    "minecraft.advancement",
    "minecraft.death",
];

const ADVANCEMENTS: [&str; 3] = [
    " has made the advancement [",
    " has completed the challenge [",
    " has reached the goal [",
];
/// Lines that start with a player name but are not death messages.
const NOT_DEATHS: [&str; 5] = [
    "lost connection",
    "logged in with entity id",
    "moved too quickly",
    "moved wrongly",
    "issued server command",
];

#[derive(Default)]
pub struct Parser {
    /// Players seen online: a death message is "<player> <anything else>".
    pub players: HashSet<String>,
}

impl Parser {
    /// Seed from the reply to `list`: "There are 1 of a max of 20 players online: A, B".
    pub fn seed(&mut self, list_reply: &str) {
        if let Some((_, names)) = list_reply.split_once("online: ") {
            self.players.extend(
                names
                    .split(", ")
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(String::from),
            );
        }
    }

    pub fn parse(&mut self, line: &str) -> Option<(&'static str, Value)> {
        // "[17:05:33] [Server thread/INFO]: <msg>"
        let (head, msg) = line.split_once("]: ")?;
        if !head.ends_with("/INFO") {
            return None;
        }
        let msg = msg.strip_prefix("[Not Secure] ").unwrap_or(msg);

        if let Some(rest) = msg.strip_prefix('<') {
            let (player, message) = rest.split_once("> ")?;
            return Some((
                "minecraft.chat",
                json!({ "player": player, "message": message }),
            ));
        }
        if let Some(player) = msg.strip_suffix(" joined the game") {
            self.players.insert(player.to_owned());
            return Some(("minecraft.join", json!({ "player": player })));
        }
        if let Some(player) = msg.strip_suffix(" left the game") {
            self.players.remove(player);
            return Some(("minecraft.leave", json!({ "player": player })));
        }
        for marker in ADVANCEMENTS {
            if let Some((player, rest)) = msg.split_once(marker) {
                let title = rest.strip_suffix(']').unwrap_or(rest);
                return Some((
                    "minecraft.advancement",
                    json!({ "player": player, "title": title }),
                ));
            }
        }
        let (player, rest) = msg.split_once(' ')?;
        if self.players.contains(player) && !NOT_DEATHS.iter().any(|n| rest.starts_with(n)) {
            return Some((
                "minecraft.death",
                json!({ "player": player, "message": msg }),
            ));
        }
        None
    }
}

/// Follows `path` from its current end, calling `on_line` for every complete
/// new line. Survives the midnight / restart rotation of latest.log.
pub fn tail(path: PathBuf, mut on_line: impl FnMut(&str) + Send + 'static) {
    thread::spawn(move || {
        let mut pos = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut buf = String::new();
        // ponytail: reopen + stat every 300 ms instead of inotify; 3 syscalls, no dependency.
        loop {
            thread::sleep(Duration::from_millis(300));
            let Ok(mut file) = File::open(&path) else {
                continue;
            };
            let len = file.metadata().map(|m| m.len()).unwrap_or(0);
            if len < pos {
                pos = 0; // rotated or truncated
            }
            if len == pos || file.seek(SeekFrom::Start(pos)).is_err() {
                continue;
            }
            let mut reader = BufReader::new(file);
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(n) if n > 0 && buf.ends_with('\n') => {
                        pos += n as u64;
                        on_line(buf.trim_end());
                    }
                    _ => break, // EOF or a half-written line: retry from `pos`
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_log_lines() {
        let mut p = Parser::default();
        p.seed("There are 1 of a max of 20 players online: Steve");
        let line = |msg: &str| format!("[17:05:33] [Server thread/INFO]: {msg}");
        let cases = [
            (
                "Tilican joined the game",
                "minecraft.join",
                json!({ "player": "Tilican" }),
            ),
            (
                "<Tilican> gg",
                "minecraft.chat",
                json!({ "player": "Tilican", "message": "gg" }),
            ),
            (
                "[Not Secure] <Tilican> a > b",
                "minecraft.chat",
                json!({ "player": "Tilican", "message": "a > b" }),
            ),
            (
                "Tilican has made the advancement [Stone Age]",
                "minecraft.advancement",
                json!({ "player": "Tilican", "title": "Stone Age" }),
            ),
            (
                "Tilican was blown up by Creeper",
                "minecraft.death",
                json!({ "player": "Tilican", "message": "Tilican was blown up by Creeper" }),
            ),
            (
                "Steve fell from a high place",
                "minecraft.death",
                json!({ "player": "Steve", "message": "Steve fell from a high place" }),
            ),
            (
                "Tilican left the game",
                "minecraft.leave",
                json!({ "player": "Tilican" }),
            ),
        ];
        for (msg, event, payload) in cases {
            assert_eq!(p.parse(&line(msg)), Some((event, payload)), "{msg}");
        }
        for msg in [
            "Tilican lost connection: Disconnected",
            "Tilican issued server command: /kill",
            "Tilican fell from a high place", // left the game: unknown player
            "Starting minecraft server version 26.3",
            "[Server] hello",
        ] {
            assert_eq!(p.parse(&line(msg)), None, "{msg}");
        }
        assert_eq!(
            p.parse("[17:05:33] [Server thread/WARN]: Tilican moved too quickly!"),
            None
        );
    }
}
