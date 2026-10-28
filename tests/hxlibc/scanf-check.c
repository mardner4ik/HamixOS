#include <stdio.h>
#include <string.h>

static int failures;

static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok)
        failures++;
}

int main(void) {
    char line[32];
    char rest[32];
    int n = 0;
    check(sscanf("", "%31[^\n]\n", line) == EOF, "%[ at end of input returns EOF");
    check(sscanf("ab\ncd", "%31[^\n]\n%31s", line, rest) == 2 && strcmp(line, "ab") == 0 && strcmp(rest, "cd") == 0, "%[ stops before the first character outside the set");
    check(sscanf("\nx", "%31[^\n]", line) == 0, "%[ with no matching characters is a matching failure");
    check(sscanf("12 34", "%d %d", &n, &n) == 2 && n == 34, "%d %d reads two numbers");
    check(sscanf("", "%d", &n) == EOF, "%d at end of input returns EOF");
    FILE *f = fmemopen((void *)"C\nC.UTF-8\n", 10, "r");
    int count = 0;
    while (f && fscanf(f, "%31[^\n]\n", line) == 1 && count < 10)
        count++;
    check(count == 2, "fscanf loop over lines ends at EOF");
    if (f)
        fclose(f);
    return failures ? 1 : 0;
}
