#include <netinet/in.h> // definies socketaddr_in
#include <stdio.h>  // defines printf
#include <stdlib.h> // defines malloc, free
#include <unistd.h> // defines close
#include <arpa/inet.h> // defines inet_addr
#include "../../pseudo_socket.h"
#define BUFSIZE 512
int main() {
    // --- setup ---
    struct sockaddr_in addrs[2]; 
    addrs[0].sin_family = AF_INET;
    addrs[0].sin_port = htons(8000);
    inet_pton(AF_INET, "127.0.0.1", &addrs[0].sin_addr.s_addr);
    addrs[1].sin_family = AF_INET;
    addrs[1].sin_port = htons(8001);
    inet_pton(AF_INET, "127.0.0.1", &addrs[1].sin_addr.s_addr);
    int fd = rw_socket(AF_INET, SOCK_DGRAM, 0); 
    rw_bind(fd, (const struct sockaddr *)&addrs[0], sizeof(addrs[0]));
    rw_connect(fd, (const struct sockaddr *)&addrs[1], sizeof(addrs[1]));
    // --- communication ---
    char * buffer = malloc(BUFSIZE);
    size_t msglen, i;
    msglen = rw_recv(fd, (void *)buffer, BUFSIZE, 0);
    for(i=0; i<msglen; i++) {
        printf("%02X", buffer[i]);
    }
    // --- cleanup ---
    rw_close(fd);
    free(buffer);
    return 0;
}
