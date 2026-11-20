#!/usr/bin/env python3
"""
crypt_sim.py - a fake Campus Crypt status display (the output-only Mega 2560
sketch), for testing the game's serial link without hardware.

It creates a pseudo-terminal (a virtual serial port), behaves like the real
sketch (HELLO:CRYPT handshake, PING reply, HB heartbeat every 0.5 s, never any
J: joystick lines), checks every line the game sends against the protocol, and
prints what the LCD would show.

Usage (macOS or Linux, Python 3.8+, no extra packages):

    python3 hardware/sim/crypt_sim.py --duration 60

It prints the port to use, e.g.  /tmp/crypt-sim-port  -> /dev/ttys004
Then start the game pointed at it:

    cargo run --release -- --hw-port /tmp/crypt-sim-port
    # or: CAMPUS_CRYPT_HW_PORT=/tmp/crypt-sim-port cargo run --release

Options:
    --link PATH          stable symlink to the pty (default /tmp/crypt-sim-port)
    --replug SECONDS     after this many seconds, "unplug" for 3 s and come back
                         on a new pty (tests auto-reconnect)
    --duration SECONDS   stop after this long (default: run until Ctrl+C)
    --no-hello           never volunteer HELLO, only answer PING
    --strict             exit 1 if any line broke the protocol (for scripts)
"""
import argparse
import os
import select
import sys
import time
import tty
from collections import Counter

HELLO = "HELLO:CRYPT"
MODES = set("MPFXD")
EVENTS = {"E:HIT", "E:LVL", "E:STAIRS", "E:DIE", "E:MENU"}


class FakePort:
    def __init__(self, link):
        self.link = link
        self.open()

    def open(self):
        self.master, self.slave = os.openpty()
        tty.setraw(self.slave)  # no echo, no CR/LF translation
        os.set_blocking(self.master, False)
        self.path = os.ttyname(self.slave)
        if self.link:
            try:
                os.remove(self.link)
            except FileNotFoundError:
                pass
            os.symlink(self.path, self.link)
        self.rx = b""

    def close(self):
        for fd in (self.master, self.slave):
            try:
                os.close(fd)
            except OSError:
                pass

    def write_line(self, s):
        try:
            os.write(self.master, (s + "\n").encode())
        except (BlockingIOError, OSError):
            pass  # nobody reading / buffer full: drop, like a real UART would

    def read_lines(self, timeout):
        r, _, _ = select.select([self.master], [], [], timeout)
        if not r:
            return []
        try:
            data = os.read(self.master, 4096)
        except (BlockingIOError, OSError):
            return []
        self.rx += data
        lines = []
        while b"\n" in self.rx:
            line, self.rx = self.rx.split(b"\n", 1)
            lines.append(line.decode(errors="replace").strip())
        return lines


def check_state(line):
    """Parse an S: line; returns (fields, problem or None)."""
    parts = line[2:].split(",")
    if len(parts) < 7:
        return None, "fewer than 7 fields"
    try:
        hp, maxhp, lvl, floor, gold, kills = (int(x) for x in parts[:6])
    except ValueError:
        return None, "non-numeric field"
    mode = parts[6]
    disp = parts[7] if len(parts) > 7 else "F"
    floors = int(parts[8]) if len(parts) > 8 and parts[8].isdigit() else 10
    f = dict(hp=hp, maxhp=maxhp, lvl=lvl, floor=floor, gold=gold, kills=kills, mode=mode, disp=disp, floors=floors)
    if mode not in MODES:
        return f, f"unknown mode {mode!r}"
    if disp not in ("F", "G", "K"):
        return f, f"unknown display {disp!r}"
    if mode != "M" and not (1 <= floor <= floors):
        return f, f"floor {floor} outside 1..{floors}"
    if mode != "M" and not (0 <= hp <= maxhp):
        return f, f"hp {hp} outside 0..{maxhp}"
    return f, None


def lcd(f):
    """The two LCD lines the sketch would build (without the clock)."""
    m, fl = f["mode"], f"F{f['floor']}/{f['floors']}"
    if m == "M":
        return "CAMPUS CRYPT", "Press Enter"
    if m == "D":
        return "OVERWHELMED x_x", fl
    if m == "F":
        return "* INBOX MIMIC *", fl + " FIGHT"
    bar = f"HP {f['hp']}/{f['maxhp']}"
    return bar, ("PAUSE " if m == "X" else f"Kn{f['lvl']} ") + fl


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--link", default="/tmp/crypt-sim-port")
    ap.add_argument("--replug", type=float, default=0)
    ap.add_argument("--duration", type=float, default=0)
    ap.add_argument("--no-hello", action="store_true",
                    help="only answer PING (tests the game's PING fallback)")
    ap.add_argument("--strict", action="store_true", help="exit 1 on any protocol problem")
    args = ap.parse_args()

    port = FakePort(args.link)
    print(f"[sim] fake status display on {port.path}" + (f"  (link: {args.link})" if args.link else ""), flush=True)
    print(f"[sim] start Campus Crypt with:  --hw-port {args.link or port.path}", flush=True)

    start = time.time()
    last_beat = 0.0
    last_state_t = 0.0
    connected_at = None
    connects = 0
    last_lcd = None
    counts = Counter()
    modes = []
    floors_seen = set()
    problems = []
    replugged = False
    pings = 0

    if not args.no_hello:
        port.write_line(HELLO)
    last_hello = time.time()
    try:
        while True:
            now = time.time()
            if args.duration and now - start > args.duration:
                break
            if args.replug and not replugged and now - start > args.replug:
                replugged = True
                print("[sim] --- unplugging for 3 s ---", flush=True)
                port.close()
                time.sleep(3)
                port.open()
                connected_at = None
                last_state_t = 0.0
                print(f"[sim] --- plugged back in on {port.path} ---", flush=True)
                port.write_line(HELLO)
                last_hello = time.time()

            for line in port.read_lines(0.005):
                if not line:
                    continue
                if line == "PING":
                    pings += 1
                    port.write_line(HELLO)
                    print(f"[sim] <- PING  (replied {HELLO})", flush=True)
                elif line.startswith("S:"):
                    counts["S"] += 1
                    last_state_t = time.time()
                    if connected_at is None:
                        connected_at = time.time()
                        connects += 1
                        print("[sim] game connected (first S: line)", flush=True)
                    f, problem = check_state(line)
                    if problem:
                        problems.append(f"{line}: {problem}")
                        print(f"[sim] !! {line}: {problem}", flush=True)
                    if f:
                        if not modes or modes[-1] != f["mode"]:
                            modes.append(f["mode"])
                        if f["mode"] != "M":
                            floors_seen.add((f["floor"], f["floors"]))
                        view = lcd(f)
                        if view != last_lcd:
                            last_lcd = view
                            print(f"[sim] <- {line:<26} LCD: [{view[0]:<16}] [{view[1]:<11}hh:mm]", flush=True)
                elif line.startswith("L"):
                    # live layout (hardware/README.md): answer like the sketch does
                    counts["layout"] += 1
                    ok = (line in ("LSAVE", "LRESET") or line[:3] in ("LT:", "LM:", "LB:") or line.startswith("LPREV:")) and len(line) <= 90
                    if not ok:
                        problems.append(f"bad layout line {line!r}")
                    port.write_line(("LOK:SAVED" if line == "LSAVE" else "LOK") if ok else "LERR")
                    print(f"[sim] <- {line}   <<< layout", flush=True)
                elif line.startswith("E:"):
                    counts[line] += 1
                    if line not in EVENTS:
                        problems.append(f"unknown event {line}")
                    print(f"[sim] <- {line}   <<< event", flush=True)
                else:
                    counts["other"] += 1
                    problems.append(f"unknown line {line!r}")
                    print(f"[sim] <- (unknown) {line!r}", flush=True)

            now = time.time()
            game_alive = last_state_t and now - last_state_t < 3.0
            if not game_alive and not args.no_hello and now - last_hello > 2.0:
                port.write_line(HELLO)  # the sketch repeats this while waiting
                last_hello = now
            if game_alive and now - last_beat >= 0.5:
                port.write_line("HB")
                counts["HB sent"] += 1
                last_beat = now
    except KeyboardInterrupt:
        pass
    finally:
        port.close()
        if args.link:
            try:
                os.remove(args.link)
            except OSError:
                pass

    print("\n[sim] ===== summary =====")
    print(f"[sim] connections          : {connects}")
    print(f"[sim] state lines received : {counts['S']}")
    for k in sorted(EVENTS):
        print(f"[sim] {k:<9} received   : {counts[k]}")
    print(f"[sim] modes seen           : {' -> '.join(modes) or '(none)'}")
    print(f"[sim] floors seen          : {', '.join(f'{a}/{b}' for a, b in sorted(floors_seen)) or '(none)'}")
    print(f"[sim] PINGs answered       : {pings}")
    print(f"[sim] heartbeats sent      : {counts['HB sent']}  (no joystick lines: output-only build)")
    print(f"[sim] protocol problems    : {len(problems)}")
    for p in problems[:10]:
        print(f"[sim]   {p}")
    return 1 if (args.strict and problems) else 0


if __name__ == "__main__":
    sys.exit(main())
