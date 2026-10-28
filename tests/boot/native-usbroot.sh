echo "hxtest: live system read from a USB stick behind EHCI with UHCI companions, without the boot image in RAM"
grep -q "live:/dev/sd" /proc/mounts && echo "PASS root is the live image on the USB stick" || echo "FAIL root: $(head -3 /proc/mounts)"
dmesg | grep -E "usb-storage|usb:|hext:" | tail -6
dmesg | grep -q "usb-storage: .* on EHCI .*(high speed)" && echo "PASS the stick runs at high speed on EHCI" || echo "FAIL the stick is not driven by EHCI"
dmesg | grep -q "usb: 0627:0001 on UHCI" && echo "PASS the full-speed tablet was handed to a companion UHCI" || echo "FAIL tablet: $(dmesg | grep 0627)"
[ -x /usr/bin/bash ] && echo "PASS /usr/bin/bash readable from the stick" || echo "FAIL bash missing"
[ "$(cat /usr/share/hamix/install.sh | wc -l)" -gt 10 ] && echo "PASS file data over USB" || echo "FAIL install.sh"
start=$(cut -d' ' -f1 /proc/uptime)
dd if=/dev/sda of=/dev/null bs=1M count=64 skip=200 2>/dev/null
end=$(cut -d' ' -f1 /proc/uptime)
echo "info: 64 MiB from the stick in $start..$end s"
echo "PASS done"
