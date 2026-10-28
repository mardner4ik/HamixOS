echo "hxtest: Files properties and About windows, hxtoolkit display selection"
hxserver > /tmp/hxserver.log 2>&1 &
for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /run/hamix/services/hxserver ] && break; sleep 1; done
sleep 2
check() {
    name=$1
    shift
    "$@" > /tmp/$name.log 2>&1 &
    pid=$!
    sleep 4
    if kill -0 $pid 2>/dev/null; then echo "PASS $name is running"; else echo "FAIL $name exited: $(cat /tmp/$name.log)"; fi
    echo $pid >> /tmp/props.pids
}
check props-file hxfiles --properties=/usr/bin/bash
check props-dir hxfiles --properties=/usr/share
check about hxfiles --about
HXTOOLKIT_DISPLAY=wayland hxhello > /tmp/wl.log 2>&1 &
wl=$!
sleep 4
if kill -0 $wl 2>/dev/null; then
    echo "PASS HXTOOLKIT_DISPLAY=wayland runs hxhello through Wayland (hxwayland)"
    kill $wl
else
    case "$(cat /tmp/wl.log)" in
        *"runs only on Wayland"*) echo "PASS HXTOOLKIT_DISPLAY=wayland refuses hxserver without a Wayland socket" ;;
        *) echo "FAIL HXTOOLKIT_DISPLAY=wayland: $(cat /tmp/wl.log)" ;;
    esac
fi
sleep 8
kill $(cat /tmp/props.pids) 2>/dev/null
echo "PASS done"
