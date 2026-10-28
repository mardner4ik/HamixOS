sleep 5
echo "membench: console idle"
cat /proc/meminfo
desktop=$(cat /etc/hamix/desktop 2>/dev/null)
if [ "$desktop" = enlightenment ]; then
    export XDG_RUNTIME_DIR=/tmp/runtime-e
    export HOME=/root
    startx enlightenment > /tmp/e.log 2>&1 &
    sleep 40
    echo "membench: enlightenment idle"
else
    hxserver &
    sleep 20
    echo "membench: nook idle"
fi
cat /proc/meminfo
ps
