echo "hxtest: Mesa (softpipe) on hxlibc"
if [ ! -x /usr/lib/hamix/tests/bin/egl-triangle ]; then
    echo "FAIL egl-triangle was not built"
    exit 0
fi
/usr/lib/hamix/tests/bin/egl-triangle || echo "FAIL egl-triangle exited with $?"
/usr/lib/hamix/tests/bin/egl-desktop || echo "FAIL egl-desktop exited with $?"
if [ -x /usr/lib/hamix/tests/bin/gtk-gl ]; then
    hxserver &
    sleep 8
    XDG_RUNTIME_DIR=/tmp/runtime-$(id -u) WAYLAND_DISPLAY=wayland-0 GTK_GL_SECONDS=8 /usr/lib/hamix/tests/bin/gtk-gl
    status=$?
    [ $status -eq 0 ] || echo "FAIL gtk-gl exited with $status"
else
    echo "FAIL gtk-gl was not built"
fi
