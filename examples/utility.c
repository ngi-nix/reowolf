#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <string.h>
#include <unistd.h>
#include "../reowolf.h"

size_t get_user_msg(char * buf, size_t cap) {
	memset(buf, 0, cap);
	printf("Insert a msg of max len %d: ", cap);
	fgets(buf, cap, stdin);
	for(size_t len = 0; len<cap; len++)
		if(buf[len]==0 || buf[len]=='\n')
			return len;
	return cap;
}
void rw_err_peek(Connector * c) {
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
}

// allocates a buffer!
char * buffer_pdl(char * filename) {
	FILE *f = fopen(filename, "rb");
	if (f == NULL) {
		printf("Opening pdl file returned errno %d!\n", errno);
		exit(1);
	}
	fseek(f, 0, SEEK_END);
	long fsize = ftell(f);
	fseek(f, 0, SEEK_SET);
	char *pdl = malloc(fsize + 1);
	fread(pdl, 1, fsize, f);
	fclose(f);
	pdl[fsize] = 0;
	return pdl;
}
