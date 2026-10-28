echo "hxtest: libhxproto.so used from C (pkg-config hxproto)"
ls -l /usr/lib/libhxproto.so.0 || echo "FAIL /usr/lib/libhxproto.so.0 is missing"
hxserver > /tmp/hxserver.log 2>&1 &
for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /run/hamix/services/hxserver ] && break; sleep 1; done
sleep 1
/usr/lib/hamix/tests/bin/hxproto-window || echo "FAIL hxproto-window exited with $?"
echo "PASS done"
