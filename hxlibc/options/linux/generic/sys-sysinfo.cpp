#include <unistd.h>
#include <errno.h>
#include <sys/sysinfo.h>

#include <bits/ensure.h>
#include <mlibc/debug.hpp>
#include <mlibc/linux-sysdeps.hpp>

int sysinfo(struct sysinfo *info) {
	MLIBC_CHECK_OR_ENOSYS(mlibc::sys_sysinfo, -1);
	if(int e = mlibc::sys_sysinfo(info); e) {
		errno = e;
		return -1;
	}
	return 0;
}

int get_nprocs(void) {
	long n = sysconf(_SC_NPROCESSORS_ONLN);
	return n > 0 ? (int)n : 1;
}

int get_nprocs_conf(void) {
	long n = sysconf(_SC_NPROCESSORS_CONF);
	return n > 0 ? (int)n : get_nprocs();
}

long get_phys_pages(void) {
	return sysconf(_SC_PHYS_PAGES);
}

long get_avphys_pages(void) {
	return sysconf(_SC_AVPHYS_PAGES);
}
