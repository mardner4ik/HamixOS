echo "hxtest: GTK 3 on hxlibc through hxwayland"
if [ ! -x /usr/lib/hamix/tests/bin/gtk-hello ]; then
    echo "FAIL gtk-hello was not built"
    exit 0
fi
/usr/lib/hamix/tests/bin/gtk-hello --no-display || echo "FAIL gtk-hello --no-display exited with $?"
hxserver &
sleep 8
socket=
for s in "$XDG_RUNTIME_DIR/wayland-0" /tmp/runtime-$(id -u)/wayland-0 /run/user/$(id -u)/wayland-0; do
    [ -S "$s" ] && socket=$s && break
done
if [ -z "$socket" ]; then
    echo "FAIL hxwayland socket was not found"
    exit 0
fi
echo "PASS hxwayland listens on $socket"
XDG_RUNTIME_DIR=${socket%/*} WAYLAND_DISPLAY=wayland-0 GTK_HELLO_SECONDS=6 GTK_HELLO_THEME=Nook /usr/lib/hamix/tests/bin/gtk-hello
status=$?
[ $status -eq 0 ] || echo "FAIL gtk-hello exited with $status"
