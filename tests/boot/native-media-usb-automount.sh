echo "hxtest: CD/DVD and USB drives are mounted automatically, users can unmount them"
for i in $(seq 1 30); do
    grep -q "/media/HXSTICK" /proc/mounts && grep -q "/media/HXMEDIA" /proc/mounts && break
    sleep 1
done
cat /proc/mounts | grep media
grep -q "/media/HXMEDIA	iso9660" /proc/mounts && echo "PASS the data CD was mounted on /media/HXMEDIA" || echo "FAIL CD not automounted"
grep -q "/media/HXSTICK	vfat" /proc/mounts && echo "PASS the USB drive was mounted on /media/HXSTICK" || echo "FAIL USB drive not automounted"
hxmount list
ls -l /dev/sr* /dev/sd*


hxmount unmount /media/HXSTICK && echo "PASS hxmount unmounted the USB drive" || echo "FAIL hxmount unmount"
sleep 1
grep -q "/media/HXSTICK" /proc/mounts && echo "FAIL still mounted" || echo "PASS /media/HXSTICK is gone"
hxmount mount $(hxmount list | awk '$3 == "usb" {print $1; exit}') > /tmp/m.out 2>&1
grep -q "/media/HXSTICK" /proc/mounts && echo "PASS hxmount mounted the USB drive again ($(cat /tmp/m.out))" || echo "FAIL user mount: $(cat /tmp/m.out)"
hxserver > /tmp/hxserver.log 2>&1 &
for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /run/hamix/services/hxserver ] && break; sleep 1; done
sleep 2
hxfiles /media > /tmp/files.log 2>&1 &
sleep 4
kill -0 $! 2>/dev/null && echo "PASS Files is running" || echo "FAIL Files: $(cat /tmp/files.log)"
echo "MARK-FILES"
sleep 8
echo "PASS done"
