echo "hxtest: hxtoolkit applications on the Nook desktop"
hxserver > /tmp/hxserver.log 2>&1 &
for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /run/hamix/services/hxserver ] && break; sleep 1; done
sleep 2
for app in hxhello hxcalc; do
    $app > /tmp/$app.log 2>&1 &
    pid=$!
    sleep 5
    if kill -0 $pid 2>/dev/null; then echo "PASS $app (hxtoolkit) is running"; else echo "FAIL $app exited: $(cat /tmp/$app.log)"; fi
    kill $pid 2>/dev/null
done
echo "PASS done"
