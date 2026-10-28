echo "hxtest: native Xwayland and X11 clients on hxlibc"
for p in /usr/bin/Xwayland /usr/bin/xkbcomp; do
    if [ -x $p ]; then echo "PASS $p"; else echo "FAIL $p is missing"; fi
done
hxserver &
sleep 8
DISPLAY=:0 /usr/lib/hamix/tests/bin/x11-window
status=$?
[ $status -eq 0 ] || { echo "FAIL x11-window exited with $status"; cat /tmp/xwayland.log; }
if [ -x /usr/bin/dillo ]; then
    printf '<html><head><title>HamixOS</title></head><body><h1>dillo on hxlibc</h1><p>Native FLTK and X11 through Xwayland.</p><ul><li>one</li><li>two</li></ul></body></html>\n' > /tmp/dillo.html
    DISPLAY=:0 HOME=/tmp dillo /tmp/dillo.html > /tmp/dillo.log 2>&1 &
    pid=$!
    sleep ${DILLO_SECONDS:-8}
    if kill -0 $pid 2>/dev/null; then echo "PASS dillo is running"; kill $pid; else echo "FAIL dillo exited"; cat /tmp/dillo.log; fi
else
    echo "FAIL /usr/bin/dillo is missing"
fi
