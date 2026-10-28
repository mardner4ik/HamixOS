#ifndef _SYS_IOCTL_H
#define _SYS_IOCTL_H

#include <mlibc-config.h>
#include <abi-bits/ioctls.h>

// On Linux, sys/ioctl.h includes the termios ioctls.
#if __MLIBC_LINUX_OPTION
#	include <asm/ioctls.h>
#	include <bits/winsize.h>
#   include <sys/ttydefaults.h>
#endif

#ifdef __cplusplus
extern "C" {
#endif

#ifndef __MLIBC_ABI_ONLY

int ioctl(int fd, unsigned long request, ...);

#endif /* !__MLIBC_ABI_ONLY */

#define FIONREAD 0x541B
#define FIONBIO 0x5421
#define FIONCLEX 0x5450
#define FIOCLEX 0x5451

#ifndef SIOCADDRT
#define SIOCADDRT 0x890B
#endif
#ifndef SIOCDELRT
#define SIOCDELRT 0x890C
#endif
#ifndef SIOCRTMSG
#define SIOCRTMSG 0x890D
#endif
#ifndef SIOCGIFNAME
#define SIOCGIFNAME 0x8910
#endif
#ifndef SIOCSIFLINK
#define SIOCSIFLINK 0x8911
#endif
#ifndef SIOCGIFCONF
#define SIOCGIFCONF 0x8912
#endif
#ifndef SIOCGIFFLAGS
#define SIOCGIFFLAGS 0x8913
#endif
#ifndef SIOCSIFFLAGS
#define SIOCSIFFLAGS 0x8914
#endif
#ifndef SIOCGIFADDR
#define SIOCGIFADDR 0x8915
#endif
#ifndef SIOCSIFADDR
#define SIOCSIFADDR 0x8916
#endif
#ifndef SIOCGIFDSTADDR
#define SIOCGIFDSTADDR 0x8917
#endif
#ifndef SIOCSIFDSTADDR
#define SIOCSIFDSTADDR 0x8918
#endif
#ifndef SIOCGIFBRDADDR
#define SIOCGIFBRDADDR 0x8919
#endif
#ifndef SIOCSIFBRDADDR
#define SIOCSIFBRDADDR 0x891a
#endif
#ifndef SIOCGIFNETMASK
#define SIOCGIFNETMASK 0x891b
#endif
#ifndef SIOCSIFNETMASK
#define SIOCSIFNETMASK 0x891c
#endif
#ifndef SIOCGIFMETRIC
#define SIOCGIFMETRIC 0x891d
#endif
#ifndef SIOCSIFMETRIC
#define SIOCSIFMETRIC 0x891e
#endif
#ifndef SIOCGIFMEM
#define SIOCGIFMEM 0x891f
#endif
#ifndef SIOCSIFMEM
#define SIOCSIFMEM 0x8920
#endif
#ifndef SIOCGIFMTU
#define SIOCGIFMTU 0x8921
#endif
#ifndef SIOCSIFMTU
#define SIOCSIFMTU 0x8922
#endif
#ifndef SIOCSIFNAME
#define SIOCSIFNAME 0x8923
#endif
#ifndef SIOCSIFHWADDR
#define SIOCSIFHWADDR 0x8924
#endif
#ifndef SIOCGIFENCAP
#define SIOCGIFENCAP 0x8925
#endif
#ifndef SIOCSIFENCAP
#define SIOCSIFENCAP 0x8926
#endif
#ifndef SIOCGIFHWADDR
#define SIOCGIFHWADDR 0x8927
#endif
#ifndef SIOCGIFSLAVE
#define SIOCGIFSLAVE 0x8929
#endif
#ifndef SIOCSIFSLAVE
#define SIOCSIFSLAVE 0x8930
#endif
#ifndef SIOCADDMULTI
#define SIOCADDMULTI 0x8931
#endif
#ifndef SIOCDELMULTI
#define SIOCDELMULTI 0x8932
#endif
#ifndef SIOCGIFINDEX
#define SIOCGIFINDEX 0x8933
#endif
#ifndef SIOCSIFPFLAGS
#define SIOCSIFPFLAGS 0x8934
#endif
#ifndef SIOCGIFPFLAGS
#define SIOCGIFPFLAGS 0x8935
#endif
#ifndef SIOCDIFADDR
#define SIOCDIFADDR 0x8936
#endif
#ifndef SIOCSIFHWBROADCAST
#define SIOCSIFHWBROADCAST 0x8937
#endif
#ifndef SIOCGIFCOUNT
#define SIOCGIFCOUNT 0x8938
#endif
#ifndef SIOCGIFBR
#define SIOCGIFBR 0x8940
#endif
#ifndef SIOCSIFBR
#define SIOCSIFBR 0x8941
#endif
#ifndef SIOCGIFTXQLEN
#define SIOCGIFTXQLEN 0x8942
#endif
#ifndef SIOCSIFTXQLEN
#define SIOCSIFTXQLEN 0x8943
#endif
#ifndef SIOCDARP
#define SIOCDARP 0x8953
#endif
#ifndef SIOCGARP
#define SIOCGARP 0x8954
#endif
#ifndef SIOCSARP
#define SIOCSARP 0x8955
#endif
#ifndef SIOCDRARP
#define SIOCDRARP 0x8960
#endif
#ifndef SIOCGRARP
#define SIOCGRARP 0x8961
#endif
#ifndef SIOCSRARP
#define SIOCSRARP 0x8962
#endif
#ifndef SIOCGIFMAP
#define SIOCGIFMAP 0x8970
#endif
#ifndef SIOCSIFMAP
#define SIOCSIFMAP 0x8971
#endif
#ifndef SIOCADDDLCI
#define SIOCADDDLCI 0x8980
#endif
#ifndef SIOCDELDLCI
#define SIOCDELDLCI 0x8981
#endif

#define SIOCPROTOPRIVATE 0x89E0
#define SIOCDEVPRIVATE 0x89F0

#ifdef __cplusplus
}
#endif

#endif // _SYS_IOCTL_H
