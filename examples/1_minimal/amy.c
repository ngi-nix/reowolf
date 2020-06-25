#include <stdio.h>
#include <string.h>
#include "../../reowolf.h"
#include "../utility.c"

int main(int argc, char** argv) {
	char * pdl_ptr = buffer_pdl("eg_protocols.pdl");
	size_t pdl_len = strlen(pdl_ptr);
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl_ptr, pdl_len);
	size_t err_len;
	printf("Ptr is %p.\nErr is `%s`.\n", pd, reowolf_error_peek(&err_len));
	free(pdl_ptr);
	return 0;
}