echo "hxtest: pantry installs an Alpine package into the shared tree"
for i in 1 2 3 4 5 6 7 8 9 10; do if pantry update; then break; fi; sleep 3; done
pantry install lua5.4
if test -x /linux/usr/bin/lua5.4; then echo "PASS lua5.4 installed into /linux"; else echo "FAIL lua5.4 is not in /linux/usr/bin"; fi
if test -e /usr/bin/lua5.4; then echo "FAIL lua5.4 leaked into the native /usr/bin"; else echo "PASS native /usr/bin stays clean"; fi
linux lua5.4 -e "print('PASS lua runs: ' .. (6*7))"
pantry owner /usr/bin/lua5.4
pantry layer linux
pantry lint
if test -e /opt/linux; then echo "FAIL /opt/linux appeared"; else echo "PASS no /opt/linux"; fi
pantry provider so:libudev.so.1 > /tmp/udev.txt
/linux/bin/busybox cat /tmp/udev.txt
if /linux/bin/busybox grep -q libudev-zero /tmp/udev.txt; then echo "PASS libudev-zero is the libudev provider"; else echo "FAIL libudev provider is not libudev-zero"; fi
