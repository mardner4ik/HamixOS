#include <bits/posix/posix_locale.h>
#include <bits/ensure.h>
#include <frg/allocation.hpp>
#include <mlibc/allocator.hpp>
#include <mlibc/debug.hpp>

namespace {

struct hamix_locale {
	int mask;
};

hamix_locale global_locale{0};
thread_local locale_t current_locale = nullptr;

}

locale_t newlocale(int mask, const char *, locale_t base) {
	if(base && base != &global_locale) {
		static_cast<hamix_locale *>(base)->mask |= mask;
		return base;
	}
	return frg::construct<hamix_locale>(getAllocator(), hamix_locale{mask});
}

void freelocale(locale_t locale) {
	if(locale && locale != &global_locale)
		frg::destruct(getAllocator(), static_cast<hamix_locale *>(locale));
}

locale_t uselocale(locale_t locale) {
	locale_t previous = current_locale ? current_locale : reinterpret_cast<locale_t>(-1L);
	if(locale == reinterpret_cast<locale_t>(-1L))
		current_locale = nullptr;
	else if(locale)
		current_locale = locale;
	return previous;
}

locale_t duplocale(locale_t locale) {
	int mask = 0;
	if(locale && locale != reinterpret_cast<locale_t>(-1L))
		mask = static_cast<hamix_locale *>(locale)->mask;
	return frg::construct<hamix_locale>(getAllocator(), hamix_locale{mask});
}
