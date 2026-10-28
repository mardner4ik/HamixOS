echo "hxtest: native fastfetch with the HamixOS config"
[ -x /usr/bin/fastfetch ] && echo "PASS /usr/bin/fastfetch" || echo "FAIL fastfetch is not installed"
out=$(fastfetch --pipe --logo none 2>&1)
echo "$out"
for key in OS Kernel CPU Memory Shell Disk; do
    echo "$out" | grep -qE "^ *$key( \(|  )" && echo "PASS $key line" || echo "FAIL no $key line"
done
/usr/lib/hamix/tests/bin/sysconf-cpus
echo "PASS done"
