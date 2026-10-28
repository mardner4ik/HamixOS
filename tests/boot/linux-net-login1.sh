echo "hxtest: hx-login1 on the system bus"
for i in 1 2 3 4 5 6 7 8 9 10; do if pantry update; then break; fi; sleep 3; done
pantry install dbus
/linux/bin/busybox mkdir -p /run/dbus
/usr/bin/dbus-daemon --system --nofork --nopidfile &
sleep 2
if test -e /run/dbus/system_bus_socket; then echo "PASS the system bus is up"; else echo "FAIL no system bus socket"; fi
/usr/libexec/hamix/hx-login1 --once &
sleep 7
/usr/bin/dbus-send --system --print-reply --dest=org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager.CanPowerOff > /tmp/l1.txt
/linux/bin/busybox cat /tmp/l1.txt
if /linux/bin/busybox grep -q '"yes"' /tmp/l1.txt; then echo "PASS CanPowerOff answers yes"; else echo "FAIL CanPowerOff"; fi
/usr/bin/dbus-send --system --print-reply --dest=org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager.ListSessions > /tmp/l2.txt
/linux/bin/busybox cat /tmp/l2.txt
if /linux/bin/busybox grep -q 'seat0' /tmp/l2.txt; then echo "PASS ListSessions lists a session on seat0"; else echo "FAIL ListSessions"; fi
/usr/bin/dbus-send --system --print-reply --dest=org.freedesktop.login1 /org/freedesktop/login1/seat/seat0 org.freedesktop.DBus.Properties.Get string:org.freedesktop.login1.Seat string:CanGraphical > /tmp/l3.txt
/linux/bin/busybox cat /tmp/l3.txt
if /linux/bin/busybox grep -q 'boolean true' /tmp/l3.txt; then echo "PASS seat0 CanGraphical is true"; else echo "FAIL seat0 properties"; fi
/usr/bin/dbus-send --system --print-reply --dest=org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.DBus.Introspectable.Introspect > /tmp/l4.txt
if /linux/bin/busybox grep -q 'org.freedesktop.login1.Manager' /tmp/l4.txt; then echo "PASS introspection names the Manager interface"; else echo "FAIL introspection"; fi
/usr/bin/dbus-send --system --print-reply --dest=org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager.Suspend boolean:false > /tmp/l5.txt
/linux/bin/busybox cat /tmp/l5.txt
/usr/bin/dbus-send --system --print-reply --dest=org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager.Inhibit string:sleep string:hxtest string:checking string:block > /tmp/l6.txt
/linux/bin/busybox cat /tmp/l6.txt
if /linux/bin/busybox grep -q 'file descriptor' /tmp/l6.txt; then echo "PASS Inhibit hands out a file descriptor"; else echo "FAIL Inhibit"; fi
echo "PASS done"
