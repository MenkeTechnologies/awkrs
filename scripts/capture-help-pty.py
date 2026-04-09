#!/usr/bin/env python3
"""Run awkrs -h under a PTY so ANSI colors are captured; write raw bytes to stdout.

Clears NO_COLOR for the child: many shells export NO_COLOR=1, which would make
awkrs emit plain text even though stdout is a TTY.
"""
import array
import fcntl
import os
import pty
import sys
import termios


def main() -> None:
    if len(sys.argv) != 2:
        print("usage: capture-help-pty.py /path/to/awkrs", file=sys.stderr)
        sys.exit(2)
    bin_path = sys.argv[1]
    master, slave = pty.openpty()
    rows, cols = 48, 256
    buf = array.array("H", [rows, cols, 0, 0])
    fcntl.ioctl(master, termios.TIOCSWINSZ, buf)
    pid = os.fork()
    if pid == 0:
        os.close(master)
        os.setsid()
        os.dup2(slave, 0)
        os.dup2(slave, 1)
        os.dup2(slave, 2)
        os.close(slave)
        os.environ.setdefault("TERM", "xterm-256color")
        os.environ.pop("NO_COLOR", None)
        os.execv(bin_path, [bin_path, "-h"])
        sys.exit(1)
    os.close(slave)
    try:
        while True:
            chunk = os.read(master, 65536)
            if not chunk:
                break
            sys.stdout.buffer.write(chunk)
    except OSError:
        pass
    os.waitpid(pid, 0)


if __name__ == "__main__":
    main()
