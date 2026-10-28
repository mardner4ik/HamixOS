echo "hxtest: hxbootctl edits a copy of /boot/hxboot.cfg and finds the Windows partition of a second disk"
mkdir -p /tmp/t/boot
cp /boot/hxboot.cfg /tmp/t/boot/hxboot.cfg
touch /tmp/t/boot/kernel.bin /tmp/t/boot/initfs.tar /tmp/t/boot/hext.img
f=/tmp/t/boot/hxboot.cfg
hxbootctl -f $f list | grep -q "HamixOS 0.7 Live" && echo "PASS list" || echo "FAIL list"
hxbootctl -f $f add "Test kernel" /boot/kernel.bin root=live quiet > /dev/null
grep -q "kernel /boot/kernel.bin root=live quiet" $f && echo "PASS add" || echo "FAIL add"
hxbootctl -f $f module "Test kernel" /boot/initfs.tar initfs > /dev/null && grep -q "module /boot/initfs.tar initfs" $f && echo "PASS module" || echo "FAIL module"
hxbootctl -f $f default "Test kernel" > /dev/null
grep -q "^default 4" $f && echo "PASS default" || echo "FAIL default: $(grep ^default $f)"
hxbootctl -f $f timeout 7 > /dev/null && grep -q "^timeout 7" $f && echo "PASS timeout" || echo "FAIL timeout"
hxbootctl -f $f probe --add
grep -q "^chain signature=1a2b3c4d partition=2" $f && echo "PASS probe found Windows" || echo "FAIL probe: $(hxbootctl -f $f probe 2>&1)"
hxbootctl -f $f list
hxbootctl -f $f list | tail -1 | grep -q "Reboot" && echo "PASS Reboot stays last" || echo "FAIL Reboot is not last"
hxbootctl -f $f check > /tmp/check.txt
grep -q "FAIL" /tmp/check.txt && echo "FAIL check: $(grep FAIL /tmp/check.txt)" || echo "PASS check"
hxbootctl -f $f rename "Test kernel" Renamed > /dev/null
grep -q "^entry Renamed" $f && echo "PASS rename" || echo "FAIL rename"
hxbootctl -f $f remove Renamed > /dev/null
grep -q "Renamed" $f && echo "FAIL remove" || echo "PASS remove"
grep -q "^# hxboot" $f && echo "PASS comments kept" || echo "FAIL comments lost"
[ -f $f.bak ] && echo "PASS backup" || echo "FAIL backup"
hxbootctl -f /nonexistent/hxboot.cfg list 2> /dev/null && echo "FAIL missing file accepted" || echo "PASS missing file refused"
echo "PASS done"
