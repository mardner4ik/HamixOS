echo "hxtest: Xorg on /dev/dri/card0 (modesetting + libinput) and an X11 client"
[ -x /usr/bin/Xorg ] && echo "PASS /usr/bin/Xorg" || echo "FAIL /usr/bin/Xorg is missing"
Xorg :0 vt2 -nolisten tcp -novtswitch > /tmp/xorg.out 2>&1 &
xpid=$!
for i in $(seq 1 30); do
    [ -S /tmp/.X11-unix/X0 ] && break
    sleep 1
done
if [ -S /tmp/.X11-unix/X0 ]; then echo "PASS Xorg listens on :0"; else echo "FAIL Xorg did not start"; tail -40 /var/log/Xorg.0.log; cat /tmp/xorg.out; fi
grep -E "\(EE\)|modeset\(0\): Output|libinput: .*is a|Using input driver" /var/log/Xorg.0.log | head -20
DISPLAY=:0 /usr/lib/hamix/tests/bin/x11-window
status=$?
[ $status -eq 0 ] && echo "PASS an X11 window on Xorg" || echo "FAIL x11-window exited with $status"
kill $xpid 2>/dev/null
sleep 2
echo "PASS done"
