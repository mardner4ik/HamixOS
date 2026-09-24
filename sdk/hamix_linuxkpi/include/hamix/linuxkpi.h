#ifndef HAMIX_LINUXKPI_H
#define HAMIX_LINUXKPI_H

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdarg.h>

#define HAMIX_KPI_VERSION 6

typedef int8_t s8;
typedef uint8_t u8;
typedef int16_t s16;
typedef uint16_t u16;
typedef int32_t s32;
typedef uint32_t u32;
typedef int64_t s64;
typedef uint64_t u64;
typedef int8_t __s8;
typedef uint8_t __u8;
typedef int16_t __s16;
typedef uint16_t __u16;
typedef int32_t __s32;
typedef uint32_t __u32;
typedef int64_t __s64;
typedef uint64_t __u64;
typedef u16 __le16;
typedef u32 __le32;
typedef u64 __le64;
typedef u16 __be16;
typedef u32 __be32;
typedef u64 __be64;
typedef u16 __sum16;
typedef u32 __wsum;
typedef u64 dma_addr_t;
typedef u64 phys_addr_t;
typedef u64 resource_size_t;
typedef unsigned int gfp_t;
typedef long ssize_t;
typedef s64 ktime_t;
typedef u64 netdev_features_t;
typedef int irqreturn_t;
typedef long long loff_t;
typedef unsigned int pci_channel_state_t;
typedef unsigned int pci_ers_result_t;
typedef int pci_power_t;
typedef unsigned long kernel_ulong_t;

#define __iomem
#define __user
#define __force
#define __bitwise
#define __must_check
#define __init
#define __exit
#define __devinit
#define __read_mostly
#define __always_unused __attribute__((unused))
#define __maybe_unused __attribute__((unused))
#undef __always_inline
#define __always_inline inline __attribute__((always_inline))
#define noinline __attribute__((noinline))
#define __packed __attribute__((packed))
#define __aligned(x) __attribute__((aligned(x)))
#define __printf(a, b) __attribute__((format(printf, a, b)))
#define __cold
#define __rcu
#define ____cacheline_aligned_in_smp __attribute__((aligned(64)))
#define ____cacheline_aligned __attribute__((aligned(64)))
#define fallthrough __attribute__((fallthrough))
#define likely(x) __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define barrier() __asm__ __volatile__("" ::: "memory")
#define READ_ONCE(x) (*(const volatile __typeof__(x) *)&(x))
#define WRITE_ONCE(x, v) (*(volatile __typeof__(x) *)&(x) = (v))
#define ACCESS_ONCE(x) READ_ONCE(x)
#define EXPORT_SYMBOL(x)
#define EXPORT_SYMBOL_GPL(x)
#define MODULE_AUTHOR(x)
#define MODULE_DESCRIPTION(x)
#define MODULE_LICENSE(x)
#define MODULE_VERSION(x)
#define MODULE_FIRMWARE(x)
#define MODULE_PARM_DESC(a, b)
#define THIS_MODULE ((void *)0)
#ifndef KBUILD_MODNAME
#define KBUILD_MODNAME "module"
#endif
#define IS_ENABLED(x) 0
#define IS_REACHABLE(x) 0
#define PAGE_SIZE 4096UL
#define PAGE_SHIFT 12
#define PAGE_MASK (~(PAGE_SIZE - 1))
#define SMP_CACHE_BYTES 64
#define L1_CACHE_BYTES 64
#define HZ 1000
#define NR_CPUS 1
#define BITS_PER_LONG 64
#define BITS_PER_BYTE 8
#define U8_MAX 0xFF
#define U16_MAX 0xFFFF
#define U32_MAX 0xFFFFFFFFU
#define U64_MAX 0xFFFFFFFFFFFFFFFFULL
#define S32_MAX 0x7FFFFFFF
#define INT_MAX 0x7FFFFFFF
#define UINT_MAX 0xFFFFFFFFU
#define LONG_MAX 0x7FFFFFFFFFFFFFFFL
#define ULONG_MAX (~0UL)
#define NSEC_PER_SEC 1000000000LL
#define NSEC_PER_MSEC 1000000LL
#define NSEC_PER_USEC 1000LL
#define USEC_PER_SEC 1000000LL
#define MSEC_PER_SEC 1000L

#define EPERM 1
#define ENOENT 2
#define EIO 5
#define ENXIO 6
#define E2BIG 7
#define EAGAIN 11
#define ENOMEM 12
#define EFAULT 14
#define EBUSY 16
#define EEXIST 17
#define ENODEV 19
#define EINVAL 22
#define ENOSPC 28
#define ERANGE 34
#define ENOSYS 38
#define ENODATA 61
#define ETIME 62
#define EOPNOTSUPP 95
#define ETIMEDOUT 110
#define EINPROGRESS 115
#define EADDRNOTAVAIL 99

#define GFP_KERNEL 0u
#define GFP_ATOMIC 1u
#define GFP_DMA 2u
#define GFP_DMA32 4u
#define __GFP_NOWARN 8u
#define __GFP_ZERO 16u
#define __GFP_COMP 32u

#define BIT(n) (1UL << (n))
#define BIT_ULL(n) (1ULL << (n))
#define GENMASK(h, l) (((~0UL) << (l)) & (~0UL >> (BITS_PER_LONG - 1 - (h))))
#define GENMASK_ULL(h, l) (((~0ULL) << (l)) & (~0ULL >> (63 - (h))))
#define BITS_TO_LONGS(n) (((n) + BITS_PER_LONG - 1) / BITS_PER_LONG)
#define DECLARE_BITMAP(name, bits) unsigned long name[BITS_TO_LONGS(bits)]
#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#define DIV_ROUND_UP(n, d) (((n) + (d) - 1) / (d))
#define DIV_ROUND_CLOSEST(x, d) (((x) + ((d) / 2)) / (d))
#define ALIGN(x, a) (((x) + ((a) - 1)) & ~((__typeof__(x))(a) - 1))
#define PTR_ALIGN(p, a) ((__typeof__(p))ALIGN((unsigned long)(p), (a)))
#define IS_ALIGNED(x, a) (((x) & ((__typeof__(x))(a) - 1)) == 0)
#define roundup(x, y) ((((x) + ((y) - 1)) / (y)) * (y))
#define upper_32_bits(n) ((u32)(((u64)(n)) >> 32))
#define lower_32_bits(n) ((u32)((n) & 0xffffffff))
#define BUILD_BUG_ON(c) _Static_assert(!(c), "BUILD_BUG_ON")
#define BUILD_BUG_ON_ZERO(e) 0
#define container_of(ptr, type, member) ((type *)((char *)(ptr) - offsetof(type, member)))
#define sizeof_field(t, m) sizeof(((t *)0)->m)
#define FIELD_SIZEOF(t, m) sizeof_field(t, m)
#define __stringify_1(x) #x
#define __stringify(x) __stringify_1(x)
#define __MODULE_STRING(x) __stringify(x)
#define swap(a, b) do { __typeof__(a) __t = (a); (a) = (b); (b) = __t; } while (0)

#define min(a, b) ({ __typeof__(a) __a = (a); __typeof__(b) __b = (b); __a < __b ? __a : __b; })
#define max(a, b) ({ __typeof__(a) __a = (a); __typeof__(b) __b = (b); __a > __b ? __a : __b; })
#define min_t(t, a, b) ({ t __a = (a); t __b = (b); __a < __b ? __a : __b; })
#define max_t(t, a, b) ({ t __a = (a); t __b = (b); __a > __b ? __a : __b; })
#define clamp(v, lo, hi) min(max(v, lo), hi)
#define clamp_t(t, v, lo, hi) min_t(t, max_t(t, v, lo), hi)
#define clamp_val(v, lo, hi) clamp_t(__typeof__(v), v, lo, hi)
#define abs(x) ({ __typeof__(x) __x = (x); __x < 0 ? -__x : __x; })

#define do_div(n, base) ({ u32 __base = (base); u32 __rem = (u32)((n) % __base); (n) = (n) / __base; __rem; })

static inline u64 div_u64(u64 dividend, u32 divisor) { return dividend / divisor; }
static inline s64 div_s64(s64 dividend, s32 divisor) { return dividend / divisor; }
static inline u64 div64_u64(u64 dividend, u64 divisor) { return dividend / divisor; }
static inline u64 mul_u64_u32_div(u64 a, u32 mul, u32 divisor) { return (u64)(((unsigned __int128)a * mul) / divisor); }
static inline int fls(unsigned int x) { return x ? 32 - __builtin_clz(x) : 0; }
static inline int fls64(u64 x) { return x ? 64 - __builtin_clzll(x) : 0; }
static inline int ffs(int x) { return __builtin_ffs(x); }
static inline unsigned long __ffs(unsigned long x) { return __builtin_ctzl(x); }
static inline unsigned int hweight32(u32 w) { return __builtin_popcount(w); }
static inline unsigned int hweight16(u16 w) { return __builtin_popcount(w); }
static inline unsigned int hweight8(u8 w) { return __builtin_popcount(w); }
static inline bool is_power_of_2(unsigned long n) { return n != 0 && (n & (n - 1)) == 0; }
static inline unsigned long roundup_pow_of_two(unsigned long n) { return n <= 1 ? 1 : 1UL << (64 - __builtin_clzl(n - 1)); }
static inline unsigned long rounddown_pow_of_two(unsigned long n) { return n ? 1UL << (63 - __builtin_clzl(n)) : 0; }
static inline int ilog2(unsigned long n) { return n ? 63 - __builtin_clzl(n) : -1; }
static inline int order_base_2(unsigned long n) { return n <= 1 ? 0 : ilog2(n - 1) + 1; }

static inline u16 __swab16(u16 x) { return __builtin_bswap16(x); }
static inline u32 __swab32(u32 x) { return __builtin_bswap32(x); }
static inline u64 __swab64(u64 x) { return __builtin_bswap64(x); }
#define swab16 __swab16
#define swab32 __swab32
#define swab64 __swab64
#define cpu_to_le16(x) ((__le16)(u16)(x))
#define cpu_to_le32(x) ((__le32)(u32)(x))
#define cpu_to_le64(x) ((__le64)(u64)(x))
#define le16_to_cpu(x) ((u16)(__le16)(x))
#define le32_to_cpu(x) ((u32)(__le32)(x))
#define le64_to_cpu(x) ((u64)(__le64)(x))
#define cpu_to_be16(x) ((__be16)__builtin_bswap16((u16)(x)))
#define cpu_to_be32(x) ((__be32)__builtin_bswap32((u32)(x)))
#define cpu_to_be64(x) ((__be64)__swab64((u64)(x)))
#define be16_to_cpu(x) ((u16)__builtin_bswap16((u16)(x)))
#define be32_to_cpu(x) __swab32((u32)(__be32)(x))
#define be64_to_cpu(x) __swab64((u64)(__be64)(x))
#define le16_to_cpus(p) do { } while (0)
#define le32_to_cpus(p) do { } while (0)
#define cpu_to_le16s(p) do { } while (0)
#define cpu_to_le32s(p) do { } while (0)
#define cpu_to_be16s(p) do { *(p) = cpu_to_be16(*(p)); } while (0)
#define be16_to_cpus(p) do { *(p) = be16_to_cpu(*(p)); } while (0)
#define htons(x) cpu_to_be16(x)
#define ntohs(x) be16_to_cpu(x)
#define htonl(x) cpu_to_be32(x)
#define ntohl(x) be32_to_cpu(x)

extern void hamix_printk(const char *text, size_t len);
extern void hamix_dev_log(u32 level, const char *text, size_t len);
extern void *hamix_kmalloc(size_t size);
extern void *hamix_kzalloc(size_t size);
extern void hamix_kfree(void *ptr);
extern void *hamix_dma_alloc(size_t len, u64 *phys_out);
extern void hamix_dma_free(void *ptr, size_t len);
extern u64 hamix_virt_to_phys(const void *addr);
extern void *hamix_ioremap(u64 phys, size_t len);
extern void hamix_iounmap(void *addr, size_t len);
extern void hamix_udelay(u64 micros);
extern void hamix_mdelay(u64 millis);
extern u64 hamix_uptime_ms(void);
extern u32 hamix_kpi_version(void);
extern u32 hamix_param_u32(const char *name, size_t len, u32 fallback);

void *memcpy(void *dest, const void *src, size_t n);
void *memset(void *dest, int c, size_t n);
void *memmove(void *dest, const void *src, size_t n);
int memcmp(const void *a, const void *b, size_t n);
size_t strlen(const char *s);
size_t strnlen(const char *s, size_t max);
int strcmp(const char *a, const char *b);
int strncmp(const char *a, const char *b, size_t n);
char *strcpy(char *dest, const char *src);
char *strncpy(char *dest, const char *src, size_t n);
ssize_t strscpy(char *dest, const char *src, size_t size);
size_t strlcpy(char *dest, const char *src, size_t size);
char *kstrdup(const char *s, gfp_t gfp);
int snprintf(char *buf, size_t size, const char *fmt, ...) __printf(3, 4);
int vsnprintf(char *buf, size_t size, const char *fmt, va_list args);
int scnprintf(char *buf, size_t size, const char *fmt, ...) __printf(3, 4);
int sprintf(char *buf, const char *fmt, ...) __printf(2, 3);

#define KERN_EMERG "<0>"
#define KERN_ALERT "<1>"
#define KERN_CRIT "<2>"
#define KERN_ERR "<3>"
#define KERN_WARNING "<4>"
#define KERN_NOTICE "<5>"
#define KERN_INFO "<6>"
#define KERN_DEBUG "<7>"
#define KERN_CONT ""
#define KERN_SOH ""
#ifndef pr_fmt
#define pr_fmt(fmt) fmt
#endif
int printk(const char *fmt, ...) __printf(1, 2);
#define pr_err(fmt, ...) printk(KERN_ERR pr_fmt(fmt), ##__VA_ARGS__)
#define pr_warn(fmt, ...) printk(KERN_WARNING pr_fmt(fmt), ##__VA_ARGS__)
#define pr_notice(fmt, ...) printk(KERN_NOTICE pr_fmt(fmt), ##__VA_ARGS__)
#define pr_info(fmt, ...) printk(KERN_INFO pr_fmt(fmt), ##__VA_ARGS__)
#define pr_debug(fmt, ...) do { if (0) printk(KERN_DEBUG pr_fmt(fmt), ##__VA_ARGS__); } while (0)
#define pr_cont(fmt, ...) printk(fmt, ##__VA_ARGS__)
#define pr_err_once pr_err
#define pr_warn_once pr_warn
#define pr_info_once pr_info
#define printk_once printk
#define net_ratelimit() 1
#define printk_ratelimit() 1

#define DUMP_PREFIX_NONE 0
#define DUMP_PREFIX_ADDRESS 1
#define DUMP_PREFIX_OFFSET 2
static inline void print_hex_dump(const char *level, const char *prefix, int type, int rowsize, int groupsize, const void *buf, size_t len, bool ascii) {}

void linuxkpi_warn(const char *file, int line);
#define WARN_ON(c) ({ int __c = !!(c); if (unlikely(__c)) linuxkpi_warn(__FILE__, __LINE__); unlikely(__c); })
#define WARN_ON_ONCE(c) WARN_ON(c)
#define WARN(c, fmt, ...) ({ int __c = !!(c); if (unlikely(__c)) printk(KERN_WARNING fmt, ##__VA_ARGS__); unlikely(__c); })
#define WARN_ONCE(c, fmt, ...) WARN(c, fmt, ##__VA_ARGS__)
#define BUG() linuxkpi_warn(__FILE__, __LINE__)
#define BUG_ON(c) do { if (unlikely(c)) BUG(); } while (0)
#define might_sleep() do { } while (0)
#define lockdep_assert_held(l) do { } while (0)
#define ASSERT_RTNL() do { } while (0)

static inline void *kmalloc(size_t size, gfp_t flags) { return (flags & __GFP_ZERO) ? hamix_kzalloc(size) : hamix_kmalloc(size); }
static inline void *kzalloc(size_t size, gfp_t flags) { return hamix_kzalloc(size); }
static inline void *kcalloc(size_t n, size_t size, gfp_t flags) { return hamix_kzalloc(n * size); }
static inline void *kmalloc_array(size_t n, size_t size, gfp_t flags) { return hamix_kmalloc(n * size); }
static inline void kfree(const void *ptr) { if (ptr) hamix_kfree((void *)ptr); }
static inline void *vmalloc(size_t size) { return hamix_kmalloc(size); }
static inline void *vzalloc(size_t size) { return hamix_kzalloc(size); }
static inline void vfree(const void *ptr) { kfree(ptr); }
static inline void kvfree(const void *ptr) { kfree(ptr); }
static inline void *kvzalloc(size_t size, gfp_t flags) { return hamix_kzalloc(size); }
static inline void *devm_kzalloc(void *dev, size_t size, gfp_t flags) { return hamix_kzalloc(size); }
static inline unsigned long copy_from_user(void *to, const void __user *from, unsigned long n) { return n; }
static inline unsigned long copy_to_user(void __user *to, const void *from, unsigned long n) { return n; }

static inline void cpu_relax(void) { __builtin_ia32_pause(); }
#if defined(__x86_64__)
#define mb() __asm__ __volatile__("mfence" ::: "memory")
#define rmb() __asm__ __volatile__("lfence" ::: "memory")
#define wmb() __asm__ __volatile__("sfence" ::: "memory")
#elif defined(__aarch64__)
#undef cpu_relax
#define mb() __asm__ __volatile__("dmb sy" ::: "memory")
#define rmb() __asm__ __volatile__("dmb ld" ::: "memory")
#define wmb() __asm__ __volatile__("dmb st" ::: "memory")
#else
#define mb() __asm__ __volatile__("fence rw,rw" ::: "memory")
#define rmb() __asm__ __volatile__("fence r,r" ::: "memory")
#define wmb() __asm__ __volatile__("fence w,w" ::: "memory")
#endif
#define dma_rmb() rmb()
#define dma_wmb() wmb()
#define smp_mb() mb()
#define smp_rmb() rmb()
#define smp_wmb() wmb()
#define smp_mb__before_atomic() mb()
#define smp_mb__after_atomic() mb()
#define prefetch(x) __builtin_prefetch(x)
#define prefetchw(x) __builtin_prefetch(x, 1)
#define num_online_cpus() 1
#define smp_processor_id() 0
#define raw_smp_processor_id() 0

typedef struct { volatile int counter; } atomic_t;
typedef struct { volatile long counter; } atomic64_t;
#define ATOMIC_INIT(i) { (i) }
static inline int atomic_read(const atomic_t *v) { return __atomic_load_n(&v->counter, __ATOMIC_SEQ_CST); }
static inline void atomic_set(atomic_t *v, int i) { __atomic_store_n(&v->counter, i, __ATOMIC_SEQ_CST); }
static inline void atomic_inc(atomic_t *v) { __atomic_add_fetch(&v->counter, 1, __ATOMIC_SEQ_CST); }
static inline void atomic_dec(atomic_t *v) { __atomic_sub_fetch(&v->counter, 1, __ATOMIC_SEQ_CST); }
static inline int atomic_inc_return(atomic_t *v) { return __atomic_add_fetch(&v->counter, 1, __ATOMIC_SEQ_CST); }
static inline int atomic_dec_return(atomic_t *v) { return __atomic_sub_fetch(&v->counter, 1, __ATOMIC_SEQ_CST); }
static inline bool atomic_dec_and_test(atomic_t *v) { return atomic_dec_return(v) == 0; }
static inline void atomic_add(int i, atomic_t *v) { __atomic_add_fetch(&v->counter, i, __ATOMIC_SEQ_CST); }
static inline void atomic_sub(int i, atomic_t *v) { __atomic_sub_fetch(&v->counter, i, __ATOMIC_SEQ_CST); }

static inline void set_bit(long nr, volatile unsigned long *addr) { __atomic_or_fetch(&addr[nr / BITS_PER_LONG], 1UL << (nr % BITS_PER_LONG), __ATOMIC_SEQ_CST); }
static inline void clear_bit(long nr, volatile unsigned long *addr) { __atomic_and_fetch(&addr[nr / BITS_PER_LONG], ~(1UL << (nr % BITS_PER_LONG)), __ATOMIC_SEQ_CST); }
static inline bool test_bit(long nr, const volatile unsigned long *addr) { return (addr[nr / BITS_PER_LONG] >> (nr % BITS_PER_LONG)) & 1; }
static inline bool test_and_set_bit(long nr, volatile unsigned long *addr) { unsigned long m = 1UL << (nr % BITS_PER_LONG); return (__atomic_fetch_or(&addr[nr / BITS_PER_LONG], m, __ATOMIC_SEQ_CST) & m) != 0; }
static inline bool test_and_clear_bit(long nr, volatile unsigned long *addr) { unsigned long m = 1UL << (nr % BITS_PER_LONG); return (__atomic_fetch_and(&addr[nr / BITS_PER_LONG], ~m, __ATOMIC_SEQ_CST) & m) != 0; }
#define __set_bit set_bit
#define __clear_bit clear_bit
#define __test_and_set_bit test_and_set_bit
#define clear_bit_unlock clear_bit
#define test_and_set_bit_lock test_and_set_bit
static inline void bitmap_zero(unsigned long *dst, unsigned int nbits) { memset(dst, 0, BITS_TO_LONGS(nbits) * sizeof(long)); }
static inline unsigned long find_next_bit(const unsigned long *addr, unsigned long size, unsigned long offset) { for (; offset < size; offset++) if (test_bit(offset, addr)) return offset; return size; }
#define for_each_set_bit(bit, addr, size) for ((bit) = find_next_bit((addr), (size), 0); (bit) < (size); (bit) = find_next_bit((addr), (size), (bit) + 1))

struct list_head { struct list_head *next, *prev; };
#define LIST_HEAD_INIT(name) { &(name), &(name) }
#define LIST_HEAD(name) struct list_head name = LIST_HEAD_INIT(name)
static inline void INIT_LIST_HEAD(struct list_head *l) { l->next = l; l->prev = l; }
static inline void list_add_tail(struct list_head *n, struct list_head *h) { n->prev = h->prev; n->next = h; h->prev->next = n; h->prev = n; }
static inline void list_add(struct list_head *n, struct list_head *h) { n->next = h->next; n->prev = h; h->next->prev = n; h->next = n; }
static inline void list_del(struct list_head *e) { e->prev->next = e->next; e->next->prev = e->prev; e->next = e; e->prev = e; }
static inline bool list_empty(const struct list_head *h) { return h->next == h; }
#define list_entry(ptr, type, member) container_of(ptr, type, member)
#define list_first_entry(ptr, type, member) list_entry((ptr)->next, type, member)
#define list_for_each_entry(pos, head, member) for (pos = list_entry((head)->next, __typeof__(*pos), member); &pos->member != (head); pos = list_entry(pos->member.next, __typeof__(*pos), member))
#define list_for_each_entry_safe(pos, n, head, member) for (pos = list_entry((head)->next, __typeof__(*pos), member), n = list_entry(pos->member.next, __typeof__(*pos), member); &pos->member != (head); pos = n, n = list_entry(n->member.next, __typeof__(*n), member))

typedef struct { volatile int locked; unsigned long flags; } spinlock_t;
typedef spinlock_t raw_spinlock_t;
#define __SPIN_LOCK_UNLOCKED(x) { 0, 0 }
#define DEFINE_SPINLOCK(x) spinlock_t x = __SPIN_LOCK_UNLOCKED(x)
unsigned long linuxkpi_irq_save(void);
void linuxkpi_irq_restore(unsigned long flags);
void linuxkpi_yield(void);
static inline void spin_lock_init(spinlock_t *l) { l->locked = 0; }
static inline void spin_lock(spinlock_t *l) { while (__atomic_exchange_n(&l->locked, 1, __ATOMIC_ACQUIRE)) cpu_relax(); }
static inline void spin_unlock(spinlock_t *l) { __atomic_store_n(&l->locked, 0, __ATOMIC_RELEASE); }
static inline int spin_trylock(spinlock_t *l) { return !__atomic_exchange_n(&l->locked, 1, __ATOMIC_ACQUIRE); }
#define spin_lock_irqsave(l, f) do { (f) = linuxkpi_irq_save(); spin_lock(l); } while (0)
#define spin_unlock_irqrestore(l, f) do { spin_unlock(l); linuxkpi_irq_restore(f); } while (0)
#define spin_lock_irq(l) do { (l)->flags = linuxkpi_irq_save(); spin_lock(l); } while (0)
#define spin_unlock_irq(l) do { unsigned long __f = (l)->flags; spin_unlock(l); linuxkpi_irq_restore(__f); } while (0)
#define spin_lock_bh spin_lock
#define spin_unlock_bh spin_unlock
#define local_irq_save(f) do { (f) = linuxkpi_irq_save(); } while (0)
#define local_irq_restore(f) linuxkpi_irq_restore(f)
#define local_bh_disable() do { } while (0)
#define local_bh_enable() do { } while (0)
#define rcu_read_lock() do { } while (0)
#define rcu_read_unlock() do { } while (0)
#define synchronize_rcu() do { } while (0)
#define rcu_dereference(p) (p)
#define rcu_assign_pointer(p, v) ((p) = (v))

struct mutex { volatile int locked; };
#define DEFINE_MUTEX(x) struct mutex x = { 0 }
static inline void mutex_init(struct mutex *m) { m->locked = 0; }
static inline void mutex_lock(struct mutex *m) { while (__atomic_exchange_n(&m->locked, 1, __ATOMIC_ACQUIRE)) linuxkpi_yield(); }
static inline void mutex_unlock(struct mutex *m) { __atomic_store_n(&m->locked, 0, __ATOMIC_RELEASE); }
static inline int mutex_trylock(struct mutex *m) { return !__atomic_exchange_n(&m->locked, 1, __ATOMIC_ACQUIRE); }
static inline bool mutex_is_locked(struct mutex *m) { return m->locked != 0; }
#define mutex_destroy(m) do { } while (0)
void rtnl_lock(void);
void rtnl_unlock(void);
int rtnl_is_locked(void);

struct completion { volatile int done; };
static inline void init_completion(struct completion *c) { c->done = 0; }
static inline void complete(struct completion *c) { c->done = 1; }

#define jiffies ((unsigned long)hamix_uptime_ms())
#define jiffies_64 ((u64)hamix_uptime_ms())
#define time_after(a, b) ((long)((b) - (a)) < 0)
#define time_before(a, b) time_after(b, a)
#define time_after_eq(a, b) ((long)((a) - (b)) >= 0)
#define time_before_eq(a, b) time_after_eq(b, a)
#define time_is_before_jiffies(a) time_after(jiffies, a)
static inline unsigned long msecs_to_jiffies(unsigned int m) { return m; }
static inline unsigned long usecs_to_jiffies(unsigned int u) { return (u + 999) / 1000; }
static inline unsigned int jiffies_to_msecs(unsigned long j) { return (unsigned int)j; }
static inline unsigned long round_jiffies(unsigned long j) { return j; }
static inline unsigned long round_jiffies_relative(unsigned long j) { return j; }
static inline void udelay(unsigned long us) { hamix_udelay(us); }
static inline void ndelay(unsigned long ns) { hamix_udelay((ns + 999) / 1000); }
static inline void mdelay(unsigned long ms) { hamix_udelay(ms * 1000); }
void msleep(unsigned int ms);
static inline void usleep_range(unsigned long min, unsigned long max) { if (min >= 1000) msleep((unsigned int)(min / 1000)); else hamix_udelay(min); }
static inline void fsleep(unsigned long us) { usleep_range(us, us); }
static inline unsigned long msleep_interruptible(unsigned int ms) { msleep(ms); return 0; }
static inline ktime_t ktime_get(void) { return (ktime_t)hamix_uptime_ms() * NSEC_PER_MSEC; }
static inline ktime_t ktime_get_real(void) { return ktime_get(); }
static inline s64 ktime_to_ns(ktime_t k) { return k; }
static inline ktime_t ns_to_ktime(u64 ns) { return (ktime_t)ns; }
static inline s64 ktime_to_us(ktime_t k) { return k / 1000; }
static inline s64 ktime_to_ms(ktime_t k) { return k / 1000000; }
static inline ktime_t ktime_add_ns(ktime_t k, u64 ns) { return k + ns; }
static inline ktime_t ktime_sub(ktime_t a, ktime_t b) { return a - b; }
static inline u64 ktime_get_ns(void) { return (u64)ktime_get(); }
struct timespec64 { s64 tv_sec; long tv_nsec; };
static inline struct timespec64 ns_to_timespec64(s64 ns) { struct timespec64 t = { ns / NSEC_PER_SEC, (long)(ns % NSEC_PER_SEC) }; return t; }
static inline s64 timespec64_to_ns(const struct timespec64 *t) { return t->tv_sec * NSEC_PER_SEC + t->tv_nsec; }

struct cyclecounter {
	u64 (*read)(const struct cyclecounter *cc);
	u64 mask;
	u32 mult;
	u32 shift;
};
struct timecounter {
	const struct cyclecounter *cc;
	u64 cycle_last;
	u64 nsec;
	u64 mask;
	u64 frac;
};
static inline u64 cyclecounter_cyc2ns(const struct cyclecounter *cc, u64 cycles, u64 mask, u64 *frac) { u64 ns = cycles * cc->mult + *frac; *frac = ns & mask; return ns >> cc->shift; }
static inline void timecounter_init(struct timecounter *tc, const struct cyclecounter *cc, u64 start) { tc->cc = cc; tc->cycle_last = cc->read(cc); tc->nsec = start; tc->mask = (1ULL << cc->shift) - 1; tc->frac = 0; }
static inline u64 timecounter_read(struct timecounter *tc) { u64 now = tc->cc->read(tc->cc); u64 delta = (now - tc->cycle_last) & tc->cc->mask; tc->nsec += cyclecounter_cyc2ns(tc->cc, delta, tc->mask, &tc->frac); tc->cycle_last = now; return tc->nsec; }
static inline u64 timecounter_cyc2time(const struct timecounter *tc, u64 cycle) { u64 delta = (cycle - tc->cycle_last) & tc->cc->mask; u64 frac = tc->frac; return tc->nsec + cyclecounter_cyc2ns(tc->cc, delta, tc->mask, &frac); }
static inline void timecounter_adjtime(struct timecounter *tc, s64 delta) { tc->nsec += delta; }
#define CLOCKSOURCE_MASK(bits) GENMASK_ULL((bits) - 1, 0)
#define CYCLECOUNTER_MASK(bits) CLOCKSOURCE_MASK(bits)

struct ptp_system_timestamp { struct timespec64 pre_ts; struct timespec64 post_ts; };
static inline void ptp_read_system_prets(struct ptp_system_timestamp *sts) {}
static inline void ptp_read_system_postts(struct ptp_system_timestamp *sts) {}
struct ptp_clock;
struct ptp_clock_request;
struct ptp_clock_info {
	char name[16];
	s32 max_adj;
	int n_alarm, n_ext_ts, n_per_out, n_pins, pps;
	int (*adjfine)(struct ptp_clock_info *ptp, long scaled_ppm);
	int (*adjtime)(struct ptp_clock_info *ptp, s64 delta);
	int (*gettimex64)(struct ptp_clock_info *ptp, struct timespec64 *ts, struct ptp_system_timestamp *sts);
	int (*settime64)(struct ptp_clock_info *ptp, const struct timespec64 *ts);
	int (*enable)(struct ptp_clock_info *ptp, struct ptp_clock_request *request, int on);
	void *owner;
};
static inline int ptp_clock_index(struct ptp_clock *ptp) { return -1; }
static inline long scaled_ppm_to_ppb(long ppm) { return (ppm * 125) >> 13; }

struct hwtstamp_config { int flags; int tx_type; int rx_filter; };
struct skb_shared_hwtstamps { ktime_t hwtstamp; };
enum hwtstamp_tx_types { HWTSTAMP_TX_OFF, HWTSTAMP_TX_ON, HWTSTAMP_TX_ONESTEP_SYNC };
enum hwtstamp_rx_filters {
	HWTSTAMP_FILTER_NONE,
	HWTSTAMP_FILTER_ALL,
	HWTSTAMP_FILTER_SOME,
	HWTSTAMP_FILTER_PTP_V1_L4_EVENT,
	HWTSTAMP_FILTER_PTP_V1_L4_SYNC,
	HWTSTAMP_FILTER_PTP_V1_L4_DELAY_REQ,
	HWTSTAMP_FILTER_PTP_V2_L4_EVENT,
	HWTSTAMP_FILTER_PTP_V2_L4_SYNC,
	HWTSTAMP_FILTER_PTP_V2_L4_DELAY_REQ,
	HWTSTAMP_FILTER_PTP_V2_L2_EVENT,
	HWTSTAMP_FILTER_PTP_V2_L2_SYNC,
	HWTSTAMP_FILTER_PTP_V2_L2_DELAY_REQ,
	HWTSTAMP_FILTER_PTP_V2_EVENT,
	HWTSTAMP_FILTER_PTP_V2_SYNC,
	HWTSTAMP_FILTER_PTP_V2_DELAY_REQ,
	HWTSTAMP_FILTER_NTP_ALL,
};
#define SIOCSHWTSTAMP 0x89b0
#define SIOCGHWTSTAMP 0x89b1
#define SIOCGMIIPHY 0x8947
#define SIOCGMIIREG 0x8948
#define SIOCSMIIREG 0x8949
#define PTP_EV_PORT 319
#define PTP_CLASS_NONE 0

struct pm_qos_request { int value; };
#define PM_QOS_DEFAULT_VALUE (-1)
#define PM_QOS_CPU_LATENCY_DEFAULT_VALUE (2000 * USEC_PER_SEC)
static inline void cpu_latency_qos_add_request(struct pm_qos_request *r, s32 v) { r->value = v; }
static inline void cpu_latency_qos_update_request(struct pm_qos_request *r, s32 v) { r->value = v; }
static inline void cpu_latency_qos_remove_request(struct pm_qos_request *r) {}

struct timer_list {
	struct timer_list *hamix_next;
	unsigned long expires;
	void (*function)(struct timer_list *t);
	u32 flags;
	int hamix_pending;
};
void timer_setup(struct timer_list *t, void (*fn)(struct timer_list *), unsigned int flags);
int mod_timer(struct timer_list *t, unsigned long expires);
int del_timer(struct timer_list *t);
int del_timer_sync(struct timer_list *t);
static inline int timer_pending(const struct timer_list *t) { return t->hamix_pending; }
#define timer_delete_sync del_timer_sync
#define timer_delete del_timer
#define from_timer(var, callback_timer, timer_fieldname) container_of(callback_timer, __typeof__(*var), timer_fieldname)
static inline void add_timer(struct timer_list *t) { mod_timer(t, t->expires); }

struct work_struct;
typedef void (*work_func_t)(struct work_struct *work);
struct work_struct {
	work_func_t func;
	volatile int hamix_pending;
	volatile int hamix_running;
};
struct delayed_work {
	struct work_struct work;
	struct timer_list timer;
};
struct workqueue_struct { int unused; };
extern struct workqueue_struct *system_wq;
#define INIT_WORK(w, f) do { (w)->func = (f); (w)->hamix_pending = 0; (w)->hamix_running = 0; } while (0)
void linuxkpi_delayed_timer(struct timer_list *t);
#define INIT_DELAYED_WORK(dw, f) do { INIT_WORK(&(dw)->work, f); timer_setup(&(dw)->timer, linuxkpi_delayed_timer, 0); } while (0)
bool schedule_work(struct work_struct *w);
bool queue_work(struct workqueue_struct *wq, struct work_struct *w);
bool queue_delayed_work(struct workqueue_struct *wq, struct delayed_work *dw, unsigned long delay);
bool schedule_delayed_work(struct delayed_work *dw, unsigned long delay);
bool cancel_work_sync(struct work_struct *w);
bool cancel_delayed_work_sync(struct delayed_work *dw);
bool cancel_delayed_work(struct delayed_work *dw);
void flush_work(struct work_struct *w);
static inline bool work_pending(struct work_struct *w) { return w->hamix_pending != 0; }
#define to_delayed_work(w) container_of(w, struct delayed_work, work)
static inline struct workqueue_struct *alloc_workqueue(const char *fmt, unsigned int flags, int max, ...) { return system_wq; }
static inline void destroy_workqueue(struct workqueue_struct *wq) {}
static inline void flush_workqueue(struct workqueue_struct *wq) {}

#define IRQ_NONE 0
#define IRQ_HANDLED 1
#define IRQ_WAKE_THREAD 2
#define IRQF_SHARED 0x80
#define IRQF_PROBE_SHARED 0x100
typedef irqreturn_t (*irq_handler_t)(int irq, void *dev_id);
int request_irq(unsigned int irq, irq_handler_t handler, unsigned long flags, const char *name, void *dev);
void free_irq(unsigned int irq, void *dev);
void disable_irq(unsigned int irq);
void enable_irq(unsigned int irq);
static inline void disable_hardirq(unsigned int irq) { disable_irq(irq); }
static inline void synchronize_irq(unsigned int irq) {}
static inline void disable_irq_nosync(unsigned int irq) { disable_irq(irq); }
struct msix_entry { u32 vector; u16 entry; };
static inline bool in_interrupt(void) { extern int hamix_in_interrupt(void); return hamix_in_interrupt() != 0; }

struct device_driver { const char *name; const struct dev_pm_ops *pm; };
struct device {
	struct device *parent;
	void *driver_data;
	const char *init_name;
	u64 dma_mask_value;
	u64 *dma_mask;
	u64 coherent_dma_mask;
	struct device_driver *driver;
	void *power_data;
};
static inline void *dev_get_drvdata(const struct device *dev) { return dev->driver_data; }
static inline void dev_set_drvdata(struct device *dev, void *data) { dev->driver_data = data; }
static inline const char *dev_name(const struct device *dev) { return dev->init_name ? dev->init_name : "pci"; }
void linuxkpi_dev_printk(u32 level, const struct device *dev, const char *fmt, ...) __printf(3, 4);
#define dev_err(dev, fmt, ...) linuxkpi_dev_printk(3, dev, fmt, ##__VA_ARGS__)
#define dev_warn(dev, fmt, ...) linuxkpi_dev_printk(4, dev, fmt, ##__VA_ARGS__)
#define dev_notice(dev, fmt, ...) linuxkpi_dev_printk(5, dev, fmt, ##__VA_ARGS__)
#define dev_info(dev, fmt, ...) linuxkpi_dev_printk(6, dev, fmt, ##__VA_ARGS__)
#define dev_dbg(dev, fmt, ...) do { if (0) linuxkpi_dev_printk(7, dev, fmt, ##__VA_ARGS__); } while (0)
#define dev_err_once dev_err
#define dev_warn_once dev_warn
#define dev_info_once dev_info

struct dev_pm_ops {
	int (*prepare)(struct device *dev);
	void (*complete)(struct device *dev);
	int (*suspend)(struct device *dev);
	int (*resume)(struct device *dev);
	int (*freeze)(struct device *dev);
	int (*thaw)(struct device *dev);
	int (*poweroff)(struct device *dev);
	int (*restore)(struct device *dev);
	int (*runtime_suspend)(struct device *dev);
	int (*runtime_resume)(struct device *dev);
	int (*runtime_idle)(struct device *dev);
};
#define SET_SYSTEM_SLEEP_PM_OPS(s, r) .suspend = s, .resume = r, .freeze = s, .thaw = r, .poweroff = s, .restore = r,
#define SYSTEM_SLEEP_PM_OPS(s, r) SET_SYSTEM_SLEEP_PM_OPS(s, r)
#define SET_RUNTIME_PM_OPS(s, r, i) .runtime_suspend = s, .runtime_resume = r, .runtime_idle = i,
#define RUNTIME_PM_OPS(s, r, i) SET_RUNTIME_PM_OPS(s, r, i)
#define DEFINE_DEV_PM_OPS(name, s, r, i) const struct dev_pm_ops name = { SET_SYSTEM_SLEEP_PM_OPS(s, r) SET_RUNTIME_PM_OPS(s, r, i) }
#define EXPORT_DEV_PM_OPS(name) const struct dev_pm_ops name
#define pm_ptr(p) (p)
#define pm_sleep_ptr(p) (p)
#define DPM_FLAG_NO_DIRECT_COMPLETE BIT(0)
#define DPM_FLAG_SMART_PREPARE BIT(1)
#define DPM_FLAG_SMART_SUSPEND BIT(2)
#define DPM_FLAG_MAY_SKIP_RESUME BIT(3)
static inline void dev_pm_set_driver_flags(struct device *dev, u32 flags) {}
static inline int pm_runtime_get_sync(struct device *dev) { return 0; }
static inline int pm_runtime_resume_and_get(struct device *dev) { return 0; }
static inline void pm_runtime_get_noresume(struct device *dev) {}
static inline int pm_runtime_put(struct device *dev) { return 0; }
static inline int pm_runtime_put_sync(struct device *dev) { return 0; }
static inline void pm_runtime_put_noidle(struct device *dev) {}
static inline int pm_runtime_resume(struct device *dev) { return 0; }
static inline bool pm_runtime_suspended(struct device *dev) { return false; }
static inline int pm_runtime_idle(struct device *dev) { return 0; }
static inline int pm_schedule_suspend(struct device *dev, unsigned int delay) { return 0; }
static inline void pm_runtime_allow(struct device *dev) {}
static inline void pm_runtime_forbid(struct device *dev) {}
static inline bool pm_suspend_via_firmware(void) { return false; }
static inline bool device_may_wakeup(struct device *dev) { return false; }
static inline int device_wakeup_enable(struct device *dev) { return 0; }
static inline int device_set_wakeup_enable(struct device *dev, bool enable) { return 0; }
static inline void device_init_wakeup(struct device *dev, bool enable) {}
static inline bool device_can_wakeup(struct device *dev) { return false; }
static inline void device_set_wakeup_capable(struct device *dev, bool capable) {}

#define IORESOURCE_IO 0x100
#define IORESOURCE_MEM 0x200
#define IORESOURCE_MEM_64 0x100000
#define PCI_ANY_ID (~0U)
#define PCI_VENDOR_ID_INTEL 0x8086
#define PCI_VDEVICE(vend, dev) .vendor = PCI_VENDOR_ID_##vend, .device = (dev), .subvendor = PCI_ANY_ID, .subdevice = PCI_ANY_ID, .class = 0, .class_mask = 0
#define PCI_DEVICE(vend, dev) .vendor = (vend), .device = (dev), .subvendor = PCI_ANY_ID, .subdevice = PCI_ANY_ID
#define PCI_D0 0
#define PCI_D1 1
#define PCI_D2 2
#define PCI_D3hot 3
#define PCI_D3cold 4
#define PCI_COMMAND 0x04
#define PCI_COMMAND_IO 0x1
#define PCI_COMMAND_MEMORY 0x2
#define PCI_COMMAND_MASTER 0x4
#define PCI_COMMAND_INTX_DISABLE 0x400
#define PCI_COMMAND_SERR 0x100
#define PCI_COMMAND_PARITY 0x40
#define PCI_STATUS 0x06
#define PCI_REVISION_ID 0x08
#define PCI_CAPABILITY_LIST 0x34
#define PCI_CAP_ID_PM 0x01
#define PCI_CAP_ID_MSI 0x05
#define PCI_CAP_ID_EXP 0x10
#define PCI_CAP_ID_MSIX 0x11
#define PCI_EXP_DEVCTL 8
#define PCI_EXP_DEVCTL_CERE 0x0001
#define PCI_EXP_DEVCTL_NFERE 0x0002
#define PCI_EXP_DEVCTL_FERE 0x0004
#define PCI_EXP_DEVCTL_URRE 0x0008
#define PCI_EXP_DEVCTL_RELAX_EN 0x0010
#define PCI_EXP_DEVCTL_PAYLOAD 0x00e0
#define PCI_EXP_DEVCTL_NOSNOOP_EN 0x0800
#define PCI_EXP_DEVCTL_READRQ 0x7000
#define PCI_EXP_DEVSTA 10
#define PCI_EXP_LNKCAP 12
#define PCI_EXP_LNKCTL 16
#define PCI_EXP_LNKCTL_ASPMC 0x0003
#define PCI_EXP_LNKCTL_ASPM_L0S 0x0001
#define PCI_EXP_LNKCTL_ASPM_L1 0x0002
#define PCI_EXP_LNKSTA 18
#define PCI_LTR_VALUE_MASK 0x000003ff
#define PCI_LTR_SCALE_MASK 0x00001c00
#define PCI_LTR_SCALE_SHIFT 10
#define PCIE_LINK_STATE_L0S 1
#define PCIE_LINK_STATE_L1 2
#define PCI_ERS_RESULT_NONE 1
#define PCI_ERS_RESULT_CAN_RECOVER 2
#define PCI_ERS_RESULT_NEED_RESET 3
#define PCI_ERS_RESULT_DISCONNECT 4
#define PCI_ERS_RESULT_RECOVERED 5
#define pci_channel_io_normal 1
#define pci_channel_io_frozen 2
#define pci_channel_io_perm_failure 3

struct pci_device_id {
	u32 vendor, device;
	u32 subvendor, subdevice;
	u32 class, class_mask;
	kernel_ulong_t driver_data;
	u32 override_only;
};
struct hamix_pci_handle {
	u8 bus;
	u8 device;
	u8 function;
	u8 pad;
	u16 vendor;
	u16 device_id;
	u8 irq;
	u8 pad2[3];
};
struct pci_bus { unsigned char number; struct pci_dev *self; };
struct pci_dev {
	struct device dev;
	struct pci_bus *bus;
	struct pci_bus hamix_bus;
	unsigned int devfn;
	u16 vendor;
	u16 device;
	u16 subsystem_vendor;
	u16 subsystem_device;
	u32 class;
	u8 revision;
	unsigned int irq;
	unsigned int msi_enabled:1;
	unsigned int msix_enabled:1;
	u8 pm_cap;
	u8 pcie_cap;
	pci_power_t current_state;
	unsigned int pme_poll:1;
	unsigned int state_saved:1;
	int hamix_irq_line;
	struct hamix_pci_handle handle;
	char hamix_name[16];
};
struct pci_error_handlers {
	pci_ers_result_t (*error_detected)(struct pci_dev *dev, pci_channel_state_t error);
	pci_ers_result_t (*mmio_enabled)(struct pci_dev *dev);
	pci_ers_result_t (*slot_reset)(struct pci_dev *dev);
	void (*resume)(struct pci_dev *dev);
};
struct pci_driver {
	const char *name;
	const struct pci_device_id *id_table;
	int (*probe)(struct pci_dev *dev, const struct pci_device_id *id);
	void (*remove)(struct pci_dev *dev);
	void (*shutdown)(struct pci_dev *dev);
	struct device_driver driver;
	const struct pci_error_handlers *err_handler;
};
#define to_pci_dev(d) container_of(d, struct pci_dev, dev)
#define MODULE_DEVICE_TABLE(type, name) extern __typeof__(name) __mod_##type##__##name##_device_table __attribute__((unused, alias(#name)))
int pci_register_driver(struct pci_driver *drv);
void pci_unregister_driver(struct pci_driver *drv);
static inline void *pci_get_drvdata(struct pci_dev *pdev) { return pdev->dev.driver_data; }
static inline void pci_set_drvdata(struct pci_dev *pdev, void *data) { pdev->dev.driver_data = data; }
static inline const char *pci_name(const struct pci_dev *pdev) { return pdev->hamix_name; }
int pci_read_config_byte(const struct pci_dev *dev, int where, u8 *val);
int pci_read_config_word(const struct pci_dev *dev, int where, u16 *val);
int pci_read_config_dword(const struct pci_dev *dev, int where, u32 *val);
int pci_write_config_byte(const struct pci_dev *dev, int where, u8 val);
int pci_write_config_word(const struct pci_dev *dev, int where, u16 val);
int pci_write_config_dword(const struct pci_dev *dev, int where, u32 val);
int pci_find_capability(struct pci_dev *dev, int cap);
int pcie_capability_read_word(struct pci_dev *dev, int pos, u16 *val);
int pcie_capability_write_word(struct pci_dev *dev, int pos, u16 val);
int pcie_capability_clear_and_set_word(struct pci_dev *dev, int pos, u16 clear, u16 set);
static inline int pcie_capability_clear_word(struct pci_dev *dev, int pos, u16 clear) { return pcie_capability_clear_and_set_word(dev, pos, clear, 0); }
static inline int pcie_capability_set_word(struct pci_dev *dev, int pos, u16 set) { return pcie_capability_clear_and_set_word(dev, pos, 0, set); }
int pci_enable_device_mem(struct pci_dev *dev);
static inline int pci_enable_device(struct pci_dev *dev) { return pci_enable_device_mem(dev); }
void pci_disable_device(struct pci_dev *dev);
void pci_set_master(struct pci_dev *dev);
void pci_clear_master(struct pci_dev *dev);
int pci_select_bars(struct pci_dev *dev, unsigned long flags);
static inline int pci_request_selected_regions(struct pci_dev *dev, int bars, const char *name) { return 0; }
static inline int pci_request_selected_regions_exclusive(struct pci_dev *dev, int bars, const char *name) { return 0; }
static inline void pci_release_selected_regions(struct pci_dev *dev, int bars) {}
static inline int pci_request_mem_regions(struct pci_dev *dev, const char *name) { return 0; }
static inline void pci_release_mem_regions(struct pci_dev *dev) {}
static inline int pci_request_regions(struct pci_dev *dev, const char *name) { return 0; }
static inline void pci_release_regions(struct pci_dev *dev) {}
resource_size_t pci_resource_start(struct pci_dev *dev, int bar);
resource_size_t pci_resource_len(struct pci_dev *dev, int bar);
unsigned long pci_resource_flags(struct pci_dev *dev, int bar);
static inline int pci_save_state(struct pci_dev *dev) { return 0; }
static inline void pci_restore_state(struct pci_dev *dev) {}
static inline int pci_set_power_state(struct pci_dev *dev, pci_power_t state) { return 0; }
static inline int pci_enable_wake(struct pci_dev *dev, pci_power_t state, bool enable) { return 0; }
static inline int pci_wake_from_d3(struct pci_dev *dev, bool enable) { return 0; }
static inline int pci_prepare_to_sleep(struct pci_dev *dev) { return 0; }
static inline bool pci_dev_run_wake(struct pci_dev *dev) { return false; }
static inline bool pci_channel_offline(struct pci_dev *dev) { return false; }
static inline int pci_disable_link_state(struct pci_dev *dev, int state) { return pcie_capability_clear_word(dev, PCI_EXP_LNKCTL, (u16)(((state & PCIE_LINK_STATE_L0S) ? PCI_EXP_LNKCTL_ASPM_L0S : 0) | ((state & PCIE_LINK_STATE_L1) ? PCI_EXP_LNKCTL_ASPM_L1 : 0))); }
static inline int pci_disable_link_state_locked(struct pci_dev *dev, int state) { return pci_disable_link_state(dev, state); }
int pci_enable_msi(struct pci_dev *dev);
void pci_disable_msi(struct pci_dev *dev);
static inline int pci_enable_msix_range(struct pci_dev *dev, struct msix_entry *entries, int minvec, int maxvec) { return -ENOSYS; }
static inline void pci_disable_msix(struct pci_dev *dev) {}
static inline struct pci_dev *pci_upstream_bridge(struct pci_dev *dev) { return NULL; }
static inline int pci_aer_clear_nonfatal_status(struct pci_dev *dev) { return 0; }

void *ioremap(phys_addr_t phys, size_t size);
void iounmap(volatile void __iomem *addr);
#define ioremap_wc ioremap
#define ioremap_uc ioremap
#define ioremap_cache ioremap
static inline void __iomem *pci_ioremap_bar(struct pci_dev *pdev, int bar) { return ioremap(pci_resource_start(pdev, bar), pci_resource_len(pdev, bar)); }
static inline u8 readb(const volatile void __iomem *a) { return *(const volatile u8 *)a; }
static inline u16 readw(const volatile void __iomem *a) { return *(const volatile u16 *)a; }
static inline u32 readl(const volatile void __iomem *a) { return *(const volatile u32 *)a; }
static inline u64 readq(const volatile void __iomem *a) { return *(const volatile u64 *)a; }
static inline void writeb(u8 v, volatile void __iomem *a) { *(volatile u8 *)a = v; }
static inline void writew(u16 v, volatile void __iomem *a) { *(volatile u16 *)a = v; }
static inline void writel(u32 v, volatile void __iomem *a) { *(volatile u32 *)a = v; }
static inline void writeq(u64 v, volatile void __iomem *a) { *(volatile u64 *)a = v; }
#define readl_relaxed readl
#define writel_relaxed writel
#define ioread8 readb
#define ioread16 readw
#define ioread32 readl
#define iowrite8 writeb
#define iowrite16 writew
#define iowrite32 writel
static inline void memcpy_fromio(void *dst, const volatile void __iomem *src, size_t n) { for (size_t i = 0; i < n; i++) ((u8 *)dst)[i] = readb((const volatile u8 *)src + i); }
static inline void memcpy_toio(volatile void __iomem *dst, const void *src, size_t n) { for (size_t i = 0; i < n; i++) writeb(((const u8 *)src)[i], (volatile u8 *)dst + i); }

enum dma_data_direction { DMA_BIDIRECTIONAL = 0, DMA_TO_DEVICE = 1, DMA_FROM_DEVICE = 2, DMA_NONE = 3 };
#define DMA_BIT_MASK(n) (~0ULL >> (64 - (n)))
static inline int dma_set_mask_and_coherent(struct device *dev, u64 mask) { return 0; }
static inline int dma_set_mask(struct device *dev, u64 mask) { return 0; }
static inline int dma_set_coherent_mask(struct device *dev, u64 mask) { return 0; }
void *dma_alloc_coherent(struct device *dev, size_t size, dma_addr_t *handle, gfp_t gfp);
void dma_free_coherent(struct device *dev, size_t size, void *cpu, dma_addr_t handle);
static inline dma_addr_t dma_map_single(struct device *dev, void *ptr, size_t size, enum dma_data_direction dir) { return hamix_virt_to_phys(ptr); }
static inline void dma_unmap_single(struct device *dev, dma_addr_t addr, size_t size, enum dma_data_direction dir) {}
static inline int dma_mapping_error(struct device *dev, dma_addr_t addr) { return addr == 0; }
static inline void dma_sync_single_for_cpu(struct device *dev, dma_addr_t addr, size_t size, enum dma_data_direction dir) { rmb(); }
static inline void dma_sync_single_for_device(struct device *dev, dma_addr_t addr, size_t size, enum dma_data_direction dir) { wmb(); }

struct page { void *hamix_virt; dma_addr_t hamix_phys; atomic_t hamix_refs; };
struct page *alloc_pages(gfp_t gfp, unsigned int order);
static inline struct page *alloc_page(gfp_t gfp) { return alloc_pages(gfp, 0); }
static inline struct page *dev_alloc_page(void) { return alloc_page(GFP_ATOMIC); }
void put_page(struct page *page);
static inline void __free_page(struct page *page) { put_page(page); }
static inline void *page_address(const struct page *page) { return page->hamix_virt; }
static inline void get_page(struct page *page) { atomic_inc(&page->hamix_refs); }
static inline dma_addr_t dma_map_page(struct device *dev, struct page *page, size_t offset, size_t size, enum dma_data_direction dir) { return page->hamix_phys + offset; }
static inline void dma_unmap_page(struct device *dev, dma_addr_t addr, size_t size, enum dma_data_direction dir) {}
static inline void *kmap_local_page(struct page *page) { return page->hamix_virt; }
static inline void kunmap_local(const void *addr) {}
#define kmap_atomic(p) page_address(p)
#define kunmap_atomic(a) do { } while (0)

u32 crc32_le(u32 crc, const unsigned char *p, size_t len);
static inline u32 ether_crc_le(int length, const unsigned char *data) { return crc32_le(~0U, data, length); }

#define ETH_ALEN 6
#define ETH_HLEN 14
#define ETH_ZLEN 60
#define ETH_DATA_LEN 1500
#define ETH_FRAME_LEN 1514
#define ETH_FCS_LEN 4
#define ETH_TLEN 2
#define ETH_P_IP 0x0800
#define ETH_P_ARP 0x0806
#define ETH_P_8021Q 0x8100
#define ETH_P_8021AD 0x88A8
#define ETH_P_IPV6 0x86DD
#define ETH_P_1588 0x88F7
#define ETH_MIN_MTU 68
#define VLAN_HLEN 4
#define VLAN_ETH_HLEN 18
#define VLAN_ETH_FRAME_LEN 1518
#define VLAN_N_VID 4096
#define VLAN_PRIO_MASK 0xe000
#define VLAN_PRIO_SHIFT 13
#define VLAN_VID_MASK 0x0fff
#define VLAN_TAG_PRESENT 0x1000
#define NET_IP_ALIGN 2
#define NET_SKB_PAD 64
#define IPPROTO_TCP 6
#define IPPROTO_UDP 17
#define NEXTHDR_TCP 6
struct ethhdr { unsigned char h_dest[ETH_ALEN]; unsigned char h_source[ETH_ALEN]; __be16 h_proto; } __packed;
struct vlan_hdr { __be16 h_vlan_TCI; __be16 h_vlan_encapsulated_proto; };
struct vlan_ethhdr { unsigned char h_dest[ETH_ALEN]; unsigned char h_source[ETH_ALEN]; __be16 h_vlan_proto; __be16 h_vlan_TCI; __be16 h_vlan_encapsulated_proto; } __packed;
struct iphdr {
	u8 ihl:4, version:4;
	u8 tos;
	__be16 tot_len;
	__be16 id;
	__be16 frag_off;
	u8 ttl;
	u8 protocol;
	__sum16 check;
	__be32 saddr;
	__be32 daddr;
};
struct in6_addr { u8 s6_addr[16]; };
struct ipv6hdr {
	u8 priority:4, version:4;
	u8 flow_lbl[3];
	__be16 payload_len;
	u8 nexthdr;
	u8 hop_limit;
	struct in6_addr saddr;
	struct in6_addr daddr;
};
struct tcphdr {
	__be16 source;
	__be16 dest;
	__be32 seq;
	__be32 ack_seq;
	u16 res1:4, doff:4, fin:1, syn:1, rst:1, psh:1, ack:1, urg:1, ece:1, cwr:1;
	__be16 window;
	__sum16 check;
	__be16 urg_ptr;
};
struct udphdr { __be16 source; __be16 dest; __be16 len; __sum16 check; };
static inline bool is_zero_ether_addr(const u8 *a) { return !(a[0] | a[1] | a[2] | a[3] | a[4] | a[5]); }
static inline bool is_multicast_ether_addr(const u8 *a) { return a[0] & 1; }
static inline bool is_broadcast_ether_addr(const u8 *a) { return (a[0] & a[1] & a[2] & a[3] & a[4] & a[5]) == 0xff; }
static inline bool is_valid_ether_addr(const u8 *a) { return !is_multicast_ether_addr(a) && !is_zero_ether_addr(a); }
static inline void ether_addr_copy(u8 *dst, const u8 *src) { memcpy(dst, src, ETH_ALEN); }
static inline bool ether_addr_equal(const u8 *a, const u8 *b) { return memcmp(a, b, ETH_ALEN) == 0; }
void eth_random_addr(u8 *addr);

#define CHECKSUM_NONE 0
#define CHECKSUM_UNNECESSARY 1
#define CHECKSUM_COMPLETE 2
#define CHECKSUM_PARTIAL 3
#define MAX_SKB_FRAGS 17
#define SKBTX_HW_TSTAMP (1 << 0)
#define SKBTX_SW_TSTAMP (1 << 1)
#define SKBTX_IN_PROGRESS (1 << 2)
#define SKB_GSO_TCPV4 (1 << 0)
#define SKB_GSO_TCPV6 (1 << 4)
#define PKT_HASH_TYPE_L3 2
#define PKT_HASH_TYPE_L4 3
#define PACKET_HOST 0
#define PACKET_BROADCAST 1
#define PACKET_MULTICAST 2
#define PACKET_OTHERHOST 3

typedef struct skb_frag { struct page *bv_page; unsigned int bv_len; unsigned int bv_offset; } skb_frag_t;
struct skb_shared_info {
	u8 flags;
	u8 tx_flags;
	unsigned short nr_frags;
	unsigned short gso_size;
	unsigned short gso_segs;
	unsigned int gso_type;
	struct skb_shared_hwtstamps hwtstamps;
	skb_frag_t frags[MAX_SKB_FRAGS];
};
struct net_device;
struct sk_buff {
	struct sk_buff *next;
	struct net_device *dev;
	unsigned char *head;
	unsigned char *data;
	unsigned int len;
	unsigned int data_len;
	unsigned int tail;
	unsigned int end;
	unsigned int truesize;
	__be16 protocol;
	u8 ip_summed;
	u8 pkt_type;
	u16 queue_mapping;
	u16 mac_header;
	u16 network_header;
	u16 transport_header;
	u16 csum_start;
	u16 csum_offset;
	__be16 vlan_proto;
	u16 vlan_tci;
	u8 vlan_present;
	u8 no_fcs;
	u32 hash;
	u32 priority;
	ktime_t tstamp;
	atomic_t users;
	u32 hamix_alloc;
	struct skb_shared_info shinfo;
};
#define skb_shinfo(skb) (&(skb)->shinfo)
static inline struct skb_shared_hwtstamps *skb_hwtstamps(struct sk_buff *skb) { return &skb_shinfo(skb)->hwtstamps; }
static inline unsigned int skb_headlen(const struct sk_buff *skb) { return skb->len - skb->data_len; }
static inline unsigned char *skb_tail_pointer(const struct sk_buff *skb) { return skb->head + skb->tail; }
static inline unsigned char *skb_end_pointer(const struct sk_buff *skb) { return skb->head + skb->end; }
static inline int skb_tailroom(const struct sk_buff *skb) { return skb->data_len ? 0 : (int)(skb->end - skb->tail); }
static inline unsigned int skb_headroom(const struct sk_buff *skb) { return (unsigned int)(skb->data - skb->head); }
static inline void *skb_put(struct sk_buff *skb, unsigned int len) { void *t = skb_tail_pointer(skb); skb->tail += len; skb->len += len; return t; }
static inline void *__skb_put(struct sk_buff *skb, unsigned int len) { return skb_put(skb, len); }
static inline void *skb_put_zero(struct sk_buff *skb, unsigned int len) { void *t = skb_put(skb, len); memset(t, 0, len); return t; }
static inline void *skb_put_data(struct sk_buff *skb, const void *d, unsigned int len) { void *t = skb_put(skb, len); memcpy(t, d, len); return t; }
static inline void *skb_push(struct sk_buff *skb, unsigned int len) { skb->data -= len; skb->len += len; return skb->data; }
static inline void *skb_pull(struct sk_buff *skb, unsigned int len) { if (len > skb->len) return NULL; skb->len -= len; skb->data += len; return skb->data; }
static inline void *__skb_pull(struct sk_buff *skb, unsigned int len) { return skb_pull(skb, len); }
static inline void skb_reserve(struct sk_buff *skb, int len) { skb->data += len; skb->tail += len; }
static inline void skb_trim(struct sk_buff *skb, unsigned int len) { if (skb->len > len) { skb->len = len; skb->tail = (unsigned int)(skb->data - skb->head) + len; } }
static inline void __skb_trim(struct sk_buff *skb, unsigned int len) { skb_trim(skb, len); }
static inline int pskb_trim(struct sk_buff *skb, unsigned int len) { skb_trim(skb, len); return 0; }
static inline bool pskb_may_pull(struct sk_buff *skb, unsigned int len) { return len <= skb_headlen(skb); }
static inline void *__pskb_pull_tail(struct sk_buff *skb, int delta) { return pskb_may_pull(skb, delta) ? skb_tail_pointer(skb) : NULL; }
static inline void skb_copy_to_linear_data_offset(struct sk_buff *skb, int offset, const void *from, unsigned int len) { memcpy(skb->data + offset, from, len); }
static inline void skb_copy_to_linear_data(struct sk_buff *skb, const void *from, unsigned int len) { memcpy(skb->data, from, len); }
static inline void skb_copy_from_linear_data(const struct sk_buff *skb, void *to, unsigned int len) { memcpy(to, skb->data, len); }
static inline void skb_reset_mac_header(struct sk_buff *skb) { skb->mac_header = (u16)(skb->data - skb->head); }
static inline void skb_reset_network_header(struct sk_buff *skb) { skb->network_header = (u16)(skb->data - skb->head); }
static inline void skb_set_network_header(struct sk_buff *skb, int offset) { skb->network_header = (u16)(skb->data - skb->head + offset); }
static inline unsigned char *skb_mac_header(const struct sk_buff *skb) { return skb->head + skb->mac_header; }
static inline unsigned char *skb_network_header(const struct sk_buff *skb) { return skb->head + skb->network_header; }
static inline unsigned char *skb_transport_header(const struct sk_buff *skb) { return skb->head + skb->transport_header; }
static inline int skb_network_offset(const struct sk_buff *skb) { return (int)(skb_network_header(skb) - skb->data); }
static inline int skb_transport_offset(const struct sk_buff *skb) { return (int)(skb_transport_header(skb) - skb->data); }
static inline int skb_checksum_start_offset(const struct sk_buff *skb) { return skb->csum_start - (int)skb_headroom(skb); }
static inline struct iphdr *ip_hdr(const struct sk_buff *skb) { return (struct iphdr *)skb_network_header(skb); }
static inline struct ipv6hdr *ipv6_hdr(const struct sk_buff *skb) { return (struct ipv6hdr *)skb_network_header(skb); }
static inline struct tcphdr *tcp_hdr(const struct sk_buff *skb) { return (struct tcphdr *)skb_transport_header(skb); }
static inline unsigned int tcp_hdrlen(const struct sk_buff *skb) { return tcp_hdr(skb)->doff * 4; }
static inline int skb_tcp_all_headers(const struct sk_buff *skb) { return skb_transport_offset(skb) + tcp_hdrlen(skb); }
static inline bool skb_is_gso(const struct sk_buff *skb) { return skb_shinfo(skb)->gso_size != 0; }
static inline bool skb_is_gso_v6(const struct sk_buff *skb) { return skb_shinfo(skb)->gso_type & SKB_GSO_TCPV6; }
static inline int skb_cow_head(struct sk_buff *skb, unsigned int headroom) { return 0; }
static inline void skb_checksum_none_assert(const struct sk_buff *skb) {}
static inline void skb_set_hash(struct sk_buff *skb, u32 hash, int type) { skb->hash = hash; }
static inline void skb_tx_timestamp(struct sk_buff *skb) {}
static inline void skb_tstamp_tx(struct sk_buff *skb, struct skb_shared_hwtstamps *h) {}
static inline struct sk_buff *skb_get(struct sk_buff *skb) { atomic_inc(&skb->users); return skb; }
static inline unsigned int skb_frag_size(const skb_frag_t *frag) { return frag->bv_len; }
static inline struct page *skb_frag_page(const skb_frag_t *frag) { return frag->bv_page; }
static inline dma_addr_t skb_frag_dma_map(struct device *dev, const skb_frag_t *frag, size_t offset, size_t size, enum dma_data_direction dir) { return frag->bv_page->hamix_phys + frag->bv_offset + offset; }
static inline void skb_fill_page_desc(struct sk_buff *skb, int i, struct page *page, int off, int size) { skb_frag_t *f = &skb_shinfo(skb)->frags[i]; f->bv_page = page; f->bv_offset = off; f->bv_len = size; skb_shinfo(skb)->nr_frags = i + 1; }
static inline bool skb_vlan_tag_present(const struct sk_buff *skb) { return skb->vlan_present; }
static inline u16 skb_vlan_tag_get(const struct sk_buff *skb) { return skb->vlan_tci; }
static inline void __vlan_hwaccel_put_tag(struct sk_buff *skb, __be16 proto, u16 tci) { skb->vlan_proto = proto; skb->vlan_tci = tci; skb->vlan_present = 1; }
static inline __be16 vlan_get_protocol(const struct sk_buff *skb) { return skb->protocol; }
struct sk_buff *linuxkpi_alloc_skb(unsigned int size);
void linuxkpi_free_skb(struct sk_buff *skb);
static inline struct sk_buff *alloc_skb(unsigned int size, gfp_t gfp) { return linuxkpi_alloc_skb(size); }
static inline struct sk_buff *__netdev_alloc_skb(struct net_device *dev, unsigned int len, gfp_t gfp) { struct sk_buff *skb = linuxkpi_alloc_skb(len + NET_SKB_PAD); if (skb) { skb_reserve(skb, NET_SKB_PAD); skb->dev = dev; } return skb; }
static inline struct sk_buff *netdev_alloc_skb(struct net_device *dev, unsigned int len) { return __netdev_alloc_skb(dev, len, GFP_ATOMIC); }
static inline struct sk_buff *__netdev_alloc_skb_ip_align(struct net_device *dev, unsigned int len, gfp_t gfp) { struct sk_buff *skb = __netdev_alloc_skb(dev, len + NET_IP_ALIGN, gfp); if (skb) skb_reserve(skb, NET_IP_ALIGN); return skb; }
static inline struct sk_buff *netdev_alloc_skb_ip_align(struct net_device *dev, unsigned int len) { return __netdev_alloc_skb_ip_align(dev, len, GFP_ATOMIC); }
struct napi_struct;
static inline struct sk_buff *napi_alloc_skb(struct napi_struct *napi, unsigned int len) { return netdev_alloc_skb_ip_align(NULL, len); }
static inline void dev_kfree_skb(struct sk_buff *skb) { linuxkpi_free_skb(skb); }
static inline void dev_kfree_skb_any(struct sk_buff *skb) { linuxkpi_free_skb(skb); }
static inline void dev_kfree_skb_irq(struct sk_buff *skb) { linuxkpi_free_skb(skb); }
static inline void dev_consume_skb_any(struct sk_buff *skb) { linuxkpi_free_skb(skb); }
static inline void kfree_skb(struct sk_buff *skb) { linuxkpi_free_skb(skb); }
static inline void consume_skb(struct sk_buff *skb) { linuxkpi_free_skb(skb); }
static inline void napi_consume_skb(struct sk_buff *skb, int budget) { linuxkpi_free_skb(skb); }
static inline int skb_padto(struct sk_buff *skb, unsigned int len) { unsigned int size = skb->len; if (size >= len) return 0; if ((unsigned int)skb_tailroom(skb) < len - size) { linuxkpi_free_skb(skb); return -ENOMEM; } memset(skb->data + size, 0, len - size); return 0; }
static inline int skb_put_padto(struct sk_buff *skb, unsigned int len) { unsigned int size = skb->len; if (size < len) { if (skb_padto(skb, len)) return -ENOMEM; skb_put(skb, len - size); } return 0; }
static inline __wsum csum_partial(const void *buf, int len, __wsum sum) { const u8 *p = buf; u64 s = sum; for (int i = 0; i + 1 < len; i += 2) s += (u16)(p[i] | (p[i + 1] << 8)); if (len & 1) s += p[len - 1]; while (s >> 32) s = (s & 0xffffffff) + (s >> 32); return (__wsum)s; }
static inline __sum16 csum_fold(__wsum csum) { u32 s = csum; s = (s & 0xffff) + (s >> 16); s = (s & 0xffff) + (s >> 16); return (__sum16)~s; }
static inline __sum16 csum_tcpudp_magic(__be32 saddr, __be32 daddr, u32 len, u8 proto, __wsum sum) { u64 s = sum; s += saddr; s += daddr; s += htons((u16)len); s += htons((u16)proto); while (s >> 32) s = (s & 0xffffffff) + (s >> 32); return csum_fold((__wsum)s); }
static inline __sum16 csum_ipv6_magic(const struct in6_addr *saddr, const struct in6_addr *daddr, u32 len, u8 proto, __wsum csum) { u64 s = csum; s = csum_partial(saddr, 16, (__wsum)s); s = csum_partial(daddr, 16, (__wsum)s); s += htonl(len); s += htonl((u32)proto); while (s >> 32) s = (s & 0xffffffff) + (s >> 32); return csum_fold((__wsum)s); }
static inline void tcp_v6_gso_csum_prep(struct sk_buff *skb) { struct ipv6hdr *ipv6h = ipv6_hdr(skb); struct tcphdr *th = tcp_hdr(skb); ipv6h->payload_len = 0; th->check = ~csum_ipv6_magic(&ipv6h->saddr, &ipv6h->daddr, 0, IPPROTO_TCP, 0); }

#define IFNAMSIZ 16
#define IFF_UP 0x1
#define IFF_BROADCAST 0x2
#define IFF_PROMISC 0x100
#define IFF_ALLMULTI 0x200
#define IFF_MULTICAST 0x1000
#define IFF_UNICAST_FLT (1 << 17)
#define IFF_SUPP_NOFCS (1 << 19)
#define IFF_LIVE_ADDR_CHANGE (1 << 20)
#define NETIF_MSG_DRV 0x0001
#define NETIF_MSG_PROBE 0x0002
#define NETIF_MSG_LINK 0x0004
#define NETIF_MSG_TIMER 0x0008
#define NETIF_MSG_IFDOWN 0x0010
#define NETIF_MSG_IFUP 0x0020
#define NETIF_MSG_RX_ERR 0x0040
#define NETIF_MSG_TX_ERR 0x0080
#define NETIF_MSG_TX_QUEUED 0x0100
#define NETIF_MSG_INTR 0x0200
#define NETIF_MSG_TX_DONE 0x0400
#define NETIF_MSG_RX_STATUS 0x0800
#define NETIF_MSG_PKTDATA 0x1000
#define NETIF_MSG_HW 0x2000
#define NETIF_MSG_WOL 0x4000
#define NETDEV_TX_OK 0
#define NETDEV_TX_BUSY 0x10
typedef int netdev_tx_t;
#define NETIF_F_SG BIT_ULL(0)
#define NETIF_F_IP_CSUM BIT_ULL(1)
#define NETIF_F_HW_CSUM BIT_ULL(3)
#define NETIF_F_IPV6_CSUM BIT_ULL(4)
#define NETIF_F_HIGHDMA BIT_ULL(5)
#define NETIF_F_HW_VLAN_CTAG_TX BIT_ULL(7)
#define NETIF_F_HW_VLAN_CTAG_RX BIT_ULL(8)
#define NETIF_F_HW_VLAN_CTAG_FILTER BIT_ULL(9)
#define NETIF_F_TSO BIT_ULL(16)
#define NETIF_F_TSO6 BIT_ULL(20)
#define NETIF_F_RXHASH BIT_ULL(28)
#define NETIF_F_RXCSUM BIT_ULL(29)
#define NETIF_F_NOCACHE_COPY BIT_ULL(30)
#define NETIF_F_LOOPBACK BIT_ULL(31)
#define NETIF_F_RXFCS BIT_ULL(32)
#define NETIF_F_RXALL BIT_ULL(33)
#define NETIF_F_ALL_TSO (NETIF_F_TSO | NETIF_F_TSO6)
#define NETIF_F_CSUM_MASK (NETIF_F_IP_CSUM | NETIF_F_HW_CSUM | NETIF_F_IPV6_CSUM)
#define NAPI_POLL_WEIGHT 64
#define MAX_ADDR_LEN 32

struct netdev_hw_addr { struct list_head list; unsigned char addr[MAX_ADDR_LEN]; };
struct netdev_hw_addr_list { struct list_head list; int count; };
#define netdev_hw_addr_list_count(l) ((l)->count)
#define netdev_hw_addr_list_empty(l) ((l)->count == 0)
#define netdev_mc_count(dev) netdev_hw_addr_list_count(&(dev)->mc)
#define netdev_mc_empty(dev) netdev_hw_addr_list_empty(&(dev)->mc)
#define netdev_uc_count(dev) netdev_hw_addr_list_count(&(dev)->uc)
#define netdev_uc_empty(dev) netdev_hw_addr_list_empty(&(dev)->uc)
#define netdev_for_each_mc_addr(ha, dev) list_for_each_entry(ha, &(dev)->mc.list, list)
#define netdev_for_each_uc_addr(ha, dev) list_for_each_entry(ha, &(dev)->uc.list, list)

struct rtnl_link_stats64 {
	u64 rx_packets, tx_packets, rx_bytes, tx_bytes, rx_errors, tx_errors, rx_dropped, tx_dropped, multicast, collisions;
	u64 rx_length_errors, rx_over_errors, rx_crc_errors, rx_frame_errors, rx_fifo_errors, rx_missed_errors;
	u64 tx_aborted_errors, tx_carrier_errors, tx_fifo_errors, tx_heartbeat_errors, tx_window_errors;
	u64 rx_compressed, tx_compressed, rx_nohandler;
};
struct net_device_stats {
	unsigned long rx_packets, tx_packets, rx_bytes, tx_bytes, rx_errors, tx_errors, rx_dropped, tx_dropped, multicast, collisions;
	unsigned long rx_length_errors, rx_over_errors, rx_crc_errors, rx_frame_errors, rx_fifo_errors, rx_missed_errors;
	unsigned long tx_aborted_errors, tx_carrier_errors, tx_fifo_errors, tx_heartbeat_errors, tx_window_errors;
};

struct ifreq;
struct netlink_ext_ack;
struct netdev_queue { unsigned long state; unsigned long trans_start; };
struct net_device_ops {
	int (*ndo_init)(struct net_device *dev);
	void (*ndo_uninit)(struct net_device *dev);
	int (*ndo_open)(struct net_device *dev);
	int (*ndo_stop)(struct net_device *dev);
	netdev_tx_t (*ndo_start_xmit)(struct sk_buff *skb, struct net_device *dev);
	void (*ndo_set_rx_mode)(struct net_device *dev);
	int (*ndo_set_mac_address)(struct net_device *dev, void *addr);
	int (*ndo_validate_addr)(struct net_device *dev);
	int (*ndo_eth_ioctl)(struct net_device *dev, struct ifreq *ifr, int cmd);
	int (*ndo_change_mtu)(struct net_device *dev, int new_mtu);
	void (*ndo_tx_timeout)(struct net_device *dev, unsigned int txqueue);
	void (*ndo_get_stats64)(struct net_device *dev, struct rtnl_link_stats64 *storage);
	int (*ndo_vlan_rx_add_vid)(struct net_device *dev, __be16 proto, u16 vid);
	int (*ndo_vlan_rx_kill_vid)(struct net_device *dev, __be16 proto, u16 vid);
	void (*ndo_poll_controller)(struct net_device *dev);
	int (*ndo_set_features)(struct net_device *dev, netdev_features_t features);
	netdev_features_t (*ndo_fix_features)(struct net_device *dev, netdev_features_t features);
	netdev_features_t (*ndo_features_check)(struct sk_buff *skb, struct net_device *dev, netdev_features_t features);
};
struct ethtool_ops;
struct net_device;
static inline netdev_features_t passthru_features_check(struct sk_buff *skb, struct net_device *dev, netdev_features_t features) { return features; }
struct net_device {
	char name[IFNAMSIZ];
	unsigned long state;
	struct device dev;
	netdev_features_t features;
	netdev_features_t hw_features;
	netdev_features_t vlan_features;
	netdev_features_t hw_enc_features;
	netdev_features_t mpls_features;
	unsigned int flags;
	unsigned int priv_flags;
	const struct net_device_ops *netdev_ops;
	const struct ethtool_ops *ethtool_ops;
	unsigned int mtu;
	unsigned int min_mtu;
	unsigned int max_mtu;
	unsigned short hard_header_len;
	unsigned char addr_len;
	unsigned char perm_addr[MAX_ADDR_LEN];
	unsigned char hamix_addr[MAX_ADDR_LEN];
	const unsigned char *dev_addr;
	unsigned char broadcast[MAX_ADDR_LEN];
	int watchdog_timeo;
	int irq;
	unsigned long mem_start;
	unsigned long mem_end;
	unsigned long base_addr;
	unsigned int gso_max_size;
	struct netdev_hw_addr_list uc;
	struct netdev_hw_addr_list mc;
	struct net_device_stats stats;
	struct netdev_queue hamix_txq;
	int hamix_id;
	int hamix_carrier;
	int hamix_running;
	int hamix_registered;
	struct napi_struct *hamix_napi;
	void *hamix_priv;
};
#define SET_NETDEV_DEV(net, pdev) ((net)->dev.parent = (pdev))
static inline void *netdev_priv(const struct net_device *dev) { return dev->hamix_priv; }
struct net_device *alloc_etherdev_mqs(int sizeof_priv, unsigned int txqs, unsigned int rxqs);
static inline struct net_device *alloc_etherdev(int sizeof_priv) { return alloc_etherdev_mqs(sizeof_priv, 1, 1); }
void free_netdev(struct net_device *dev);
int register_netdev(struct net_device *dev);
void unregister_netdev(struct net_device *dev);
static inline void eth_hw_addr_set(struct net_device *dev, const u8 *addr) { memcpy(dev->hamix_addr, addr, ETH_ALEN); }
static inline void dev_addr_set(struct net_device *dev, const u8 *addr) { eth_hw_addr_set(dev, addr); }
static inline int eth_validate_addr(struct net_device *dev) { return is_valid_ether_addr(dev->dev_addr) ? 0 : -EINVAL; }
static inline void netdev_rss_key_fill(void *buffer, size_t len) { u8 *b = buffer; for (size_t i = 0; i < len; i++) b[i] = (u8)(i * 37 + 11); }
__be16 eth_type_trans(struct sk_buff *skb, struct net_device *dev);
void linuxkpi_netif_carrier(struct net_device *dev, int on);
static inline void netif_carrier_on(struct net_device *dev) { linuxkpi_netif_carrier(dev, 1); }
static inline void netif_carrier_off(struct net_device *dev) { linuxkpi_netif_carrier(dev, 0); }
static inline bool netif_carrier_ok(const struct net_device *dev) { return dev->hamix_carrier != 0; }
static inline bool netif_running(const struct net_device *dev) { return dev->hamix_running != 0; }
static inline bool netif_device_present(const struct net_device *dev) { return true; }
static inline void netif_device_attach(struct net_device *dev) {}
static inline void netif_device_detach(struct net_device *dev) {}
static inline void netif_start_queue(struct net_device *dev) { dev->hamix_txq.state = 0; }
static inline void netif_stop_queue(struct net_device *dev) { dev->hamix_txq.state = 1; }
static inline void netif_wake_queue(struct net_device *dev) { dev->hamix_txq.state = 0; }
static inline void netif_tx_start_all_queues(struct net_device *dev) { netif_start_queue(dev); }
static inline void netif_tx_stop_all_queues(struct net_device *dev) { netif_stop_queue(dev); }
static inline void netif_tx_wake_all_queues(struct net_device *dev) { netif_wake_queue(dev); }
static inline bool netif_queue_stopped(const struct net_device *dev) { return dev->hamix_txq.state != 0; }
static inline struct netdev_queue *netdev_get_tx_queue(const struct net_device *dev, unsigned int index) { return (struct netdev_queue *)&dev->hamix_txq; }
static inline bool netif_xmit_stopped(const struct netdev_queue *q) { return q->state != 0; }
static inline void netif_trans_update(struct net_device *dev) { dev->hamix_txq.trans_start = jiffies; }
static inline unsigned long dev_trans_start(struct net_device *dev) { return dev->hamix_txq.trans_start; }
static inline void netdev_sent_queue(struct net_device *dev, unsigned int bytes) { dev->hamix_txq.trans_start = jiffies; }
static inline void netdev_completed_queue(struct net_device *dev, unsigned int pkts, unsigned int bytes) {}
static inline void netdev_reset_queue(struct net_device *dev) {}
static inline bool netdev_xmit_more(void) { return false; }
#define smp_mb__after_netif_stop_queue() smp_mb()
static inline u32 netif_msg_init(int debug_value, int default_msg_enable_bits) { if (debug_value < 0 || debug_value >= 32) return default_msg_enable_bits; if (debug_value == 0) return 0; return (1U << debug_value) - 1; }
#define netif_msg_drv(p) ((p)->msg_enable & NETIF_MSG_DRV)
#define netif_msg_probe(p) ((p)->msg_enable & NETIF_MSG_PROBE)
#define netif_msg_link(p) ((p)->msg_enable & NETIF_MSG_LINK)
#define netif_msg_hw(p) ((p)->msg_enable & NETIF_MSG_HW)
#define netif_msg_rx_status(p) ((p)->msg_enable & NETIF_MSG_RX_STATUS)
#define netif_msg_tx_done(p) ((p)->msg_enable & NETIF_MSG_TX_DONE)
#define netif_msg_pktdata(p) ((p)->msg_enable & NETIF_MSG_PKTDATA)
#define netif_msg_tx_err(p) ((p)->msg_enable & NETIF_MSG_TX_ERR)
#define netif_msg_rx_err(p) ((p)->msg_enable & NETIF_MSG_RX_ERR)
void linuxkpi_netdev_printk(u32 level, const struct net_device *dev, const char *fmt, ...) __printf(3, 4);
#define netdev_err(dev, fmt, ...) linuxkpi_netdev_printk(3, dev, fmt, ##__VA_ARGS__)
#define netdev_warn(dev, fmt, ...) linuxkpi_netdev_printk(4, dev, fmt, ##__VA_ARGS__)
#define netdev_notice(dev, fmt, ...) linuxkpi_netdev_printk(5, dev, fmt, ##__VA_ARGS__)
#define netdev_info(dev, fmt, ...) linuxkpi_netdev_printk(6, dev, fmt, ##__VA_ARGS__)
#define netdev_dbg(dev, fmt, ...) do { if (0) linuxkpi_netdev_printk(7, dev, fmt, ##__VA_ARGS__); } while (0)

struct napi_struct {
	int (*poll)(struct napi_struct *napi, int budget);
	struct net_device *dev;
	int weight;
	volatile unsigned long state;
	struct work_struct hamix_work;
};
#define NAPI_STATE_SCHED 0
#define NAPI_STATE_DISABLE 1
void netif_napi_add(struct net_device *dev, struct napi_struct *napi, int (*poll)(struct napi_struct *, int));
static inline void netif_napi_add_weight(struct net_device *dev, struct napi_struct *napi, int (*poll)(struct napi_struct *, int), int weight) { netif_napi_add(dev, napi, poll); napi->weight = weight; }
static inline void netif_napi_del(struct napi_struct *napi) {}
bool napi_schedule_prep(struct napi_struct *napi);
void __napi_schedule(struct napi_struct *napi);
static inline void napi_schedule(struct napi_struct *napi) { if (napi_schedule_prep(napi)) __napi_schedule(napi); }
static inline void __napi_schedule_irqoff(struct napi_struct *napi) { __napi_schedule(napi); }
bool napi_complete_done(struct napi_struct *napi, int work_done);
static inline bool napi_complete(struct napi_struct *napi) { return napi_complete_done(napi, 0); }
void napi_enable(struct napi_struct *napi);
void napi_disable(struct napi_struct *napi);
void napi_synchronize(const struct napi_struct *napi);
typedef int gro_result_t;
gro_result_t napi_gro_receive(struct napi_struct *napi, struct sk_buff *skb);
int netif_receive_skb(struct sk_buff *skb);
static inline int netif_rx(struct sk_buff *skb) { return netif_receive_skb(skb); }

#define ADVERTISED_10baseT_Half BIT(0)
#define ADVERTISED_10baseT_Full BIT(1)
#define ADVERTISED_100baseT_Half BIT(2)
#define ADVERTISED_100baseT_Full BIT(3)
#define ADVERTISED_1000baseT_Half BIT(4)
#define ADVERTISED_1000baseT_Full BIT(5)
#define ADVERTISED_Autoneg BIT(6)
#define ADVERTISED_TP BIT(7)
#define ADVERTISED_AUI BIT(8)
#define ADVERTISED_MII BIT(9)
#define ADVERTISED_FIBRE BIT(10)
#define ADVERTISED_Pause BIT(13)
#define ADVERTISED_Asym_Pause BIT(14)
#define SPEED_10 10
#define SPEED_100 100
#define SPEED_1000 1000
#define SPEED_UNKNOWN -1
#define DUPLEX_HALF 0
#define DUPLEX_FULL 1
#define DUPLEX_UNKNOWN 0xff
#define AUTONEG_DISABLE 0
#define AUTONEG_ENABLE 1
#define ETH_TP_MDI_INVALID 0
#define ETH_TP_MDI 1
#define ETH_TP_MDI_X 2
#define ETH_TP_MDI_AUTO 3
#define WAKE_PHY BIT(0)
#define WAKE_UCAST BIT(1)
#define WAKE_MCAST BIT(2)
#define WAKE_BCAST BIT(3)
#define WAKE_ARP BIT(4)
#define WAKE_MAGIC BIT(5)
#define ETH_GSTRING_LEN 32

#define MII_BMCR 0x00
#define MII_BMSR 0x01
#define MII_PHYSID1 0x02
#define MII_PHYSID2 0x03
#define MII_ADVERTISE 0x04
#define MII_LPA 0x05
#define MII_EXPANSION 0x06
#define MII_CTRL1000 0x09
#define MII_STAT1000 0x0a
#define MII_ESTATUS 0x0f
#define BMCR_RESV 0x003f
#define BMCR_SPEED1000 0x0040
#define BMCR_CTST 0x0080
#define BMCR_FULLDPLX 0x0100
#define BMCR_ANRESTART 0x0200
#define BMCR_ISOLATE 0x0400
#define BMCR_PDOWN 0x0800
#define BMCR_ANENABLE 0x1000
#define BMCR_SPEED100 0x2000
#define BMCR_LOOPBACK 0x4000
#define BMCR_RESET 0x8000
#define BMSR_LSTATUS 0x0004
#define BMSR_ANEGCAPABLE 0x0008
#define BMSR_ESTATEN 0x0100
#define BMSR_100HALF2 0x0200
#define BMSR_100FULL2 0x0400
#define BMSR_10HALF 0x0800
#define BMSR_10FULL 0x1000
#define BMSR_100HALF 0x2000
#define BMSR_100FULL 0x4000
#define BMSR_100BASE4 0x8000
#define ADVERTISE_CSMA 0x0001
#define ADVERTISE_ALL (ADVERTISE_10HALF | ADVERTISE_10FULL | ADVERTISE_100HALF | ADVERTISE_100FULL)
#define EXPANSION_NWAY 0x0001
#define EXPANSION_ENABLENPAGE 0x0004
#define ESTATUS_1000_TFULL 0x2000
#define ESTATUS_1000_THALF 0x1000
#define LPA_LPACK 0x4000
#define BMSR_ANEGCOMPLETE 0x0020
#define BMSR_ERCAP 0x0001
#define ADVERTISE_10HALF 0x0020
#define ADVERTISE_10FULL 0x0040
#define ADVERTISE_100HALF 0x0080
#define ADVERTISE_100FULL 0x0100
#define ADVERTISE_PAUSE_CAP 0x0400
#define ADVERTISE_PAUSE_ASYM 0x0800
#define LPA_10HALF 0x0020
#define LPA_10FULL 0x0040
#define LPA_100HALF 0x0080
#define LPA_100FULL 0x0100
#define LPA_PAUSE_CAP 0x0400
#define LPA_PAUSE_ASYM 0x0800
#define ADVERTISE_1000FULL 0x0200
#define ADVERTISE_1000HALF 0x0100
#define LPA_1000FULL 0x0800
#define LPA_1000HALF 0x0400
#define LPA_1000LOCALRXOK 0x2000
#define LPA_1000REMRXOK 0x1000
#define CTL1000_AS_MASTER 0x0800
#define CTL1000_ENABLE_MASTER 0x1000
#define MDIO_MMD_PCS 3
#define MDIO_MMD_AN 7
#define MDIO_PCS_EEE_ABLE 20
#define MDIO_AN_EEE_ADV 60
#define MDIO_AN_EEE_LPABLE 61
#define MDIO_EEE_100TX 0x0002
#define MDIO_EEE_1000T 0x0004
struct mii_ioctl_data { u16 phy_id; u16 reg_num; u16 val_in; u16 val_out; };
static inline struct mii_ioctl_data *if_mii(struct ifreq *rq) { return NULL; }
struct sockaddr { unsigned short sa_family; char sa_data[14]; };
struct ifreq { char ifr_name[IFNAMSIZ]; void *ifr_data; };

#define TRACE_EVENT(name, proto, args, tstruct, assign, print) static inline void trace_##name(proto) {}
#define TP_PROTO(...) __VA_ARGS__
#define TP_ARGS(...) __VA_ARGS__
#define TP_STRUCT__entry(...)
#define TP_fast_assign(...)
#define TP_printk(...)

void rtnl_lock(void);
struct module;
#define module_param(name, type, perm)
#define module_param_named(name, value, type, perm)
#define module_param_array(name, type, nump, perm)
#define module_param_array_named(name, array, type, nump, perm)

int linuxkpi_module_init(int (*fn)(void));
void linuxkpi_module_exit(void (*fn)(void));
#define module_init(fn) int hamix_module_init(void) { if (hamix_kpi_version() < HAMIX_KPI_VERSION) return -1; return linuxkpi_module_init(fn); }
#define module_exit(fn) void hamix_module_exit(void) { linuxkpi_module_exit(fn); }

#endif
