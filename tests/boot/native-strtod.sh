echo "hxtest: strtod stops before an exponent without digits (CSS units like 1.5em)"
/usr/lib/hamix/tests/bin/strtod-check || echo "FAIL strtod-check exited with $?"
echo "PASS done"
