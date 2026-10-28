#ifndef NL_TYPES_H
#define NL_TYPES_H

#include <bits/nl_item.h>

#define NL_SETD 1
#define NL_CAT_LOCALE 1

#ifdef __cplusplus
extern "C" {
#endif

typedef void *nl_catd;

#ifndef __MLIBC_ABI_ONLY

nl_catd catopen(const char *name, int flag);
char *catgets(nl_catd catalog, int set, int number, const char *message);
int catclose(nl_catd catalog);

#endif /* !__MLIBC_ABI_ONLY */

#ifdef __cplusplus
}
#endif

#endif // NL_TYPES_H
