echo "hxtest: OpenRC scripts under hxsvc"
mkdir -p /linux/etc/init.d
cp /usr/lib/hamix/tests/linux-openrc-demo /linux/etc/init.d/demo
chmod 755 /linux/etc/init.d/demo
/usr/libexec/hamix/hxsvc-openrc sync
/usr/libexec/hamix/hxsvc-openrc list
hxctl start openrc-demo
sleep 3
hxctl status openrc-demo
/linux/bin/busybox ash -c 'if /etc/init.d/demo status; then echo "PASS the OpenRC daemon is running under hxsvc"; else echo "FAIL the OpenRC daemon is not running"; fi'
hxctl stop openrc-demo
