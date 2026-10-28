echo "hxtest: block devices, sysfs, setuid sudo, reboot(2) permissions"
disk=$(ls /sys/block | head -n 1)
if [ -z "$disk" ]; then echo "FAIL /sys/block is empty"; exit 1; fi
echo "PASS /sys/block lists $disk"
[ -b /dev/$disk ] && echo "PASS /dev/$disk is a block device" || echo "FAIL /dev/$disk: $(ls -l /dev/$disk)"
sectors=$(cat /sys/block/$disk/size)
[ "$sectors" -gt 0 ] && echo "PASS $disk has $sectors sectors" || echo "FAIL size of $disk: $sectors"
[ "$(cat /sys/block/$disk/dev)" = "8:0" ] && echo "PASS dev number 8:0" || echo "FAIL dev $(cat /sys/block/$disk/dev)"
grep -q " $disk\$" /proc/partitions && echo "PASS /proc/partitions" || { echo "FAIL /proc/partitions"; cat /proc/partitions; }
bytes=$(dd if=/dev/$disk bs=512 count=4 2>/dev/null | wc -c)
[ "$bytes" = 2048 ] && echo "PASS read 4 sectors with dd" || echo "FAIL dd read $bytes bytes"
printf 'hamix-block-test' | dd of=/dev/$disk bs=1 seek=1000000 2>/dev/null
back=$(dd if=/dev/$disk bs=1 skip=1000000 count=16 2>/dev/null)
[ "$back" = hamix-block-test ] && echo "PASS unaligned write and read back" || echo "FAIL read back '$back'"
tail=$(dd if=/dev/$disk bs=512 skip=$((sectors - 1)) count=2 2>/dev/null | wc -c)
[ "$tail" = 512 ] && echo "PASS reads stop at the end of the device" || echo "FAIL tail read $tail bytes"
lsblk | grep -q "$disk" && echo "PASS lsblk reads sysfs" || { echo "FAIL lsblk"; lsblk; }
ls -l /usr/bin/sudo | grep -q '^-rwsr-xr-x' && echo "PASS sudo is setuid root" || echo "FAIL sudo mode: $(ls -l /usr/bin/sudo)"
out=$(/usr/libexec/hamix/hxsvc --as user -- /usr/bin/id -u)
[ "$out" = "$(id -u user)" ] && echo "PASS id as user" || echo "FAIL id as user: $out"
out=$(printf 'user\n' | /usr/libexec/hamix/hxsvc --as user -- /usr/bin/sudo -S /usr/bin/id -u 2>&1)
[ "$out" = 0 ] && echo "PASS sudo -S as user gives uid 0" || echo "FAIL sudo as user: $out"
out=$(printf 'wrong\nwrong\nwrong\n' | /usr/libexec/hamix/hxsvc --as user -- /usr/bin/sudo -S /usr/bin/id -u 2>&1)
[ "$out" != 0 ] && echo "PASS sudo refuses a wrong password" || echo "FAIL sudo accepted a wrong password"
useradd plain > /dev/null
out=$(/usr/libexec/hamix/hxsvc --as plain -- /usr/bin/reboot 2>&1)
echo "$out" | grep -qi "not permitted\|permission\|must be root\|only root" && echo "PASS reboot refused for a user outside the power group" || echo "FAIL reboot as plain: $out"
userdel plain
echo "PASS done"
