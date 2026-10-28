echo "hxtest: USB mouse and keyboard on an EHCI controller without companion controllers (Intel 6/7 series)"
found=""
for i in $(seq 1 20); do
    found=$(dmesg | grep -E "on EHCI .*HID (mouse|keyboard)" | wc -l)
    [ "$found" -ge 2 ] && break
    sleep 1
done
dmesg | grep -E "usb:|ehci:" | tail -8
dmesg | grep -q "on EHCI .*HID mouse" && echo "PASS the EHCI mouse is driven" || echo "FAIL the EHCI mouse was not taken"
dmesg | grep -q "on EHCI .*HID keyboard" && echo "PASS the EHCI keyboard is driven" || echo "FAIL the EHCI keyboard was not taken"
dmesg | grep -q "handed to the companion controller" && echo "FAIL a HID device was handed to a missing companion controller" || echo "PASS nothing was handed to a missing companion controller"
dmesg | grep -E "ehci: transfer .* (failed|timed out)" && echo "FAIL EHCI transfers failed" || echo "PASS no EHCI transfer errors"
echo "PASS done"
