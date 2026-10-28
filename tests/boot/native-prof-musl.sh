echo "hxtest: hxprof of single Linux/musl kbench tests"
for t in mmap thread pipe; do
echo "prof: $t"
hxctl prof reset
hxctl prof start
KBENCH_ONLY=$t /usr/lib/hamix/bench/kbench-musl 4
hxctl prof stop
hxctl prof report
done
