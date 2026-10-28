echo "hxtest: hamix-core has no Linux layer"
if test -x /usr/libexec/hamix/hxwayland; then echo "PASS native hxwayland"; else echo "FAIL hxwayland is missing"; fi
if test -e /linux/usr/bin/Xwayland; then echo "FAIL Xwayland is in the core image"; else echo "PASS no Xwayland"; fi
if test -e /linux; then echo "FAIL /linux is in the core image"; else echo "PASS no /linux"; fi
if test -e /linux/lib/ld-musl-x86_64.so.1 || test -e /lib/ld-musl-x86_64.so.1; then echo "FAIL musl is in the core image"; else echo "PASS no musl"; fi
if test -e /etc/pantry/repositories; then echo "FAIL Alpine repositories in the core image"; else echo "PASS no Alpine repositories"; fi
if test -e /opt/linux; then echo "FAIL /opt/linux exists"; else echo "PASS no /opt/linux"; fi
