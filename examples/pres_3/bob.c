
#include "../../reowolf.h"
#include "../utility.c"


int main(int argc, char** argv) {
	// Create a connector, configured with our (trivial) protocol.
	Arc_ProtocolDescription * pd = protocol_description_parse("", 0);
	char logpath[] = "./pres_3_bob.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	rw_err_peek(c);

	// ... with 2 outgoing network connections
	PortId ports[2];
	FfiSocketAddr addr = {{127,0,0,1}, 8000};
	connector_add_net_port(c, &ports[0], addr, Polarity_Getter, EndpointPolarity_Active);
	rw_err_peek(c);
	addr.port = 8001;
	connector_add_net_port(c, &ports[1], addr, Polarity_Getter, EndpointPolarity_Active);
	rw_err_peek(c);
	
	// Connect with peers (5000ms timeout).
	connector_connect(c, 5000);
	rw_err_peek(c);

	for(int i=0; i<3; i++) {
		printf("\nGetting from both...\n");
		connector_get(c, ports[0]);
		rw_err_peek(c);
		connector_get(c, ports[1]);
		rw_err_peek(c);
		connector_sync(c, 1000);
		rw_err_peek(c);
	}
	
	printf("Exiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	sleep(1.0);
	return 0;
}