#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <unistd.h>

static pthread_rwlock_t lock = PTHREAD_RWLOCK_INITIALIZER;
static long shared_a, shared_b;
static atomic_long reads, torn;

static void *writer(void *arg)
{
	long rounds = (long)arg;
	for (long i = 0; i < rounds; i++) {
		pthread_rwlock_wrlock(&lock);
		shared_a++;
		shared_b++;
		pthread_rwlock_unlock(&lock);
	}
	return NULL;
}

static void *reader(void *arg)
{
	long rounds = (long)arg;
	for (long i = 0; i < rounds; i++) {
		pthread_rwlock_rdlock(&lock);
		if (shared_a != shared_b)
			atomic_fetch_add(&torn, 1);
		atomic_fetch_add(&reads, 1);
		pthread_rwlock_unlock(&lock);
	}
	return NULL;
}

int main(void)
{
	alarm(60);
	pthread_t threads[6];
	long rounds = 200000;
	for (int i = 0; i < 6; i++)
		pthread_create(&threads[i], NULL, i % 3 == 0 ? writer : reader, (void *)rounds);
	for (int i = 0; i < 6; i++)
		pthread_join(threads[i], NULL);
	int failed = 0;
	if (shared_a == 2 * rounds && shared_b == 2 * rounds)
		printf("PASS writers finished: %ld updates\n", shared_a);
	else
		printf("FAIL writers lost updates: %ld %ld\n", shared_a, shared_b), failed++;
	if (torn == 0 && reads == 4 * rounds)
		printf("PASS readers saw consistent data (%ld reads)\n", (long)reads);
	else
		printf("FAIL readers: %ld torn, %ld reads\n", (long)torn, (long)reads), failed++;
	if (pthread_rwlock_trywrlock(&lock) == 0) {
		pthread_rwlock_unlock(&lock);
		printf("PASS the lock is free afterwards\n");
	} else {
		printf("FAIL the lock stays taken\n"), failed++;
	}
	return failed;
}
