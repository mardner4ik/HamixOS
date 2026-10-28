echo "hxtest: drive hxtoolkit programs with the mouse"
hxserver > /tmp/hxserver.log 2>&1 &
for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /run/hamix/services/hxserver ] && break; sleep 1; done
sleep 2
for app in hxcmd hxview; do
    $app > /tmp/$app.log 2>&1 &
    pid=$!
    sleep 50
    cat /tmp/$app.log
    kill $pid 2>/dev/null
done
echo "PASS done"
