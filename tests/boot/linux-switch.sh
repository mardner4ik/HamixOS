echo "hxtest: Linux layer switch"
hxctl linux status
/linux/bin/busybox echo "PASS busybox runs while the layer is on"
hxctl linux off
hxctl linux status
if /linux/bin/busybox echo "FAIL busybox ran with the layer off"; then echo "FAIL the layer did not refuse"; else echo "PASS busybox refused while the layer is off"; fi
if test -e /dev/dri/card0; then echo "PASS /dev/dri/card0 serves native programs with the layer off"; else echo "FAIL /dev/dri/card0 vanished with the layer off"; fi
hxctl linux on
if test -e /dev/dri/card0; then echo "PASS /dev/dri/card0 is still there"; else echo "FAIL /dev/dri/card0 is missing"; fi
/linux/bin/busybox echo "PASS busybox runs again"
