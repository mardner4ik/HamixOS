#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
static __thread int tls_value = 7;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static int counter;
static void *worker(void *arg) {
    long id = (long)arg;
    tls_value = 100 + id;
    for (int i = 0; i < 2000; i++) {
        char *p = malloc(16 + (i % 200));
        memset(p, 'a', 16);
        pthread_mutex_lock(&lock);
        counter++;
        pthread_mutex_unlock(&lock);
        free(p);
    }
    printf("thread %ld tls=%d self=%p\n", id, tls_value, (void*)pthread_self());
    return (void*)(id * 10);
}
int main(void) {
    pthread_t t[4];
    printf("main self=%p tls=%d\n", (void*)pthread_self(), tls_value);
    for (long i = 0; i < 4; i++) pthread_create(&t[i], 0, worker, (void*)i);
    for (int i = 0; i < 4; i++) { void *r; pthread_join(t[i], &r); printf("joined %d -> %ld\n", i, (long)r); }
    printf("counter=%d tls=%d\n", counter, tls_value);
    return 0;
}
