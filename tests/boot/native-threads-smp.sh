echo "hxtest: threads of one process on several CPUs (pthread_rwlock, mutex)"
/usr/lib/hamix/tests/bin/rwlock-stress || echo "FAIL rwlock-stress exited with $?"
echo "PASS done"
