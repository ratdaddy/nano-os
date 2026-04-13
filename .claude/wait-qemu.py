#!/usr/bin/env python3
"""Waits for a new QEMU session to complete by watching for QEMU: Terminated
in the qemu log, only triggering on content written after this script starts."""
import os, time, sys

log = sys.argv[1] if len(sys.argv) > 1 else '/tmp/nano-os-qemu.log'

# Remove the log file so any content we see is guaranteed to be from a new session.
try:
    os.remove(log)
except FileNotFoundError:
    pass

while True:
    try:
        if 'QEMU: Terminated' in open(log).read():
            break
    except FileNotFoundError:
        pass
    time.sleep(2)
