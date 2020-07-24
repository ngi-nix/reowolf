#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include "../../pseudo_socket.h"
#define BUFSIZE 512
int main() {
  // --- setup ---
  struct sockaddr_in addrs[2]; 
  /* (address structure initializations omitted) */
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
