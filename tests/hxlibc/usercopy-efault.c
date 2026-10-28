#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/utsname.h>
#include <sys/wait.h>
#include <unistd.h>

static int in_child(void (*body)(void *), void *arg)
{
	pid_t pid = fork();
	if (pid == 0) {
		body(arg);
		_exit(0);
	}
	int status = 0;
	waitpid(pid, &status, 0);
	return WIFSIGNALED(status) ? WTERMSIG(status) : 0;
}

static void write_byte(void *p)
{
	*(volatile char *)p = 1;
}

static void call_code(void *p)
{
	((void (*)(void))p)();
}

static atomic_int reader_state;
static volatile char *volatile shared_page;

static void *reader(void *arg)
{
	(void)arg;
	atomic_store(&reader_state, 1);
	for (;;)
		shared_page[0]++;
}

static void unmap_while_writing(void *arg)
{
	(void)arg;
	shared_page = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	pthread_t thread;
	pthread_create(&thread, NULL, reader, NULL);
	while (atomic_load(&reader_state) == 0)
		sched_yield();
	usleep(20000);
	munmap((void *)shared_page, 4096);
	for (;;)
		pause();
}

static void kernel_writes(char *ro)
{
	int fd = open("/proc/self/status", O_RDONLY);
	errno = 0;
	ssize_t n = read(fd, ro, 64);
	close(fd);
	if (n < 0 && errno == EFAULT)
		printf("PASS read() into a read-only page returns EFAULT\n");
	else
		printf("FAIL read() into a read-only page: %zd errno %d\n", n, errno);

	errno = 0;
	int st = stat("/", (struct stat *)ro);
	if (st < 0 && errno == EFAULT)
		printf("PASS stat() into a read-only page returns EFAULT\n");
	else
		printf("FAIL stat() into a read-only page: %d errno %d\n", st, errno);

	errno = 0;
	int p = pipe((int *)ro);
	if (p < 0 && errno == EFAULT)
		printf("PASS pipe() into a read-only page returns EFAULT\n");
	else
		printf("FAIL pipe() into a read-only page: %d errno %d\n", p, errno);

	errno = 0;
	int u = uname((struct utsname *)ro);
	if (u < 0 && errno == EFAULT)
		printf("PASS uname() into a read-only page returns EFAULT\n");
	else
		printf("FAIL uname() into a read-only page: %d errno %d\n", u, errno);

	char *none = mmap(NULL, 4096, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	errno = 0;
	ssize_t w = write(1, none, 1);
	if (w < 0 && errno == EFAULT)
		printf("PASS write() from a PROT_NONE page returns EFAULT\n");
	else
		printf("FAIL write() from a PROT_NONE page: %zd errno %d\n", w, errno);
	munmap(none, 4096);
}

static void unmap_under_thread(void)
{
	pid_t pid = fork();
	if (pid == 0) {
		unmap_while_writing(NULL);
		_exit(0);
	}
	int status = 0;
	for (int i = 0; i < 400; i++) {
		if (waitpid(pid, &status, WNOHANG) == pid) {
			if (WIFSIGNALED(status) && WTERMSIG(status) == SIGSEGV)
				printf("PASS a thread writing to a page another thread unmapped gets SIGSEGV\n");
			else
				printf("FAIL unmap under a writing thread: status %#x\n", status);
			return;
		}
		usleep(5000);
	}
	kill(pid, SIGKILL);
	waitpid(pid, &status, 0);
	printf("FAIL a thread kept writing to an unmapped page (stale TLB on another CPU)\n");
}

int main(void)
{
	setvbuf(stdout, NULL, _IONBF, 0);
	char *ro = mmap(NULL, 4096, PROT_READ, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	if (ro == MAP_FAILED) {
		printf("FAIL mmap: %s\n", strerror(errno));
		return 1;
	}
	volatile char touch = ro[0];
	(void)touch;
	errno = 0;
	char *r = getcwd(ro, 4096);
	if (r == NULL && errno == EFAULT)
		printf("PASS getcwd into a read-only page returns EFAULT\n");
	else
		printf("FAIL getcwd into a read-only page: result %p errno %d\n", (void *)r, errno);

	int sig = in_child(write_byte, ro);
	if (sig == SIGSEGV)
		printf("PASS writing to a PROT_READ page raises SIGSEGV\n");
	else
		printf("FAIL writing to a PROT_READ page: signal %d\n", sig);

	if (mprotect(ro, 4096, PROT_READ | PROT_WRITE) == 0 && in_child(write_byte, ro) == 0) {
		ro[1] = 7;
		printf("PASS mprotect(PROT_READ|PROT_WRITE) makes the page writable (%d)\n", ro[1]);
	} else {
		printf("FAIL mprotect to read-write\n");
	}

	unsigned char *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	code[0] = 0xc3;
	sig = in_child(call_code, code);
	if (sig == SIGSEGV)
		printf("PASS executing a page without PROT_EXEC raises SIGSEGV\n");
	else
		printf("FAIL executing a non-executable page: signal %d\n", sig);
	if (mprotect(code, 4096, PROT_READ | PROT_EXEC) == 0 && in_child(call_code, code) == 0)
		printf("PASS the same page runs after mprotect(PROT_READ|PROT_EXEC)\n");
	else
		printf("FAIL mprotect to read-exec\n");
	sig = in_child(write_byte, code);
	if (sig == SIGSEGV)
		printf("PASS the executable page is no longer writable (W^X)\n");
	else
		printf("FAIL W^X: writing to the code page gave signal %d\n", sig);

	if (mprotect(ro, 4096, PROT_READ) == 0)
		kernel_writes(ro);
	unmap_under_thread();

	char good[256];
	if (getcwd(good, sizeof good))
		printf("PASS the process is still alive, cwd %s\n", good);
	return 0;
}
