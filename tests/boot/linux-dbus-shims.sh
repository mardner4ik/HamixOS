echo "hxtest: login1, UPower and polkit shims on the native system bus"
mkdir -p /run/dbus
[ -e /run/dbus/system_bus_socket ] || hxctl start dbus-system > /tmp/dbus-system.log 2>&1
for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /run/dbus/system_bus_socket ] && break; sleep 0.3; done
if [ -e /run/dbus/system_bus_socket ]; then echo "PASS native dbus-daemon --system"; else echo "FAIL no system bus"; cat /tmp/dbus-system.log; exit 1; fi
for shim in hx-login1 hx-upower hx-polkit; do /usr/libexec/hamix/$shim --once & done
sleep 2
call() { dbus-send --system --print-reply "$@" 2>&1; }
out=$(call --dest=org.freedesktop.UPower /org/freedesktop/UPower org.freedesktop.DBus.Properties.Get string:org.freedesktop.UPower string:OnBattery)
case "$out" in *"boolean false"*) echo "PASS UPower OnBattery is false" ;; *) echo "FAIL UPower OnBattery: $out" ;; esac
out=$(call --dest=org.freedesktop.UPower /org/freedesktop/UPower org.freedesktop.UPower.EnumerateDevices)
case "$out" in *line_power_AC*) echo "PASS UPower lists the AC adapter" ;; *) echo "FAIL UPower EnumerateDevices: $out" ;; esac
out=$(call --dest=org.freedesktop.UPower /org/freedesktop/UPower/devices/DisplayDevice org.freedesktop.DBus.Properties.GetAll string:org.freedesktop.UPower.Device)
case "$out" in *IsPresent*"boolean false"*) echo "PASS UPower display device has no battery" ;; *) echo "FAIL UPower DisplayDevice: $out" ;; esac
out=$(call --dest=org.freedesktop.PolicyKit1 /org/freedesktop/PolicyKit1/Authority org.freedesktop.DBus.Properties.Get string:org.freedesktop.PolicyKit1.Authority string:BackendName)
case "$out" in *hx-polkit*) echo "PASS polkit backend is hx-polkit" ;; *) echo "FAIL polkit BackendName: $out" ;; esac
out=$(call --dest=org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager.CanPowerOff)
case "$out" in *'"yes"'*) echo "PASS login1 CanPowerOff through the shared hxdbus crate" ;; *) echo "FAIL login1: $out" ;; esac
/usr/lib/hamix/tests/bin/polkit-check && echo "PASS polkit CheckAuthorization answers root and a user" || echo "FAIL polkit-check"
