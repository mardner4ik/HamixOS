echo "hxtest: busybox ash on hxlibc"
/usr/bin/busybox ash -c 'echo PASS ash runs; i=0; while [ $i -lt 3 ]; do i=$((i+1)); done; echo "PASS arithmetic $i"'
/usr/bin/busybox ash -c 'echo hello > /tmp/bb.txt; cat /tmp/bb.txt | tr a-z A-Z | grep -q HELLO && echo PASS pipes and redirection'
/usr/bin/busybox ls /usr/lib/hamix/tests
/usr/bin/busybox ash -c 'x=$(echo sub); [ "$x" = sub ] && echo PASS command substitution'
/usr/bin/busybox ash -c 'sleep 0.1 & p=$!; echo "started $p"; wait $p; s=$?; if [ $s = 0 ]; then echo "PASS background job and wait"; else echo "FAIL background job and wait: status $s"; fi'
/usr/bin/busybox ash -c 'sleep 0.2 && echo "PASS sleep with a fraction" || echo "FAIL sleep with a fraction"'
/usr/bin/busybox awk 'BEGIN { print "PASS awk " 6*7 }'
/usr/bin/busybox sed -n 's/hello/PASS sed/p' /tmp/bb.txt
/usr/bin/busybox ash -c 'cd /tmp && tar czf t.tgz bb.txt && rm bb.txt && tar xzf t.tgz && cat bb.txt' | /usr/bin/busybox grep -q hello && echo "PASS tar gzip roundtrip"
