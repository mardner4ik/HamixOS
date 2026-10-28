echo "hxtest: desktop stress, apps opened and closed while the shell works"
hxserver > /tmp/hxserver.log 2>&1 &
for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /run/hamix/services/hxserver ] && break; sleep 1; done
sleep 1
( for i in $(seq 1 40); do ls -lR /usr/share > /dev/null 2>&1; cat /proc/meminfo /proc/stat > /dev/null; done; echo "PASS shell load finished" ) &
load=$!
for round in 1 2 3 4 5 6; do
    pids=""
    for app in hxterm hxfiles hxcalc hxnotes hxmon hxhello; do
        $app > /tmp/$app.$round.log 2>&1 &
        pids="$pids $!"
    done
    sleep 3
    for p in $pids; do kill -0 $p 2>/dev/null || echo "FAIL an app exited early in round $round"; done
    kill $pids 2>/dev/null
    sleep 1
    echo "PASS round $round"
done
wait $load
grep -E "^(Mem|Kernel)" /proc/meminfo | head -3
echo "PASS done"
