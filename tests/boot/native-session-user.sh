echo "hxtest: Nook as a normal user, clients from the user and from root"
uid=$(id -u user)
export XDG_RUNTIME_DIR=/tmp/runtime-$uid
mkdir -p $XDG_RUNTIME_DIR
chown user $XDG_RUNTIME_DIR
chmod 700 $XDG_RUNTIME_DIR
/usr/libexec/hamix/hxsvc --as user -- /usr/bin/env XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR HOME=/home/user /usr/bin/hxserver > /tmp/hxserver.log 2>&1 &
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do [ -e $XDG_RUNTIME_DIR/hamix-services/hxserver ] && break; sleep 1; done
if [ -e $XDG_RUNTIME_DIR/hamix-services/hxserver ]; then
    owner=$(ls -ln $XDG_RUNTIME_DIR/hamix-services/hxserver | awk '{print $3}')
    [ "$owner" = "$uid" ] && echo "PASS hxserver registered in the user's runtime directory" || echo "FAIL hxserver registry owned by $owner"
else
    echo "FAIL hxserver did not register"; cat /tmp/hxserver.log
fi
sleep 4
/usr/libexec/hamix/hxproto-check
