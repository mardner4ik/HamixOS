echo "hxtest: safe mode (nomodules): the live system must work with the built-in drivers only"
grep -q nomodules /proc/cmdline && echo "PASS booted with nomodules" || echo "FAIL nomodules is not on the command line"
for p in /usr/bin/bash /usr/bin/busybox /usr/libexec/hamix/hxsvc /lib/ld.hx /usr/lib/libhxc.so; do
    a=$(cat $p | wc -c)
    b=$(cat $p | wc -c)
    [ "$a" -gt 1000 ] && [ "$a" = "$b" ] && echo "PASS read $p ($a bytes)" || echo "FAIL $p: read $a then $b bytes"
done
ls -R /usr/share > /dev/null && echo "PASS listed /usr/share"
bash -c 'echo PASS bash runs'
echo "PASS done"
