#!/usr/bin/env python3
"""Runs a command (the test suite) and fails if it leaves processes running.

Usage: python3 -I scripts/check-test-leaks.py [--grace SECONDS] CMD [ARG...]

Linux only (elsewhere the command just runs). This process becomes a child
subreaper (PR_SET_CHILD_SUBREAPER), so a process whose parent exits while
the command runs, such as a `sleep` started by a test's fake server that
was never killed, is reparented here instead of to init. Orphans are
listed every second; one still running GRACE seconds after it was first
seen is a leak: it is reported with its command line and killed. Orphans
that end on their own within GRACE (a shell's last short `sleep`) are
fine. Checking while the suite runs matters: a `sleep 30` leaked by an
early test would be gone by the end of a long run.

Exits with the command's status if that failed, else 1 if anything leaked.
"""

import ctypes
import os
import signal
import subprocess
import sys
import time

PR_SET_CHILD_SUBREAPER = 36


def orphans(me, skip):
    """(pid, start time, zombie?) of every process whose parent is `me`."""
    found = []
    for name in os.listdir("/proc"):
        if not name.isdigit() or int(name) == skip:
            continue
        try:
            with open(f"/proc/{name}/stat") as f:
                fields = f.read().rsplit(")", 1)[1].split()
        except (OSError, IndexError):
            continue
        # After the command name: state, ppid, ...; field 22 is start time.
        if int(fields[1]) == me:
            found.append((int(name), fields[19], fields[0] == "Z"))
    return found


def command_line(pid):
    try:
        with open(f"/proc/{pid}/cmdline", "rb") as f:
            text = f.read().replace(b"\0", b" ").decode(errors="replace")
    except OSError:
        return "?"
    return text.strip()[:200]


def main(argv):
    grace = 10.0
    if argv[:1] == ["--grace"]:
        grace, argv = float(argv[1]), argv[2:]
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    if not sys.platform.startswith("linux"):
        return subprocess.call(argv)
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0:
        print("check-test-leaks: cannot become a subreaper", file=sys.stderr)
        return subprocess.call(argv)

    me = os.getpid()
    command = subprocess.Popen(argv)
    first_seen = {}  # (pid, start time) -> monotonic time
    leaks = []

    def report(pid):
        line = command_line(pid)
        leaks.append(line)
        # The first few are enough to find the test; a bug often leaks
        # hundreds.
        if len(leaks) <= 20:
            print(f"::error::a test left this process running: {pid} {line}", flush=True)
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    def check(now):
        """Reaps finished orphans, reports those past their grace period
        and returns the pids of the others."""
        running = []
        for pid, start, zombie in orphans(me, command.pid):
            if zombie:
                try:
                    os.waitpid(pid, os.WNOHANG)
                except ChildProcessError:
                    pass
                continue
            key = (pid, start)
            if now - first_seen.setdefault(key, now) >= grace:
                report(pid)
                del first_seen[key]
            else:
                running.append(pid)
        return running

    status = None
    while status is None:
        try:
            status = command.wait(timeout=1)
        except subprocess.TimeoutExpired:
            pass
        check(time.monotonic())
    # The last tests' orphans get their grace period too; whatever runs
    # after that has outlived its test.
    end = time.monotonic()
    while True:
        now = time.monotonic()
        running = check(now)
        if not running:
            break
        if now - end > grace:
            for pid in running:
                report(pid)
            break
        time.sleep(1)

    if leaks:
        print(f"check-test-leaks: {len(leaks)} process(es) outlived their test",
              file=sys.stderr)
    if status != 0:
        return status if status > 0 else 128 - status
    return 1 if leaks else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
