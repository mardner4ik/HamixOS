echo "hxtest: kernel hardening (W^X of the kernel, SMEP, UMIP)"
line=$(dmesg | grep "boot: SMEP" | tail -1)
echo "$line"
case "$line" in *"kernel W^X on"*) echo "PASS kernel sections and the direct map have W^X" ;; *) echo "FAIL kernel W^X is off: $line" ;; esac
rwx=$(dmesg | grep "smp: kernel W^X" | tail -1)
echo "$rwx"
case "$rwx" in *" 0 writable and executable pages"*) echo "PASS no kernel page is writable and executable" ;; *) echo "FAIL kernel has RWX pages: $rwx" ;; esac
/usr/lib/hamix/tests/bin/usercopy-efault >/dev/null || echo "FAIL usercopy-efault exited with $?"
echo "PASS done"
