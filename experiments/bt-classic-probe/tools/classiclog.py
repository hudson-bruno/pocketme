#!/usr/bin/env python3
"""Read the bt-classic-probe firmware's serial log and say what happened.

The firmware prints one tagged line per fact (BOOT, MEM, HCI, INQ, FOUND,
NAME, LE, SUMMARY, ERROR, ...). This tool turns a log into a short report.
Same style as experiments/ble-scan/tools/scanlog.py.

Two ways to use it:

  1. On a log file captured earlier:

         tools/classiclog.py run.log

  2. Straight from the board. On this board (CH340 attached through WSL)
     opening the port restarts the board anyway, so each session starts
     from boot; --reset makes that sure. The board may be shared, so run
     it under the lock and a timeout, and keep a session under 290 s:

         flock <board.lock> timeout 200 tools/classiclog.py --port /dev/ttyUSB0 --reset --secs 180 --save run.log

The report is safe to paste anywhere: counts and device classes, never an
address. Devices whose class says "gamepad" or "joystick" (GAMEPAD-LIKE)
are listed with class, signal and rounds; their name is shown only when it
is one of the names the owner's pad uses (OWNER_PAD_NAMES below), because
a gamepad nearby may be someone else's. `--devices` adds every device
heard, with addresses and names: keep that to yourself.

While reading from the port it echoes the important lines live (INQ done,
GAMEPAD-LIKE finds and names with the address hidden, LE, PAGETEST,
CONNTEST, SUMMARY, ERROR), never the plain FOUND / NAME lines of other
people's devices.

Exit code: 0 = no fault, 1 = errors, a panic or a refused memory request,
2 = no probe lines at all.

Only the standard library is used (no pyserial needed).
"""

import argparse
import json
import os
import re
import sys
import time

ANSI = re.compile(r"\x1b\[[0-9;]*m")
WORD = re.compile(r'(\w[\w.+-]*)=("(?:[^"\\]|\\.)*"|\S+)')
TAGS = ("BOOT", "MEM", "HCI", "INFO", "LE", "INQ", "FOUND", "NAME", "CONN",
        "PAGETEST", "CONNTEST", "PEERTEST", "SUMMARY", "EVT", "ACL", "ERROR")
# The names the owner's GuliKit pad shows, one per mode. Only these names are
# printed in the report; any other gamepad's name is hidden.
OWNER_PAD_NAMES = ("Pro Controller", "GuliKit", "Xbox Wireless Controller")
ADDR = re.compile(r"addr=\S+")
QUOTED_NAME = re.compile(r'((?:eir_)?name)="([^"]*)"')


def owner_name(name):
    return bool(name) and any(name.startswith(n) for n in OWNER_PAD_NAMES)


def hide(line):
    """A line with the address hidden, and any name that is not the owner's pad's."""
    line = ADDR.sub("addr=<hidden>", line)
    return QUOTED_NAME.sub(lambda m: m.group(0) if owner_name(m.group(2)) else f'{m.group(1)}=<hidden>', line)
MAJORS = {0: "misc", 1: "computer", 2: "phone", 3: "network", 4: "audio-video",
          5: "peripheral", 6: "imaging", 7: "wearable", 8: "toy", 9: "health",
          0x1F: "uncategorized"}


def fields(line):
    return {k: v.strip('"') for k, v in WORD.findall(line)}


def num(text, default=None):
    try:
        return int(text, 0)
    except (TypeError, ValueError):
        return default


class Report:
    def __init__(self):
        self.tagged = 0
        self.boot = {}
        self.radio = {}
        self.mem = []
        self.hci_ok = 0
        self.hci_failed = []          # (cmd, status)
        self.rounds = []              # INQ done fields
        self.devices = {}             # addr -> dict (private)
        self.names = {}               # addr -> name (private)
        self.name_fail = 0
        self.le = []
        self.pagetest = None
        self.conntests = []           # CONNTEST / PEERTEST lines (no address in them)
        self.errors = []
        self.panics = []
        self.info = {}
        self.conn_req = 0
        self.vendor = 0

    def feed(self, raw):
        line = ANSI.sub("", raw).strip("\r\n")
        plain = line.strip()
        if "panic" in plain.lower() or "Exception" in plain or "assert" in plain.lower():
            self.panics.append(plain[:200])
        tag = plain.split(" ", 1)[0]
        if tag not in TAGS:
            return
        self.tagged += 1
        kv = fields(plain)
        if tag == "BOOT":
            if "radio-up" in plain:
                self.radio = kv
            elif "mode" in kv:
                self.boot = kv
        elif tag == "MEM":
            self.mem.append(kv)
        elif tag == "HCI" and "cmd" in kv:
            if num(kv.get("status")) == 0:
                self.hci_ok += 1
            else:
                self.hci_failed.append((kv.get("cmd"), kv.get("status")))
        elif tag == "INFO":
            what = plain.split(" ", 2)[1]
            if what != "own_address":
                self.info[what] = plain
        elif tag == "INQ" and plain.startswith("INQ done"):
            self.rounds.append(kv)
        elif tag == "FOUND":
            addr = kv.get("addr")
            d = self.devices.setdefault(addr, {"cod": kv.get("cod"), "best_rssi": -999, "rounds": 0, "gamepad": False,
                                              "hid": False, "device_id": None})
            d["rounds"] += 1
            d["cod"] = kv.get("cod")
            d["best_rssi"] = max(d["best_rssi"], num(kv.get("rssi"), -999))
            d["gamepad"] |= "GAMEPAD-LIKE" in plain
            d["hid"] |= kv.get("hid_uuid") == "1"
            d["device_id"] = kv.get("device_id") or d["device_id"]
            if "eir_name" in kv:
                self.names[addr] = kv["eir_name"]
        elif tag == "NAME":
            if kv.get("result") == "ok":
                self.names[kv.get("addr")] = kv.get("name")
            else:
                self.name_fail += 1
        elif tag == "LE":
            self.le.append(plain)
        elif tag == "PAGETEST":
            self.pagetest = kv
        elif tag in ("CONNTEST", "PEERTEST"):
            self.conntests.append(hide(plain).split(" (")[0])
        elif tag == "CONN" and "request" in plain:
            self.conn_req += 1
        elif tag == "EVT" and "vendor" in plain:
            self.vendor += 1
        elif tag == "ERROR":
            self.errors.append(hide(plain)[:200])

    def faults(self):
        refused = max((num(m.get("heap_refused"), 0) for m in self.mem), default=0)
        return bool(self.errors or self.panics or refused)

    def summary(self):
        mem = self.mem
        stacks = [num(m.get("stack_used")) for m in mem if num(m.get("stack_used")) is not None]
        by_major = {}
        for d in self.devices.values():
            major = (num(d["cod"], 0) >> 8) & 0x1F
            name = MAJORS.get(major, hex(major))
            by_major[name] = by_major.get(name, 0) + 1
        gamepads = []
        for addr, d in self.devices.items():
            if d["gamepad"]:
                name = self.names.get(addr)
                gamepads.append({"name": name if owner_name(name) else ("<hidden>" if name else None), "cod": d["cod"],
                                 "best_rssi": d["best_rssi"], "rounds_seen": d["rounds"], "hid_uuid": d["hid"],
                                 "device_id": d["device_id"]})
        statuses = {}
        for r in self.rounds:
            statuses[r.get("status")] = statuses.get(r.get("status"), 0) + 1
        return {
            "build": self.boot.get("mode"),
            "controller_mode": self.radio.get("controller_mode"),
            "radio_heap": num(self.radio.get("radio_heap")),
            "radio_up_ms": num(self.radio.get("ms")),
            "heap_size": num(mem[0].get("heap_size")) if mem else None,
            "heap_free_last": num(mem[-1].get("heap_free")) if mem else None,
            "heap_min_free": min((num(m.get("heap_min_free"), 1 << 30) for m in mem), default=None),
            "heap_largest_last": num(mem[-1].get("heap_largest")) if mem else None,
            "heap_refused": max((num(m.get("heap_refused"), 0) for m in mem), default=0),
            "stack_used_max": max(stacks) if stacks else None,
            "mem_lines": len(mem),
            "hci_ok": self.hci_ok,
            "hci_failed": self.hci_failed,
            "inquiry_rounds": len(self.rounds),
            "inquiry_status_counts": statuses,
            "classic_devices": len(self.devices),
            "classic_by_major": by_major,
            "named": sum(1 for a in self.devices if a in self.names),
            "name_failures": self.name_fail,
            "gamepad_like": gamepads,
            "le_lines": self.le,
            "pagetest": self.pagetest,
            "conntests": self.conntests,
            "connection_requests": self.conn_req,
            "vendor_events": self.vendor,
            "errors": self.errors[:20],
            "panics": self.panics[:5],
            "info": self.info,
        }

    def text(self, show_devices=False):
        s = self.summary()
        out = []
        say = out.append
        say(f"build={s['build']} controller_mode={s['controller_mode']} radio_up_ms={s['radio_up_ms']} radio_heap={s['radio_heap']}")
        say(f"heap: size={s['heap_size']} free_last={s['heap_free_last']} min_free={s['heap_min_free']} "
            f"largest_last={s['heap_largest_last']} refused={s['heap_refused']}  stack_used_max={s['stack_used_max']}  ({s['mem_lines']} MEM lines)")
        say(f"HCI commands: ok={s['hci_ok']} failed={len(s['hci_failed'])} {s['hci_failed'][:8] if s['hci_failed'] else ''}")
        for key in ("version", "features", "buffers", "visible"):
            if key in s["info"]:
                say("  " + s["info"][key])
        say(f"inquiry rounds={s['inquiry_rounds']} end statuses={s['inquiry_status_counts']}")
        say(f"classic devices heard={s['classic_devices']} by class={s['classic_by_major'] or '-'} named={s['named']} name_failures={s['name_failures']}")
        for line in s["le_lines"]:
            say("  " + line)
        if s["pagetest"]:
            say(f"  PAGETEST {s['pagetest']}")
        for line in s["conntests"]:
            say("  " + line)
        say(f"connection requests (phones tapping us)={s['connection_requests']} vendor events={s['vendor_events']}")
        if s["gamepad_like"]:
            say("GAMEPAD-LIKE devices:")
            for g in s["gamepad_like"]:
                say(f"  {g}")
        else:
            say("no GAMEPAD-LIKE device heard")
        for e in s["errors"]:
            say("  " + e)
        for p in s["panics"]:
            say("  PANIC? " + p)
        if show_devices:
            say("ALL DEVICES (private):")
            for addr, d in self.devices.items():
                say(f"  {addr} cod={d['cod']} best_rssi={d['best_rssi']} rounds={d['rounds']} name={self.names.get(addr)!r}")
        if not self.tagged:
            say("NOTHING from the bt-classic-probe firmware in this log.")
        return "\n".join(out)


# ------------------------------------------------------------- the port
# Same as ble-scan's scanlog.py.

def open_port(path, baud=115200):
    import termios

    fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    attrs = termios.tcgetattr(fd)
    speed = getattr(termios, f"B{baud}")
    attrs[0] = 0
    attrs[1] = 0
    # HUPCL left out, so closing the port does not pull the reset line.
    attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
    attrs[3] = 0
    attrs[4] = speed
    attrs[5] = speed
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    return fd


def set_lines(fd, dtr, rts):
    """On this board RTS pulls the chip's reset pin and DTR its boot-mode pin."""
    import fcntl
    import struct
    import termios

    for bit, on in ((termios.TIOCM_DTR, dtr), (termios.TIOCM_RTS, rts)):
        fcntl.ioctl(fd, termios.TIOCMBIS if on else termios.TIOCMBIC, struct.pack("I", bit))


def read_port(path, secs, reset, save, echo, report, show_all=False):
    import select

    fd = open_port(path)
    try:
        set_lines(fd, dtr=False, rts=False)
        if reset:
            set_lines(fd, dtr=False, rts=True)
            time.sleep(0.12)
            set_lines(fd, dtr=False, rts=False)
        out = open(save, "w", encoding="utf-8", errors="replace") if save else None
        deadline = time.monotonic() + secs
        pending = b""
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                break
            ready, _, _ = select.select([fd], [], [], min(left, 0.5))
            if not ready:
                continue
            try:
                chunk = os.read(fd, 4096)
            except BlockingIOError:
                continue
            if not chunk:
                continue
            pending += chunk
            *whole, pending = pending.split(b"\n")
            for raw in whole:
                line = raw.decode("utf-8", errors="replace")
                if out:
                    out.write(line.rstrip("\r") + "\n")
                report.feed(line)
                if echo(line):
                    shown = ANSI.sub("", line).rstrip("\r")
                    print(shown if show_all else hide(shown), flush=True)
        if out:
            out.close()
    finally:
        os.close(fd)


def main():
    parser = argparse.ArgumentParser(description="Summarise the bt-classic-probe firmware's serial log.", epilog=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("logs", nargs="*", help="log files captured earlier ('-' for standard input)")
    parser.add_argument("--port", help="read from this serial port instead (run under the board lock and a timeout)")
    parser.add_argument("--secs", type=float, default=120, help="with --port: how long to read (default 120, at most 290)")
    parser.add_argument("--reset", action="store_true", help="with --port: restart the board first")
    parser.add_argument("--save", help="with --port: also write the raw log to this file")
    parser.add_argument("--watch", choices=("quiet", "events", "all"), default="events",
                        help="with --port: what to show while reading (all = every line, shows addresses)")
    parser.add_argument("--devices", action="store_true", help="add the full list of devices (PRIVATE)")
    parser.add_argument("--json", action="store_true", help="print the report as JSON")
    args = parser.parse_args()

    if not args.logs and not args.port:
        parser.error("give a log file, or --port")

    reports = []
    if args.port:
        if args.secs > 290:
            parser.error("--secs above 290: keep a session short")
        report = Report()

        def echo(line):
            if args.watch == "all":
                return True
            if args.watch == "quiet":
                return False
            plain = ANSI.sub("", line).strip()
            if "GAMEPAD-LIKE" in plain or "panic" in plain.lower():
                return True
            return plain.startswith(("BOOT", "INQ done", "LE ", "PAGETEST", "CONNTEST", "PEERTEST", "SUMMARY", "ERROR"))

        read_port(args.port, args.secs, args.reset, args.save, echo, report, show_all=args.watch == "all")
        reports.append((args.port, report))
    for path in args.logs:
        report = Report()
        stream = sys.stdin if path == "-" else open(path, encoding="utf-8", errors="replace")
        with stream:
            for line in stream:
                report.feed(line)
        reports.append((path, report))

    status = 0
    if args.json:
        print(json.dumps({name: r.summary() for name, r in reports}, indent=2))
    for name, r in reports:
        if not args.json:
            print(f"==== {name}")
            print(r.text(show_devices=args.devices))
            print()
        if not r.tagged:
            status = max(status, 2)
        elif r.faults():
            status = max(status, 1)
    return status


if __name__ == "__main__":
    sys.exit(main())
