#define _GNU_SOURCE
#include <sched.h>
#include <stdio.h>
#include <sys/sysinfo.h>
#include <unistd.h>

int main(void)
{
	cpu_set_t set;
	CPU_ZERO(&set);
	int r = sched_getaffinity(0, sizeof set, &set);
	unsigned long first = *(unsigned long *)&set;
	printf("sched_getaffinity=%d mask=%#lx count=%d\n", r, first, CPU_COUNT(&set));
	long onln = sysconf(_SC_NPROCESSORS_ONLN), conf = sysconf(_SC_NPROCESSORS_CONF);
	printf("onln=%ld conf=%ld get_nprocs=%d get_nprocs_conf=%d\n", onln, conf, get_nprocs(), get_nprocs_conf());
	if (onln >= 1 && onln <= 64 && conf == onln && get_nprocs() == onln)
		printf("PASS processor count %ld\n", onln);
	else
		printf("FAIL processor count is inconsistent\n");
	return 0;
}
