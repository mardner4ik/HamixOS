echo "hxtest: C programs on hxlibc"
/usr/lib/hamix/tests/bin/spike
/usr/lib/hamix/tests/bin/posix-io
/usr/lib/hamix/tests/bin/stdio-pipe
/usr/lib/hamix/tests/bin/scanf-check || echo "FAIL scanf-check exited with $?"
/usr/lib/hamix/tests/bin/remap
