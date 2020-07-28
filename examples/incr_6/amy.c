/* This example demonstrates:
- Synchronized PUTs and GETs always succeed together.
- Attemping to put without a corresponding GET results in `connector_sync` failing at runtime.
*/
#include <stdio.h>
#include <string.h>
#include "../../reowolf.h"
#include "../utility.c"

int main(int argc, char** argv) {
	char * pdl_ptr = buffer_pdl("eg_protocols.pdl");
	size_t pdl_len = strlen(pdl_ptr);
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl_ptr, pdl_len);
	char logpath[] = "./6_amy_log.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	printf("Err %s\n", reowolf_error_peek(NULL));
	
	PortId putter, getter;
	connector_add_port_pair(c, &putter, &getter);
	connector_connect(c, -1);
	connector_print_debug(c);
	
	printf("Let's try to put without get\n");
	connector_put_bytes(c, putter, "hello", 5);
	// connector_get(c, getter);
	int err = connector_sync(c, 5000); // 5000 means 5000millisec timeout duration
	printf("Error code %d with string `%s`\n", err, reowolf_error_peek(NULL));
	
	protocol_description_destroy(pd);
	connector_destroy(c);
	free(pdl_ptr);
	return 0;
}