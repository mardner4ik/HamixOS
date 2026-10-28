echo "hxtest: boot without initfs: the built-in AHCI driver keeps the live root, the ahci module must not take it over"
for p in /usr/bin/bash /usr/bin/busybox /usr/libexec/hamix/hxsvc /lib/ld.hx /usr/lib/libhxc.so; do
    a=$(cat $p | wc -c)
    b=$(cat $p | wc -c)
    [ "$a" -gt 1000 ] && [ "$a" = "$b" ] && echo "PASS read $p ($a bytes)" || echo "FAIL $p: read $a then $b bytes"
done
dmesg | grep -q "module ahci: skipped" && echo "PASS the ahci module left the built-in controller alone" || echo "FAIL the ahci module was not skipped: $(dmesg | grep -i ahci | tail -3)"
bash -c 'echo PASS bash runs'
echo "PASS done"
