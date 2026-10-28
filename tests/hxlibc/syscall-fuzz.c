#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define ARENA_SIZE (64 * 1024)

struct call_record {
	volatile uint64_t number;
	volatile uint64_t args[6];
	volatile uint64_t count;
	volatile uint8_t missing[9216];
};

static struct call_record *record;
static uint64_t rng_state;
static unsigned char *arena;
static int fds[24];
static int fd_count;
static int trace_fd = -1;

static const char *paths[] = {
	".", "..", "/", "/tmp", "/proc/self/stat", "/proc/self/maps", "/proc/meminfo", "/dev/null",
	"/dev/zero", "/dev/ptmx", "/dev/tty", "/dev/dsp", "/dev/mixer", "/dev/dri/card0", "/dev/fb0",
	"/dev/input/event0", "/dev/sda", "/sys/block", "/etc/passwd", "/etc/shadow", "/usr/bin/bash",
	"f0", "f1", "d0", "d0/f2", "link", "/run", "/proc/1/status", "/proc/self/fd/0", "",
	"/nonexistent/deep/path", "/linux/usr/bin",
};

static const uint64_t interesting[] = {
	0, 1, 2, 3, 7, 8, 15, 16, 31, 32, 63, 64, 127, 128, 255, 256, 511, 512, 4095, 4096, 4097,
	65535, 65536, 0x7fffffff, 0x80000000, 0xffffffff, 0x100000000ull, 0x7fffffffffffffffull,
	0x8000000000000000ull, 0xffffffffffffffffull, 0xfffffffffffff000ull, 0xffffffffffffff9cull,
};

static const uint64_t bad_pointers[] = {
	0, 1, 8, 0x1000, 0x10000, 0x400000, 0x7ffffffff000ull, 0x7fffffffffffull, 0x800000000000ull,
	0xffff800000000000ull, 0xffffffff80000000ull, 0xfffffffffffff000ull, 0xffffffffffffffffull,
};

static int skipped(uint64_t n)
{
	switch (n) {
	case SYS_exit:
	case SYS_exit_group:
	case SYS_fork:
	case SYS_vfork:
	case SYS_clone:
	case 435:
	case SYS_execve:
	case SYS_execveat:
	case SYS_reboot:
	case SYS_vhangup:
	case 9002:
	case 9005:
	case 9010:
	case 9043:
	case 9111:
	case 9131:
	case 9133:
	case 9134:
	case 9135:
	case 9137:
	case 9138:
	case 9161:
	case 9162:
	case 9163:
	case 9166:
		return 1;
	}
	return 0;
}

static uint64_t next(void)
{
	rng_state ^= rng_state << 13;
	rng_state ^= rng_state >> 7;
	rng_state ^= rng_state << 17;
	return rng_state;
}

static uint64_t below(uint64_t n)
{
	return n ? next() % n : 0;
}

static uint64_t arena_pointer(void)
{
	uint64_t offset = below(8) == 0 ? ARENA_SIZE - below(64) : below(ARENA_SIZE);
	return (uint64_t)(uintptr_t)(arena + offset);
}

static uint64_t argument(void)
{
	switch (below(12)) {
	case 0:
		return below(8);
	case 1:
	case 2:
		return fd_count ? (uint64_t)fds[below(fd_count)] : 0;
	case 3:
		return interesting[below(sizeof interesting / sizeof interesting[0])];
	case 4:
		return bad_pointers[below(sizeof bad_pointers / sizeof bad_pointers[0])];
	case 5:
	case 6:
		return arena_pointer();
	case 7:
		return (uint64_t)(uintptr_t)paths[below(sizeof paths / sizeof paths[0])];
	case 8:
		return 1ull << below(64);
	case 9:
		return (uint64_t)-(int64_t)below(140);
	case 10:
		return below(65536);
	default:
		return next();
	}
}

static void scramble_arena(void)
{
	uint64_t *words = (uint64_t *)arena;
	for (size_t i = 0; i < ARENA_SIZE / 8; i++) {
		switch (below(6)) {
		case 0:
			words[i] = arena_pointer();
			break;
		case 1:
			words[i] = below(4096);
			break;
		case 2:
			words[i] = (uint64_t)(uintptr_t)paths[below(sizeof paths / sizeof paths[0])];
			break;
		case 3:
			words[i] = 0;
			break;
		default:
			words[i] = next();
		}
	}
	size_t at = below(ARENA_SIZE - 64);
	memcpy(arena + at, paths[below(sizeof paths / sizeof paths[0])], 16);
}

static void open_fds(void)
{
	int pipe_fds[2];
	int pair[2];
	fd_count = 0;
	if (pipe(pipe_fds) == 0) {
		fds[fd_count++] = pipe_fds[0];
		fds[fd_count++] = pipe_fds[1];
	}
	if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair) == 0) {
		fds[fd_count++] = pair[0];
		fds[fd_count++] = pair[1];
	}
	if (socketpair(AF_UNIX, SOCK_DGRAM, 0, pair) == 0)
		fds[fd_count++] = pair[0];
	int candidates[] = {
		open("f0", O_RDWR | O_CREAT, 0644),
		open("/dev/null", O_RDWR),
		open("/dev/zero", O_RDONLY),
		open("/proc/self/stat", O_RDONLY),
		open("/", O_RDONLY | O_DIRECTORY),
		open("/dev/ptmx", O_RDWR | O_NOCTTY),
		open("/dev/dri/card0", O_RDWR),
		open("/dev/dsp", O_WRONLY | O_NONBLOCK),
		open("/dev/input/event0", O_RDONLY | O_NONBLOCK),
		socket(AF_UNIX, SOCK_SEQPACKET, 0),
		socket(AF_INET, SOCK_DGRAM, 0),
		socket(AF_NETLINK, SOCK_RAW, 15),
		(int)syscall(SYS_eventfd2, 0, 0),
		(int)syscall(SYS_epoll_create1, 0),
		(int)syscall(SYS_memfd_create, "fuzz", 0),
		(int)syscall(SYS_timerfd_create, 1, 0),
	};
	for (size_t i = 0; i < sizeof candidates / sizeof candidates[0]; i++)
		if (candidates[i] >= 0 && fd_count < (int)(sizeof fds / sizeof fds[0]))
			fds[fd_count++] = candidates[i];
}

static uint64_t pick_number(void)
{
	for (;;) {
		uint64_t n = below(4) == 0 ? 9000 + below(192) : below(450);
		if (!skipped(n) && !record->missing[n])
			return n;
	}
}

static void child(uint64_t seed, unsigned calls, uid_t uid)
{
	rng_state = seed * 0x9e3779b97f4a7c15ull + 1;
	char dir[64];
	snprintf(dir, sizeof dir, "/tmp/fuzz/%llu", (unsigned long long)seed);
	mkdir(dir, 0755);
	chown(dir, uid, uid);
	if (chdir(dir) != 0)
		_exit(2);
	gid_t groups[] = {29, 44, 97, 3003};
	if (uid == 65534)
		setgroups(0, NULL);
	else
		setgroups(4, groups);
	if (setgid(uid) != 0 || setuid(uid) != 0)
		_exit(3);
	if (trace_fd >= 0)
		trace_fd = dup(1);
	int devnull = open("/dev/null", O_RDWR);
	dup2(devnull, 0);
	dup2(devnull, 1);
	dup2(devnull, 2);
	setsid();
	arena = mmap(NULL, ARENA_SIZE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	if (arena == MAP_FAILED)
		_exit(4);
	mkdir("d0", 0755);
	symlink("f0", "link");
	open_fds();
	alarm(3);
	for (unsigned i = 0; i < calls; i++) {
		if (i % 64 == 0)
			scramble_arena();
		uint64_t n = pick_number();
		uint64_t a[6];
		for (int k = 0; k < 6; k++)
			a[k] = argument();
		record->number = n;
		for (int k = 0; k < 6; k++)
			record->args[k] = a[k];
		record->count++;
		if (trace_fd >= 0) {
			char line[200];
			int len = snprintf(line, sizeof line, "call %u: %llu(%#llx, %#llx, %#llx, %#llx, %#llx, %#llx)\n", i,
					   (unsigned long long)n, (unsigned long long)a[0], (unsigned long long)a[1],
					   (unsigned long long)a[2], (unsigned long long)a[3], (unsigned long long)a[4],
					   (unsigned long long)a[5]);
			write(trace_fd, line, len);
		}
		if (syscall(n, a[0], a[1], a[2], a[3], a[4], a[5]) == -1 && errno == ENOSYS)
			record->missing[n] = 1;
	}
	_exit(0);
}

static int reap(pid_t pid, int ms, int *status)
{
	for (int waited = 0; waited < ms; waited += 10) {
		pid_t got = waitpid(pid, status, WNOHANG);
		if (got == pid)
			return 1;
		struct timespec ts = {0, 10 * 1000 * 1000};
		nanosleep(&ts, NULL);
	}
	return 0;
}

static void show_record(const char *what, uint64_t seed)
{
	printf("%s seed %llu after %llu calls: syscall %llu(%#llx, %#llx, %#llx, %#llx, %#llx, %#llx)\n", what,
	       (unsigned long long)seed, (unsigned long long)record->count, (unsigned long long)record->number,
	       (unsigned long long)record->args[0], (unsigned long long)record->args[1],
	       (unsigned long long)record->args[2], (unsigned long long)record->args[3],
	       (unsigned long long)record->args[4], (unsigned long long)record->args[5]);
	fflush(stdout);
}

int main(int argc, char **argv)
{
	unsigned children = argc > 1 ? (unsigned)strtoul(argv[1], NULL, 0) : 200;
	unsigned calls = argc > 2 ? (unsigned)strtoul(argv[2], NULL, 0) : 2000;
	uint64_t first = argc > 3 ? strtoull(argv[3], NULL, 0) : 1;
	if (argc > 4 && strcmp(argv[4], "trace") == 0)
		trace_fd = 1;
	record = mmap(NULL, sizeof *record, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
	if (record == MAP_FAILED) {
		perror("mmap");
		return 1;
	}
	mkdir("/tmp/fuzz", 01777);
	chmod("/tmp/fuzz", 01777);
	unsigned stuck = 0;
	unsigned timeouts = 0;
	unsigned signalled = 0;
	for (unsigned c = 0; c < children; c++) {
		uint64_t seed = first + c;
		record->count = 0;
		record->number = 0;
		printf("fuzz: seed %llu\n", (unsigned long long)seed);
		fflush(stdout);
		pid_t pid = fork();
		if (pid < 0) {
			perror("fork");
			return 1;
		}
		if (pid == 0)
			child(seed, calls, c % 2 ? 1000 : 65534);
		int status = 0;
		if (!reap(pid, 5000, &status)) {
			timeouts++;
			show_record("fuzz: timeout", seed);
			kill(pid, SIGKILL);
			if (!reap(pid, 4000, &status)) {
				stuck++;
				show_record("FAIL child ignores SIGKILL", seed);
				continue;
			}
		}
		if (WIFSIGNALED(status))
			signalled++;
	}
	printf("fuzz: %u children, %u killed by signals, %u timeouts, %u stuck\n", children, signalled, timeouts, stuck);
	return stuck ? 1 : 0;
}
