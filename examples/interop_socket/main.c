#include <netinet/in.h> // definies socketaddr_in
#include <stdio.h>  // defines printf
#include <stdlib.h> // defines malloc, free
#include <unistd.h> // defines close
#define BUFSIZE 512
int main() {
    // --- setup ---
    struct sockaddr_in addrs[2]; 
    /* (address structure initializations omitted) */
    int fd = socket(AF_INET, SOCK_DGRAM, 0); 
    bind(fd, (const struct sockaddr *)&addrs[0], sizeof(addrs[0]));
    connect(fd, (const struct sockaddr *)&addrs[1], sizeof(addrs[1]));
    // --- communication ---
    char * buffer = malloc(BUFSIZE);
    size_t msglen, i;
    msglen = recv(fd, (void *)buffer, BUFSIZE, 0);
    for(i=0; i<msglen; i++) {
        printf("%02X", buffer[i]);
    }
    // --- cleanup ---
    close(fd);
    free(buffer);
    return 0;
}
