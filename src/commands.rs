// Method call → the console commands to run. Everything that lands in a
// command string is validated here: this is the Twitch-chat trust boundary,
// an entity id must never smuggle NBT or a second command.

use rawtoh_module_sdk::RpcError;
use serde_json::{Value, json};

pub fn build(method: &str, params: &Value, default_player: &str) -> Result<Vec<String>, RpcError> {
    let player = || player(params, default_player);
    Ok(match method {
        "minecraft.summon" => {
            let entity = id(params, "entity")?;
            let cmd = format!("execute at {} run summon {entity} ~ ~ ~", player()?);
            vec![cmd; int(params, "count", 1, 1, 32)? as usize]
        }
        "minecraft.give" => vec![format!(
            "give {} {} {}",
            player()?,
            id(params, "item")?,
            int(params, "count", 1, 1, 64)?
        )],
        "minecraft.effect" => vec![format!(
            "effect give {} {} {} {}",
            player()?,
            id(params, "effect")?,
            int(params, "seconds", 30, 1, 3600)?,
            int(params, "amplifier", 0, 0, 255)?
        )],
        "minecraft.say" => {
            let message = text(params, "message", 256)?;
            let line = match params["from"].as_str() {
                Some(_) => json!([
                    { "text": format!("{}: ", text(params, "from", 64)?), "color": "aqua" },
                    { "text": message }
                ]),
                None => json!({ "text": message }),
            };
            vec![format!("tellraw @a {line}")]
        }
        "minecraft.title" => {
            let mut cmds = Vec::new();
            if params["subtitle"].is_string() {
                let sub = json!({ "text": text(params, "subtitle", 128)? });
                cmds.push(format!("title @a subtitle {sub}"));
            }
            let title = json!({ "text": text(params, "title", 64)? });
            cmds.push(format!("title @a title {title}"));
            cmds
        }
        "minecraft.weather" => {
            let weather = choice(params, "weather", &["clear", "rain", "thunder"])?;
            vec![format!(
                "weather {weather} {}",
                int(params, "seconds", 600, 1, 86400)?
            )]
        }
        "minecraft.time" => {
            let time = match params["time"].as_i64() {
                Some(t) if (0..=24000).contains(&t) => t.to_string(),
                Some(_) => return Err(RpcError::invalid_params("time: 0..=24000")),
                None => choice(params, "time", &["day", "noon", "night", "midnight"])?,
            };
            vec![format!("time set {time}")]
        }
        "minecraft.teleport" => {
            let dest = match params["to"].as_str() {
                Some(_) => {
                    let to = text(params, "to", 16)?;
                    if !to.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        return Err(RpcError::invalid_params("to: username"));
                    }
                    to
                }
                None => format!(
                    "{} {} {}",
                    int(params, "x", 0, -30_000_000, 30_000_000)?,
                    int(params, "y", 64, -64, 320)?,
                    int(params, "z", 0, -30_000_000, 30_000_000)?
                ),
            };
            vec![format!("tp {} {dest}", player()?)]
        }
        "minecraft.gamemode" => vec![format!(
            "gamemode {} {}",
            choice(
                params,
                "mode",
                &["survival", "creative", "adventure", "spectator"]
            )?,
            player()?
        )],
        "minecraft.kill" => vec![format!("kill {}", player()?)],
        "minecraft.clear" => vec![format!("clear {}", player()?)],
        "minecraft.xp" => vec![format!(
            "xp add {} {} {}",
            player()?,
            int(params, "amount", 1, -10_000, 10_000)?,
            if params["levels"].as_bool().unwrap_or(false) {
                "levels"
            } else {
                "points"
            }
        )],
        "minecraft.sound" => vec![format!(
            "playsound {} master {}",
            id(params, "sound")?,
            player()?
        )],
        "minecraft.command" => {
            let cmd = text(params, "command", 1400)?;
            vec![cmd.trim_start_matches('/').to_owned()]
        }
        _ => return Err(RpcError::method_not_found(method)),
    })
}

/// Required string, no control characters (a newline would end the command).
fn text(params: &Value, key: &str, max: usize) -> Result<String, RpcError> {
    params[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| RpcError::invalid_params(format!("{key}: string of 1..={max} chars")))
}

/// Namespaced id (`creeper` → `minecraft:creeper`, `mod:thing` kept as is).
fn id(params: &Value, key: &str) -> Result<String, RpcError> {
    let raw = text(params, key, 64)?.to_lowercase();
    if !raw
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-' | '/'))
    {
        return Err(RpcError::invalid_params(format!(
            "{key}: id like tnt or minecraft:tnt"
        )));
    }
    Ok(if raw.contains(':') {
        raw
    } else {
        format!("minecraft:{raw}")
    })
}

/// One of a fixed set of words.
fn choice(params: &Value, key: &str, allowed: &[&str]) -> Result<String, RpcError> {
    let v = text(params, key, 16)?.to_lowercase();
    if allowed.contains(&v.as_str()) {
        Ok(v)
    } else {
        Err(RpcError::invalid_params(format!(
            "{key}: one of {}",
            allowed.join(", ")
        )))
    }
}

fn int(params: &Value, key: &str, default: i64, min: i64, max: i64) -> Result<i64, RpcError> {
    match &params[key] {
        Value::Null => Ok(default),
        v => v
            .as_i64()
            .filter(|v| (min..=max).contains(v))
            .ok_or_else(|| RpcError::invalid_params(format!("{key}: integer {min}..={max}"))),
    }
}

/// A username or one of the player-only selectors.
fn player(params: &Value, default: &str) -> Result<String, RpcError> {
    let p = match &params["player"] {
        Value::Null => default.to_owned(),
        _ => text(params, "player", 16)?,
    };
    let username = p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if username || matches!(p.as_str(), "@a" | "@p" | "@r") {
        Ok(p)
    } else {
        Err(RpcError::invalid_params("player: username or @a / @p / @r"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_commands_and_rejects_injection() {
        let b = |m, p| build(m, &p, "Streamer");
        assert_eq!(
            b("minecraft.summon", json!({ "entity": "TNT", "count": 2 })).unwrap(),
            ["execute at Streamer run summon minecraft:tnt ~ ~ ~"; 2]
        );
        assert_eq!(
            b(
                "minecraft.give",
                json!({ "item": "diamond", "player": "@a" })
            )
            .unwrap(),
            ["give @a minecraft:diamond 1"]
        );
        assert_eq!(
            b(
                "minecraft.effect",
                json!({ "effect": "levitation", "seconds": 5 })
            )
            .unwrap(),
            ["effect give Streamer minecraft:levitation 5 0"]
        );
        assert_eq!(
            b(
                "minecraft.say",
                json!({ "message": "he\"llo", "from": "bob" })
            )
            .unwrap(),
            [r#"tellraw @a [{"color":"aqua","text":"bob: "},{"text":"he\"llo"}]"#]
        );
        assert_eq!(
            b(
                "minecraft.title",
                json!({ "title": "BOOM", "subtitle": "from chat" })
            )
            .unwrap(),
            [
                "title @a subtitle {\"text\":\"from chat\"}",
                "title @a title {\"text\":\"BOOM\"}"
            ]
        );
        assert_eq!(
            b("minecraft.command", json!({ "command": "/list" })).unwrap(),
            ["list"]
        );

        for (m, p) in [
            (
                "minecraft.summon",
                json!({ "entity": "tnt ~ ~ ~ {Fuse:0}" }),
            ),
            ("minecraft.summon", json!({ "entity": "tnt", "count": 999 })),
            (
                "minecraft.give",
                json!({ "item": "diamond", "player": "@e" }),
            ),
            (
                "minecraft.give",
                json!({ "item": "diamond", "player": "a b" }),
            ),
            ("minecraft.say", json!({ "message": "a\nop bob" })),
            ("minecraft.command", json!({ "command": "" })),
            ("minecraft.weather", json!({ "weather": "snow" })),
            ("minecraft.time", json!({ "time": 99999 })),
            ("minecraft.teleport", json!({ "to": "@e" })),
            ("minecraft.nope", json!({})),
        ] {
            assert!(build(m, &p, "Streamer").is_err(), "{m} {p}");
        }
    }

    #[test]
    fn manifest_lists_every_method() {
        let manifest: Value = serde_json::from_str(include_str!("../manifest.json")).unwrap();
        for m in manifest["methods"].as_array().unwrap() {
            let name = m["name"].as_str().unwrap();
            assert!(
                !matches!(build(name, &json!({}), "x"), Err(e) if e.code == -32601),
                "{name}"
            );
        }
    }
}
