
#include "../../reowolf.h"
#include "../utility.c"


int main(int argc, char** argv) {
	// Create a connector, configured with a protocol defined in a file
	char * pdl = buffer_pdl("./eg_protocols.pdl");
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, strlen(pdl));
	char logpath[] = "./pres_3_bob.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	rw_err_peek(c);

	// ... with 2 outgoing network connections
	PortId ports[4];
	char * addr = "127.0.0.1:8000";
	connector_add_net_port(c, &ports[0], addr, strlen(addr),
			Polarity_Getter, EndpointPolarity_Active);
	rw_err_peek(c);
	addr = "127.0.0.1:8001";
	connector_add_net_port(c, &ports[1], addr, strlen(addr),
			Polarity_Getter, EndpointPolarity_Active);
	connector_add_port_pair(c, &ports[2], &ports[3]);
	connector_add_component(c, "alt_round_merger", 16, ports, 3);
	rw_err_peek(c);
	
	// Connect with peers (5000ms timeout).
	connector_connect(c, 5000);
	rw_err_peek(c);

	for(int round=0; round<3; round++) {
		printf("\nRound %d\n", round);
		connector_get(c, ports[3]);
		rw_err_peek(c);
		connector_sync(c, 1000);
		rw_err_peek(c);

		size_t msg_len = 0;
		const char * msg_ptr = connector_gotten_bytes(c, ports[3], &msg_len);
		printf("Got msg `%.*s`\n", msg_len, msg_ptr);
	}
	
	printf("Exiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	free(pdl);
	sleep(1.0);
	return 0;
}