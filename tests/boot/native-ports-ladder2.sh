echo "hxtest: hx-ports ladder step 2 on hxlibc"
for t in posix-sockets tcp-loopback pselect-mask printf-float cxx-runtime glib-basic cairo-draw pango-text wayland-loop xkb-keymap; do
    if [ -x /usr/lib/hamix/tests/bin/$t ]; then
        /usr/lib/hamix/tests/bin/$t || echo "FAIL $t exited with $?"
    else
        echo "FAIL $t was not built"
    fi
done
if [ -x /usr/bin/dbus-daemon ]; then
    dbus-daemon --session --nofork --nopidfile --address=unix:path=/tmp/test-bus > /tmp/dbus.log 2>&1 &
    bus=$!
    for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /tmp/test-bus ] && break; sleep 0.3; done
    names=$(DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/test-bus dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.ListNames 2>&1)
    case "$names" in
        *org.freedesktop.DBus*) echo "PASS native dbus-daemon answers ListNames" ;;
        *) echo "FAIL dbus: $names"; cat /tmp/dbus.log ;;
    esac
    kill $bus
else
    echo "FAIL /usr/bin/dbus-daemon is missing"
fi
