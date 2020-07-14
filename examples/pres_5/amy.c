
#include "../../reowolf.h"
#include "../utility.c"


int main(int argc, char** argv) {
	// Create a connector, configured with our (trivial) protocol.
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./pres_3_amy.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	rw_err_peek(c);
	
	// ... with 2 outgoing network connections
	PortId ports[2];
	char * addr = "127.0.0.1:8000";
	connector_add_net_port(c, &ports[0], addr, strlen(addr),
			Polarity_Putter, EndpointPolarity_Passive);
	rw_err_peek(c);
	addr = "127.0.0.1:8001";
	connector_add_net_port(c, &ports[1], addr, strlen(addr),
			Polarity_Putter, EndpointPolarity_Passive);
	rw_err_peek(c);
	
	// Connect with peers (5000ms timeout).
	connector_connect(c, 5000);
	rw_err_peek(c);

	printf("Round 0. Putting {ports[0]=\"r0p0\", ports[1]=\"r0p1\"}\n");
	connector_put_bytes(c, ports[0], "r0p0", 4);
	connector_put_bytes(c, ports[1], "r0p1", 4);
	connector_sync(c, 1000);
	rw_err_peek(c);

	printf("Round 1. Putting {ports[1]=\"r1p1\"}\n");
	connector_put_bytes(c, ports[1], "r1p1", 4);
	connector_sync(c, 1000);
	rw_err_peek(c);

	printf("Round 2. Putting {ports[0]=\"r2p0\"}\n");
	connector_put_bytes(c, ports[0], "r2p0", 4);
	connector_sync(c, 1000);
	rw_err_peek(c);

	printf("\nExiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	sleep(1.0);
	return 0;
}