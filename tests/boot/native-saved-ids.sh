echo "hxtest: real, effective and saved user ids (setresuid/getresuid)"
/usr/lib/hamix/tests/bin/saved-ids || echo "FAIL saved-ids exited with $?"
echo "PASS done"
