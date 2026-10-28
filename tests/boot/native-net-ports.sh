echo "hxtest: curl over the network on hxlibc"
for i in 1 2 3 4 5 6 7 8 9 10; do if ping -c 1 10.0.2.2; then break; fi; sleep 2; done
/usr/bin/busybox ash -c '/usr/bin/curl -s -o /tmp/index.html -w "%{http_code}" http://example.com/ > /tmp/code; if grep -q 200 /tmp/code; then echo "PASS curl http"; else echo "FAIL curl http: $(cat /tmp/code)"; fi'
echo "hxtest: hxstd sockets over libc"
ping -c 1 10.0.2.2 > /tmp/ping.txt 2>&1 && echo "PASS ping uses an ICMP socket" || { echo "FAIL ping"; cat /tmp/ping.txt; }
nslookup example.com > /tmp/ns.txt 2>&1 && echo "PASS nslookup via getaddrinfo" || { echo "FAIL nslookup"; cat /tmp/ns.txt; }
wget -O /tmp/wget.html http://example.com/ > /tmp/wget.txt 2>&1
if grep -qi "example" /tmp/wget.html; then echo "PASS wget over a libc TCP socket"; else echo "FAIL wget"; cat /tmp/wget.txt; fi
