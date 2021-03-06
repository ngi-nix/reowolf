/* This example demonstrates:
- Once created, ports transport messages regardless of whether
	they were created with `connector_add_port_pair` or `connector_add_net_port`.
*/
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
	FfiSocketAddr addr = {{127, 0, 0, 1}, 8000};
	connector_add_net_port(c, &putter, addr, Polarity_Putter, EndpointPolarity_Active);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	connector_add_net_port(c, &getter, addr, Polarity_Getter, EndpointPolarity_Passive);
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