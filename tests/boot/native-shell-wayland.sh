echo "hxtest: nook-shell as a Wayland client (layer-shell and foreign-toplevel through hxwayland)"
export XDG_RUNTIME_DIR=/tmp/runtime-$(id -u)
NOOK_SHELL=none hxserver > /tmp/hxserver.log 2>&1 &
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do [ -S $XDG_RUNTIME_DIR/wayland-0 ] && break; sleep 1; done
NOOK_PLATFORM=wayland WAYLAND_DISPLAY=wayland-0 nook-shell > /tmp/shell.log 2>&1 &
shell=$!
sleep 4
hxcalc > /dev/null 2>&1 &
sleep 6
cat /tmp/shell.log
grep -q "running on wayland" /tmp/shell.log && echo "PASS shell started on Wayland" || echo "FAIL shell did not start on Wayland"
kill -0 $shell 2>/dev/null && echo "PASS shell is alive" || echo "FAIL shell exited"
echo "PASS done"
