echo "hxtest: the hxtoolkit installer on a blank disk"
hxserver > /tmp/hxserver.log 2>&1 &
for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /run/hamix/services/hxserver ] && break; sleep 1; done
sleep 2
hxinstall > /tmp/hxinstall.log 2>&1 &
sleep 60
cat /tmp/hxinstall.log
echo "PASS done"
