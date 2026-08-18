#!/usr/bin/env python3
"""Runs a command and prints its peak resident set size in kilobytes.

os.wait4 reports rusage for one child, which is what is needed here.
getrusage(RUSAGE_CHILDREN) reports the maximum over every child the caller has
ever reaped, so a second measurement in the same process silently returns the
first one's number.
"""
import os
import sys

if len(sys.argv) < 2:
    sys.exit("usage: peak-rss.py <command> [args...]")

pid = os.fork()
if pid == 0:
    devnull = os.open(os.devnull, os.O_WRONLY)
    os.dup2(devnull, 1)
    os.execvp(sys.argv[1], sys.argv[1:])

_, status, usage = os.wait4(pid, 0)
if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 0:
    sys.exit(f"peak-rss: {sys.argv[1]} did not exit cleanly (status {status})")
print(usage.ru_maxrss)
