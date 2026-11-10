#!/usr/bin/env python3
"""Press a key in a niri-dash started with NIRI_DASH_TEST_KEYS=1, over its control socket.

Usage: atspi_key.py SOCKET KEY      (KEY as the dashboard names keys: j, ctrl+h, esc, shift+J)
"""
import socket
import sys

s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
s.sendall(("key " + sys.argv[2] + "\n").encode())
