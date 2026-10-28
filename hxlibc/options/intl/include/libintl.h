
#ifndef _LIBINTL_H
#define _LIBINTL_H

#ifdef __cplusplus
extern "C" {
#endif

#ifndef __MLIBC_ABI_ONLY

char *gettext(const char *msgid) __attribute__((__format_arg__(1)));
char *dgettext(const char *domainname, const char *msgid) __attribute__((__format_arg__(2)));
char *dcgettext(const char *domainname, const char *msgid,
		int category) __attribute__((__format_arg__(2)));

char *ngettext(const char *msgid, const char *msgid_plural, unsigned long int n)
		__attribute__((__format_arg__(1))) __attribute__((__format_arg__(2)));
char *dngettext(const char *domainname, const char *msgid,
		const char *msgid_plural, unsigned long int n)
		__attribute__((__format_arg__(2))) __attribute__((__format_arg__(3)));
char *dcngettext(const char *domainname, const char *msgid,
		const char *msgid_plural, unsigned long int n, int category)
		__attribute__((__format_arg__(2))) __attribute__((__format_arg__(3)));

char *textdomain(const char *domainname);
char *bindtextdomain(const char *domainname, const char *dirname);
char *bind_textdomain_codeset(const char *domainname, const char *codeset);

#endif /* !__MLIBC_ABI_ONLY */

#ifdef __cplusplus
}
#endif

#endif // _LIBINTL_H

