#!/bin/bash
# Live changes to the Campus Crypt Arduino display (LCD1602 + MAX7219), macOS / Linux.
#
#   hardware/tools/live.sh status                 is the game running? which board ports?
#   hardware/tools/live.sh push <layout.txt>      send a layout (LT:/LM:/LB: lines) and keep it
#   hardware/tools/live.sh preview <W|M|P|F|X|D|L|H|V|S>   show one screen / event on the board
#   hardware/tools/live.sh flash                  compile + upload campus_crypt_lcd_matrix.ino
#
# While the game runs it owns the USB port, so push/preview go through it (files in the
# settings folder, see src/display_live.rs) and flash asks it to let go of the port first.
# With no game running they talk to the board directly (target/*/crypt_display).
set -u
HERE="$(cd "$(dirname "$0")/../.." && pwd)"
SKETCH="$HERE/hardware/campus_crypt_lcd_matrix"
if [ -n "${CAMPUS_CRYPT_SETTINGS:-}" ]; then
  DIR="$(dirname "$CAMPUS_CRYPT_SETTINGS")"
elif [ "$(uname)" = "Darwin" ]; then
  DIR="$HOME/Library/Application Support/campus-crypt"
else
  DIR="${XDG_CONFIG_HOME:-$HOME/.config}/campus-crypt"
fi
mkdir -p "$DIR"
CLI="${ARDUINO_CLI:-}"
if [ -z "$CLI" ]; then
  CLI="$(command -v arduino-cli || true)"
  [ -z "$CLI" ] && CLI="/Applications/Arduino IDE.app/Contents/Resources/app/lib/backend/resources/arduino-cli"
fi
LIBS="$HOME/Library/Arduino15/libraries"  # the IDE's built-in libraries (LiquidCrystal)
[ -d "$LIBS" ] || LIBS="$HOME/.arduino15/libraries"

status_val() { sed -n "s/^$1=//p" "$DIR/display_status.txt" 2>/dev/null; }

# the game wrote its status in the last 4 s
game_running() {
  local up now
  up="$(status_val updated)"; now="$(date +%s)"
  [ -n "$up" ] && [ $((now - up)) -le 4 ]
}
game_connected() { game_running && [ "$(status_val connected)" = "1" ]; }

tool() {
  local t
  for t in "$HERE/target/release/crypt_display" "$HERE/target/debug/crypt_display"; do
    [ -x "$t" ] && { echo "$t"; return; }
  done
  (cd "$HERE" && cargo build --release --bin crypt_display >&2) && echo "$HERE/target/release/crypt_display"
}

mtime_ms() { python3 -c 'import os,sys; print(int(os.stat(sys.argv[1]).st_mtime*1000))' "$1"; }

case "${1:-}" in
  status)
    if game_running; then
      echo "{\"game\":\"running\",\"board\":\"$(status_val status)\",\"connected\":$(status_val connected)}"
    else
      echo "{\"game\":\"not running\",\"ports\":$("$(tool)" ports | sed 's/.*"ports"://; s/}$//')}"
    fi
    ;;
  push)
    src="${2:?usage: live.sh push <layout.txt>}"
    # always keep it in the settings folder: the game sends it whenever a board connects
    cp "$src" "$DIR/display_layout.txt.tmp" && mv "$DIR/display_layout.txt.tmp" "$DIR/display_layout.txt"
    if game_connected; then
      want="$(mtime_ms "$DIR/display_layout.txt")"
      for _ in $(seq 1 20); do
        sleep 0.25
        sent="$(status_val layout_sent)"
        # mtime precision differs a little between the game and python: allow 2 ms
        if [ -n "$sent" ] && [ $(( sent > want ? sent - want : want - sent )) -le 2 ]; then
          echo '{"ok":true,"via":"game"}'; exit 0
        fi
      done
      echo '{"ok":false,"via":"game","error":"the game has not sent it yet"}'; exit 1
    elif game_running; then
      echo '{"ok":true,"via":"game","note":"saved; the game sends it when the board connects"}'
    else
      { cat "$src"; echo; echo "LSAVE"; } | "$(tool)" send -
    fi
    ;;
  preview)
    c="${2:?usage: live.sh preview <W|M|P|F|X|D|L|H|V|S>}"
    if game_connected; then
      echo "LPREV:$c" > "$DIR/display_send.txt"
      echo '{"ok":true,"via":"game"}'
    else
      echo "LPREV:$c" | "$(tool)" send -
    fi
    ;;
  flash)
    [ -x "$CLI" ] || { echo "{\"ok\":false,\"error\":\"arduino-cli not found (install the Arduino IDE or set ARDUINO_CLI)\"}"; exit 1; }
    touch "$DIR/upload.lock"
    trap 'rm -f "$DIR/upload.lock"' EXIT
    if game_running; then
      # wait for the game to let go of the port
      for _ in $(seq 1 20); do
        case "$(status_val status)" in Paused*) break ;; esac
        sleep 0.25
      done
    fi
    log="$DIR/flash.log"
    if ! "$CLI" compile --fqbn arduino:avr:mega --libraries "$LIBS" "$SKETCH" >"$log" 2>&1; then
      echo "{\"ok\":false,\"step\":\"compile\",\"log\":\"$log\"}"
      grep -E "error|Error" "$log" | head -10 >&2
      exit 1
    fi
    size="$(grep -o 'Sketch uses [0-9]* bytes' "$log" | grep -o '[0-9]*' | head -1)"
    port="${CAMPUS_CRYPT_HW_PORT:-$(ls /dev/cu.usbmodem* /dev/cu.usbserial* /dev/cu.wchusbserial* /dev/ttyACM* /dev/ttyUSB* 2>/dev/null | head -1)}"
    if [ -z "$port" ]; then
      echo "{\"ok\":false,\"step\":\"upload\",\"compiled\":true,\"bytes\":${size:-0},\"error\":\"no board found: plug the Mega in over USB\"}"
      exit 1
    fi
    if "$CLI" upload --fqbn arduino:avr:mega -p "$port" "$SKETCH" >>"$log" 2>&1; then
      echo "{\"ok\":true,\"port\":\"$port\",\"bytes\":${size:-0}}"
    else
      echo "{\"ok\":false,\"step\":\"upload\",\"port\":\"$port\",\"log\":\"$log\"}"
      tail -8 "$log" >&2
      exit 1
    fi
    ;;
  *)
    sed -n '2,12p' "$0"; exit 2 ;;
esac
