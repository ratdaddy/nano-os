#!/usr/bin/env python3
"""Waits for a new QEMU session to complete by watching for QEMU: Terminated
in the qemu log, only triggering on content written after this script starts."""
import os, time, sys

log = sys.argv[1] if len(sys.argv) > 1 else '/tmp/nano-os-qemu.log'
t = time.time()
while True:
    try:
        if os.path.getmtime(log) > t and 'QEMU: Terminated' in open(log).read():
            break
    except:
        pass
    time.sleep(2)
