#include <stdio.h>
#include <string.h>
#include "../../reowolf.h"
#include "../utility.c"

int main(int argc, char** argv) {
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./8_amy_log.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	PortId putter, getter;
	char addr_str[] = "127.0.0.1:8000";
	connector_add_net_port(
		c, &putter, addr_str, sizeof(addr_str)-1, Polarity_Putter, EndpointPolarity_Active);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	connector_add_net_port(
		c, &getter, addr_str, sizeof(addr_str)-1, Polarity_Getter, EndpointPolarity_Passive);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	connector_connect(c, 4000);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	
	protocol_description_destroy(pd);
	connector_destroy(c);
	return 0;
}