echo "hxtest: random system calls from unprivileged processes must not panic or hang the kernel"
args="160 1500 1"
for word in $(cat /proc/cmdline); do
	case "$word" in fuzz=*) args=$(echo "${word#fuzz=}" | tr ',' ' ') ;; esac
done
/usr/lib/hamix/tests/bin/syscall-fuzz $args || echo "FAIL syscall-fuzz exited with $?"
echo "PASS done"
