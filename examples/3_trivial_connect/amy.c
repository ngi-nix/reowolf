#include <stdio.h>
#include <string.h>
#include "../../reowolf.h"
#include "../utility.c"

int main(int argc, char** argv) {
	char * pdl_ptr = buffer_pdl("eg_protocols.pdl");
	size_t pdl_len = strlen(pdl_ptr);
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl_ptr, pdl_len);
	Connector * c = connector_new(pd);
	connector_connect(c, 100);
	
	connector_print_debug(c);
	
	protocol_description_destroy(pd);
	connector_destroy(c);
	free(pdl_ptr);
	return 0;
}