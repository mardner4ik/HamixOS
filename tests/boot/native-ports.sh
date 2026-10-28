echo "hxtest: programs from hx-ports on hxlibc"
/usr/bin/lua -e "print('PASS lua ' .. string.format('%d', 6*7) .. ' ' .. _VERSION)"
/usr/bin/sqlite3 :memory: "create table t(x); insert into t values (6),(7); select 'PASS sqlite ' || ((select x from t where x=6)*(select x from t where x=7));"
echo hello > /tmp/h.txt
/usr/bin/openssl dgst -sha256 /tmp/h.txt
/usr/bin/openssl version
/usr/bin/curl --version
/usr/bin/busybox ash -c '[ "$(/usr/bin/openssl dgst -sha256 -r /tmp/h.txt | cut -c1-16)" = 5891b5b522d5df08 ] && echo "PASS openssl sha256" || echo "FAIL openssl sha256"'
/usr/bin/busybox ash -c '/usr/bin/curl -s file:///tmp/h.txt | grep -q hello && echo "PASS curl file://" || echo "FAIL curl file://"'
