#!/usr/bin/env python3
"""A screen reader in miniature, for tests/atspi.rs: libatspi, the library Orca is built on,
reading the dashboard over a real accessibility bus.

Usage: atspi_client.py APP_PREFIX
Prints one line of JSON about what it found and did; exits 1 if the dashboard never showed up.
"""
import json
import sys
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


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


def walk(node):
    yield node
    for c in kids(node):
        yield from walk(c)


def name(node):
    try:
        return node.get_name() or ""
    except Exception:
        return ""


def role(node):
    try:
        return node.get_role_name()
    except Exception:
        return ""


def states(node):
    try:
        s = node.get_state_set()
        return {t.value_nick for t in Atspi.StateType.__enum_values__.values() if s.contains(t)}
    except Exception:
        return set()


def find(app, r, n):
    for node in walk(app):
        if role(node) == r and name(node) == n:
            return node
    return None


def main():
    prefix = sys.argv[1]
    Atspi.init()
    deadline = time.time() + 40
    app = None
    while time.time() < deadline:
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            a = desktop.get_child_at_index(i)
            if a is not None and name(a).startswith(prefix) and find(a, "toggle button", "Wi-Fi"):
                app = a
        if app is not None:
            break
        time.sleep(0.3)
    if app is None:
        # Say what was there, to see what went wrong.
        seen = []
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            a = desktop.get_child_at_index(i)
            if a is None:
                continue
            ns = list(walk(a))
            seen.append({"app": name(a), "nodes": len(ns), "roles": sorted({role(n) for n in ns})[:30],
                         "names": sorted({name(n) for n in ns if name(n)})[:20]})
        print(json.dumps({"error": "the dashboard never showed up on the bus", "apps": seen}))
        return 1

    nodes = list(walk(app))
    result = {
        "app": name(app),
        "nodes": len(nodes),
        "roles": sorted({role(n) for n in nodes}),
    }

    switch = find(app, "toggle button", "Wi-Fi")
    result["switch_states"] = sorted(states(switch))
    actions = []
    try:
        for i in range(Atspi.Action.get_n_actions(switch)):
            actions.append(Atspi.Action.get_action_name(switch, i))
    except Exception as e:
        actions.append(f"error: {e}")
    result["switch_actions"] = actions
    try:
        result["did_action"] = bool(Atspi.Action.do_action(switch, 0))
    except Exception as e:
        result["did_action"] = f"error: {e}"

    knob = find(app, "slider", "Brightness")
    if knob is not None:
        try:
            result["slider"] = {
                "value": Atspi.Value.get_current_value(knob),
                "min": Atspi.Value.get_minimum_value(knob),
                "max": Atspi.Value.get_maximum_value(knob),
            }
        except Exception as e:
            result["slider"] = {"error": str(e)}
        try:
            result["grabbed_focus"] = bool(Atspi.Component.grab_focus(knob))
        except Exception as e:
            result["grabbed_focus"] = f"error: {e}"
        time.sleep(1.0)
        result["slider_states_after_grab"] = sorted(states(knob))
    else:
        result["slider"] = None

    print(json.dumps(result))
    return 0


sys.exit(main())
