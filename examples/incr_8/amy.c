/* This example demonstrates:
- Synchronous channels can span the network if created by pairs of `connector_add_net_port` calls,
	each binding a port to the same address, with complementary polarities.
- Ports created this way have two notions of polarity:
	- (port) Polarity in {Putter, Getter}:
		- Putter => resulting port can `connector_put`,
		- Getter => resulting port can `connector_get`,
	- Endpoint Polarity in {Active, Passive}:
		- Passive => underlying transport socket will `bind` to the given address,
		- Active  => underlying transport socket will `connect` to the given address,
*/
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
	FfiSocketAddr addr = {{127, 0, 0, 1}, 8000}; // ipv4 127.0.0.1 (localhost) transport port 8000
	connector_add_net_port(c, &putter, addr, Polarity_Putter, EndpointPolarity_Active);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	connector_add_net_port(c, &getter, addr, Polarity_Getter, EndpointPolarity_Passive);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	connector_connect(c, 4000);
	printf("Error str `%s`\n", reowolf_error_peek(NULL));
	
	protocol_description_destroy(pd);
	connector_destroy(c);
	return 0;
}