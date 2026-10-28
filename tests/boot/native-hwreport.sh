echo "hxtest: hwreport collects a hardware report"
hwreport /tmp/report.txt || echo "FAIL hwreport exited with $?"
for s in "boot units (hxinit)" "PCI devices" "interrupts" "kernel log" "kernel command line"; do
    grep -q "===== $s =====" /tmp/report.txt && echo "PASS section $s" || echo "FAIL section $s is missing"
done
grep -q "vendor=0x8086" /tmp/report.txt && echo "PASS PCI ids listed" || echo "FAIL no PCI ids: $(grep -A3 'PCI devices' /tmp/report.txt)"
echo "PASS done"
