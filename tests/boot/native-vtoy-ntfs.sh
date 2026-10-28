echo "hxtest: live system from a HamixOS .iso on an NTFS Ventoy stick (5 fragments, reversed), opened through the ventoy_os_param table that hxboot passes to the kernel"
grep -q "live:/dev/loop" /proc/mounts && echo "PASS root is the image inside the .iso (loop device)" || echo "FAIL root: $(head -3 /proc/mounts)"
dmesg | grep -E "hext:" | tail -3
dmesg | grep -q "through the Ventoy image table" && echo "PASS found through the Ventoy table" || echo "FAIL the Ventoy table was not used"
[ "$(wc -l < /usr/share/hamix/install.sh)" -gt 10 ] && echo "PASS file data from the .iso" || echo "FAIL install.sh"
echo "PASS done"
