echo "hxtest: process list and memory through /proc"
out=$(ps)
echo "$out"
case "$out" in *hxsvc*) echo "PASS ps lists hxsvc" ;; *) echo "FAIL ps has no hxsvc" ;; esac
case "$out" in *"[hxinit]"*) echo "PASS ps lists kernel threads" ;; *) echo "FAIL ps has no kernel threads" ;; esac
case "$out" in *tty1*) echo "PASS ps shows the tty" ;; *) echo "FAIL ps shows no tty" ;; esac
grep -q "^Abi:" /proc/1/status && echo "PASS /proc/1/status has Abi" || echo "FAIL /proc/1/status has no Abi"
tty=$(cut -d' ' -f7 /proc/self/stat)
[ "$tty" = "1025" ] && echo "PASS tty_nr is /dev/tty1" || echo "FAIL tty_nr is $tty"
grep -q "^MemTotal:" /proc/meminfo && grep -q "^KernelHeap:" /proc/meminfo && echo "PASS meminfo has Linux and Hamix fields" || echo "FAIL meminfo"
grep -q "^cpu0 " /proc/stat && echo "PASS /proc/stat is Linux format" || echo "FAIL /proc/stat: $(head -1 /proc/stat)"
free
uptime
