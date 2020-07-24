#include <sys/socket.h> // defines {sockaddr, socklen_t}

int rw_socket(int domain, int type, int protocol);
int rw_connect(int fd, const struct sockaddr *address, socklen_t address_len);
int rw_bind(int socket, const struct sockaddr *address, socklen_t address_len);
int rw_close(int fd);
ssize_t rw_send(int fd, const void * message, size_t length, int flags);
ssize_t rw_recv(int fd, void * buffer, size_t length, int flags);