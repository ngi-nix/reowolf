#include <stdio.h>
#include <string.h>
#include "../../reowolf.h"
#include "../utility.c"

int main(int argc, char** argv) {
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./9_amy_log.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	PortId putter, getter;
	char addr_str[] = "127.0.0.1:8000";
	connector_add_net_port(
		c, &putter, addr_str, sizeof(addr_str)-1, Putter, Active);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	connector_add_net_port(
		c, &getter, addr_str, sizeof(addr_str)-1, Getter, Passive);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	connector_connect(c, 4000);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	connector_put_bytes(c, putter, "hi", 2);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	connector_get(c, getter);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	connector_sync(c, 4000);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	
	protocol_description_destroy(pd);
	connector_destroy(c);
	return 0;
}