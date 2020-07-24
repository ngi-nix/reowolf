#include <sys/socket.h> // socket addresses, constants
#include <stdio.h>
#include "pseudo_socket.h"
#define BUFSIZE 512
int main() {
  // --- setup ---
  struct sockaddr_in local, peer; 
  /* (address structure initializations omitted) */
  int fd = rw_socket(AF_INET, SOCK_DGRAM, 0); 
  rw_bind(fd, (const struct sockaddr *)&local, sizeof(local));
  rw_connect(fd, (const struct sockaddr *)&peer, sizeof(peer));
  // --- communication ---
  char buffer = malloc(BUFSIZE);
  size_t msglen, i;
  msglen = rw_recv(fd, (const void *)buffer, BUFSIZE, 0);
  for(i=0; i<msglen; i++) {
    printf("%02X", buffer[i]);
  }
  // --- cleanup ---
  rw_close(fd);
  free(buffer);
  return 0;
}