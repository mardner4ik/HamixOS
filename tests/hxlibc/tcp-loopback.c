#include <arpa/inet.h>
#include <netinet/in.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

int main(void)
{
    int server = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in addr = { 0 };
    addr.sin_family = AF_INET;
    addr.sin_port = htons(5021);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    int one = 1;
    setsockopt(server, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
    if (bind(server, (struct sockaddr *)&addr, sizeof(addr)) < 0 || listen(server, 4) < 0) {
        printf("FAIL tcp loopback listen\n");
        return 1;
    }
    pid_t child = fork();
    if (child == 0) {
        int client = socket(AF_INET, SOCK_STREAM, 0);
        if (connect(client, (struct sockaddr *)&addr, sizeof(addr)) < 0)
            _exit(2);
        const char *hello = "ping over 127.0.0.1";
        write(client, hello, strlen(hello));
        char reply[64] = { 0 };
        ssize_t n = read(client, reply, sizeof(reply) - 1);
        close(client);
        _exit(n > 0 && strcmp(reply, "pong") == 0 ? 0 : 3);
    }
    struct sockaddr_in peer;
    socklen_t len = sizeof(peer);
    int conn = accept(server, (struct sockaddr *)&peer, &len);
    char buf[64] = { 0 };
    ssize_t n = conn >= 0 ? read(conn, buf, sizeof(buf) - 1) : -1;
    printf("%s tcp loopback accept and read (%zd bytes: %s)\n", n > 0 && strcmp(buf, "ping over 127.0.0.1") == 0 ? "PASS" : "FAIL", n, buf);
    write(conn, "pong", 4);
    int status = 0;
    waitpid(child, &status, 0);
    printf("%s tcp loopback client got the reply (exit %d)\n", WIFEXITED(status) && WEXITSTATUS(status) == 0 ? "PASS" : "FAIL", WEXITSTATUS(status));
    char tail[8];
    printf("%s tcp loopback EOF after peer close\n", read(conn, tail, sizeof(tail)) == 0 ? "PASS" : "FAIL");
    close(conn);
    close(server);
    return 0;
}
