
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
	FfiSocketAddr addr = {{127,0,0,1}, 8000};
	connector_add_net_port(c, &ports[0], addr, Polarity_Putter, EndpointPolarity_Passive);
	rw_err_peek(c);
	addr.port = 8001;
	connector_add_net_port(c, &ports[1], addr, Polarity_Putter, EndpointPolarity_Passive);
	rw_err_peek(c);
	
	// Connect with peers (5000ms timeout).
	connector_connect(c, 5000);
	rw_err_peek(c);
	
	printf("\nputting {A}...\n");
	connector_put_bytes(c, ports[0], "A", 1);
	connector_sync(c, 1000);
	rw_err_peek(c);

	printf("\nputting {B}...\n");
	connector_put_bytes(c, ports[1], "B", 1);
	connector_sync(c, 1000);
	rw_err_peek(c);

	printf("\nputting {A, B}...\n");
	connector_put_bytes(c, ports[0], "A", 1);
	connector_put_bytes(c, ports[1], "B", 1);
	connector_sync(c, 1000);
	rw_err_peek(c);
	
	printf("\nExiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	sleep(1.0);
	return 0;
}