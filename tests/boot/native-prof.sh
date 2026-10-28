echo "hxtest: hxprof"
hxctl prof reset
hxctl prof start
/usr/lib/hamix/bench/kbench 1
hxctl prof stop
hxctl prof report
