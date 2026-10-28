echo "hxtest: programs started from Enlightenment (Wayland, GTK, X11, Nook apps)"
if [ ! -x /usr/bin/enlightenment-session ]; then
    echo "FAIL enlightenment is not installed"
    exit 0
fi
export XDG_RUNTIME_DIR=/tmp/runtime-e
export HOME=/root
startx enlightenment > /tmp/e.log 2>&1 &
for i in $(seq 1 60); do
    [ -S $XDG_RUNTIME_DIR/wayland-0 ] && break
    sleep 1
done
sleep 15
pid=""
for p in /proc/[0-9]*; do [ "$(cat $p/comm 2>/dev/null)" = enlightenment ] && pid=${p#/proc/}; done
[ -n "$pid" ] && echo "PASS enlightenment pid $pid" || { echo "FAIL enlightenment is not running"; tail -40 /tmp/e.log; exit 0; }
echo "--- enlightenment environment"
tr '\0' '\n' < /proc/$pid/environ | grep -E '^(WAYLAND_DISPLAY|DISPLAY|XDG_RUNTIME_DIR|XDG_SESSION_TYPE|GDK_BACKEND|DBUS_SESSION_BUS_ADDRESS|PATH)='
run_like_e() {
    name=$1
    shift
    env -i $(tr '\0' '\n' < /proc/$pid/environ | grep -E '^[A-Za-z_][A-Za-z0-9_]*=' | tr '\n' ' ') "$@" > /tmp/app-$name.log 2>&1 &
    app=$!
    sleep 8
    if kill -0 $app 2>/dev/null; then
        echo "PASS $name keeps running when started with the Enlightenment environment"
        kill $app 2>/dev/null
    else
        wait $app
        echo "FAIL $name exited ($?): $(tail -5 /tmp/app-$name.log | tr '\n' ' ')"
    fi
}
run_like_e foot foot
run_like_e gtk3 env GTK_HELLO_SECONDS=30 /usr/lib/hamix/tests/bin/gtk-hello
echo "SKIP dillo (X11): EFL is built without ecore_x, so Enlightenment cannot start Xwayland yet"
run_like_e hxcalc hxcalc
run_like_e hxfiles hxfiles
echo "--- enlightenment log"
tail -30 $HOME/.e-log.log 2>/dev/null
kill $pid
echo "PASS done"
