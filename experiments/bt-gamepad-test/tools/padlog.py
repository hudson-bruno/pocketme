#!/usr/bin/env python3
"""Read the bt-gamepad-test firmware's serial log and say what happened.

Two ways to use it:

  1. Straight from the board (opening the port restarts it; --reset makes
     that sure). The board may be shared: run it under the lock and a
     timeout, and keep a session under 150 s:

         flock <board.lock> timeout 140 tools/padlog.py --port /dev/ttyUSB0 --reset --secs 130 --save logs/run.log

  2. On a log saved earlier:

         tools/padlog.py logs/run.log

While reading it echoes the lines worth watching (PAD-STATE, PAD-ERR, MEM,
PAD-UI, the SDP summary, the first HID reports) with every Bluetooth address
replaced by <addr>. --all echoes every line, still without addresses.
The saved file is the raw log (with the gamepad's address): keep it in a
git-ignored folder.

The summary at the end has counts only, never an address or a name. Its
"button trace" part lists, per connection, the distinct button states the
PAD-HID change/same/trace lines showed (held buttons and unmapped input),
every unmapped item seen, and how many "same" lines each report id had.
Exit code: 0 = fine, 1 = errors or a panic, 2 = no firmware lines.

Only the standard library is used.
"""

import argparse
import os
import re
import sys
import time

ANSI = re.compile(r"\x1b\[[0-9;]*m")
ADDR = re.compile(r"\b[0-9A-Fa-f]{2}(?::[0-9A-Fa-f]{2}){5}\b")
WORD = re.compile(r"(\w[\w.+-]*)=(\"(?:[^\"\\]|\\.)*\"|\S+)")
WATCH = ("PAD-STATE", "PAD-ERR", "MEM", "PAD-UI", "PAD-SUMMARY", "PAD-STORE")
NAME = re.compile(r'name="(?:[^"\\]|\\.)*"')


def hide(line):
    return ADDR.sub("<addr>", line)


def words(line):
    return {k: v.strip('"') for k, v in WORD.findall(line)}


class Report:
    def __init__(self):
        self.switch_lines = []
        self.switch_outcomes = []
        self.switch_sent = {}
        self.switch_handshakes = []
        self.outputs_completed = 0
        self.lines = 0
        self.tags = {}
        self.mem = []
        self.errors = []
        self.panic = False
        self.rounds = 0
        self.inquiry_complete = 0
        self.found_gamepads = 0
        self.found_other = 0
        self.statuses = []
        self.controller_mode = None
        self.reports = 0
        self.descriptor = None
        self.sdp_done = []
        self.ui = []
        self.ui_events = []
        self.summaries = []
        self.timeouts = []
        # Saved pairings (PAD-STORE lines). The firmware never prints a
        # key there; addresses are hidden anyway, to be safe.
        self.store = []
        # Button trace, one dict per connection (see new_connection).
        self.conns = []
        self.trace_skipped = 0
        self.trace_starts = 0

    def new_connection(self):
        self.conns.append({
            "layout": None,
            "states": {},
            "unmapped": {},
            "same": {},
            "kinds": {},
        })

    def conn(self):
        if not self.conns:
            self.new_connection()
        return self.conns[-1]

    def trace_line(self, kind, w):
        c = self.conn()
        c["kinds"][kind] = c["kinds"].get(kind, 0) + 1
        if kind == "same":
            rid = w.get("id", "?")
            c["same"][rid] = c["same"].get(rid, 0) + 1
        held = w.get("held", "-")
        un = w.get("unmapped", "")
        state = (held, un)
        c["states"][state] = c["states"].get(state, 0) + 1
        for item in filter(None, un.split("|")):
            c["unmapped"][item] = c["unmapped"].get(item, 0) + 1

    def feed(self, line):
        line = ANSI.sub("", line).rstrip("\r")
        if not line:
            return
        tag = line.split(" ", 1)[0]
        if "panicked" in line or "Exception" in line or "Guru" in line:
            self.panic = True
        if not (tag.startswith("PAD-") or tag == "MEM"):
            return
        self.lines += 1
        self.tags[tag] = self.tags.get(tag, 0) + 1
        w = words(line)
        if " timeout:" in line:
            self.timeouts.append(hide(line))
        if tag == "PAD-SUMMARY":
            # One line per connection step (flowing / end) plus the
            # screen's "view" line. The name is left out of the summary.
            self.summaries.append(NAME.sub('name=<hidden>', hide(line)))
            if "switch_startup" in w:
                self.switch_outcomes.append(
                    f"switch_startup={w['switch_startup']} switch_info={w.get('switch_info', '?')} "
                    f"switch_handshake={w.get('switch_handshake', '?')} outputs_sent={w.get('outputs_sent', '?')}"
                )
        if line.startswith("PAD-STATE ui switch start-up: "):
            text = hide(line[len("PAD-STATE ui switch start-up: "):]).rstrip()
            self.switch_lines.append(text)
            m = re.match(r"variant=(\S+) sent ", text)
            if m:
                self.switch_sent[m.group(1)] = self.switch_sent.get(m.group(1), 0) + 1
        if line.startswith("PAD-HID handshake "):
            self.switch_handshakes.append(hide(line).rstrip())
        if line.startswith("PAD-HCI completed "):
            self.outputs_completed += 1
        if tag == "MEM":
            self.mem.append(w)
        elif tag == "PAD-STORE":
            self.store.append(hide(line).rstrip())
        elif tag == "PAD-ERR":
            self.errors.append(hide(line))
        elif tag == "PAD-STATE":
            if "controller_mode" in w:
                self.controller_mode = w["controller_mode"]
            if line.startswith("PAD-STATE ui status="):
                self.statuses.append(w.get("status", ""))
            if line.startswith("PAD-STATE ui device "):
                # A device event starts a connection (a second one for the
                # same link, with the ids, only when nothing came between).
                if not self.conns or self.conns[-1]["kinds"]:
                    self.new_connection()
            if line.startswith("PAD-STATE ui layout="):
                self.conn()["layout"] = w.get("layout")
        elif tag == "PAD-HCI":
            if " inquiry-complete " in line:
                self.inquiry_complete += 1
            if " found gamepad " in line:
                self.found_gamepads += 1
            if " found other " in line:
                self.found_other += 1
        elif tag == "PAD-HID" and line.startswith("PAD-HID report n="):
            self.reports = max(self.reports, int(w.get("n", "0")))
        elif tag == "PAD-HID" and line.startswith("PAD-HID trace skipped="):
            self.trace_skipped += int(w.get("skipped", "0"))
        elif tag == "PAD-HID" and line.startswith("PAD-HID trace start"):
            self.trace_starts += 1
        elif tag == "PAD-HID":
            kind = line.split(" ", 2)[1] if " " in line else ""
            if kind in ("change", "same", "analog", "trace") and "held=" in line:
                self.trace_line(kind, w)
        elif tag == "PAD-UI":
            if "draws" in w:
                self.ui.append(w)
            else:
                self.ui_events.append(hide(line))
        elif tag == "PAD-SDP":
            if " done records=" in line:
                self.sdp_done.append(f"uuid={w.get('uuid')} records={w.get('records')} bytes={w.get('bytes')} pieces={w.get('pieces')}")
            if line.startswith("PAD-SDP descriptor bytes="):
                self.descriptor = w.get("bytes")

    def print(self):
        print("---- summary (counts only)")
        print(f"firmware lines: {self.lines}  by tag: {self.tags}")
        print(f"controller_mode: {self.controller_mode}")
        print(f"search rounds started: {sum(1 for s in self.statuses if s.startswith('Searching'))}  inquiries completed: {self.inquiry_complete}")
        print(f"devices heard: gamepads={self.found_gamepads} other={self.found_other} (answers, not unique devices)")
        if self.mem:
            first, last = self.mem[0], self.mem[-1]
            frees = [int(m.get("heap_free", 0)) for m in self.mem]
            print(
                f"MEM lines={len(self.mem)} heap_free first={first.get('heap_free')} last={last.get('heap_free')} "
                f"low={min(frees)} high={max(frees)} heap_min_free={last.get('heap_min_free')} refused={last.get('heap_refused')} "
                f"stack_used={last.get('stack_used')}/{last.get('stack_size')} log_dropped={last.get('log_dropped')} tx_dropped={last.get('tx_dropped')} read_errors={last.get('read_errors')}"
            )
        seen = []
        for s in self.statuses:
            short = re.sub(r"round \d+", "round N", s)
            if not seen or seen[-1] != short:
                seen.append(short)
        print("status changes: " + " | ".join(seen[-30:]))
        for d in self.sdp_done:
            print(f"sdp query: {d}")
        if self.descriptor:
            print(f"descriptor bytes: {self.descriptor}")
        print(f"input reports: {self.reports}")
        if self.ui:
            last = self.ui[-1]
            def top(key):
                vals = [int(u.get(key, 0)) for u in self.ui if u.get(key, "0").isdigit()]
                return max(vals) if vals else None
            print(
                f"screen: PAD-UI lines={len(self.ui)} draws={last.get('draws')} drawn={last.get('drawn')} full={last.get('full')} "
                f"full_us_max={last.get('full_us_max')} small_us_max={last.get('small_us_max')} (px {last.get('small_px')}) "
                f"idle_us_max={last.get('idle_us_max')} draw_errors={last.get('draw_errors')}"
            )
            print(
                f"radio turns: gap_us_max_all={last.get('gap_us_max_all')} gaps_over_50ms={last.get('gaps_over_50ms')} "
                f"tick_late_us_max(top of all windows)={top('tick_late_us_max')} gap_shot_us_max={last.get('gap_shot_us_max')} "
                f"view reports={last.get('reports')} event_us_max={last.get('event_us_max')}"
            )
            print(f"pages={last.get('pages')} boot={last.get('boot')} keys={last.get('keys')} shots={last.get('shots')}")
        for e in self.ui_events[:20]:
            print(f"  {e}")
        print(f"saved pairings (PAD-STORE lines): {len(self.store)}")
        for sline in self.store[:20]:
            print(f"  {sline[:200]}")
        print(f"PAD-SUMMARY lines: {len(self.summaries)}")
        for sline in self.summaries[-12:]:
            print(f"  {sline[:300]}")
        print(f"timeouts: {len(self.timeouts)}")
        for t in self.timeouts[:12]:
            print(f"  {t[:300]}")
        print(f"errors: {len(self.errors)}  panic: {self.panic}")
        for e in self.errors[:20]:
            print(f"  {e}")
        self.print_switch()
        self.print_trace()

    def print_switch(self):
        """The Switch-mode start-up prober: which way of talking won."""
        if not (self.switch_lines or self.switch_outcomes):
            return
        print("---- switch start-up")
        for o in self.switch_outcomes[-6:]:
            print(f"  {o}")
        sent = " ".join(f"{k}={n}" for k, n in self.switch_sent.items()) or "none"
        print(f"  packets sent per variant: {sent}")
        print(f"  outputs our controller completed (PAD-HCI completed): {self.outputs_completed}")
        wins = [l for l in self.switch_lines if " WON " in l]
        fails = [l for l in self.switch_lines if l.startswith("all ") and "variants failed" in l]
        answers = [l for l in self.switch_lines if "answer to subcommand" in l]
        print(f"  answers: {len(answers)}  wins: {len(wins)}  all-failed: {len(fails)}")
        for l in wins + fails:
            print(f"    {l[:300]}")
        print(f"  HID handshakes: {len(self.switch_handshakes)}")
        for h in self.switch_handshakes[:6]:
            print(f"    {h[:200]}")
        print(f"  start-up lines ({len(self.switch_lines)}, first 60):")
        for l in self.switch_lines[:60]:
            print(f"    {l[:300]}")

    def print_trace(self):
        conns = [c for c in self.conns if c["kinds"]]
        print(f"---- button trace: {len(conns)} connection(s) with trace lines, skipped by the cap={self.trace_skipped}, full traces asked={self.trace_starts}")
        for i, c in enumerate(conns, 1):
            kinds = " ".join(f"{k}={n}" for k, n in sorted(c["kinds"].items()))
            print(f"connection {i}: layout={c['layout']} lines: {kinds}")
            print(f"  distinct digital states: {len(c['states'])} (held | unmapped : lines)")
            for (held, un), n in list(c["states"].items())[:40]:
                print(f"    {held} | {un or '-'} : {n}")
            if len(c["states"]) > 40:
                print(f"    ... {len(c['states']) - 40} more")
            if c["unmapped"]:
                print("  unmapped items seen (lines):")
                for item, n in sorted(c["unmapped"].items()):
                    print(f"    {item} : {n}")
            else:
                print("  unmapped items seen: none")
            same = " ".join(f"id {k}: {n}" for k, n in sorted(c["same"].items())) or "none"
            print(f"  \"same\" lines (identical report after a silence): {same}")

    def code(self):
        if self.lines == 0:
            return 2
        refused = self.mem and self.mem[-1].get("heap_refused", "0") != "0"
        return 1 if self.panic or refused else 0


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


def echo_wanted(line, show_all):
    if show_all:
        return True
    tag = line.split(" ", 1)[0]
    if tag in WATCH:
        return True
    if tag == "PAD-SDP" and (" done records=" in line or line.startswith("PAD-SDP device-id") or line.startswith("PAD-SDP descriptor bytes")):
        return True
    if tag == "PAD-HID":
        return True
    if " timeout:" in line or " waiting:" in line:
        return True
    if tag == "PAD-HCI" and (" found gamepad " in line or "connection-complete" in line or "disconnection-complete" in line or "inquiry-complete" in line):
        return True
    return "panicked" in line or "Exception" in line


def read_port(path, secs, reset, save, report, show_all):
    import select

    fd = open_port(path)
    out = open(save, "w", encoding="utf-8", errors="replace") if save else None
    try:
        set_lines(fd, dtr=False, rts=False)
        if reset:
            set_lines(fd, dtr=False, rts=True)
            time.sleep(0.12)
            set_lines(fd, dtr=False, rts=False)
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
                line = ANSI.sub("", raw.decode("utf-8", errors="replace")).rstrip("\r")
                if out:
                    out.write(line + "\n")
                report.feed(line)
                if echo_wanted(line, show_all):
                    print(hide(line)[:300], flush=True)
    finally:
        if out:
            out.close()
        os.close(fd)


def main():
    p = argparse.ArgumentParser(description="Summarise the bt-gamepad-test serial log.", epilog=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("log", nargs="?", help="a log file saved earlier")
    p.add_argument("--port", help="read from this serial port instead")
    p.add_argument("--secs", type=float, default=60, help="with --port: how long to read (at most 145)")
    p.add_argument("--reset", action="store_true", help="with --port: restart the board first")
    p.add_argument("--save", help="with --port: also write the raw log here")
    p.add_argument("--all", action="store_true", help="echo every line (addresses still hidden)")
    a = p.parse_args()
    report = Report()
    if a.port:
        read_port(a.port, min(a.secs, 145), a.reset, a.save, report, a.all)
    elif a.log:
        with open(a.log, encoding="utf-8", errors="replace") as f:
            for line in f:
                report.feed(line)
    else:
        p.error("give a log file or --port")
    report.print()
    sys.exit(report.code())


if __name__ == "__main__":
    main()
