echo "hxtest: memory of native processes on hxlibc"
/usr/lib/hamix/tests/bin/memory-probe
for p in /proc/[0-9]*; do
    name=$(grep -a '^Name:' $p/status 2>/dev/null | cut -f2)
    rss=$(grep -a '^VmRSS:' $p/status 2>/dev/null | tr -s ' \t' ' ' | cut -d' ' -f2)
    case "$name" in
        hxsvc|hx-authd|hxlogin|hx-login1|hx-polkit|hx-upower)
            if [ "${rss:-0}" -lt 3072 ]; then echo "PASS $name uses ${rss} kB"; else echo "FAIL $name uses ${rss} kB"; fi ;;
    esac
done
