#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int check(const char *text, double want, const char *rest)
{
	char *end;
	double got = strtod(text, &end);
	if (got == want && strcmp(end, rest) == 0) {
		printf("PASS strtod(\"%s\") = %g, rest \"%s\"\n", text, got, end);
		return 0;
	}
	printf("FAIL strtod(\"%s\") = %g, rest \"%s\" (want %g, \"%s\")\n", text, got, end, want, rest);
	return 1;
}

int main(void)
{
	int failed = 0;
	failed += check("1.5em", 1.5, "em");
	failed += check("2e", 2, "e");
	failed += check("3e+", 3, "e+");
	failed += check("4e2x", 400, "x");
	failed += check("0.5ex", 0.5, "ex");
	failed += check("6E-1", 0.6, "");
	return failed;
}
