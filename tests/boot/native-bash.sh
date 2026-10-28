echo "hxtest: bash on hxlibc is the system shell"
echo "PASS bash runs $BASH_VERSION"
x=0; for i in 1 2 3; do x=$((x+i)); done; [ $x = 6 ] && echo "PASS arithmetic loop" || echo "FAIL arithmetic loop: $x"
a=(one two three); [ "${a[1]}" = two ] && [ ${#a[@]} = 3 ] && echo "PASS arrays" || echo "FAIL arrays"
declare -A m=([k]=v); [ "${m[k]}" = v ] && echo "PASS associative arrays" || echo "FAIL associative arrays"
[[ abc == a* && -d /usr ]] && echo "PASS [[ ]]" || echo "FAIL [[ ]]"
f() { local v=$1; echo "$v"; }; [ "$(f inner)" = inner ] && echo "PASS functions and command substitution" || echo "FAIL functions"
out=$(echo hello | tr a-z A-Z | cat); [ "$out" = HELLO ] && echo "PASS three-stage pipeline" || echo "FAIL pipeline: $out"
sleep 0.1 & p=$!; wait $p && echo "PASS background job and wait" || echo "FAIL background job"
( exit 7 ); [ $? = 7 ] && echo "PASS subshell status" || echo "FAIL subshell status"
hxctl abi > /tmp/abi.txt; grep -q "Hamix ABI" /tmp/abi.txt && echo "PASS bash runs native Rust programs" || echo "FAIL native program output"
v=$(uname -s); [ "$v" = HamixOS ] && echo "PASS uname is the Hamix program" || echo "FAIL uname says $v"
cat <<END > /tmp/heredoc.txt
line $x
END
grep -q "line 6" /tmp/heredoc.txt && echo "PASS here-document" || echo "FAIL here-document"
printf '%s\n' c b a | sort | head -1 | grep -q a && echo "PASS busybox applets through /usr/bin wrappers" || echo "FAIL busybox applets"
/bin/sh -c 'echo "PASS /bin/sh runs ($0)"'
/bin/sh -c 'shopt -qo posix' && echo "PASS /bin/sh is bash in POSIX mode" || echo "FAIL /bin/sh is not in POSIX mode"
printf '#!/bin/sh\necho "PASS shebang script args: $1"\n' > /tmp/s.sh; chmod 755 /tmp/s.sh; /tmp/s.sh ok
case "$(id -u)" in 0) echo "PASS test runs as root" ;; *) echo "FAIL not root" ;; esac
sudo -k
sudo true && echo "PASS sudo as root needs no password" || echo "FAIL sudo as root"
v=$(sudo whoami); [ "$v" = root ] && echo "PASS sudo runs a program" || echo "FAIL sudo whoami: $v"
v=$(sudo sh -c 'echo $HOME'); [ "$v" = /root ] && echo "PASS sudo sets HOME" || echo "FAIL sudo HOME: $v"
sudo nonexistent-command-xyz 2>/dev/null; [ $? = 127 ] && echo "PASS sudo reports a missing command" || echo "FAIL sudo missing command status"
v=$(bash -lc 'echo $PATH'); case "$v" in /usr/bin:*) echo "PASS login shell reads /etc/profile" ;; *) echo "FAIL login PATH: $v" ;; esac
echo 'echo "PASS interactive bashrc: $(type -t ll)"' | bash -i 2>/dev/null | grep PASS
setconf /tmp/c.conf key value1; setconf /tmp/c.conf key value2; [ "$(hxconf /tmp/c.conf key)" = value2 ] && echo "PASS setconf and hxconf" || echo "FAIL setconf"
