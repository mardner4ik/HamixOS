echo "hxtest: Enlightenment 0.26 as a Wayland compositor on DRM and evdev"
if [ ! -x /usr/bin/enlightenment-session ]; then
    echo "FAIL enlightenment is not installed"
    exit 0
fi
export XDG_RUNTIME_DIR=/tmp/runtime-e
export HOME=/root
case "$(session list)" in
    *enlightenment*) echo "PASS session list offers enlightenment" ;;
    *) echo "FAIL session list does not offer enlightenment" ;;
esac
startx enlightenment > /tmp/e.log 2>&1 &
e_pid() {
    for p in /proc/[0-9]*; do
        [ "$(cat $p/comm 2>/dev/null)" = enlightenment ] && echo ${p#/proc/}
    done
}
rss() {
    grep -s VmRSS /proc/$1/status | tr -s ' \t' ' ' | cut -d' ' -f2
}
sleep 30
pid=$(e_pid)
if [ -z "$pid" ]; then
    echo "FAIL enlightenment is not running"
    cat /tmp/e.log
    tail -60 $HOME/.e-log.log 2>/dev/null
    exit 0
fi
first=$(rss $pid)
sleep 15
second=$(rss $pid)
echo "enlightenment pid $pid rss ${first} kB -> ${second} kB"
if kill -0 $pid 2>/dev/null; then echo "PASS enlightenment is running"; else echo "FAIL enlightenment exited"; fi
if [ -n "$first" ] && [ -n "$second" ] && [ $((second - first)) -lt 8192 ]; then
    echo "PASS enlightenment memory is stable"
else
    echo "FAIL enlightenment memory grows"
fi
if [ -S $XDG_RUNTIME_DIR/wayland-0 ]; then echo "PASS wayland socket $XDG_RUNTIME_DIR/wayland-0"; else echo "FAIL no wayland socket"; fi
for fd in /proc/$pid/fd/*; do readlink $fd; done | sort | uniq > /tmp/e.fds
grep -q "^/dev/dri/card0$" /tmp/e.fds && echo "PASS enlightenment drives /dev/dri/card0" || echo "FAIL enlightenment does not hold /dev/dri/card0"
grep -q "^/dev/input/event0$" /tmp/e.fds && echo "PASS enlightenment reads /dev/input/event0" || echo "FAIL enlightenment does not read the keyboard"
kill $pid
true
