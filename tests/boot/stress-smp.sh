echo "hxtest: SMP stress"
cat /proc/cpuinfo | /usr/bin/busybox grep -c processor
/usr/bin/busybox ash -c '
for n in 1 2 3 4; do
    ( /usr/lib/hamix/bench/kbench 1 > /tmp/k$n.txt 2>&1; echo "kbench $n exit $?" > /tmp/k$n.done ) &
done
for n in 1 2 3 4; do
    ( i=0; while [ $i -lt 200 ]; do echo $i > /tmp/s$n; cat /tmp/s$n > /dev/null; i=$((i+1)); done; echo done > /tmp/s$n.done ) &
done
wait
ok=0
for n in 1 2 3 4; do
    grep -q "\"done\":true" /tmp/k$n.txt && ok=$((ok+1))
    [ -f /tmp/s$n.done ] && ok=$((ok+1))
done
if [ $ok = 8 ]; then echo "PASS 8 parallel workers finished"; else echo "FAIL only $ok of 8 workers finished"; fi
'
