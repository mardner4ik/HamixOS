echo "hxtest: USB flash drive (UHCI, Bulk-Only Transport, SCSI) with a FAT32 filesystem"
dev=""
for i in $(seq 1 20); do
    for d in sda sdb sdc; do
        grep -q " $d\$" /proc/partitions && dev=$d
    done
    [ -n "$dev" ] && break
    sleep 1
done
dmesg | grep -E "usb-storage|usb:" | tail -5
[ -n "$dev" ] && echo "PASS USB disk /dev/$dev appeared" || { echo "FAIL no USB disk"; cat /proc/partitions; echo "PASS done"; exit 0; }
sleep 2
hxmount unmount /dev/$dev >/dev/null 2>&1
mkdir -p /media/stick
mount /dev/$dev /media/stick && echo "PASS mount /dev/$dev" || echo "FAIL mount /dev/$dev"
grep "vfat" /proc/mounts | grep -q HXSTICK && echo "PASS label HXSTICK" || echo "FAIL label: $(cat /proc/mounts)"
[ "$(cat /media/stick/readme.txt 2>/dev/null)" = "HamixOS media test" ] && echo "PASS small file over USB" || echo "FAIL small file: $(ls -la /media/stick)"
[ "$(wc -l < "/media/stick/Photos/Holiday 2026/A rather long file name.txt")" = "20000" ] && echo "PASS 170 KiB file over USB" || echo "FAIL big file over USB"
umount /media/stick && echo "PASS umount" || echo "FAIL umount"
echo "PASS done"
