# module-local-minecraft

Drives a Minecraft **Java** server from a Rawtoh hub over RCON, built on
`rawtoh-module-sdk`. No server plugin: RCON ships with vanilla, Paper and
Fabric. Wire a Twitch redeem or chat command on the board to `minecraft.summon`
and the viewers drop TNT on the streamer.

```bash
# server.properties: enable-rcon=true, rcon.port=25575, rcon.password=...
# Hub UI → Module → Instances → create an instance with manifest.json → copy the token
module-local-minecraft --state-dir ~/.config/rawtoh-minecraft --rcon-password '...' --player Streamer --enroll rth_e_...
module-local-minecraft --state-dir ~/.config/rawtoh-minecraft --rcon-password '...' --player Streamer
```

| Method              | Params                                                   |
| ------------------- | -------------------------------------------------------- |
| `minecraft.summon`  | `entity` (tnt, creeper, zombie...), `player?`, `count?` 1-32 |
| `minecraft.give`    | `item` (diamond, tnt...), `player?`, `count?` 1-64       |
| `minecraft.effect`  | `effect` (speed, blindness...), `player?`, `seconds?`, `amplifier?` |
| `minecraft.say`     | `message`, `from?` (colored prefix, e.g. the Twitch name) |
| `minecraft.title`   | `title`, `subtitle?`: big text on every screen            |
| `minecraft.command` | `command`: raw console command, returns the server reply  |

`player` is a username or `@a` / `@p` / `@r`; it defaults to `--player`. Ids
are plain (`tnt`) or namespaced (`minecraft:tnt`, `mod:thing`), nothing else
gets through, so chat input cannot smuggle NBT or a second command.

| Flag              | Env                   | Default                 |
| ----------------- | --------------------- | ----------------------- |
| `--rawtoh-api`    | `RAWTOH_API_URL`      | `https://app.rawtoh.io` |
| `--rawtoh-ws`     | `RAWTOH_WS_URL`       | `wss://rpc.rawtoh.io`   |
| `--enroll`        | `RAWTOH_ENROLL_TOKEN` | one-shot, re-enrolls    |
| `--state-dir`     | `STATE_DIR`           | `./state`               |
| `--rcon`          | `RCON_ADDR`           | `127.0.0.1:25575`       |
| `--rcon-password` | `RCON_PASSWORD`       | required                |
| `--player`        | `MC_PLAYER`           | `@a`                    |

Single-player "Open to LAN" has no RCON: run a dedicated server (the vanilla
`server.jar` on the same machine is enough). Bedrock is not supported.

```bash
cargo test
```

## Dev loop

`dev.sh` builds the module, downloads the latest vanilla `server.jar` into
`./server` (RCON on, password `rawtoh`), waits for it, then runs the module.
Ctrl-C stops both; the server saves the world on the way out.

```bash
RAWTOH_ENROLL_TOKEN=rth_e_... MC_PLAYER=Pseudo ./dev.sh   # first run
MC_PLAYER=Pseudo ./dev.sh                                  # next runs
```

Then join `localhost` from the game client. Server console output lands in
`server/console.log`.
# module-local-minecraft
