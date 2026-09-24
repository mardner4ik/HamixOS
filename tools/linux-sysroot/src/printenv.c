#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern char **environ;

int main(int argc, char **argv) {
    if (argc > 1) {
        int missing = 0;
        for (int i = 1; i < argc; i++) {
            const char *value = getenv(argv[i]);
            if (value) puts(value); else missing = 1;
        }
        return missing;
    }
    for (char **e = environ; *e; e++) puts(*e);
    return 0;
}
