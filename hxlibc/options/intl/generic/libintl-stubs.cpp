#include <libintl.h>
#include <stdlib.h>
#include <string.h>

namespace {

char current_domain[256] = "messages";
char current_codeset[64] = "UTF-8";

}

char *gettext(const char *msgid) {
	return const_cast<char *>(msgid);
}

char *dgettext(const char *, const char *msgid) {
	return const_cast<char *>(msgid);
}

char *dcgettext(const char *, const char *msgid, int) {
	return const_cast<char *>(msgid);
}

char *ngettext(const char *msgid, const char *msgid_plural, unsigned long int n) {
	return const_cast<char *>(n == 1 ? msgid : msgid_plural);
}

char *dngettext(const char *, const char *msgid, const char *msgid_plural, unsigned long int n) {
	return const_cast<char *>(n == 1 ? msgid : msgid_plural);
}

char *dcngettext(const char *, const char *msgid, const char *msgid_plural, unsigned long int n, int) {
	return const_cast<char *>(n == 1 ? msgid : msgid_plural);
}

char *textdomain(const char *domainname) {
	if(domainname && *domainname) {
		strncpy(current_domain, domainname, sizeof(current_domain) - 1);
		current_domain[sizeof(current_domain) - 1] = 0;
	}
	return current_domain;
}

char *bindtextdomain(const char *, const char *dirname) {
	return const_cast<char *>(dirname ? dirname : "/usr/share/locale");
}

char *bind_textdomain_codeset(const char *, const char *codeset) {
	if(codeset && *codeset) {
		strncpy(current_codeset, codeset, sizeof(current_codeset) - 1);
		current_codeset[sizeof(current_codeset) - 1] = 0;
	}
	return current_codeset;
}
