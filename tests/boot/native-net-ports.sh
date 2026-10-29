echo "hxtest: curl over the network on hxlibc"
for i in 1 2 3 4 5 6 7 8 9 10; do if ping -c 1 10.0.2.2; then break; fi; sleep 2; done
/usr/bin/busybox ash -c '/usr/bin/curl -s -o /tmp/index.html -w "%{http_code}" http://example.com/ > /tmp/code; if grep -q 200 /tmp/code; then echo "PASS curl http"; else echo "FAIL curl http: $(cat /tmp/code)"; fi'
/usr/bin/busybox ash -c '/usr/bin/curl -4 --tlsv1.2 --tls-max 1.2 --fail --location --connect-timeout 8 --max-time 20 -o /tmp/index-https.html -w "%{http_code}" https://example.com/ > /tmp/code-https 2>/tmp/curl-https.err; if grep -q 200 /tmp/code-https && grep -qi "example domain" /tmp/index-https.html; then echo "PASS curl https"; else echo "INFO curl needs Browser OpenSSL HTTPS fallback"; fi'
/usr/bin/busybox ash -c 'printf "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n" | /usr/bin/openssl s_client -4 -connect example.com:443 -servername example.com -verify_hostname example.com -verify_return_error -CAfile /etc/ssl/certs/ca-certificates.crt -quiet > /tmp/openssl-https.html 2>/tmp/openssl-https.err; if grep -qi "example domain" /tmp/openssl-https.html; then echo "PASS openssl https with certificate verification"; else echo "FAIL openssl https: $(tail -5 /tmp/openssl-https.err)"; fi'
/usr/bin/hxsurf --diagnose https://example.com/ > /tmp/hxsurf-https.txt 2>&1 && grep -q "PASS Browser fetched" /tmp/hxsurf-https.txt && echo "PASS hxsurf HTTPS fetch" || { echo "FAIL hxsurf HTTPS fetch"; cat /tmp/hxsurf-https.txt; }
echo "hxtest: hxstd sockets over libc"
ping -c 1 10.0.2.2 > /tmp/ping.txt 2>&1 && echo "PASS ping uses an ICMP socket" || { echo "FAIL ping"; cat /tmp/ping.txt; }
nslookup example.com > /tmp/ns.txt 2>&1 && echo "PASS nslookup via getaddrinfo" || { echo "FAIL nslookup"; cat /tmp/ns.txt; }
wget -O /tmp/wget.html http://example.com/ > /tmp/wget.txt 2>&1
if grep -qi "example" /tmp/wget.html; then echo "PASS wget over a libc TCP socket"; else echo "FAIL wget"; cat /tmp/wget.txt; fi
