#include <hamix/linuxkpi.h>

struct hamix_net_ops {
	u32 abi;
	u32 flags;
	u64 context;
	u8 mac[8];
	int (*transmit)(u64 context, const u8 *frame, size_t len);
	void (*set_enabled)(u64 context, int enabled);
};

extern int hamix_pci_find_class(u8 class, u8 subclass, u8 prog_if, u32 index, struct hamix_pci_handle *out);
extern u32 hamix_pci_read32(const struct hamix_pci_handle *handle, u8 offset);
extern void hamix_pci_write32(const struct hamix_pci_handle *handle, u8 offset, u32 value);
extern u16 hamix_pci_read16(const struct hamix_pci_handle *handle, u8 offset);
extern void hamix_pci_write16(const struct hamix_pci_handle *handle, u8 offset, u16 value);
extern u64 hamix_pci_bar(const struct hamix_pci_handle *handle, u8 index, u64 *len_out);
extern int hamix_pci_msi_enable(const struct hamix_pci_handle *handle);
extern int hamix_request_irq(const struct hamix_pci_handle *handle, int (*handler)(void *), void *context);
extern void hamix_free_irq(void);
extern int hamix_schedule_work(void (*work)(void *), void *context);
extern int hamix_claim_device(u32 class, const char *name, size_t name_len, const struct hamix_pci_handle *handle, void (*poll)(void *), void *context);
extern int hamix_register_netdev(const char *driver, size_t len, const struct hamix_net_ops *ops);
extern void hamix_unregister_netdev(int id);
extern void hamix_net_receive(int id, const u8 *frame, size_t len);
extern void hamix_net_carrier(int id, int up);
extern int hamix_in_interrupt(void);

#define HAMIX_CLASS_NETWORK 1
#define HAMIX_CLASS_DISPLAY 2
#define HAMIX_CLASS_BLOCK 3
#define HAMIX_CLASS_AUDIO 5
#define HAMIX_CLASS_USB_HCD 6
#define MAX_PCI_DEVS 8
#define MAX_IRQS 8
#define MAX_MAPS 16
#define CHUNK 2048

static struct workqueue_struct hamix_system_wq;
struct workqueue_struct *system_wq = &hamix_system_wq;

size_t strlen(const char *s)
{
	size_t n = 0;
	while (s[n])
		n++;
	return n;
}

size_t strnlen(const char *s, size_t max)
{
	size_t n = 0;
	while (n < max && s[n])
		n++;
	return n;
}

int strcmp(const char *a, const char *b)
{
	while (*a && *a == *b) {
		a++;
		b++;
	}
	return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n)
{
	for (size_t i = 0; i < n; i++) {
		if (a[i] != b[i] || !a[i])
			return (unsigned char)a[i] - (unsigned char)b[i];
	}
	return 0;
}

char *strcpy(char *dest, const char *src)
{
	char *d = dest;
	while ((*d++ = *src++))
		;
	return dest;
}

char *strncpy(char *dest, const char *src, size_t n)
{
	size_t i = 0;
	for (; i < n && src[i]; i++)
		dest[i] = src[i];
	for (; i < n; i++)
		dest[i] = 0;
	return dest;
}

ssize_t strscpy(char *dest, const char *src, size_t size)
{
	size_t i = 0;
	if (!size)
		return -E2BIG;
	for (; i + 1 < size && src[i]; i++)
		dest[i] = src[i];
	dest[i] = 0;
	return src[i] ? -E2BIG : (ssize_t)i;
}

size_t strlcpy(char *dest, const char *src, size_t size)
{
	size_t len = strlen(src);
	if (size) {
		size_t n = len < size - 1 ? len : size - 1;
		memcpy(dest, src, n);
		dest[n] = 0;
	}
	return len;
}

char *kstrdup(const char *s, gfp_t gfp)
{
	size_t len = strlen(s) + 1;
	char *copy = kmalloc(len, gfp);
	if (copy)
		memcpy(copy, s, len);
	return copy;
}

struct out {
	char *buf;
	size_t size;
	size_t len;
};

static void put(struct out *o, char c)
{
	if (o->len + 1 < o->size)
		o->buf[o->len] = c;
	o->len++;
}

static void put_number(struct out *o, unsigned long long value, int base, bool upper, bool negative, int width, bool zero, bool left)
{
	char digits[24];
	int n = 0;
	const char *set = upper ? "0123456789ABCDEF" : "0123456789abcdef";
	do {
		digits[n++] = set[value % base];
		value /= base;
	} while (value && n < 24);
	int total = n + (negative ? 1 : 0);
	if (!left && !zero)
		for (int i = total; i < width; i++)
			put(o, ' ');
	if (negative)
		put(o, '-');
	if (!left && zero)
		for (int i = total; i < width; i++)
			put(o, '0');
	while (n)
		put(o, digits[--n]);
	if (left)
		for (int i = total; i < width; i++)
			put(o, ' ');
}

int vsnprintf(char *buf, size_t size, const char *fmt, va_list args)
{
	struct out o = { buf, size, 0 };
	while (*fmt) {
		if (*fmt != '%') {
			put(&o, *fmt++);
			continue;
		}
		fmt++;
		bool left = false, zero = false;
		for (;; fmt++) {
			if (*fmt == '-')
				left = true;
			else if (*fmt == '0')
				zero = true;
			else if (*fmt == '+' || *fmt == ' ' || *fmt == '#')
				;
			else
				break;
		}
		int width = 0;
		if (*fmt == '*') {
			width = va_arg(args, int);
			fmt++;
		}
		while (*fmt >= '0' && *fmt <= '9')
			width = width * 10 + (*fmt++ - '0');
		int precision = -1;
		if (*fmt == '.') {
			fmt++;
			precision = 0;
			if (*fmt == '*') {
				precision = va_arg(args, int);
				fmt++;
			}
			while (*fmt >= '0' && *fmt <= '9')
				precision = precision * 10 + (*fmt++ - '0');
		}
		int length = 0;
		while (*fmt == 'l' || *fmt == 'h' || *fmt == 'z' || *fmt == 'L' || *fmt == 't') {
			if (*fmt == 'l' || *fmt == 'z' || *fmt == 'L' || *fmt == 't')
				length++;
			fmt++;
		}
		char c = *fmt ? *fmt++ : 0;
		switch (c) {
		case 'd':
		case 'i': {
			long long v = length ? va_arg(args, long long) : va_arg(args, int);
			put_number(&o, v < 0 ? -(unsigned long long)v : (unsigned long long)v, 10, false, v < 0, width, zero, left);
			break;
		}
		case 'u':
		case 'x':
		case 'X':
		case 'o': {
			unsigned long long v = length ? va_arg(args, unsigned long long) : va_arg(args, unsigned int);
			put_number(&o, v, c == 'u' ? 10 : c == 'o' ? 8 : 16, c == 'X', false, width, zero, left);
			break;
		}
		case 'c':
			put(&o, (char)va_arg(args, int));
			break;
		case 's': {
			const char *s = va_arg(args, const char *);
			if (!s)
				s = "(null)";
			size_t n = precision >= 0 ? strnlen(s, precision) : strlen(s);
			if (!left)
				for (int i = (int)n; i < width; i++)
					put(&o, ' ');
			for (size_t i = 0; i < n; i++)
				put(&o, s[i]);
			if (left)
				for (int i = (int)n; i < width; i++)
					put(&o, ' ');
			break;
		}
		case 'p': {
			const void *p = va_arg(args, const void *);
			if ((*fmt == 'M' || *fmt == 'm') && p) {
				const u8 *mac = p;
				bool colons = *fmt == 'M';
				fmt++;
				if (*fmt == 'F' || *fmt == 'R')
					fmt++;
				for (int i = 0; i < 6; i++) {
					if (i && colons)
						put(&o, ':');
					put_number(&o, mac[i], 16, false, false, 2, true, false);
				}
				break;
			}
			while ((*fmt >= 'a' && *fmt <= 'z') || (*fmt >= 'A' && *fmt <= 'Z'))
				fmt++;
			put(&o, '0');
			put(&o, 'x');
			put_number(&o, (unsigned long)p, 16, false, false, 0, false, false);
			break;
		}
		case '%':
			put(&o, '%');
			break;
		default:
			break;
		}
	}
	if (size)
		buf[o.len < size ? o.len : size - 1] = 0;
	return (int)o.len;
}

int snprintf(char *buf, size_t size, const char *fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	int n = vsnprintf(buf, size, fmt, args);
	va_end(args);
	return n;
}

int scnprintf(char *buf, size_t size, const char *fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	int n = vsnprintf(buf, size, fmt, args);
	va_end(args);
	if (!size)
		return 0;
	return n < (int)size ? n : (int)size - 1;
}

int sprintf(char *buf, const char *fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	int n = vsnprintf(buf, 4096, fmt, args);
	va_end(args);
	return n;
}

static u32 level_of(const char **fmt)
{
	const char *f = *fmt;
	if (f[0] == '<' && f[1] >= '0' && f[1] <= '7' && f[2] == '>') {
		*fmt = f + 3;
		return (u32)(f[1] - '0');
	}
	return 6;
}

static void emit(u32 level, const char *prefix, const char *fmt, va_list args)
{
	char line[256];
	size_t used = 0;
	if (prefix) {
		used = strlen(prefix);
		if (used > 64)
			used = 64;
		memcpy(line, prefix, used);
		line[used++] = ':';
		line[used++] = ' ';
	}
	vsnprintf(line + used, sizeof(line) - used, fmt, args);
	size_t len = strlen(line);
	while (len && line[len - 1] == '\n')
		line[--len] = 0;
	u32 mapped = level <= 3 ? 0 : level == 4 ? 1 : level == 7 ? 3 : 2;
	hamix_dev_log(mapped, line, len);
}

int printk(const char *fmt, ...)
{
	va_list args;
	u32 level = level_of(&fmt);
	va_start(args, fmt);
	emit(level, NULL, fmt, args);
	va_end(args);
	return 0;
}

void linuxkpi_dev_printk(u32 level, const struct device *dev, const char *fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	emit(level, dev ? dev_name(dev) : NULL, fmt, args);
	va_end(args);
}

void linuxkpi_netdev_printk(u32 level, const struct net_device *dev, const char *fmt, ...)
{
	va_list args;
	va_start(args, fmt);
	emit(level, dev ? dev->name : NULL, fmt, args);
	va_end(args);
}

void linuxkpi_warn(const char *file, int line)
{
	printk(KERN_WARNING "linuxkpi: WARN_ON at %s:%d\n", file, line);
}

unsigned long linuxkpi_irq_save(void)
{
	unsigned long flags;
#if defined(__x86_64__)
	__asm__ __volatile__("pushfq; popq %0; cli" : "=r"(flags) : : "memory");
#elif defined(__aarch64__)
	__asm__ __volatile__("mrs %0, daif; msr daifset, #2" : "=r"(flags) : : "memory");
#else
	__asm__ __volatile__("csrrci %0, sstatus, 2" : "=r"(flags) : : "memory");
#endif
	return flags;
}

void linuxkpi_irq_restore(unsigned long flags)
{
#if defined(__x86_64__)
	if (flags & (1UL << 9))
		__asm__ __volatile__("sti" : : : "memory");
#elif defined(__aarch64__)
	__asm__ __volatile__("msr daif, %0" : : "r"(flags) : "memory");
#else
	if (flags & 2)
		__asm__ __volatile__("csrsi sstatus, 2" : : : "memory");
#endif
}

void linuxkpi_yield(void)
{
	if (hamix_in_interrupt())
		cpu_relax();
	else
		hamix_mdelay(1);
}

void msleep(unsigned int ms)
{
	hamix_mdelay(ms ? ms : 1);
}

static struct mutex rtnl_mutex;

void rtnl_lock(void)
{
	mutex_lock(&rtnl_mutex);
}

void rtnl_unlock(void)
{
	mutex_unlock(&rtnl_mutex);
}

int rtnl_is_locked(void)
{
	return mutex_is_locked(&rtnl_mutex);
}

static struct timer_list *timers;

void timer_setup(struct timer_list *t, void (*fn)(struct timer_list *), unsigned int flags)
{
	t->function = fn;
	t->flags = flags;
	t->hamix_pending = 0;
	t->hamix_next = NULL;
	t->expires = 0;
}

static bool unlink_timer(struct timer_list *t)
{
	struct timer_list **at = &timers;
	while (*at) {
		if (*at == t) {
			*at = t->hamix_next;
			t->hamix_next = NULL;
			t->hamix_pending = 0;
			return true;
		}
		at = &(*at)->hamix_next;
	}
	t->hamix_pending = 0;
	return false;
}

int mod_timer(struct timer_list *t, unsigned long expires)
{
	unsigned long flags = linuxkpi_irq_save();
	bool was = unlink_timer(t);
	t->expires = expires;
	t->hamix_pending = 1;
	t->hamix_next = timers;
	timers = t;
	linuxkpi_irq_restore(flags);
	return was;
}

int del_timer(struct timer_list *t)
{
	unsigned long flags = linuxkpi_irq_save();
	bool was = unlink_timer(t);
	linuxkpi_irq_restore(flags);
	return was;
}

int del_timer_sync(struct timer_list *t)
{
	return del_timer(t);
}

static void run_timers(void)
{
	for (int round = 0; round < 32; round++) {
		unsigned long flags = linuxkpi_irq_save();
		unsigned long now = jiffies;
		struct timer_list *due = NULL;
		for (struct timer_list *t = timers; t; t = t->hamix_next) {
			if (time_after_eq(now, t->expires)) {
				due = t;
				break;
			}
		}
		if (due)
			unlink_timer(due);
		linuxkpi_irq_restore(flags);
		if (!due)
			return;
		due->function(due);
	}
}

static void work_trampoline(void *context)
{
	struct work_struct *w = context;
	int state = __atomic_exchange_n(&w->hamix_pending, 0, __ATOMIC_ACQ_REL);
	if (state != 1)
		return;
	w->hamix_running = 1;
	w->func(w);
	w->hamix_running = 0;
}

bool schedule_work(struct work_struct *w)
{
	int expected = 0;
	if (!__atomic_compare_exchange_n(&w->hamix_pending, &expected, 1, false, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) {
		if (expected == 2) {
			w->hamix_pending = 1;
			return true;
		}
		return false;
	}
	if (hamix_schedule_work(work_trampoline, w) != 0) {
		w->hamix_pending = 0;
		return false;
	}
	return true;
}

bool queue_work(struct workqueue_struct *wq, struct work_struct *w)
{
	return schedule_work(w);
}

void linuxkpi_delayed_timer(struct timer_list *t)
{
	struct delayed_work *dw = container_of(t, struct delayed_work, timer);
	schedule_work(&dw->work);
}

bool queue_delayed_work(struct workqueue_struct *wq, struct delayed_work *dw, unsigned long delay)
{
	if (!delay)
		return schedule_work(&dw->work);
	if (timer_pending(&dw->timer) || work_pending(&dw->work))
		return false;
	mod_timer(&dw->timer, jiffies + delay);
	return true;
}

bool schedule_delayed_work(struct delayed_work *dw, unsigned long delay)
{
	return queue_delayed_work(system_wq, dw, delay);
}

bool cancel_work_sync(struct work_struct *w)
{
	int expected = 1;
	bool was = __atomic_compare_exchange_n(&w->hamix_pending, &expected, 2, false, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE);
	for (int i = 0; i < 1000 && w->hamix_running; i++)
		linuxkpi_yield();
	return was;
}

bool cancel_delayed_work(struct delayed_work *dw)
{
	bool timer = del_timer(&dw->timer);
	int expected = 1;
	bool work = __atomic_compare_exchange_n(&dw->work.hamix_pending, &expected, 2, false, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE);
	return timer || work;
}

bool cancel_delayed_work_sync(struct delayed_work *dw)
{
	bool timer = del_timer_sync(&dw->timer);
	return cancel_work_sync(&dw->work) || timer;
}

void flush_work(struct work_struct *w)
{
	int expected = 1;
	if (__atomic_compare_exchange_n(&w->hamix_pending, &expected, 2, false, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) {
		w->hamix_running = 1;
		w->func(w);
		w->hamix_running = 0;
	}
	for (int i = 0; i < 1000 && w->hamix_running; i++)
		linuxkpi_yield();
}

static struct pci_dev *pci_devs[MAX_PCI_DEVS];
static int pci_dev_count;
static struct pci_driver *bound_driver[MAX_PCI_DEVS];

int pci_read_config_dword(const struct pci_dev *dev, int where, u32 *val)
{
	if (where < 0 || where > 252) {
		*val = ~0U;
		return -EINVAL;
	}
	*val = hamix_pci_read32(&dev->handle, (u8)(where & ~3));
	return 0;
}

int pci_read_config_word(const struct pci_dev *dev, int where, u16 *val)
{
	if (where < 0 || where > 254) {
		*val = 0xFFFF;
		return -EINVAL;
	}
	*val = hamix_pci_read16(&dev->handle, (u8)where);
	return 0;
}

int pci_read_config_byte(const struct pci_dev *dev, int where, u8 *val)
{
	u32 v;
	int err = pci_read_config_dword(dev, where & ~3, &v);
	*val = (u8)(v >> ((where & 3) * 8));
	return err;
}

int pci_write_config_dword(const struct pci_dev *dev, int where, u32 val)
{
	if (where < 0 || where > 252)
		return -EINVAL;
	hamix_pci_write32(&dev->handle, (u8)(where & ~3), val);
	return 0;
}

int pci_write_config_word(const struct pci_dev *dev, int where, u16 val)
{
	if (where < 0 || where > 254)
		return -EINVAL;
	hamix_pci_write16(&dev->handle, (u8)where, val);
	return 0;
}

int pci_write_config_byte(const struct pci_dev *dev, int where, u8 val)
{
	u32 v;
	int err = pci_read_config_dword(dev, where & ~3, &v);
	if (err)
		return err;
	int shift = (where & 3) * 8;
	v = (v & ~(0xFFU << shift)) | ((u32)val << shift);
	return pci_write_config_dword(dev, where & ~3, v);
}

int pci_find_capability(struct pci_dev *dev, int cap)
{
	u16 status;
	pci_read_config_word(dev, PCI_STATUS, &status);
	if (!(status & 0x10))
		return 0;
	u8 pos;
	pci_read_config_byte(dev, PCI_CAPABILITY_LIST, &pos);
	for (int guard = 0; pos >= 0x40 && guard < 48; guard++) {
		u8 id, next;
		pci_read_config_byte(dev, pos, &id);
		pci_read_config_byte(dev, pos + 1, &next);
		if (id == cap)
			return pos;
		pos = next & ~3;
	}
	return 0;
}

int pcie_capability_read_word(struct pci_dev *dev, int pos, u16 *val)
{
	*val = 0;
	if (!dev->pcie_cap)
		return -EINVAL;
	return pci_read_config_word(dev, dev->pcie_cap + pos, val);
}

int pcie_capability_write_word(struct pci_dev *dev, int pos, u16 val)
{
	if (!dev->pcie_cap)
		return -EINVAL;
	return pci_write_config_word(dev, dev->pcie_cap + pos, val);
}

int pcie_capability_clear_and_set_word(struct pci_dev *dev, int pos, u16 clear, u16 set)
{
	u16 val;
	int err = pcie_capability_read_word(dev, pos, &val);
	if (err)
		return err;
	return pcie_capability_write_word(dev, pos, (val & ~clear) | set);
}

static void command_bits(struct pci_dev *dev, u16 clear, u16 set)
{
	u16 cmd;
	pci_read_config_word(dev, PCI_COMMAND, &cmd);
	pci_write_config_word(dev, PCI_COMMAND, (cmd & ~clear) | set);
}

int pci_enable_device_mem(struct pci_dev *dev)
{
	command_bits(dev, 0, PCI_COMMAND_MEMORY);
	return 0;
}

void pci_disable_device(struct pci_dev *dev)
{
	command_bits(dev, PCI_COMMAND_MASTER, 0);
}

void pci_set_master(struct pci_dev *dev)
{
	command_bits(dev, 0, PCI_COMMAND_MASTER);
}

void pci_clear_master(struct pci_dev *dev)
{
	command_bits(dev, PCI_COMMAND_MASTER, 0);
}

unsigned long pci_resource_flags(struct pci_dev *dev, int bar)
{
	if (bar < 0 || bar > 5)
		return 0;
	for (int i = 0; i <= bar; i++) {
		u32 raw;
		pci_read_config_dword(dev, 0x10 + i * 4, &raw);
		bool io = raw & 1;
		bool wide = !io && ((raw >> 1) & 3) == 2;
		if (i == bar) {
			u64 len = 0;
			hamix_pci_bar(&dev->handle, (u8)bar, &len);
			if (!len)
				return 0;
			return io ? IORESOURCE_IO : (IORESOURCE_MEM | (wide ? IORESOURCE_MEM_64 : 0));
		}
		if (wide)
			i++;
		if (i == bar)
			return 0;
	}
	return 0;
}

resource_size_t pci_resource_start(struct pci_dev *dev, int bar)
{
	u64 len = 0;
	if (bar < 0 || bar > 5)
		return 0;
	return hamix_pci_bar(&dev->handle, (u8)bar, &len);
}

resource_size_t pci_resource_len(struct pci_dev *dev, int bar)
{
	u64 len = 0;
	if (bar < 0 || bar > 5)
		return 0;
	hamix_pci_bar(&dev->handle, (u8)bar, &len);
	return len;
}

int pci_select_bars(struct pci_dev *dev, unsigned long flags)
{
	int bars = 0;
	for (int i = 0; i < 6; i++)
		if (pci_resource_flags(dev, i) & flags)
			bars |= 1 << i;
	return bars;
}

int pci_enable_msi(struct pci_dev *dev)
{
	if (hamix_pci_msi_enable(&dev->handle) != 1)
		return -EINVAL;
	dev->msi_enabled = 1;
	return 0;
}

void pci_disable_msi(struct pci_dev *dev)
{
	dev->msi_enabled = 0;
}

struct mapping {
	void *addr;
	size_t len;
};
static struct mapping maps[MAX_MAPS];

void *ioremap(phys_addr_t phys, size_t size)
{
	if (!phys || !size)
		return NULL;
	void *addr = hamix_ioremap(phys, size);
	if (!addr)
		return NULL;
	for (int i = 0; i < MAX_MAPS; i++) {
		if (!maps[i].addr) {
			maps[i].addr = addr;
			maps[i].len = size;
			break;
		}
	}
	return addr;
}

void iounmap(volatile void __iomem *addr)
{
	for (int i = 0; i < MAX_MAPS; i++) {
		if (maps[i].addr == addr) {
			hamix_iounmap(maps[i].addr, maps[i].len);
			maps[i].addr = NULL;
			return;
		}
	}
}

void *dma_alloc_coherent(struct device *dev, size_t size, dma_addr_t *handle, gfp_t gfp)
{
	u64 phys = 0;
	void *cpu = hamix_dma_alloc(size, &phys);
	if (!cpu)
		return NULL;
	memset(cpu, 0, size);
	*handle = phys;
	return cpu;
}

void dma_free_coherent(struct device *dev, size_t size, void *cpu, dma_addr_t handle)
{
	if (cpu)
		hamix_dma_free(cpu, size);
}

struct page *alloc_pages(gfp_t gfp, unsigned int order)
{
	struct page *page = kzalloc(sizeof(*page), gfp);
	if (!page)
		return NULL;
	u64 phys = 0;
	page->hamix_virt = hamix_dma_alloc(PAGE_SIZE << order, &phys);
	if (!page->hamix_virt) {
		kfree(page);
		return NULL;
	}
	page->hamix_phys = phys;
	atomic_set(&page->hamix_refs, 1);
	return page;
}

void put_page(struct page *page)
{
	if (!page || !atomic_dec_and_test(&page->hamix_refs))
		return;
	hamix_dma_free(page->hamix_virt, PAGE_SIZE);
	kfree(page);
}

static void *chunk_free_list;

static void *chunk_get(void)
{
	unsigned long flags = linuxkpi_irq_save();
	void *chunk = chunk_free_list;
	if (chunk)
		chunk_free_list = *(void **)chunk;
	linuxkpi_irq_restore(flags);
	if (chunk)
		return chunk;
	u64 phys = 0;
	u8 *page = hamix_dma_alloc(PAGE_SIZE, &phys);
	if (!page)
		return NULL;
	flags = linuxkpi_irq_save();
	*(void **)(page + CHUNK) = chunk_free_list;
	chunk_free_list = page + CHUNK;
	linuxkpi_irq_restore(flags);
	return page;
}

static void chunk_put(void *chunk)
{
	unsigned long flags = linuxkpi_irq_save();
	*(void **)chunk = chunk_free_list;
	chunk_free_list = chunk;
	linuxkpi_irq_restore(flags);
}

struct sk_buff *linuxkpi_alloc_skb(unsigned int size)
{
	struct sk_buff *skb = hamix_kzalloc(sizeof(*skb));
	if (!skb)
		return NULL;
	unsigned int capacity = size <= CHUNK ? CHUNK : (unsigned int)ALIGN(size, PAGE_SIZE);
	u64 phys = 0;
	void *data = size <= CHUNK ? chunk_get() : hamix_dma_alloc(capacity, &phys);
	if (!data) {
		hamix_kfree(skb);
		return NULL;
	}
	skb->head = data;
	skb->data = data;
	skb->end = capacity;
	skb->truesize = capacity + sizeof(*skb);
	skb->hamix_alloc = capacity;
	atomic_set(&skb->users, 1);
	return skb;
}

void linuxkpi_free_skb(struct sk_buff *skb)
{
	if (!skb || !atomic_dec_and_test(&skb->users))
		return;
	for (int i = 0; i < skb_shinfo(skb)->nr_frags; i++)
		put_page(skb_shinfo(skb)->frags[i].bv_page);
	if (skb->hamix_alloc <= CHUNK)
		chunk_put(skb->head);
	else
		hamix_dma_free(skb->head, skb->hamix_alloc);
	hamix_kfree(skb);
}

u32 crc32_le(u32 crc, const unsigned char *p, size_t len)
{
	while (len--) {
		crc ^= *p++;
		for (int i = 0; i < 8; i++)
			crc = (crc >> 1) ^ (0xEDB88320U & -(crc & 1));
	}
	return crc;
}

void eth_random_addr(u8 *addr)
{
	u64 seed = hamix_uptime_ms() * 6364136223846793005ULL + 1442695040888963407ULL;
	for (int i = 0; i < ETH_ALEN; i++) {
		seed = seed * 6364136223846793005ULL + 1442695040888963407ULL;
		addr[i] = (u8)(seed >> 33);
	}
	addr[0] &= 0xfe;
	addr[0] |= 0x02;
}

__be16 eth_type_trans(struct sk_buff *skb, struct net_device *dev)
{
	skb->dev = dev;
	skb_reset_mac_header(skb);
	struct ethhdr *eth = (struct ethhdr *)skb->data;
	skb_pull(skb, ETH_HLEN);
	if (is_multicast_ether_addr(eth->h_dest))
		skb->pkt_type = is_broadcast_ether_addr(eth->h_dest) ? PACKET_BROADCAST : PACKET_MULTICAST;
	else if (dev && !ether_addr_equal(eth->h_dest, dev->dev_addr))
		skb->pkt_type = PACKET_OTHERHOST;
	else
		skb->pkt_type = PACKET_HOST;
	return eth->h_proto;
}

static void deliver(struct sk_buff *skb)
{
	struct net_device *dev = skb->dev;
	if (!dev || dev->hamix_id <= 0) {
		linuxkpi_free_skb(skb);
		return;
	}
	u8 *frame = skb_mac_header(skb);
	size_t len = skb->len + (size_t)(skb->data - frame);
	if (skb->vlan_present && len >= 12) {
		u8 *tagged = hamix_kmalloc(len + VLAN_HLEN);
		if (tagged) {
			memcpy(tagged, frame, 12);
			tagged[12] = 0x81;
			tagged[13] = 0x00;
			tagged[14] = (u8)(skb->vlan_tci >> 8);
			tagged[15] = (u8)skb->vlan_tci;
			memcpy(tagged + 16, frame + 12, len - 12);
			hamix_net_receive(dev->hamix_id, tagged, len + VLAN_HLEN);
			hamix_kfree(tagged);
		}
	} else {
		hamix_net_receive(dev->hamix_id, frame, len);
	}
	dev->stats.rx_packets++;
	linuxkpi_free_skb(skb);
}

gro_result_t napi_gro_receive(struct napi_struct *napi, struct sk_buff *skb)
{
	if (!skb->dev && napi)
		skb->dev = napi->dev;
	deliver(skb);
	return 0;
}

int netif_receive_skb(struct sk_buff *skb)
{
	deliver(skb);
	return 0;
}

static void napi_run(struct work_struct *w)
{
	struct napi_struct *napi = container_of(w, struct napi_struct, hamix_work);
	if (!test_bit(NAPI_STATE_SCHED, &napi->state) || test_bit(NAPI_STATE_DISABLE, &napi->state))
		return;
	int done = napi->poll(napi, napi->weight);
	if (done >= napi->weight && test_bit(NAPI_STATE_SCHED, &napi->state) && !test_bit(NAPI_STATE_DISABLE, &napi->state))
		schedule_work(&napi->hamix_work);
}

void netif_napi_add(struct net_device *dev, struct napi_struct *napi, int (*poll)(struct napi_struct *, int))
{
	napi->poll = poll;
	napi->dev = dev;
	napi->weight = NAPI_POLL_WEIGHT;
	napi->state = 0;
	set_bit(NAPI_STATE_SCHED, &napi->state);
	INIT_WORK(&napi->hamix_work, napi_run);
	dev->hamix_napi = napi;
}

bool napi_schedule_prep(struct napi_struct *napi)
{
	if (test_bit(NAPI_STATE_DISABLE, &napi->state))
		return false;
	return !test_and_set_bit(NAPI_STATE_SCHED, &napi->state);
}

void __napi_schedule(struct napi_struct *napi)
{
	schedule_work(&napi->hamix_work);
}

bool napi_complete_done(struct napi_struct *napi, int work_done)
{
	clear_bit(NAPI_STATE_SCHED, &napi->state);
	return true;
}

void napi_enable(struct napi_struct *napi)
{
	clear_bit(NAPI_STATE_DISABLE, &napi->state);
	clear_bit(NAPI_STATE_SCHED, &napi->state);
}

void napi_disable(struct napi_struct *napi)
{
	set_bit(NAPI_STATE_DISABLE, &napi->state);
	cancel_work_sync(&napi->hamix_work);
	set_bit(NAPI_STATE_SCHED, &napi->state);
}

void napi_synchronize(const struct napi_struct *napi)
{
	for (int i = 0; i < 1000 && napi->hamix_work.hamix_running; i++)
		linuxkpi_yield();
}

struct net_device *alloc_etherdev_mqs(int sizeof_priv, unsigned int txqs, unsigned int rxqs)
{
	size_t base = ALIGN(sizeof(struct net_device), 64);
	struct net_device *dev = hamix_kzalloc(base + sizeof_priv);
	if (!dev)
		return NULL;
	dev->hamix_priv = (u8 *)dev + base;
	dev->dev_addr = dev->hamix_addr;
	dev->addr_len = ETH_ALEN;
	dev->mtu = ETH_DATA_LEN;
	dev->min_mtu = ETH_MIN_MTU;
	dev->max_mtu = ETH_DATA_LEN;
	dev->hard_header_len = ETH_HLEN;
	dev->flags = IFF_BROADCAST | IFF_MULTICAST;
	memset(dev->broadcast, 0xff, ETH_ALEN);
	INIT_LIST_HEAD(&dev->uc.list);
	INIT_LIST_HEAD(&dev->mc.list);
	strscpy(dev->name, "eth%d", sizeof(dev->name));
	return dev;
}

void free_netdev(struct net_device *dev)
{
	hamix_kfree(dev);
}

void linuxkpi_netif_carrier(struct net_device *dev, int on)
{
	dev->hamix_carrier = on;
	if (dev->hamix_id > 0)
		hamix_net_carrier(dev->hamix_id, on);
}

static const char *current_driver = "linuxkpi";

static int net_transmit(u64 context, const u8 *frame, size_t len)
{
	struct net_device *dev = (struct net_device *)context;
	if (!dev->hamix_running || netif_queue_stopped(dev) || !len || len > dev->mtu + VLAN_ETH_HLEN)
		return -EBUSY;
	struct sk_buff *skb = linuxkpi_alloc_skb((unsigned int)len + NET_SKB_PAD);
	if (!skb)
		return -ENOMEM;
	skb_reserve(skb, NET_SKB_PAD);
	skb_put_data(skb, frame, (unsigned int)len);
	skb->dev = dev;
	skb_reset_mac_header(skb);
	skb_set_network_header(skb, ETH_HLEN);
	skb->protocol = len >= ETH_HLEN ? ((const struct ethhdr *)frame)->h_proto : 0;
	skb->ip_summed = CHECKSUM_NONE;
	netdev_tx_t rc = dev->netdev_ops->ndo_start_xmit(skb, dev);
	if (rc != NETDEV_TX_OK) {
		linuxkpi_free_skb(skb);
		return -EBUSY;
	}
	dev->stats.tx_packets++;
	return 0;
}

static void net_set_enabled(u64 context, int enabled)
{
}

static int net_open(struct net_device *dev)
{
	int err = 0;
	rtnl_lock();
	if (dev->netdev_ops->ndo_open)
		err = dev->netdev_ops->ndo_open(dev);
	if (!err) {
		dev->hamix_running = 1;
		dev->flags |= IFF_UP;
		if (dev->netdev_ops->ndo_set_rx_mode)
			dev->netdev_ops->ndo_set_rx_mode(dev);
	}
	rtnl_unlock();
	return err;
}

int register_netdev(struct net_device *dev)
{
	struct hamix_net_ops ops;
	memset(&ops, 0, sizeof(ops));
	ops.abi = 1;
	ops.context = (u64)dev;
	memcpy(ops.mac, dev->dev_addr, ETH_ALEN);
	ops.transmit = net_transmit;
	ops.set_enabled = net_set_enabled;
	if (dev->netdev_ops->ndo_init) {
		int err = dev->netdev_ops->ndo_init(dev);
		if (err)
			return err;
	}
	int id = hamix_register_netdev(current_driver, strlen(current_driver), &ops);
	if (id <= 0)
		return -ENODEV;
	dev->hamix_id = id;
	dev->hamix_registered = 1;
	strscpy(dev->name, current_driver, sizeof(dev->name));
	int err = net_open(dev);
	if (err)
		netdev_err(dev, "open failed: %d\n", err);
	if (dev->hamix_carrier)
		hamix_net_carrier(id, 1);
	return 0;
}

void unregister_netdev(struct net_device *dev)
{
	rtnl_lock();
	if (dev->hamix_running && dev->netdev_ops->ndo_stop)
		dev->netdev_ops->ndo_stop(dev);
	dev->hamix_running = 0;
	dev->flags &= ~IFF_UP;
	rtnl_unlock();
	if (dev->hamix_id > 0)
		hamix_unregister_netdev(dev->hamix_id);
	dev->hamix_id = 0;
	dev->hamix_registered = 0;
}

struct irq_entry {
	irq_handler_t handler;
	void *dev_id;
	unsigned int irq;
	int disabled;
	int used;
};
static struct irq_entry irqs[MAX_IRQS];

static int irq_trampoline(void *context)
{
	struct irq_entry *e = context;
	if (!e->used || e->disabled)
		return IRQ_NONE;
	return e->handler((int)e->irq, e->dev_id) == IRQ_NONE ? IRQ_NONE : IRQ_HANDLED;
}

static struct pci_dev *pdev_of_irq(unsigned int irq)
{
	for (int i = 0; i < pci_dev_count; i++)
		if (pci_devs[i] && pci_devs[i]->irq == irq)
			return pci_devs[i];
	return NULL;
}

int request_irq(unsigned int irq, irq_handler_t handler, unsigned long flags, const char *name, void *dev)
{
	struct pci_dev *pdev = pdev_of_irq(irq);
	if (!pdev)
		return -EINVAL;
	for (int i = 0; i < MAX_IRQS; i++) {
		if (irqs[i].used && irqs[i].irq == irq) {
			irqs[i].handler = handler;
			irqs[i].dev_id = dev;
			irqs[i].disabled = 0;
			return 0;
		}
	}
	for (int i = 0; i < MAX_IRQS; i++) {
		if (irqs[i].used)
			continue;
		irqs[i].handler = handler;
		irqs[i].dev_id = dev;
		irqs[i].irq = irq;
		irqs[i].disabled = 0;
		irqs[i].used = 1;
		int vector = hamix_request_irq(&pdev->handle, irq_trampoline, &irqs[i]);
		if (vector < 0) {
			irqs[i].used = 0;
			return vector;
		}
		pdev->hamix_irq_line = vector;
		return 0;
	}
	return -ENOSPC;
}

void free_irq(unsigned int irq, void *dev)
{
	for (int i = 0; i < MAX_IRQS; i++)
		if (irqs[i].used && irqs[i].irq == irq)
			irqs[i].disabled = 1;
}

void disable_irq(unsigned int irq)
{
	for (int i = 0; i < MAX_IRQS; i++)
		if (irqs[i].used && irqs[i].irq == irq)
			irqs[i].disabled = 1;
}

void enable_irq(unsigned int irq)
{
	for (int i = 0; i < MAX_IRQS; i++)
		if (irqs[i].used && irqs[i].irq == irq)
			irqs[i].disabled = 0;
}

static bool polling;

static void linuxkpi_poll(void *context)
{
	run_timers();
}

static u32 claim_class(u32 class)
{
	switch (class >> 16) {
	case 0x01:
		return HAMIX_CLASS_BLOCK;
	case 0x03:
		return HAMIX_CLASS_DISPLAY;
	case 0x04:
		return HAMIX_CLASS_AUDIO;
	case 0x0c:
		return HAMIX_CLASS_USB_HCD;
	default:
		return HAMIX_CLASS_NETWORK;
	}
}

static const struct pci_device_id *match(const struct pci_device_id *table, struct pci_dev *dev)
{
	for (; table->vendor || table->subvendor || table->class_mask; table++) {
		if (table->vendor != PCI_ANY_ID && table->vendor != dev->vendor)
			continue;
		if (table->device != PCI_ANY_ID && table->device != dev->device)
			continue;
		if (table->subvendor != PCI_ANY_ID && table->subvendor != dev->subsystem_vendor)
			continue;
		if (table->subdevice != PCI_ANY_ID && table->subdevice != dev->subsystem_device)
			continue;
		if ((table->class ^ dev->class) & table->class_mask)
			continue;
		return table;
	}
	return NULL;
}

static struct pci_dev *make_pci_dev(const struct hamix_pci_handle *handle, int index)
{
	struct pci_dev *dev = hamix_kzalloc(sizeof(*dev));
	if (!dev)
		return NULL;
	dev->handle = *handle;
	dev->vendor = handle->vendor;
	dev->device = handle->device_id;
	dev->devfn = (handle->device << 3) | handle->function;
	dev->hamix_bus.number = handle->bus;
	dev->bus = &dev->hamix_bus;
	u32 v;
	pci_read_config_dword(dev, 0x08, &v);
	dev->revision = (u8)v;
	dev->class = v >> 8;
	pci_read_config_dword(dev, 0x2c, &v);
	dev->subsystem_vendor = (u16)v;
	dev->subsystem_device = (u16)(v >> 16);
	dev->irq = 1000 + index;
	dev->pcie_cap = (u8)pci_find_capability(dev, PCI_CAP_ID_EXP);
	dev->pm_cap = (u8)pci_find_capability(dev, PCI_CAP_ID_PM);
	dev->current_state = PCI_D0;
	dev->dev.dma_mask_value = DMA_BIT_MASK(32);
	dev->dev.dma_mask = &dev->dev.dma_mask_value;
	snprintf(dev->hamix_name, sizeof(dev->hamix_name), "0000:%02x:%02x.%x", handle->bus, handle->device, handle->function);
	dev->dev.init_name = dev->hamix_name;
	return dev;
}

int pci_register_driver(struct pci_driver *drv)
{
	struct hamix_pci_handle handle;
	int bound = 0;
	current_driver = drv->name ? drv->name : "linuxkpi";
	for (u32 index = 0; hamix_pci_find_class(0xFF, 0xFF, 0xFF, index, &handle) == 0; index++) {
		if (pci_dev_count >= MAX_PCI_DEVS)
			break;
		struct pci_dev probe_dev;
		memset(&probe_dev, 0, sizeof(probe_dev));
		probe_dev.handle = handle;
		probe_dev.vendor = handle.vendor;
		probe_dev.device = handle.device_id;
		u32 v;
		pci_read_config_dword(&probe_dev, 0x08, &v);
		probe_dev.class = v >> 8;
		pci_read_config_dword(&probe_dev, 0x2c, &v);
		probe_dev.subsystem_vendor = (u16)v;
		probe_dev.subsystem_device = (u16)(v >> 16);
		const struct pci_device_id *id = match(drv->id_table, &probe_dev);
		if (!id)
			continue;
		struct pci_dev *dev = make_pci_dev(&handle, pci_dev_count);
		if (!dev)
			return -ENOMEM;
		pci_devs[pci_dev_count] = dev;
		bound_driver[pci_dev_count] = drv;
		pci_dev_count++;
		dev->dev.driver = &drv->driver;
		int err = drv->probe(dev, id);
		if (err) {
			dev_err(&dev->dev, "probe failed: %d\n", err);
			pci_dev_count--;
			pci_devs[pci_dev_count] = NULL;
			bound_driver[pci_dev_count] = NULL;
			hamix_kfree(dev);
			continue;
		}
		hamix_claim_device(claim_class(dev->class), current_driver, strlen(current_driver), &dev->handle, polling ? NULL : linuxkpi_poll, NULL);
		polling = true;
		bound++;
	}
	return bound ? 0 : -ENODEV;
}

void pci_unregister_driver(struct pci_driver *drv)
{
	for (int i = pci_dev_count - 1; i >= 0; i--) {
		if (bound_driver[i] != drv || !pci_devs[i])
			continue;
		if (drv->remove)
			drv->remove(pci_devs[i]);
		hamix_kfree(pci_devs[i]);
		pci_devs[i] = NULL;
		bound_driver[i] = NULL;
	}
	pci_dev_count = 0;
	hamix_free_irq();
}

int linuxkpi_module_init(int (*fn)(void))
{
	mutex_init(&rtnl_mutex);
	return fn();
}

void linuxkpi_module_exit(void (*fn)(void))
{
	fn();
}
