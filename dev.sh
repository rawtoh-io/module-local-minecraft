#!/usr/bin/env bash
# Local dev loop: build the module, run a vanilla Minecraft server in ./server
# with RCON on, then start the module against it. Ctrl-C stops both.
#
#   RAWTOH_ENROLL_TOKEN=rth_e_... MC_PLAYER=Pseudo ./dev.sh   # first run
#   MC_PLAYER=Pseudo ./dev.sh                                  # next runs
set -euo pipefail
cd "$(dirname "$0")"

SERVER_DIR=server
RCON_PORT=25575
RCON_PASSWORD=${RCON_PASSWORD:-rawtoh}
JAVA_OPTS=${JAVA_OPTS:--Xmx2G}

cargo build --release

mkdir -p "$SERVER_DIR"
if [ ! -f "$SERVER_DIR/server.jar" ]; then
  echo "downloading latest vanilla server.jar"
  version_url=$(curl -fsSL https://piston-meta.mojang.com/mc/game/version_manifest_v2.json \
    | jq -r '.latest.release as $v | .versions[] | select(.id == $v) | .url')
  curl -fsSL "$version_url" | jq -r .downloads.server.url | xargs curl -fsSL -o "$SERVER_DIR/server.jar"
fi
echo "eula=true" > "$SERVER_DIR/eula.txt"
# Only the keys we need: the server fills in the rest on first start.
grep -q '^enable-rcon=true' "$SERVER_DIR/server.properties" 2>/dev/null || cat >> "$SERVER_DIR/server.properties" <<PROPS
enable-rcon=true
rcon.port=$RCON_PORT
rcon.password=$RCON_PASSWORD
white-list=false
PROPS

(cd "$SERVER_DIR" && exec java $JAVA_OPTS -jar server.jar nogui </dev/null >console.log 2>&1) &
SERVER_PID=$!
# SIGTERM: the server saves the world and exits cleanly.
trap 'kill $SERVER_PID 2>/dev/null; wait $SERVER_PID 2>/dev/null' EXIT

echo "server starting (log: $SERVER_DIR/console.log), waiting for rcon :$RCON_PORT"
for _ in $(seq 180); do
  if (exec 3<>"/dev/tcp/127.0.0.1/$RCON_PORT") 2>/dev/null; then break; fi
  kill -0 $SERVER_PID 2>/dev/null || { tail -20 "$SERVER_DIR/console.log"; exit 1; }
  sleep 1
done

target/release/module-local-minecraft \
  --state-dir "$SERVER_DIR/state" \
  --rcon "127.0.0.1:$RCON_PORT" \
  --rcon-password "$RCON_PASSWORD" \
  --player "${MC_PLAYER:-@a}" \
  --log "$SERVER_DIR/logs/latest.log" \
  ${RAWTOH_ENROLL_TOKEN:+--enroll "$RAWTOH_ENROLL_TOKEN"}
