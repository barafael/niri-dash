#!/usr/bin/python3
"""Starts a system bus and a session bus of their own, puts the mocked services niri-dash talks to
on them (NetworkManager with a Wi-Fi device, BlueZ with an adapter, UPower with a battery), then
runs the command it is given, with the buses in its environment, and exits with its status."""

import subprocess
import sys

import dbus
from dbusmock import MOCK_IFACE, DBusTestCase as T

NM_DEVICE_STATE_ACTIVATED = 100
NM_802_11_MODE_INFRA = 2


def main(command):
    T.start_system_bus()
    T.start_session_bus()
    quiet = subprocess.DEVNULL

    _, nm = T.spawn_server_template("networkmanager", {}, stdout=quiet)
    wifi = nm.AddWiFiDevice("mock_wlan0", "wlan0", NM_DEVICE_STATE_ACTIVATED, dbus_interface=MOCK_IFACE)
    for i, (ssid, strength) in enumerate([("Lighthouse", 80), ("Harbour Cafe", 54), ("Neighbour", 31)]):
        nm.AddAccessPoint(
            wifi, f"mock_ap{i}", ssid, f"00:23:F8:7E:12:B{i}", NM_802_11_MODE_INFRA, 2425, 5400, strength, 0,
            dbus_interface=MOCK_IFACE,
        )

    _, bluez = T.spawn_server_template("bluez5", {}, stdout=quiet)
    bluez.AddAdapter("hci0", "mock-computer", dbus_interface="org.bluez.Mock")
    bluez.AddDevice("hci0", "11:22:33:44:55:66", "Mock Keys", dbus_interface="org.bluez.Mock")

    _, upower = T.spawn_server_template("upower", {}, stdout=quiet)
    # The machine's own battery comes from sysfs, which a container can't fake; what UPower adds
    # is the batteries of mice, keyboards and the like.
    upower.AddObject(
        "/org/freedesktop/UPower/devices/mock_mouse",
        "org.freedesktop.UPower.Device",
        {
            "PowerSupply": dbus.Boolean(False),
            "IsPresent": dbus.Boolean(True),
            "Model": dbus.String("Mock Mouse"),
            "Percentage": dbus.Double(55.0),
            "State": dbus.UInt32(2),
            "Type": dbus.UInt32(5),
        },
        [],
        dbus_interface=MOCK_IFACE,
    )

    return subprocess.call(command)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit("usage: mocks.py COMMAND [ARGS...]")
    sys.exit(main(sys.argv[1:]))
