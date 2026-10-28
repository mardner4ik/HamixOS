echo "hxtest: memory protection (mmap/mprotect, W^X, NX) and EFAULT instead of a kill"
/usr/lib/hamix/tests/bin/usercopy-efault || echo "FAIL usercopy-efault exited with $?"
echo "PASS done"
