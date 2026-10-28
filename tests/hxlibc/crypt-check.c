#include <crypt.h>
#include <stdio.h>
#include <string.h>

static int check(const char *key, const char *setting, const char *expected)
{
	const char *got = crypt(key, setting);
	if (got && strcmp(got, expected) == 0) {
		printf("PASS crypt %s\n", setting);
		return 0;
	}
	printf("FAIL crypt %s: %s\n", setting, got ? got : "(null)");
	return 1;
}

int main(void)
{
	int failed = 0;
	failed += check("Hello world!", "$6$saltstring",
			"$6$saltstring$svn8UoSVapNtMuq1ukKS4tPQd8iKwSMHWjl/O817G3uBnIFNjnQJuesI68u4OTLiBFdcbYEdFCoEOfaS35inz1");
	failed += check("Hello world!", "$6$rounds=10000$saltstringsaltstring",
			"$6$rounds=10000$saltstringsaltst$OW1/O6BYHV6BcXZu8QVeXbDWra3Oeqh0sbHbbMCVNSnCM/UrjmM0Dp8vOuZeHBy/YTBmSK6H9qs/y3RnOaw5v.");
	if (crypt("x", "ab") != NULL) {
		printf("FAIL crypt accepted an unknown method\n");
		failed++;
	}
	return failed;
}
