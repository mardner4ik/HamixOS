echo "hxtest: LightDM with the GTK greeter on Xorg, sessions list with Nook, autologin into Nook"
ls /usr/share/wayland-sessions
grep -q "^Name=Nook" /usr/share/wayland-sessions/nook.desktop && echo "PASS Nook is in the session list" || echo "FAIL no nook.desktop"
hxctl start dbus-system
for i in $(seq 1 20); do [ -S /run/dbus/system_bus_socket ] && break; sleep 0.5; done
[ -S /run/dbus/system_bus_socket ] && echo "PASS system D-Bus" || echo "FAIL no system bus"
dbus-send --system --print-reply --dest=org.freedesktop.DBus / org.freedesktop.DBus.ListNames > /tmp/names 2>&1 && echo "PASS dbus-send to the system bus" || { echo "FAIL dbus-send"; cat /tmp/names; hxctl log dbus-system | tail -20; }
timeout 10 gdbus call --system --dest org.freedesktop.DBus --object-path / --method org.freedesktop.DBus.GetId > /tmp/gdbus 2>&1 && echo "PASS gdbus on the system bus" || { echo "FAIL gdbus: $(cat /tmp/gdbus)"; }
G_DBUS_DEBUG=transport,authentication timeout 10 gdbus call --system --dest org.freedesktop.DBus --object-path / --method org.freedesktop.DBus.GetId 2>&1 | tail -30
hxctl start lightdm
greeter=""
for i in $(seq 1 40); do
    greeter=$(ps | grep -v grep | grep lightdm-gtk-gr)
    [ -n "$greeter" ] && break
    sleep 1
done
[ -n "$greeter" ] || { syscalltrace lightdm; sleep 0.3; syscalltrace ""; hxctl prof reset; hxctl prof start; sleep 1; hxctl prof stop; hxctl prof report | grep -E "^(uip|u) " | sed 's/^/prof: /'; for q in $(hxctl prof report | grep "^u " | awk '{print $2}'); do grep -E "^(Name|Tgid|PPid|State|HamixWait)" /proc/$q/status | tr '\n' ' ' | sed "s/^/proc $q: /"; echo; done; for p in $(ps | grep -v grep | grep -E " lightdm$" | awk '{print $1}'); do ls /proc/$p/task | tr '\n' ' ' | sed 's/^/tasks: /'; echo; cat /proc/$p/maps | sed 's/^/maps: /'; done; }
[ -n "$greeter" ] || for p in $(ps | grep -v grep | grep -E " lightdm$| X$| dbus-daemon$" | awk '{print $1}'); do for t in /proc/$p/task/*; do grep -E "^(Name|Pid|State|HamixWait)" $t/status | tr '\n' ' ' | sed 's/^/thread: /'; echo; done; done
[ -n "$greeter" ] && echo "PASS the greeter is running" || { echo "FAIL no greeter"; tail -30 /var/log/lightdm/lightdm.log; grep -v strtod_l /var/log/lightdm/seat0-greeter.log 2>/dev/null | sed 's/^/greeter-log: /' | tail -40; tail -20 /var/log/lightdm/x-0.log 2>/dev/null; }
echo "greeter: screen ready"
sleep ${GREETER_SECONDS:-6}
grep -E "Seat seat0|Greeter|Session|error|Error" /var/log/lightdm/lightdm.log | tail -15
hxctl stop lightdm
for i in $(seq 1 20); do ps | grep -v grep | grep -qE " X$| lightdm$" || break; sleep 0.5; done
ps | grep -v grep | grep -qE " X$" && echo "FAIL X outlived LightDM" || echo "PASS LightDM stopped its X server"
mkdir -p /etc/lightdm/lightdm.conf.d
printf '[Seat:*]\nautologin-user=user\nautologin-session=nook\n' > /etc/lightdm/lightdm.conf.d/90-test-autologin.conf
hxctl start lightdm
session=""
for i in $(seq 1 40); do
    session=$(ps | grep -v grep | grep hxserver)
    [ -n "$session" ] && break
    sleep 1
done
[ -n "$session" ] && echo "PASS autologin started Nook: $session" || { echo "FAIL Nook did not start"; grep -v "Loading conf" /var/log/lightdm/lightdm.log | head -45 | sed 's/^/ldm: /'; ls -la /run/user /run/hamix | sed 's/^/runtime: /'; cat /run/user/1000/nook-session.log 2>/dev/null | sed 's/^/nook-log: /'; dmesg | grep -E "display:|drm" | tail -5 | sed 's/^/kmsg: /'; ps | sed 's/^/ps: /'; grep -v strtod_l /home/user/.xsession-errors 2>/dev/null | sed 's/^/session-log: /' | tail -25; }
ps | grep -E "hxserver" | grep -v grep | head -3
echo "PASS done"
