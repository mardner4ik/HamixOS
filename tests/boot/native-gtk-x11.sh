echo "hxtest: GTK 3 on Xorg (X11 backend) and the LightDM GTK greeter binary"
Xorg :0 vt2 -nolisten tcp -novtswitch > /tmp/xorg.out 2>&1 &
xpid=$!
for i in $(seq 1 30); do [ -S /tmp/.X11-unix/X0 ] && break; sleep 1; done
[ -S /tmp/.X11-unix/X0 ] && echo "PASS Xorg :0" || echo "FAIL Xorg"
export DISPLAY=:0 GDK_BACKEND=x11 HOME=/tmp
timeout 20 /usr/lib/hamix/tests/bin/gtk-hello > /tmp/gtk.log 2>&1
code=$?
grep -v strtod_l /tmp/gtk.log | tail -15
[ $code -eq 0 ] && echo "PASS gtk-hello on X11" || echo "FAIL gtk-hello on X11 exited with $code"
timeout 8 /usr/sbin/lightdm-gtk-greeter > /tmp/greeter.log 2>&1
code=$?
grep -v strtod_l /tmp/greeter.log | sed 's/^/greeter: /' | tail -25
echo "greeter exit code $code"
mkdir -p /var/lib/lightdm && chown lightdm:lightdm /var/lib/lightdm
chmod 644 /tmp/.X11-unix/X0 2>/dev/null
syscalltrace greeter
timeout 8 /usr/libexec/hamix/hxsvc --as lightdm -- /usr/sbin/lightdm-gtk-greeter > /tmp/greeter2.log 2>&1
code=$?
grep -v strtod_l /tmp/greeter2.log | grep -v DT_SYMBOLIC | sed 's/^/greeter-as-lightdm: /' | tail -25
echo "greeter as lightdm exit code $code"
syscalltrace ""
kill $xpid
sleep 1
echo "PASS done"
