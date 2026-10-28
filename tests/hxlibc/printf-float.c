#include <stdio.h>
#include <string.h>

static int failures;

static void expect(const char *got, const char *want)
{
    int ok = strcmp(got, want) == 0;
    if (!ok)
        failures++;
    printf("%s printf -> \"%s\"%s%s%s\n", ok ? "PASS" : "FAIL", got, ok ? "" : " (want \"", ok ? "" : want, ok ? "" : "\")");
}

int main(void)
{
    char b[128];
    snprintf(b, sizeof b, "%f", 1.05); expect(b, "1.050000");
    snprintf(b, sizeof b, "%.2f", 1.2); expect(b, "1.20");
    snprintf(b, sizeof b, "%.1f", 0.25); expect(b, "0.2");
    snprintf(b, sizeof b, "%.0f", 2.5); expect(b, "2");
    snprintf(b, sizeof b, "%8.3f|%-8.2f|%+.1f", 3.14159, 2.5, 7.0); expect(b, "   3.142|2.50    |+7.0");
    snprintf(b, sizeof b, "%f", 1e20); expect(b, "100000000000000000000.000000");
    snprintf(b, sizeof b, "%g %g %g %g", 3.25, 0.0001, 123456789.0, 1e-5); expect(b, "3.25 0.0001 1.23457e+08 1e-05");
    snprintf(b, sizeof b, "%e", 12345.678); expect(b, "1.234568e+04");
    snprintf(b, sizeof b, "%.3E", -0.000123456); expect(b, "-1.235E-04");
    snprintf(b, sizeof b, "%G|%#g|%08.3f", 1e100, 1.5, -3.5); expect(b, "1E+100|1.50000|-003.500");
    snprintf(b, sizeof b, "%a", 1.0); expect(b, "0x1p+0");
    snprintf(b, sizeof b, "%f %F %g", 1.0 / 0.0, -1.0 / 0.0, __builtin_nan("")); expect(b, "inf -INF nan");
    snprintf(b, sizeof b, "%Lf", 2.5L); expect(b, "2.500000");
    snprintf(b, sizeof b, "%.15g", 0.1 + 0.2); expect(b, "0.3");
    snprintf(b, sizeof b, "%.17g", 0.1); expect(b, "0.10000000000000001");
    printf("printf-float: %d failure(s)\n", failures);
    return failures ? 1 : 0;
}
