echo "hxtest: evdev, DRM and libinput for native programs"
for node in /dev/input/event0 /dev/input/event1 /dev/input/event2 /dev/dri/card0; do
    if test -c $node; then echo "PASS $node is a character device"; else echo "FAIL $node is missing"; fi
done
/usr/lib/hamix/tests/bin/evdev-check || echo "FAIL evdev-check exited with $?"
/usr/lib/hamix/tests/bin/drm-check || echo "FAIL drm-check exited with $?"
if [ -x /usr/bin/libinput ]; then
    out=$(/usr/bin/libinput list-devices 2>&1)
    echo "$out"
    for want in "AT Translated Set 2 keyboard" "ImPS/2 Generic Wheel Mouse" "HamixOS Absolute Pointer"; do
        case "$out" in
            *"$want"*) echo "PASS libinput sees $want" ;;
            *) echo "FAIL libinput does not list $want" ;;
        esac
    done
else
    echo "FAIL libinput is not installed"
fi
