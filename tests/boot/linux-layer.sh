echo "hxtest: Linux layer present in the shared tree"
if test -x /usr/libexec/hamix/hxwayland; then echo "PASS hxwayland"; else echo "FAIL hxwayland is missing"; fi
if test -x /linux/bin/busybox; then echo "PASS /linux/bin/busybox"; else echo "FAIL /linux/bin/busybox is missing"; fi
if test -e /opt/linux; then echo "FAIL /opt/linux still exists"; else echo "PASS no /opt/linux"; fi
/linux/bin/busybox sh -c "echo PASS busybox sh runs"
/linux/bin/ls /bin > /tmp/ls.txt
if test -s /tmp/ls.txt; then echo "PASS busybox ls writes files"; else echo "FAIL busybox ls"; fi
/linux/bin/hello-musl
/linux/bin/hello-musl-static
pantry layer
pantry lint
