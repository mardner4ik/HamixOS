echo "hxtest: live system from an .iso file on an exFAT USB stick (Ventoy, fragmented, found by its volume label), without the boot image in RAM"
grep -q "live:/dev/loop" /proc/mounts && echo "PASS root is the image inside the fragmented .iso (loop device)" || echo "FAIL root: $(head -3 /proc/mounts)"
dmesg | grep -E "hext:" | tail -3
dmesg | grep -q "Ventoy-style" && echo "PASS found through exFAT" || echo "FAIL exFAT lookup not logged"
[ "$(wc -l < /usr/share/hamix/install.sh)" -gt 10 ] && echo "PASS file data from the .iso" || echo "FAIL install.sh"
echo "PASS done"
