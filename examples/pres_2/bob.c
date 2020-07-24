
#include "../../reowolf.h"
#include "../utility.c"


int main(int argc, char** argv) {
	// Create a connector, configured with a protocol defined in a file
	char * pdl = buffer_pdl("./eg_protocols.pdl");
	Arc_ProtocolDescription * pd = protocol_description_parse(pdl, strlen(pdl));
	char logpath[] = "./pres_2_bob.txt";
	Connector * c = connector_new_logging(pd, logpath, sizeof(logpath)-1);
	rw_err_peek(c);
	
	// ... with 1 outgoing network connection
	PortId ports[3];
	FfiSocketAddr addr = {{127,0,0,1}, 8000};
	connector_add_net_port(c, &ports[0], addr, Polarity_Getter, EndpointPolarity_Active);
	connector_add_port_pair(c, &ports[1], &ports[2]);
	connector_add_component(c, "pres_2", 6, ports, 2);
	rw_err_peek(c);
	
	// Connect with peers (5000ms timeout).
	connector_connect(c, 5000);
	rw_err_peek(c);
	
	// Prepare to receive a message.
	connector_get(c, ports[2]);
	rw_err_peek(c);
	
	// ... reach new consistent state within 1000ms deadline.
	connector_sync(c, 1000);
	rw_err_peek(c);

	// Read our received message
	size_t msg_len;
	const char * msg_ptr = connector_gotten_bytes(c, ports[2], &msg_len);
	printf("Got msg `%.*s`\n", (int) msg_len, msg_ptr);
	
	printf("Exiting\n");
	protocol_description_destroy(pd);
	connector_destroy(c);
	free(pdl);
	sleep(1.0);
	return 0;
}
