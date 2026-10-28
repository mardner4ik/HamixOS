#define _GNU_SOURCE
#include <stdio.h>
#include <unistd.h>

int main(void)
{
	int failed = 0;
	uid_t r, e, s;
	if (setresuid(-1, 1000, -1) != 0) {
		printf("FAIL drop effective uid\n");
		return 1;
	}
	getresuid(&r, &e, &s);
	if (r == 0 && e == 1000 && s == 0)
		printf("PASS dropped privileges keep the saved uid (0/1000/0)\n");
	else
		printf("FAIL ids after drop %u/%u/%u\n", r, e, s), failed++;
	if (setresuid(0, 0, -1) == 0 && geteuid() == 0)
		printf("PASS privileges come back through the saved uid\n");
	else
		printf("FAIL cannot reclaim root\n"), failed++;
	if (setuid(1000) == 0) {
		getresuid(&r, &e, &s);
		if (r == 1000 && e == 1000 && s == 1000 && setuid(0) != 0)
			printf("PASS setuid as root drops all three ids for good\n");
		else
			printf("FAIL after setuid %u/%u/%u\n", r, e, s), failed++;
	}
	return failed;
}
