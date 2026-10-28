echo "hxtest: foot (Wayland terminal) on hxlibc through hxwayland"
if [ ! -x /usr/bin/foot ]; then
    echo "FAIL foot is not installed"
    exit 0
fi
version=$(foot --version 2>&1)
case "$version" in
    foot*) echo "PASS $version" ;;
    *) echo "FAIL foot --version: $version" ;;
esac
hxserver &
sleep 8
rm -f /tmp/foot.out
XDG_RUNTIME_DIR=/tmp/runtime-$(id -u) WAYLAND_DISPLAY=wayland-0 foot sh -c 'echo "TERM=$TERM cols=$(tput cols 2>/dev/null || stty size)" > /tmp/foot.out; sleep 2' > /tmp/foot.log 2>&1 &
pid=$!
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do [ -s /tmp/foot.out ] && break; sleep 1; done
if [ -s /tmp/foot.out ]; then echo "PASS foot ran a shell: $(cat /tmp/foot.out)"; else echo "FAIL foot did not run its shell"; cat /tmp/foot.log; fi
wait $pid
echo "foot exited with $?"
