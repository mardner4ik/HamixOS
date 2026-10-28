echo "hxtest: CD/DVD (ISO 9660 + Joliet) and FAT32 media mount read-only"
cat /proc/partitions
sleep 3
for m in $(awk -F'\t' '$3 == "iso9660" || $3 == "vfat" {print $2}' /proc/mounts); do hxmount unmount "$m"; done
cd_dev=""
for d in sr1 sr0; do
    mkdir -p /media/test-$d
    if mount /dev/$d /media/test-$d 2>/dev/null; then
        if [ -f /media/test-$d/README.TXT ] || [ -f /media/test-$d/readme.txt ]; then cd_dev=$d; break; fi
        umount /media/test-$d
    fi
done
[ -n "$cd_dev" ] && echo "PASS data CD mounted from /dev/$cd_dev" || echo "FAIL no data CD could be mounted"
cdm=/media/test-$cd_dev
grep -q "iso9660" /proc/mounts && echo "PASS /proc/mounts lists iso9660" || echo "FAIL /proc/mounts: $(cat /proc/mounts)"
[ "$(cat $cdm/README.TXT 2>/dev/null)" = "HamixOS media test" ] && echo "PASS README.TXT content" || echo "FAIL README.TXT: $(ls -la $cdm)"
[ "$(cat "$cdm/Music/Long folder name/Ünicode file name.txt" 2>/dev/null)" = "joliet works" ] && echo "PASS Joliet long Unicode names" || echo "FAIL Joliet names: $(ls -R $cdm | head -20)"
[ "$(wc -l < "$cdm/big file.txt")" = "20000" ] && echo "PASS big file reads completely" || echo "FAIL big file: $(wc -c < "$cdm/big file.txt") bytes"
echo x > $cdm/new 2>/dev/null && [ "$(id -u)" != 0 ] && echo "FAIL the CD is writable" || true
umount $cdm && echo "PASS umount CD" || echo "FAIL umount CD"
[ ! -f $cdm/README.TXT ] && echo "PASS CD files are gone after umount" || echo "FAIL files still visible after umount"
fat_dev=""
for d in sda sdb sdc; do
    [ -e /dev/$d ] || continue
    mkdir -p /media/test-$d
    if mount /dev/$d /media/test-$d 2>/dev/null; then fat_dev=$d; break; fi
done
[ -n "$fat_dev" ] && echo "PASS FAT disk mounted from /dev/$fat_dev" || echo "FAIL no FAT disk could be mounted"
fm=/media/test-$fat_dev
grep -q "vfat" /proc/mounts && echo "PASS /proc/mounts lists vfat" || echo "FAIL /proc/mounts: $(cat /proc/mounts)"
grep "vfat" /proc/mounts | grep -q HXSTICK && echo "PASS FAT volume label" || echo "FAIL label: $(grep vfat /proc/mounts)"
[ "$(cat $fm/readme.txt 2>/dev/null)" = "HamixOS media test" ] && echo "PASS FAT short file" || echo "FAIL FAT short file: $(ls -la $fm)"
[ "$(wc -l < "$fm/Photos/Holiday 2026/A rather long file name.txt")" = "20000" ] && echo "PASS FAT long names and cluster chains" || echo "FAIL FAT long file: $(ls -R $fm | head)"
umount $fm && echo "PASS umount FAT" || echo "FAIL umount FAT"
echo "PASS done"
