#!/usr/bin/env python3
"""Print the accessibility tree an application publishes on the AT-SPI bus, as libatspi (the
library Orca is built on) reads it: one line per node, indented by depth.

Usage: atspi_dump.py APP_PREFIX [WAIT_SECONDS]      (run by scripts/atspi-dump.sh, on a private bus)

Each line: ROLE "name" [states] value=.. description=.. actions=[..]
The states shown are the ones a screen reader says or follows: focused, selected, checked,
pressed, expanded, insensitive (disabled). Exits 1 if the application never shows up.
"""
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

SHOWN = {"focused", "selected", "checked", "pressed", "expanded", "collapsed", "checkable", "editable", "active"}


def kids(node):
    out = []
    try:
        for i in range(node.get_child_count()):
            c = node.get_child_at_index(i)
            if c is not None:
                out.append(c)
    except Exception:
        pass
    return out


def name(node):
    try:
        return node.get_name() or ""
    except Exception:
        return ""


def states(node):
    try:
        s = node.get_state_set()
        return sorted(t.value_nick for t in Atspi.StateType.__enum_values__.values()
                      if s.contains(t) and t.value_nick in SHOWN)
    except Exception:
        return []


def extras(node):
    out = []
    try:
        d = node.get_description()
        if d:
            out.append(f"description={d!r}")
    except Exception:
        pass
    try:
        interfaces = node.get_interfaces()
    except Exception:
        interfaces = []
    if "Value" in interfaces:
        try:
            v = Atspi.Value.get_current_value(node)
            lo = Atspi.Value.get_minimum_value(node)
            hi = Atspi.Value.get_maximum_value(node)
            out.append(f"value={v:g} ({lo:g}..{hi:g})")
        except Exception:
            pass
    if "Text" in interfaces:
        try:
            t = Atspi.Text.get_text(node, 0, Atspi.Text.get_character_count(node))
            if t:
                out.append(f"text={t!r}")
        except Exception:
            pass
    if "Action" in interfaces:
        try:
            n = Atspi.Action.get_n_actions(node)
            if n:
                out.append("actions=" + ",".join(Atspi.Action.get_action_name(node, i) for i in range(n)))
        except Exception:
            pass
    return out


def dump(node, depth, out):
    try:
        role = node.get_role_name()
    except Exception:
        role = "?"
    st = states(node)
    line = "  " * depth + f"{role} {name(node)!r}"
    if st:
        line += " [" + ",".join(st) + "]"
    ex = extras(node)
    if ex:
        line += " " + " ".join(ex)
    out.append(line)
    for c in kids(node):
        dump(c, depth + 1, out)


def main():
    prefix = sys.argv[1]
    wait = float(sys.argv[2]) if len(sys.argv) > 2 else 40
    Atspi.init()
    deadline = time.time() + wait
    last = None
    while time.time() < deadline:
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            a = desktop.get_child_at_index(i)
            if a is not None and name(a).startswith(prefix):
                # A tree takes a moment to fill: wait until it has some nodes.
                lines = []
                dump(a, 0, lines)
                last = lines
                if len(lines) > 5:
                    print("\n".join(lines))
                    return 0
        time.sleep(0.5)
    if last:
        # Something is there, if little: show it (a widget that shows nothing on this system).
        print("\n".join(last))
        return 0
    print(f"{prefix}: never showed up on the accessibility bus", file=sys.stderr)
    return 1


sys.exit(main())
